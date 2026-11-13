use serde_json::json;
use tracing::{debug, warn};

use super::*;

/// One Algolia search hit a tool result carried, as the cards show it.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub object_id: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub snippet: Option<String>,
}

/// The Algolia search hits a tool result's JSON carries, when it has that shape: a top-level
/// `hits` array, or the first non-empty `results[].hits` of a multi-query search. Anything
/// else — a call's query args, another tool's payload — is `None` and stays raw JSON.
impl SearchHit {
    pub fn from_json(json: &str) -> Option<Vec<SearchHit>> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        let hits = v.get("hits").and_then(|h| h.as_array()).or_else(|| {
            v.get("results")?
                .as_array()?
                .iter()
                .find_map(|r| r.get("hits")?.as_array().filter(|h| !h.is_empty()))
        })?;
        let hits: Vec<SearchHit> = hits.iter().filter_map(hit_card).collect();
        (!hits.is_empty()).then_some(hits)
    }
}

impl SearchHit {
    /// What identifies this hit across searches: its `objectID`, or without one its title and
    /// url.
    pub fn key(&self) -> String {
        match &self.object_id {
            Some(id) => format!("id:{id}"),
            None => format!(
                "t:{}|{}",
                self.title,
                self.url.as_deref().unwrap_or_default()
            ),
        }
    }
}

