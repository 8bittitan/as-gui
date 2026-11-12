//! "Search it yourself": re-running an agent's Algolia query against the index, in a panel
//! beside the chat. This file holds the panel's state and its pure rules; the view follows.

use std::collections::HashSet;

use rune::TextEdit;
use rune::{El, ScrollSpec, Tx, col, row, spacer, text, text_input};
use rune_kit::{
    Badge, BadgeVariant, Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant,
    Markdown,
};
use serde_json::{Map, Value};

use crate::agent::{QueryParams, RankedHit, SearchPage, path_segment};
use crate::chat::Chat;
use crate::conversation::skeleton_bar;
use crate::loading::{Load, Phase};

pub const PANEL_WIDTH: f32 = 400.0;

/// The search params the `query` endpoint accepts. A tool call's other arguments (an agent
/// tool's own `facet_lang`, say) would be rejected as unknown, so they aren't sent.
const ALGOLIA_PARAMS: &[&str] = &[
    "advancedSyntax",
    "advancedSyntaxFeatures",
    "allowTyposOnNumericTokens",
    "alternativesAsExact",
    "analytics",
    "analyticsTags",
    "aroundLatLng",
    "aroundLatLngViaIP",
    "aroundPrecision",
    "aroundRadius",
    "attributeCriteriaComputedByMinProximity",
    "attributesToHighlight",
    "attributesToRetrieve",
    "attributesToSnippet",
    "clickAnalytics",
    "disableExactOnAttributes",
    "disableTypoToleranceOnAttributes",
    "distinct",
    "enableABTest",
    "enablePersonalization",
    "enableReRanking",
    "enableRules",
    "exactOnSingleWordQuery",
    "facetFilters",
    "facetingAfterDistinct",
    "facets",
    "filters",
    "getRankingInfo",
    "highlightPostTag",
    "highlightPreTag",
    "hitsPerPage",
    "ignorePlurals",
    "insideBoundingBox",
    "insidePolygon",
    "length",
    "maxFacetHits",
    "maxValuesPerFacet",
    "minProximity",
    "minWordSizefor1Typo",
    "minWordSizefor2Typos",
    "minimumAroundRadius",
    "naturalLanguages",
    "numericFilters",
    "offset",
    "optionalFilters",
    "optionalWords",
    "page",
    "percentileComputation",
    "personalizationImpact",
    "queryLanguages",
    "queryType",
    "relevancyStrictness",
    "removeStopWords",
    "removeWordsIfNoResults",
    "renderingContent",
    "replaceSynonymsInHighlight",
    "responseFields",
    "restrictHighlightAndSnippetArrays",
    "reRankingApplyFilter",
    "ruleContexts",
    "similarQuery",
    "snippetEllipsisText",
    "sortFacetValuesBy",
    "sumOrFiltersScores",
    "synonyms",
    "tagFilters",
    "typoTolerance",
    "userToken",
];

#[derive(Default)]
pub struct SearchPanel {
    pub open: bool,
    pub index: TextEdit,
    pub query: TextEdit,
    /// Every search param besides the index and query, as editable JSON.
    pub params: TextEdit,
    pub advanced_open: bool,
    /// Params of the call that aren't Algolia search params, left out of the JSON.
    pub not_sent: Vec<String>,
    pub load: Load,
    pub page: Option<SearchPage>,
    /// The keys of the hits the agent showed, to mark them in the results.
    pub agent_saw: HashSet<String>,
    /// The latest run: a result tagged with an earlier one is dropped.
    pub run: u64,
}

impl SearchPanel {
    /// Fills the fields from the call's request; `index` stands in when the call names none.
    pub fn prefill(&mut self, request: QueryParams, index: Option<&str>, saw: HashSet<String>) {
        let index = request.index.as_deref().or(index).unwrap_or_default();
        self.index = TextEdit::new(index);
        self.query = TextEdit::new(request.query);
        let (sent, not_sent): (Map<String, Value>, Map<String, Value>) = request
            .params
            .into_iter()
            .partition(|(k, _)| ALGOLIA_PARAMS.contains(&k.as_str()));
        self.not_sent = not_sent.keys().cloned().collect();
        self.params =
            TextEdit::new(serde_json::to_string_pretty(&Value::Object(sent)).unwrap_or_default());
        self.advanced_open = false;
        self.load = Load::Idle;
        self.page = None;
        self.agent_saw = saw;
        self.open = true;
    }

