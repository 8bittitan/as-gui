//! "Search it yourself": re-running an agent's Algolia query against the index, in a panel
//! beside the chat. This file holds the panel's state and its pure rules; the view follows.

use std::collections::HashSet;

use rune::TextEdit;
use rune::{El, ScrollSpec, Tx, col, el, icon, row, spacer, text, text_input};
use rune_kit::{ButtonVariant, IconButton, IconButtonVariant};
use serde_json::{Map, Value};

use crate::agent::{QueryParams, RankedHit, SearchPage, path_segment};
use crate::chat::Chat;
use crate::conversation::skeleton_bar;
use crate::loading::{Load, Phase};
use crate::satellite::{self, Fit, icons, typography};
use crate::shell::field_box;

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

/// The side panel beside the chat: a header, the labelled fields, then the results and a
/// footer with the totals, the pager and Copy as curl.
pub fn panel(s: &Chat, k: &rune_kit::Kit) -> El {
    let panel = &s.search;
    let phase = panel.load.phase(s.now);
    let body = match &phase {
        Phase::Failed(error) => error_state(k, error),
        Phase::Skeleton => skeleton(k, s.now),
        _ => match &panel.page {
            Some(page) => results(s, k, page),
            None => empty_state(k),
        },
    };
    let mut view = col()
        .key("search-panel")
        .role("complementary")
        .label("Search it yourself")
        .w(PANEL_WIDTH)
        .h_full()
        .fill(k.c.bg_base)
        .border_sides([0.0, 0.0, 0.0, 1.0], k.c.border_base)
        .child(header(k))
        .child(fields(s, k))
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_base)
                .child(body),
        );
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
        .min_h(56.0)
        .pl(16.0)
        .pr(12.0)
        .gap(8.0)
        .center_y()
        .border_sides([0.0, 0.0, 1.0, 0.0], k.c.border_base)
        .child(
            text("Search it yourself")
                .font(typography::HEADING)
                .color(k.c.text_base)
                .truncate(),
        )
        .child(spacer())
        .child(
            IconButton::new(k.glyph.close)
                .variant(IconButtonVariant::GhostMuted)
                .label("Close search")
                .on_press(|s: &mut Chat| s.close_search())
                .sized(k, satellite::Size::Medium)
                .key("search-close"),
        )
}

/// A form field: its label over the control, and a hint under it when there is one.
fn labelled(k: &rune_kit::Kit, key: &str, label: &str, control: El) -> El {
    col()
        .key(key)
        .w_full()
        .gap(4.0)
        .child(text(label).font(k.ty.ui_medium).color(k.c.text_base))
        .child(control)
}

fn fields(s: &Chat, k: &rune_kit::Kit) -> El {
    let panel = &s.search;
    let run = |s: &mut Chat, _: &mut Tx| s.run_search();
    let index = text_input(&panel.index, k.ty.ui)
        .key("search-index")
        .single_line()
        .w_full()
        .color(k.c.text_base)
        .placeholder("Index name", k.c.text_muted)
        .caret(k.c.text_base, k.selection())
        .label("Index")
        .on_edit(|s: &mut Chat| &mut s.search.index)
        .on_submit(run);
    let query = text_input(&panel.query, k.ty.ui)
        .key("search-query")
        .single_line()
        .w_full()
        .color(k.c.text_base)
        .placeholder("Query", k.c.text_muted)
        .caret(k.c.text_base, k.selection())
        .label("Query")
        .on_edit(|s: &mut Chat| &mut s.search.query)
        .on_submit(run);
    let blocked = panel.blocked();
    let blocked_now = blocked.is_some();
    let mut query_field = col().w_full().gap(8.0).child(field_box(k, query));
    if !panel.not_sent.is_empty() {
        query_field = query_field.child(not_sent(k, &panel.not_sent));
    }
    let mut view = col()
        .w_full()
        .padding(16.0)
        .gap(16.0)
        .child(labelled(
            k,
            "search-index-field",
            "Index",
            field_box(k, index),
        ))
        .child(labelled(k, "search-query-field", "Query", query_field));
    if let Some(reason) = blocked {
        view = view.child(
            row()
                .key("search-blocked")
                .w_full()
                .gap(8.0)
                .child(
                    icon(&icons::CIRCLE_ALERT, 14.0)
                        .color(k.c.danger.icon)
                        .mt(3.0),
                )
                .child(
                    text(reason)
                        .wrap()
                        .grow(1.0)
                        .min_w(0.0)
                        .font(k.ty.text_small)
                        .color(k.c.danger.fg),
                ),
        );
    }
    view.child(advanced(s, k)).child(
        satellite::button("Run search")
            .icon(&icons::SEARCH)
            .variant(ButtonVariant::Submit)
            .disabled(blocked_now)
            .on_press(|s: &mut Chat| s.run_search())
            .sized(k, satellite::Size::Medium)
            .key("search-run")
            .w_full(),
    )
}