impl SearchHit {
    /// The snippet's prose on one line, for a result row: markdown markup stripped, links
    /// read as their text, and headings left out when there is prose besides (a snippet's
    /// headings are metadata such as "Updated for …").
    pub fn excerpt(&self) -> String {
        let Some(md) = &self.snippet else {
            return String::new();
        };
        let lines: Vec<(bool, &str)> = md
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("```"))
            .map(|l| match l.trim_start_matches('#') {
                rest if rest.len() < l.len() && rest.starts_with(' ') => (true, rest),
                _ => (false, l),
            })
            .collect();
        let prose = lines.iter().any(|(heading, _)| !heading);
        let text = lines
            .iter()
            .filter(|(heading, _)| !prose || !heading)
            .map(|(_, l)| strip_list_marker(l))
            .collect::<Vec<_>>()
            .join(" ");
        strip_inline(&text)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn strip_list_marker(line: &str) -> &str {
    let line = line.trim_start_matches('>').trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest;
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    match line[digits..].strip_prefix(". ") {
        Some(rest) if digits > 0 => rest,
        _ => line,
    }
}

/// Inline markdown to its text: `[text](url)` and `![alt](url)` to their text, and emphasis,
/// strong and code marks dropped. An underscore inside a word stays.
fn strip_inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let word = |j: Option<&char>| j.is_some_and(|c| c.is_alphanumeric());
        match c {
            '!' if chars.get(i + 1) == Some(&'[') => {}
            '[' => {}
            ']' if chars.get(i + 1) == Some(&'(') => {
                let close = chars[i..].iter().position(|&c| c == ')');
                i += close.unwrap_or(0);
            }
            '*' | '`' | '~' => {}
            '_' if !(i > 0 && word(chars.get(i - 1)) && word(chars.get(i + 1))) => {}
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// The keys of the hits the agent showed, to mark them in a search of our own.
pub fn agent_saw(shown: &[SearchHit]) -> std::collections::HashSet<String> {
    shown.iter().map(SearchHit::key).collect()
}

/// A tool call's search request: the index and query, with every other search param kept
/// as it came so it can be re-run and tweaked.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryParams {
    pub index: Option<String>,
    pub query: String,
    pub params: serde_json::Map<String, serde_json::Value>,
}

/// Fields of a tool call's arguments that aren't Algolia search params.
const NOT_SEARCH_PARAMS: [&str; 5] = ["indexName", "index_name", "index", "query", "userIntent"];

/// The search a tool call's input describes, in either shape: a top-level query object or a
/// `queries[]` array (the first query). `None` when there's no query string.
pub fn query_params(input: &str) -> Option<QueryParams> {
    let v: serde_json::Value = serde_json::from_str(input).ok()?;
    let first = match v.get("queries") {
        Some(queries) => queries.as_array()?.first()?,
        None => &v,
    };
    let obj = first.as_object()?;
    let query = obj.get("query")?.as_str()?.to_string();
    let index = ["indexName", "index_name", "index"]
        .iter()
        .find_map(|k| obj.get(*k)?.as_str())
        .filter(|i| !i.is_empty())
        .map(String::from);
    let params = obj
        .iter()
        .filter(|(k, _)| !NOT_SEARCH_PARAMS.contains(&k.as_str()) && *k != "sessionId")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Some(QueryParams {
        index,
        query,
        params,
    })
}

const INDEX_TOOL_PREFIX: &str = "algolia_search_index_";

/// The index a search tool is named for (`algolia_search_index_<index>`), when it is.
pub fn index_from_tool_name(name: &str) -> Option<&str> {
    name.strip_prefix(INDEX_TOOL_PREFIX)
        .filter(|i| !i.is_empty())
}

/// Searches an index directly: the read-only `query` endpoint, nothing else.
pub trait IndexSearch: Send + Sync {
    fn search(
        &self,
        app_id: &str,
        api_key: &str,
        index: &str,
        params: &serde_json::Value,
    ) -> Result<SearchPage, String>;
}

/// The real index when an application is connected, otherwise the mock's canned page.
pub struct AutoSearch;

impl IndexSearch for AutoSearch {
    fn search(
        &self,
        app_id: &str,
        api_key: &str,
        index: &str,
        params: &serde_json::Value,
    ) -> Result<SearchPage, String> {
        if app_id.is_empty() {
            return MockSearch.search(app_id, api_key, index, params);
        }
        AlgoliaSearch.search(app_id, api_key, index, params)
    }
}

pub struct AlgoliaSearch;

impl IndexSearch for AlgoliaSearch {
    fn search(
        &self,
        app_id: &str,
        api_key: &str,
        index: &str,
        params: &serde_json::Value,
    ) -> Result<SearchPage, String> {
        let url = format!(
            "https://{app_id}-dsn.algolia.net/1/indexes/{}/query",
            path_segment(index)
        );
        debug!("POST {url}");
        let response = authed(http().post(&url), app_id, api_key)
            .send_json(params)
            .map_err(|e| {
                warn!("search failed: {e}");
                format!("search failed: {e}")
            })?;
        let status = response.status();
        let body = response
            .into_body()
            .read_to_string()
            .map_err(|e| format!("reading the search response failed: {e}"))?;
        if !status.is_success() {
            return Err(parse_search_page(&body)
                .err()
                .unwrap_or_else(|| format!("HTTP {status}")));
        }
        parse_search_page(&body)
    }
}

/// Answers every search with the same three guides.
pub struct MockSearch;

impl IndexSearch for MockSearch {
    fn search(
        &self,
        _app_id: &str,
        _api_key: &str,
        _index: &str,
        _params: &serde_json::Value,
    ) -> Result<SearchPage, String> {
        parse_search_page(
            &json!({
                "hits": [
                    { "objectID": "mock-1", "title": "Fit guide: wide feet", "path": "/guides/wide-fit",
                      "content": "The current best **shoe and insole** pairings." },
                    { "objectID": "mock-2", "title": "Shoe tier list", "path": "/guides/shoes",
                      "content": "Where every shoe lands after the latest release." },
                    { "objectID": "mock-3", "title": "Sizing cheat sheet", "path": "/guides/sizing",
                      "content": "How to compare shoes without running the numbers." },
                ],
                "nbHits": 3, "page": 0, "nbPages": 1, "hitsPerPage": 20, "processingTimeMS": 2,
            })
            .to_string(),
        )
    }
}

/// One hit of a search of our own, with its place in the whole result list (1-based).
#[derive(Clone, Debug, PartialEq)]
pub struct RankedHit {
    pub rank: usize,
    pub hit: SearchHit,
}

/// One page of an index's response to a search.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchPage {
    pub hits: Vec<RankedHit>,
    pub nb_hits: u64,
    pub processing_ms: u64,
    pub page: u32,
    pub nb_pages: u32,
}

/// An index search response, or the API's own error message when the body is one.
pub fn parse_search_page(body: &str) -> Result<SearchPage, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("bad search response: {e}"))?;
    let Some(raw_hits) = v.get("hits").and_then(|h| h.as_array()) else {
        return Err(match v.get("message").and_then(|m| m.as_str()) {
            Some(message) => match v.get("status").and_then(|s| s.as_u64()) {
                Some(status) => format!("{message} (HTTP {status})"),
                None => message.to_string(),
            },
            None => "bad search response: no hits".into(),
        });
    };
    let number = |key: &str| v.get(key).and_then(|n| n.as_u64()).unwrap_or(0);
    let page = number("page");
    let per_page = v
        .get("hitsPerPage")
        .and_then(|n| n.as_u64())
        .unwrap_or(raw_hits.len() as u64);
    let first_rank = (page * per_page) as usize;
    Ok(SearchPage {
        hits: raw_hits
            .iter()
            .enumerate()
            .filter_map(|(i, raw)| {
                Some(RankedHit {
                    rank: first_rank + i + 1,
                    hit: hit_card(raw)?,
                })
            })
            .collect(),
        nb_hits: number("nbHits"),
        processing_ms: number("processingTimeMS"),
        page: page as u32,
        nb_pages: number("nbPages") as u32,
    })
}