    fn parsed_params(&self) -> Result<Map<String, Value>, String> {
        match serde_json::from_str::<Value>(&self.params.text) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err("Params must be a JSON object".into()),
            Err(e) => Err(format!("Params aren't valid JSON: {e}")),
        }
    }

    pub fn params_error(&self) -> Option<String> {
        self.parsed_params().err()
    }

    /// Why Run can't go yet, if it can't.
    pub fn blocked(&self) -> Option<String> {
        if self.index.text.trim().is_empty() {
            return Some("Enter an index name".into());
        }
        self.params_error()
    }

    /// The index and the request body to send: the params, with the query set.
    pub fn request(&self) -> Result<(String, Value), String> {
        if let Some(reason) = self.blocked() {
            return Err(reason);
        }
        let mut body = self.parsed_params()?;
        body.insert("query".into(), Value::String(self.query.text.clone()));
        Ok((self.index.text.trim().to_string(), Value::Object(body)))
    }

    /// Sets the `page` param in the params JSON, for the page controls.
    pub fn set_page(&mut self, page: u32) -> Result<(), String> {
        let mut params = self.parsed_params()?;
        params.insert("page".into(), page.into());
        self.params =
            TextEdit::new(serde_json::to_string_pretty(&Value::Object(params)).unwrap_or_default());
        Ok(())
    }

    /// The search as a shell command; the key is a variable, never its value.
    pub fn curl(&self, app_id: &str) -> Result<String, String> {
        let (index, body) = self.request()?;
        let url = shell_quote(&format!(
            "https://{app_id}-dsn.algolia.net/1/indexes/{}/query",
            path_segment(&index)
        ));
        let app_header = shell_quote(&format!("x-algolia-application-id: {app_id}"));
        let body = shell_quote(&body.to_string());
        Ok(format!(
            "curl -X POST {url} \
             -H {app_header} \
             -H \"x-algolia-api-key: $ALGOLIA_SEARCH_API_KEY\" \
             -H 'content-type: application/json' \
             --data-raw {body}"
        ))
    }
}

/// The slide-over beside the chat: the fields on top, then results, then the footer.
pub fn panel(s: &Chat, k: &rune_kit::Kit) -> El {
    let panel = &s.search;
    let phase = panel.load.phase(s.now);
    let mut body = col().grow(1.0).min_h(0.0).w_full();
    body = match &phase {
        Phase::Failed(error) => body.child(error_state(k, error)),
        Phase::Skeleton => body.child(skeleton(k, s.now)),
        _ => match &panel.page {
            Some(page) => body.child(results(s, k, page)),
            None => body.child(
                col().w_full().padding(16.0).child(
                    text("Run the search to see what the index returns.")
                        .wrap()
                        .color(k.c.text_muted),
                ),
            ),
        },
    };
    let mut view = col()
        .key("search-panel")
        .role("complementary")
        .label("Search it yourself")
        .w(PANEL_WIDTH)
        .h_full()
        .fill(k.c.layer_01)
        .child(header(k))
        .child(fields(s, k))
        .child(body);
    if let Some(page) = panel
        .page
        .as_ref()
        .filter(|_| !matches!(phase, Phase::Failed(_)))
    {
        view = view.child(footer(s, k, page));
    }
    view
}

fn header(k: &rune_kit::Kit) -> El {
    row()
        .w_full()
        .px(16.0)
        .pt(12.0)
        .pb(8.0)
        .center_y()
        .child(
            text("Search it yourself")
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        )
        .child(spacer())
        .child(
            IconButton::new(k.glyph.close)
                .variant(IconButtonVariant::Ghost)
                .control_size(ControlSize::Regular)
                .label("Close search")
                .on_press(|s: &mut Chat| s.close_search())
                .build(k)
                .key("search-close"),
        )
}

