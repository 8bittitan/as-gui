//! The tools the app brings itself. Each checks its own input and does its one thing through
//! a seam, so no test touches the OS.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::menubar::AppShell;
use crate::notify::Notifier;
use crate::selection::SelectionSource;

pub const BUILTIN_TOOLS: [&str; 4] = ["open_url", "copy_to_clipboard", "notify", "read_selection"];

pub fn is_builtin(name: &str) -> bool {
    BUILTIN_TOOLS.contains(&name)
}

/// What a built-in tool reaches the machine through.
pub struct Builtins {
    pub shell: Arc<dyn AppShell + Send + Sync>,
    pub notifier: Arc<dyn Notifier>,
    pub selection: Arc<dyn SelectionSource>,
}

static NOTIFICATIONS: AtomicU64 = AtomicU64::new(0);

const MAX_CLIPBOARD_BYTES: usize = 1_000_000;

impl Builtins {
    /// The tool's result, or `None` when `name` isn't a built-in.
    pub fn run(&self, name: &str, input: &Value) -> Option<Result<Value, String>> {
        Some(match name {
            "open_url" => self.open_url(input),
            "copy_to_clipboard" => self.copy_to_clipboard(input),
            "notify" => self.notify(input),
            "read_selection" => self.read_selection(),
            _ => return None,
        })
    }

    fn open_url(&self, input: &Value) -> Result<Value, String> {
        let url = string_field(input, "url")?;
        if !is_web_url(url) {
            return Err("Only http and https URLs can be opened.".into());
        }
        self.shell.open_url(url);
        Ok(json!({ "opened": url }))
    }

    fn copy_to_clipboard(&self, input: &Value) -> Result<Value, String> {
        let text = string_field(input, "text")?;
        if text.len() > MAX_CLIPBOARD_BYTES {
            return Err("text is over 1 MB.".into());
        }
        self.shell.copy_text(text);
        Ok(json!({ "copied": text.chars().count() }))
    }

    fn notify(&self, input: &Value) -> Result<Value, String> {
        let title = string_field(input, "title")?;
        if title.trim().is_empty() {
            return Err("title must not be empty.".into());
        }
        let body = match input.get("body") {
            None | Some(Value::Null) => "",
            Some(Value::String(body)) => body,
            Some(_) => return Err("body must be a string.".into()),
        };
        let id = format!(
            "client-tool-{}",
            NOTIFICATIONS.fetch_add(1, Ordering::Relaxed)
        );
        let notification = rune_notify::Notification::new(id, title).body(body);
        if self.notifier.post(&notification) {
            Ok(json!({ "posted": true }))
        } else {
            Err("Notifications aren't available for this app right now.".into())
        }
    }

    fn read_selection(&self) -> Result<Value, String> {
        match self.selection.capture() {
            Some(capture) => Ok(json!({ "text": capture.text })),
            None => Err("Nothing is selected, or the clipboard holds a secret.".into()),
        }
    }
}

fn string_field<'a>(input: &'a Value, field: &str) -> Result<&'a str, String> {
    let object = input
        .as_object()
        .ok_or("The input must be a JSON object.")?;
    match object.get(field) {
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(format!("{field} must be a string.")),
        None => Err(format!("{field} is required.")),
    }
}