/// The fields one hit shows: a title from the usual suspects, a link when it has one, and a
/// snippet to read under the title.
fn hit_card(hit: &serde_json::Value) -> Option<SearchHit> {
    let obj = hit.as_object()?;
    let field = |names: &[&str]| -> Option<String> {
        names
            .iter()
            .find_map(|n| obj.get(*n))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let title = field(&["title", "name", "objectID"])?;
    Some(SearchHit {
        object_id: field(&["objectID"]),
        title,
        url: field(&["url", "link", "path"]),
        // The markdown body reads best on a card, so it wins over a short summary.
        snippet: field(&["content", "text", "description", "summary", "excerpt"]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(json: &str) -> QueryParams {
        query_params(json).expect("a recognisable query")
    }

    #[test]
    fn a_single_query_call_yields_its_index_query_and_other_params() {
        let q = params(
            r#"{"indexName":"products_markdown","query":"wide running shoes","hitsPerPage":5,"userIntent":"why"}"#,
        );
        assert_eq!(q.index.as_deref(), Some("products_markdown"));
        assert_eq!(q.query, "wide running shoes");
        assert_eq!(
            q.params,
            json!({ "hitsPerPage": 5 }).as_object().unwrap().clone()
        );
    }

    #[test]
    fn a_queries_array_yields_its_first_query_and_keeps_unknown_params() {
        let q = params(
            r#"{"queries":[{"indexName":"products_markdown","query":"wide running shoes","facet_lang":["en-US"]},{"indexName":"other","query":"second"}],"userIntent":"…"}"#,
        );
        assert_eq!(q.index.as_deref(), Some("products_markdown"));
        assert_eq!(q.query, "wide running shoes");
        assert_eq!(
            q.params,
            json!({ "facet_lang": ["en-US"] })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn a_query_without_an_index_name_leaves_the_index_open() {
        let q = params(r#"{"queries":[{"query":"x"}]}"#);
        assert_eq!(q.index, None);
        assert!(q.params.is_empty());
    }

    #[test]
    fn a_call_with_no_query_has_no_query_params() {
        for input in [
            "{}",
            r#"{"hits":[]}"#,
            r#"{"queries":[]}"#,
            r#"{"queries":[{"indexName":"i"}]}"#,
            r#"{"queries":[{"query":7}]}"#,
            "not json",
            "[]",
        ] {
            assert_eq!(query_params(input), None, "{input}");
        }
    }

    #[test]
    fn a_search_response_parses_hits_ranks_and_totals() {
        let page = parse_search_page(
            r#"{"hits":[{"objectID":"a1","title":"A","url":"/a","content":"body"},{"objectID":"b2"}],
                "nbHits":42,"page":1,"nbPages":21,"hitsPerPage":2,"processingTimeMS":4}"#,
        )
        .unwrap();
        assert_eq!(
            (page.nb_hits, page.page, page.nb_pages, page.processing_ms),
            (42, 1, 21, 4)
        );
        let ranks: Vec<usize> = page.hits.iter().map(|h| h.rank).collect();
        assert_eq!(ranks, [3, 4], "rank counts the pages before this one");
        assert_eq!(page.hits[0].hit.object_id.as_deref(), Some("a1"));
        assert_eq!(page.hits[0].hit.title, "A");
        assert_eq!(
            page.hits[1].hit.title, "b2",
            "no title falls back to the objectID"
        );
    }

    #[test]
    fn an_empty_search_response_is_a_page_with_no_hits() {
        let page = parse_search_page(r#"{"hits":[],"nbHits":0,"page":0,"nbPages":0}"#).unwrap();
        assert!(page.hits.is_empty());
        assert_eq!(page.nb_hits, 0);
    }

    #[test]
    fn a_hit_the_cards_cannot_show_still_counts_in_the_ranking() {
        let page = parse_search_page(
            r#"{"hits":[{"foo":1},{"objectID":"b2"}],"nbHits":2,"page":0,"hitsPerPage":20}"#,
        )
        .unwrap();
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].rank, 2);
    }

    #[test]
    fn an_error_body_becomes_a_readable_message() {
        let err = parse_search_page(r#"{"message":"Index products does not exist","status":404}"#)
            .unwrap_err();
        assert_eq!(err, "Index products does not exist (HTTP 404)");
    }

    #[test]
    fn a_response_that_is_not_json_is_an_error_not_a_panic() {
        assert!(
            parse_search_page("<html>")
                .unwrap_err()
                .contains("search response")
        );
    }

    fn hit(object_id: Option<&str>, title: &str, url: Option<&str>) -> SearchHit {
        SearchHit {
            object_id: object_id.map(Into::into),
            title: title.into(),
            url: url.map(Into::into),
            snippet: None,
        }
    }

    #[test]
    fn hits_the_agent_showed_are_matched_on_object_id() {
        let shown = [hit(Some("a1"), "Anything", None)];
        let saw = agent_saw(&shown);
        assert!(saw.contains(&hit(Some("a1"), "Renamed since", None).key()));
        assert!(!saw.contains(&hit(Some("b2"), "Anything", None).key()));
    }

    #[test]
    fn hits_without_an_object_id_match_on_title_and_url() {
        let shown = [hit(None, "Guide", Some("/guide"))];
        let saw = agent_saw(&shown);
        assert!(saw.contains(&hit(None, "Guide", Some("/guide")).key()));
        assert!(!saw.contains(&hit(None, "Guide", Some("/other")).key()));
        assert!(!saw.contains(&hit(Some("g"), "Guide", Some("/guide")).key()));
    }

    #[test]
    fn top_level_hits_become_cards() {
        let json = r#"{ "hits": [
            { "title": "Reindexing guide", "url": "https://example.com/reindex",
              "description": "How to reindex safely" },
            { "name": "FAQ", "path": "/faq", "summary": "Common questions" },
            { "objectID": "hit-3" }
        ] }"#;
        let hits = SearchHit::from_json(json).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].title, "Reindexing guide");
        assert_eq!(hits[0].url.as_deref(), Some("https://example.com/reindex"));
        assert_eq!(hits[1].title, "FAQ");
        assert_eq!(hits[1].url.as_deref(), Some("/faq"));
        assert_eq!(hits[2].title, "hit-3", "an id-only hit still names itself");
        assert_eq!(hits[2].url, None);
    }

    #[test]
    fn the_markdown_body_of_a_hit_wins_over_a_short_summary() {
        let json = "{ \"hits\": [ { \"title\": \"Guide\", \"summary\": \"short\", \
            \"content\": \"###### *Updated*\\n\\nThe **full** body\" } ] }";
        assert_eq!(
            SearchHit::from_json(json).unwrap()[0].snippet.as_deref(),
            Some("###### *Updated*\n\nThe **full** body"),
            "cards render markdown, so the body wins"
        );
    }

    #[test]
    fn a_multi_query_search_yields_its_first_non_empty_hits() {
        let json = r#"{ "results": [
            { "hits": [] },
            { "hits": [{ "title": "From the second query" }] }
        ] }"#;
        assert_eq!(
            SearchHit::from_json(json).unwrap()[0].title,
            "From the second query"
        );
    }

    #[test]
    fn query_args_and_other_payloads_are_not_hits() {
        assert_eq!(SearchHit::from_json(r#"{ "query": "index health" }"#), None);
        assert_eq!(SearchHit::from_json(r#"{ "hits": "not an array" }"#), None);
        assert_eq!(SearchHit::from_json("not json"), None);
        assert_eq!(
            SearchHit::from_json(r#"{ "hits": [] }"#),
            None,
            "no hits, no cards"
        );
    }

    #[test]
    fn an_excerpt_is_the_snippets_prose_without_markup() {
        let hit = |md: &str| SearchHit {
            object_id: None,
            title: "t".into(),
            url: None,
            snippet: Some(md.into()),
        };
        assert_eq!(
            hit("###### *Updated for the 2026 season*\n\nThe current best **shoe and insole** pairings.\n\n- W = Wide\n- XW = Extra wide").excerpt(),
            "The current best shoe and insole pairings. W = Wide XW = Extra wide",
            "headings are metadata; emphasis and list markers go"
        );
        assert_eq!(
            hit("Full sheet: [the sizing sheet](https://example.com/sizing) and `code`").excerpt(),
            "Full sheet: the sizing sheet and code",
            "a link reads as its text"
        );
        assert_eq!(
            hit("snake_case_name stays, _this_ goes").excerpt(),
            "snake_case_name stays, this goes",
            "underscores inside words are not emphasis"
        );
        assert_eq!(
            hit("## Only a heading").excerpt(),
            "Only a heading",
            "a lone heading is kept"
        );
        assert_eq!(
            SearchHit {
                snippet: None,
                ..hit("")
            }
            .excerpt(),
            ""
        );
    }
}