fn fields(s: &Chat, k: &rune_kit::Kit) -> El {
    let panel = &s.search;
    let run = |s: &mut Chat, _: &mut Tx| s.run_search();
    let index = text_input(&panel.index, k.ty.ui)
        .key("search-index")
        .single_line()
        .w_full()
        .color(k.c.text_base)
        .placeholder("Index name", k.c.text_faint)
        .caret(k.c.text_base, k.selection())
        .label("Index")
        .on_edit(|s: &mut Chat| &mut s.search.index)
        .on_submit(run);
    let query = text_input(&panel.query, k.ty.ui)
        .key("search-query")
        .single_line()
        .w_full()
        .color(k.c.text_base)
        .placeholder("Query", k.c.text_faint)
        .caret(k.c.text_base, k.selection())
        .label("Query")
        .on_edit(|s: &mut Chat| &mut s.search.query)
        .on_submit(run);
    let blocked = panel.blocked();
    let mut view = col()
        .w_full()
        .px(16.0)
        .pb(12.0)
        .gap(8.0)
        .child(input_box(k, index))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(input_box(k, query).grow(1.0).min_w(0.0))
                .child(
                    Button::new("Run")
                        .variant(ButtonVariant::Submit)
                        .disabled(blocked.is_some())
                        .on_press(|s: &mut Chat| s.run_search())
                        .build(k)
                        .key("search-run"),
                ),
        );
    if !panel.not_sent.is_empty() {
        view = view.child(
            col().key("search-not-sent").w_full().child(
                text(format!(
                    "Not sent (not Algolia search params): {}",
                    panel.not_sent.join(", ")
                ))
                .font(k.ty.ui_compact)
                .color(k.c.text_faint)
                .wrap(),
            ),
        );
    }
    if let Some(reason) = blocked {
        view = view.child(
            text(reason)
                .key("search-blocked")
                .font(k.ty.ui_compact)
                .color(k.c.warning.fg)
                .wrap(),
        );
    }
    view = view.child(
        Button::new("Advanced")
            .variant(ButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .expanded(panel.advanced_open)
            .on_press(|s: &mut Chat| s.search.advanced_open = !s.search.advanced_open)
            .build(k)
            .key("search-advanced"),
    );
    if panel.advanced_open {
        let params = text_input(&panel.params, k.ty.mono)
            .key("search-params")
            .w_full()
            .color(k.c.text_base)
            .caret(k.c.text_base, k.selection())
            .label("Search params (JSON)")
            .on_edit(|s: &mut Chat| &mut s.search.params)
            .on_submit(run);
        view = view.child(input_box(k, rune::auto_grow(params, 4, 10)));
    }
    view
}

fn input_box(k: &rune_kit::Kit, input: El) -> El {
    col()
        .w_full()
        .padding(8.0)
        .px(10.0)
        .radius(8.0)
        .fill(k.c.layer_02)
        .border(1.0, k.c.border_muted)
        .child(input)
}

fn error_state(k: &rune_kit::Kit, error: &str) -> El {
    col()
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .child(
            col()
                .key("search-error")
                .w_full()
                .child(text(error).wrap().color(k.c.danger.fg)),
        )
        .child(
            Button::new("Retry")
                .on_press(|s: &mut Chat| s.run_search())
                .build(k)
                .key("search-retry"),
        )
}

fn skeleton(k: &rune_kit::Kit, now: f64) -> El {
    let mut list = col()
        .key("search-skeleton")
        .w_full()
        .padding(16.0)
        .gap(12.0);
    for _ in 0..3 {
        list = list.child(
            col()
                .w_full()
                .gap(8.0)
                .child(skeleton_bar(k, 220.0, 14.0, now))
                .child(skeleton_bar(k, 320.0, 12.0, now))
                .child(skeleton_bar(k, 260.0, 12.0, now)),
        );
    }
    list
}