/// The call's arguments Algolia wouldn't accept, listed one per line under the query.
fn not_sent(k: &rune_kit::Kit, keys: &[String]) -> El {
    let mut list = col()
        .key("search-not-sent")
        .role("list")
        .w_full()
        .gap(2.0)
        .child(
            text("Not sent, not Algolia search params:")
                .font(typography::CAPTION)
                .color(k.c.text_muted),
        );
    for key in keys {
        list = list.child(
            row()
                .key(format!("search-not-sent-{key}"))
                .role("listitem")
                .gap(8.0)
                .center_y()
                .pl(4.0)
                .child(
                    text("•")
                        .font(typography::CAPTION)
                        .color(k.c.text_muted)
                        .a11y_hidden(),
                )
                .child(
                    text(key)
                        .font(typography::CAPTION.mono())
                        .color(k.c.std_text),
                ),
        );
    }
    list
}

/// The search params besides the query, folded behind a disclosure row.
fn advanced(s: &Chat, k: &rune_kit::Kit) -> El {
    let panel = &s.search;
    let open = panel.advanced_open;
    let count = serde_json::from_str::<Value>(&panel.params.text)
        .ok()
        .and_then(|v| v.as_object().map(Map::len))
        .unwrap_or(0);
    let mut toggle = row()
        .key("search-advanced")
        .role("button")
        .label("Search parameters")
        .expanded(open)
        .focusable(true)
        .w_full()
        .min_h(32.0)
        .gap(8.0)
        .center_y()
        .on_press(|s: &mut Chat| s.search.advanced_open = !s.search.advanced_open)
        .child(
            icon(
                if open {
                    k.glyph.chevron_down
                } else {
                    k.glyph.caret_right
                },
                14.0,
            )
            .color(k.c.icon_muted),
        )
        .child(
            text("Search parameters")
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        );
    if count > 0 {
        toggle = toggle.child(
            text(format!("· {count}"))
                .font(k.ty.ui)
                .color(k.c.text_muted),
        );
    }
    let mut view = col().w_full().gap(8.0).child(toggle);
    if open {
        let params = text_input(&panel.params, k.ty.mono)
            .key("search-params")
            .w_full()
            .color(k.c.text_base)
            .caret(k.c.text_base, k.selection())
            .label("Search params (JSON)")
            .on_edit(|s: &mut Chat| &mut s.search.params)
            .on_submit(|s: &mut Chat, _: &mut Tx| s.run_search());
        view = view.child(field_box(k, rune::auto_grow(params, 4, 10)));
    }
    view
}

/// Before the first run: what the panel is for and how to start.
fn empty_state(k: &rune_kit::Kit) -> El {
    col()
        .key("search-empty")
        .w_full()
        .px(24.0)
        .py(48.0)
        .gap(8.0)
        .items_center()
        .child(
            el().size(40.0, 40.0)
                .radius(20.0)
                .fill(k.c.layer_02)
                .center()
                .child(icon(&icons::SEARCH, 18.0).color(k.c.icon_muted)),
        )
        .child(
            text("Run the search")
                .font(typography::HEADING)
                .color(k.c.text_base)
                .mt(4.0),
        )
        .child(
            text("See what the index returns for the agent's query, then tweak it and run again.")
                .wrap()
                .max_w(280.0)
                .font(k.ty.text_small)
                .color(k.c.text_muted),
        )
}

fn error_state(k: &rune_kit::Kit, error: &str) -> El {
    col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(
            row()
                .key("search-error")
                .w_full()
                .px(12.0)
                .py(10.0)
                .gap(8.0)
                .radius(satellite::RADIUS_CARD)
                .fill(k.c.danger.bg)
                .border(1.0, k.c.danger.border)
                .child(
                    icon(&icons::CIRCLE_ALERT, 14.0)
                        .color(k.c.danger.icon)
                        .mt(3.0),
                )
                .child(
                    text(error)
                        .wrap()
                        .grow(1.0)
                        .min_w(0.0)
                        .font(k.ty.text_small)
                        .color(k.c.danger.fg),
                ),
        )
        .child(
            satellite::button("Retry")
                .icon(&icons::ROTATE_CW)
                .on_press(|s: &mut Chat| s.run_search())
                .sized(k, satellite::Size::Small)
                .key("search-retry"),
        )
}

