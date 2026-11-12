//! The hit reader and the "search it yourself" panel: opening them from a tool call, and
//! running the panel's search on its own thread.

use std::sync::Arc;

use super::Chat;
use crate::agent::{SearchHit, StreamEvent};
use crate::loading::Load;

impl Chat {
    /// Opens the search panel on the query tool call `index` of message `message_id` made.
    pub fn open_search(&mut self, message_id: &str, index: usize) {
        let Some(tool) = self
            .message_mut(message_id)
            .and_then(|m| m.tools.get(index))
            .cloned()
        else {
            return;
        };
        let Some(request) = tool.search_request() else {
            return;
        };
        let shown = tool
            .input
            .as_deref()
            .and_then(SearchHit::from_json)
            .unwrap_or_default();
        self.reader = None;
        self.search.prefill(
            request,
            crate::agent::index_from_tool_name(&tool.name),
            crate::agent::agent_saw(&shown),
        );
    }

    /// Runs page `page` of the current search.
    pub fn search_page(&mut self, page: u32) {
        if self.search.set_page(page).is_ok() {
            self.run_search();
        }
    }

    /// The panel's search as a curl command, for the footer's copy button.
    pub fn search_curl(&self) -> Option<String> {
        let app_id = self.credentials().map_or("APP_ID", |(a, _)| a);
        self.search.curl(app_id).ok()
    }

    pub fn close_search(&mut self) {
        self.search.open = false;
    }

    /// Whether the reader's tool made a search the panel can re-run.
    pub fn reader_can_search(&self) -> bool {
        self.reader_tool_position().is_some()
    }

    pub fn open_search_from_reader(&mut self) {
        if let Some((message_id, index)) = self.reader_tool_position() {
            self.open_search(&message_id, index);
        }
    }

    fn reader_tool_position(&self) -> Option<(String, usize)> {
        let (name, _) = self.reader.as_ref()?;
        self.sessions[self.current].messages.iter().find_map(|m| {
            let i = m.tools.iter().position(|t| &t.name == name)?;
            m.tools[i].search_request()?;
            Some((m.id.clone(), i))
        })
    }

    /// Runs the panel's search on its own thread; a result for an earlier run is dropped.
    pub fn run_search(&mut self) {
        let Ok((index, body)) = self.search.request() else {
            return;
        };
        let (app_id, api_key) = self
            .credentials()
            .map(|(a, k)| (a.to_string(), k.to_string()))
            .unwrap_or_default();
        self.search.run += 1;
        let run = self.search.run;
        self.search.load = Load::started(self.now);
        let sender = self.sender();
        let searcher = Arc::clone(&self.index_search);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let _ = sender.send(match searcher.search(&app_id, &api_key, &index, &body) {
                Ok(page) => StreamEvent::SearchLoaded { run, page },
                Err(error) => StreamEvent::SearchFailed { run, error },
            });
            if let Some(w) = waker {
                w();
            }
        });
    }

    pub fn open_reader(&mut self, tool: &str, hit: usize) {
        self.reader = Some((tool.to_string(), hit));
    }

    pub fn close_reader(&mut self) {
        self.reader = None;
    }

    /// The hit the reader shows, re-resolved from the current session's tool results.
    pub fn reader_hit(&self) -> Option<SearchHit> {
        let (name, index) = self.reader.as_ref()?;
        self.sessions[self.current]
            .messages
            .iter()
            .flat_map(|m| &m.tools)
            .filter(|t| &t.name == name)
            .find_map(|t| SearchHit::from_json(t.input.as_deref()?))?
            .into_iter()
            .nth(*index)
    }

    /// Applies a search run's result; one for a superseded run is dropped.
    pub(super) fn apply_search(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::SearchLoaded { run, page } if run == self.search.run => {
                self.search.page = Some(page);
                self.search.load = self.search.load.finished(self.now);
            }
            StreamEvent::SearchFailed { run, error } if run == self.search.run => {
                self.search.load = Load::Failed(error);
            }
            StreamEvent::SearchLoaded { .. } | StreamEvent::SearchFailed { .. } => {}
            _ => {}
        }
    }
}