fn results(s: &Chat, k: &rune_kit::Kit, page: &SearchPage) -> El {
    let mut list = col()
        .key("search-results")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(16.0)
        .pt(4.0)
        .gap(8.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .role("list")
        .label("Search results");
    if page.hits.is_empty() {
        list = list.child(text("No hits.").color(k.c.text_muted));
    }
    for ranked in &page.hits {
        list = list.child(hit_card(s, k, ranked));
    }
    list
}

fn hit_card(s: &Chat, k: &rune_kit::Kit, ranked: &RankedHit) -> El {
    let hit = &ranked.hit;
    let rank = ranked.rank;
    let mut head = row().w_full().gap(8.0).center_y().child(
        text(format!("#{rank}"))
            .font(k.ty.ui_compact)
            .color(k.c.text_faint),
    );
    head = head.child(
        text(&hit.title)
            .font(k.ty.ui_medium)
            .color(k.c.text_accent)
            .truncate()
            .grow(1.0)
            .min_w(0.0),
    );
    if s.search.agent_saw.contains(&hit.key()) {
        head = head.child(
            Badge::new("agent saw this")
                .variant(BadgeVariant::Subtle)
                .build(k)
                .key(format!("search-saw-{rank}")),
        );
    }
    let mut card = col()
        .key(format!("search-hit-{rank}"))
        .role("listitem")
        .w_full()
        .h(s.hit_card_height().min(130.0))
        .padding(12.0)
        .gap(6.0)
        .radius(10.0)
        .fill(k.c.layer_02)
        .border(1.0, k.c.border_muted)
        .clip_rounded(10.0)
        .child(head);
    if let Some(snippet) = &hit.snippet {
        card = card.child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .min_w(0.0)
                .child(Markdown::new(snippet.clone()).build(k)),
        );
    }
    card
}

fn footer(s: &Chat, k: &rune_kit::Kit, page: &SearchPage) -> El {
    let first = page.page == 0;
    let last = page.page + 1 >= page.nb_pages;
    let curl = s.search_curl();
    let totals = format!(
        "{} hits · {} ms · Page {} of {}",
        page.nb_hits,
        page.processing_ms,
        page.page + 1,
        page.nb_pages.max(1)
    );
    col()
        .key("search-footer")
        .w_full()
        .padding(12.0)
        .px(16.0)
        .gap(6.0)
        .child(text(totals).font(k.ty.ui_compact).color(k.c.text_muted))
        .child(
            row()
                .w_full()
                .gap(6.0)
                .center_y()
                .child(
                    Button::new("Previous")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .disabled(first)
                        .on_press(move |s: &mut Chat| s.search_page(page_before(s)))
                        .build(k)
                        .key("search-prev"),
                )
                .child(
                    Button::new("Next")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .disabled(last)
                        .on_press(move |s: &mut Chat| s.search_page(page_after(s)))
                        .build(k)
                        .key("search-next"),
                )
                .child(spacer())
                .child(
                    Button::new("Copy as curl")
                        .variant(ButtonVariant::Outline)
                        .control_size(ControlSize::Small)
                        .disabled(curl.is_none())
                        .on_press_tx(move |_: &mut Chat, tx: &mut Tx| {
                            if let Some(curl) = &curl {
                                tx.copy(curl.clone());
                            }
                        })
                        .build(k)
                        .key("search-curl"),
                ),
        )
}

fn page_before(s: &Chat) -> u32 {
    s.search
        .page
        .as_ref()
        .map_or(0, |p| p.page.saturating_sub(1))
}