fn skeleton(k: &rune_kit::Kit, now: f64) -> El {
    let mut list = col()
        .key("search-skeleton")
        .w_full()
        .padding(16.0)
        .gap(16.0);
    for _ in 0..3 {
        list = list.child(
            col()
                .w_full()
                .gap(4.0)
                .child(skeleton_bar(k, 200.0, 16.0, now))
                .child(skeleton_bar(k, 320.0, 16.0, now))
                .child(skeleton_bar(k, 260.0, 16.0, now)),
        );
    }
    list
}

/// The hits as the chat lists them: one card of rows, rank, title and a two-line excerpt.
fn results(s: &Chat, k: &rune_kit::Kit, page: &SearchPage) -> El {
    let mut scroll = col()
        .key("search-results")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    if page.hits.is_empty() {
        return scroll.child(
            text("No hits for this search.")
                .font(k.ty.text_small)
                .color(k.c.text_muted),
        );
    }
    let mut list = col()
        .key("search-results-list")
        .role("list")
        .label("Search results")
        .w_full()
        .radius(satellite::RADIUS_CARD)
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_base)
        .border_layout()
        .clip_rounded(satellite::RADIUS_CARD);
    for (i, ranked) in page.hits.iter().enumerate() {
        list = list.child(hit_row(s, k, ranked, i > 0));
    }
    scroll = scroll.child(list);
    scroll
}

fn hit_row(s: &Chat, k: &rune_kit::Kit, ranked: &RankedHit, divided: bool) -> El {
    let hit = &ranked.hit;
    let rank = ranked.rank;
    let mut title = row().w_full().gap(8.0).center_y().child(
        text(&hit.title)
            .font(k.ty.ui_medium)
            .color(k.c.text_accent)
            .truncate()
            .grow(1.0)
            .min_w(0.0),
    );
    if s.search.agent_saw.contains(&hit.key()) {
        title = title.child(
            row()
                .key(format!("search-saw-{rank}"))
                .h(20.0)
                .px(6.0)
                .center_y()
                .radius(satellite::RADIUS_CONTROL)
                .fill(k.c.info.bg)
                .border(1.0, k.c.info.border)
                .child(
                    text("Agent saw")
                        .font(typography::CAPTION)
                        .color(k.c.info.fg),
                ),
        );
    }
    let excerpt = hit.excerpt();
    let mut body = col().grow(1.0).min_w(0.0).gap(2.0).child(title);
    if !excerpt.is_empty() {
        body = body.child(
            text(excerpt)
                .font(k.ty.text_small)
                .color(k.c.text_muted)
                .max_lines(2),
        );
    }
    let mut r = row()
        .key(format!("search-hit-{rank}"))
        .role("listitem")
        .w_full()
        .min_w(0.0)
        .px(12.0)
        .py(10.0)
        .gap(12.0)
        .child(
            text(format!("{rank}"))
                .font(typography::CAPTION)
                .color(k.c.text_muted)
                .w(20.0)
                .mt(2.0),
        )
        .child(body);
    if divided {
        r = r.border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_muted);
    }
    r
}

fn footer(s: &Chat, k: &rune_kit::Kit, page: &SearchPage) -> El {
    let first = page.page == 0;
    let last = page.page + 1 >= page.nb_pages;
    let curl = s.search_curl();
    let pager = |glyph: &'static rune::Icon, label: &str, disabled: bool| {
        IconButton::new(glyph)
            .variant(IconButtonVariant::GhostMuted)
            .label(label)
            .disabled(disabled)
    };
    row()
        .key("search-footer")
        .w_full()
        .min_h(48.0)
        .px(16.0)
        .gap(4.0)
        .center_y()
        .border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_base)
        .child(
            text(format!("{} hits · {} ms", page.nb_hits, page.processing_ms))
                .font(typography::CAPTION)
                .color(k.c.text_muted)
                .truncate(),
        )
        .child(spacer())
        .child(
            pager(&icons::CHEVRON_LEFT, "Previous page", first)
                .on_press(move |s: &mut Chat| s.search_page(page_before(s)))
                .sized(k, satellite::Size::Small)
                .key("search-prev"),
        )
        .child(
            text(format!(
                "Page {} of {}",
                page.page + 1,
                page.nb_pages.max(1)
            ))
            .font(typography::CAPTION)
            .color(k.c.text_muted),
        )
        .child(
            pager(&icons::CHEVRON_RIGHT, "Next page", last)
                .on_press(move |s: &mut Chat| s.search_page(page_after(s)))
                .build(k)
                .key("search-next"),
        )
        .child(
            satellite::button("Copy as curl")
                .icon(k.glyph.copy)
                .disabled(curl.is_none())
                .on_press_tx(move |_: &mut Chat, tx: &mut Tx| {
                    if let Some(curl) = &curl {
                        tx.copy(curl.clone());
                    }
                })
                .sized(k, satellite::Size::Small)
                .key("search-curl")
                .ml(8.0),
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