/// An `http` or `https` URL with a host. Anything else (`file:`, `javascript:`, a custom
/// scheme) is refused.
fn is_web_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && !host.is_empty()
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Access;
    use crate::selection::fakes::FakeSource;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeShell {
        opened: Mutex<Vec<String>>,
        copied: Mutex<Vec<String>>,
    }

    impl AppShell for FakeShell {
        fn open_window(&self) {}
        fn set_dock_icon(&self, _: bool) {}
        fn toggle_quick_ask(&self) {}
        fn quit(&self) {}
        fn open_url(&self, url: &str) {
            self.opened.lock().unwrap().push(url.into());
        }
        fn copy_text(&self, text: &str) {
            self.copied.lock().unwrap().push(text.into());
        }
    }

    struct FakeNotifier {
        posted: Mutex<Vec<String>>,
        available: bool,
    }

    impl Notifier for FakeNotifier {
        fn post(&self, n: &rune_notify::Notification) -> bool {
            self.posted.lock().unwrap().push(format!("{n:?}"));
            self.available
        }
        fn remove(&self, _: &[&str]) {}
        fn focused(&self) -> bool {
            false
        }
    }

    struct Rig {
        builtins: Builtins,
        shell: Arc<FakeShell>,
        notifier: Arc<FakeNotifier>,
    }

    fn rig(selection: FakeSource, notifications: bool) -> Rig {
        let shell = Arc::new(FakeShell::default());
        let notifier = Arc::new(FakeNotifier {
            posted: Mutex::new(Vec::new()),
            available: notifications,
        });
        Rig {
            builtins: Builtins {
                shell: shell.clone(),
                notifier: notifier.clone(),
                selection: Arc::new(selection),
            },
            shell,
            notifier,
        }
    }

    fn default_rig() -> Rig {
        rig(FakeSource::selection("the selected words"), true)
    }

    fn call(rig: &Rig, name: &str, input: Value) -> Result<Value, String> {
        rig.builtins.run(name, &input).expect("a built-in")
    }

    #[test]
    fn open_url_opens_an_http_or_https_url() {
        let rig = default_rig();
        for url in [
            "https://algolia.com/doc?a=1#x",
            "http://localhost:3000",
            "HTTPS://Example.com",
        ] {
            assert!(
                call(&rig, "open_url", json!({ "url": url })).is_ok(),
                "{url}"
            );
        }
        assert_eq!(rig.shell.opened.lock().unwrap().len(), 3);
    }

    #[test]
    fn open_url_refuses_every_other_scheme() {
        let rig = default_rig();
        for url in [
            "file:///etc/passwd",
            "file://localhost/etc/passwd",
            "javascript:alert(1)",
            "tel:+15551234",
            "myapp://open",
            "ftp://host/file",
            "https://",
            "https:///path",
            "example.com",
            "https://exa mple.com",
            "",
        ] {
            let result = call(&rig, "open_url", json!({ "url": url }));
            assert!(result.is_err(), "{url:?} must be refused");
        }
        assert!(rig.shell.opened.lock().unwrap().is_empty());
    }

    #[test]
    fn open_url_wants_a_url_string() {
        let rig = default_rig();
        assert_eq!(
            call(&rig, "open_url", json!({})),
            Err("url is required.".into())
        );
        assert_eq!(
            call(&rig, "open_url", json!({ "url": 4 })),
            Err("url must be a string.".into())
        );
        assert_eq!(
            call(&rig, "open_url", json!("https://a.com")),
            Err("The input must be a JSON object.".into())
        );
    }

    #[test]
    fn copy_to_clipboard_puts_the_text_on_the_clipboard() {
        let rig = default_rig();
        let result = call(&rig, "copy_to_clipboard", json!({ "text": "héllo" }));
        assert_eq!(result, Ok(json!({ "copied": 5 })));
        assert_eq!(*rig.shell.copied.lock().unwrap(), ["héllo"]);
    }

    #[test]
    fn copy_to_clipboard_refuses_bad_input_and_huge_text() {
        let rig = default_rig();
        assert!(call(&rig, "copy_to_clipboard", json!({})).is_err());
        assert!(call(&rig, "copy_to_clipboard", json!({ "text": ["a"] })).is_err());
        let huge = "x".repeat(MAX_CLIPBOARD_BYTES + 1);
        assert!(call(&rig, "copy_to_clipboard", json!({ "text": huge })).is_err());
        assert!(rig.shell.copied.lock().unwrap().is_empty());
    }

    #[test]
    fn notify_posts_a_notification_with_its_title_and_body() {
        let rig = default_rig();
        let result = call(
            &rig,
            "notify",
            json!({ "title": "Build done", "body": "All green" }),
        );
        assert_eq!(result, Ok(json!({ "posted": true })));
        let posted = rig.notifier.posted.lock().unwrap();
        assert!(posted[0].contains("Build done") && posted[0].contains("All green"));
    }

    #[test]
    fn notify_needs_a_title_and_allows_no_body() {
        let rig = default_rig();
        assert!(call(&rig, "notify", json!({ "body": "x" })).is_err());
        assert!(call(&rig, "notify", json!({ "title": "  " })).is_err());
        assert!(call(&rig, "notify", json!({ "title": "t", "body": 3 })).is_err());
        assert!(call(&rig, "notify", json!({ "title": "t" })).is_ok());
    }

    #[test]
    fn notify_says_so_when_the_app_cannot_post() {
        let rig = rig(FakeSource::selection("x"), false);
        let error = call(&rig, "notify", json!({ "title": "t" })).unwrap_err();
        assert!(error.contains("aren't available"), "{error}");
    }

    #[test]
    fn read_selection_returns_the_selected_text() {
        let rig = default_rig();
        assert_eq!(
            call(&rig, "read_selection", json!({})),
            Ok(json!({ "text": "the selected words" }))
        );
    }

    #[test]
    fn read_selection_returns_nothing_when_the_capture_refuses() {
        let rig = rig(FakeSource::with_access(Access::Granted), true);
        let error = call(&rig, "read_selection", json!({})).unwrap_err();
        assert!(error.contains("secret"), "{error}");
    }

    #[test]
    fn only_the_four_tools_are_built_in() {
        let rig = default_rig();
        assert!(rig.builtins.run("throw_confetti", &json!({})).is_none());
        assert!(BUILTIN_TOOLS.iter().all(|name| is_builtin(name)));
        assert!(!is_builtin("throw_confetti"));
    }
}