fn page_after(s: &Chat) -> u32 {
    s.search.page.as_ref().map_or(0, |p| p.page + 1)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(index: &str, query: &str, params: &str) -> SearchPanel {
        SearchPanel {
            index: TextEdit::new(index),
            query: TextEdit::new(query),
            params: TextEdit::new(params),
            ..SearchPanel::default()
        }
    }

    fn request(index: Option<&str>, params: Value) -> QueryParams {
        QueryParams {
            index: index.map(Into::into),
            query: "q".into(),
            params: params.as_object().unwrap().clone(),
        }
    }

    #[test]
    fn prefilling_sends_only_params_algolia_knows() {
        let mut p = SearchPanel::default();
        p.prefill(
            request(
                None,
                serde_json::json!({ "hitsPerPage": 5, "facet_lang": ["en-US"] }),
            ),
            None,
            HashSet::new(),
        );
        let sent: Value = serde_json::from_str(&p.params.text).unwrap();
        assert_eq!(sent, serde_json::json!({ "hitsPerPage": 5 }));
        assert_eq!(p.not_sent, ["facet_lang"]);
    }

    #[test]
    fn a_param_the_user_adds_is_sent_whatever_it_is_called() {
        let mut p = SearchPanel::default();
        p.prefill(
            request(Some("i"), serde_json::json!({})),
            None,
            HashSet::new(),
        );
        p.index = TextEdit::new("i");
        p.params = TextEdit::new(r#"{"facet_lang": ["en-US"]}"#);
        let (_, body) = p.request().unwrap();
        assert_eq!(body["facet_lang"], serde_json::json!(["en-US"]));
    }

    #[test]
    fn a_new_prefill_forgets_the_last_calls_dropped_params() {
        let mut p = SearchPanel::default();
        p.prefill(
            request(None, serde_json::json!({ "facet_lang": 1 })),
            None,
            HashSet::new(),
        );
        p.prefill(request(None, serde_json::json!({})), None, HashSet::new());
        assert!(p.not_sent.is_empty());
    }

    #[test]
    fn copy_as_curl_cannot_be_broken_out_of_by_the_index_or_app_id() {
        let p = panel("x'; touch /tmp/pwn; '", "q", "{}");
        let curl = p.curl("APP'; id; '").unwrap();
        assert!(
            curl.contains("/indexes/x%27%3B%20touch%20%2Ftmp%2Fpwn%3B%20%27/query"),
            "{curl}"
        );
        assert!(!curl.contains("touch /tmp/pwn"), "{curl}");
        assert!(
            curl.contains(r#"-H 'x-algolia-application-id: APP'\''; id; '\'''"#),
            "{curl}"
        );
    }

    #[test]
    fn copy_as_curl_builds_the_query_request_with_the_key_masked() {
        let p = panel(
            "products_markdown",
            "wide running shoes",
            r#"{"hitsPerPage": 5}"#,
        );
        assert_eq!(
            p.curl("APP123").unwrap(),
            "curl -X POST 'https://APP123-dsn.algolia.net/1/indexes/products_markdown/query' \
             -H 'x-algolia-application-id: APP123' \
             -H \"x-algolia-api-key: $ALGOLIA_SEARCH_API_KEY\" \
             -H 'content-type: application/json' \
             --data-raw '{\"hitsPerPage\":5,\"query\":\"wide running shoes\"}'"
        );
    }

    #[test]
    fn copy_as_curl_quotes_a_single_quote_in_the_query() {
        let p = panel("i", "it's", "{}");
        assert!(
            p.curl("A")
                .unwrap()
                .ends_with(r#"--data-raw '{"query":"it'\''s"}'"#)
        );
    }

    #[test]
    fn copy_as_curl_refuses_what_run_refuses() {
        assert!(panel("", "x", "{}").curl("A").is_err());
        assert!(panel("i", "x", "{").curl("A").is_err());
    }

    #[test]
    fn going_to_a_page_rewrites_the_page_param() {
        let mut p = panel("i", "x", r#"{"hitsPerPage": 5}"#);
        p.set_page(2).unwrap();
        let params: Value = serde_json::from_str(&p.params.text).unwrap();
        assert_eq!(params, serde_json::json!({ "hitsPerPage": 5, "page": 2 }));
    }

    #[test]
    fn going_to_a_page_needs_valid_params() {
        let mut p = panel("i", "x", "{");
        assert!(p.set_page(1).is_err());
        assert_eq!(p.params.text, "{", "the user's text is left alone");
    }
}
