//! Persisted app state: the last selected agent and the appearance settings, as JSON at
//! `paths::state_path()`.
//! Missing or corrupt files are fine: the app falls back to its defaults.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::chat::{Accent, ChatWidth, ThemePref};

/// One saved Algolia application.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedApplication {
    pub app_id: String,
    pub api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The agent last open in this application, with the name to show before it's listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_agent_name: Option<String>,
    /// The application's colour id (`AppColour::id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Persisted {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applications: Vec<SavedApplication>,
    /// The app id of the active application.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_app: Option<String>,
    /// The single-application fields of earlier versions: read once, then migrated into
    /// `applications` and never written again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub theme: Option<ThemePref>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub accent: Option<Accent>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub width: Option<ChatWidth>,
    /// Whether the window's top edge takes the active application's colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint_window: Option<bool>,
    /// Whether the main window opens at launch (macOS launches to the menu bar otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_at_launch: Option<bool>,
    /// Whether a reply that finishes in the background posts a notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_replies: Option<bool>,
    /// Conversation ids hidden from the sidebar.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<String>,
    /// Shortcuts changed from their defaults, by action; `null` is one the user unbound.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keybinds: BTreeMap<String, Option<String>>,
}

pub fn load() -> Persisted {
    load_from(&crate::paths::state_path())
}

pub fn load_from(path: &std::path::Path) -> Persisted {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Persisted>(&raw).ok())
        .map(Persisted::migrated)
        .unwrap_or_default()
}

impl Persisted {
    /// A file from before multiple applications becomes a one-application list.
    fn migrated(mut self) -> Persisted {
        if self.applications.is_empty()
            && let (Some(app_id), Some(api_key)) = (self.app_id.take(), self.api_key.take())
        {
            self.active_app = Some(app_id.clone());
            self.applications.push(SavedApplication {
                app_id,
                api_key,
                last_agent_id: self.agent_id.take(),
                ..SavedApplication::default()
            });
        }
        self.app_id = None;
        self.api_key = None;
        self.agent_id = None;
        self
    }
}

pub fn save_to(path: &std::path::Path, state: &Persisted) {
    let json = match serde_json::to_string_pretty(state) {
        Ok(json) => json,
        Err(error) => {
            warn!("could not encode state: {error}");
            return;
        }
    };
    if let Some(parent) = path.parent()
        && let Err(error) = create_private_dir(parent)
    {
        warn!("could not create {}: {error}", parent.display());
        return;
    }
    // A temp file renamed over the target, so a crash mid-write can't truncate the state.
    let temp = path.with_extension("json.tmp");
    if let Err(error) =
        write_private(&temp, json.as_bytes()).and_then(|()| std::fs::rename(&temp, path))
    {
        warn!("could not write {}: {error}", path.display());
    }
}

pub(crate) fn create_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// The file holds an API key, so it is created readable by its owner only. A leftover temp
/// file is removed first: an existing file would keep its old mode.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let _ = std::fs::remove_file(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

/// A value the app no longer knows reads as unset, so one stale setting can't cost the rest.
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn state_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("as-gui-store-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("state.json")
    }

    #[test]
    fn a_missing_file_loads_as_defaults() {
        let state = load_from(&state_path("missing"));
        assert_eq!(state, Persisted::default());
    }

    #[test]
    fn a_corrupt_file_loads_as_defaults() {
        let path = state_path("corrupt");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_from(&path), Persisted::default());
    }

    #[test]
    fn an_unknown_appearance_value_reads_as_unset_and_keeps_the_rest() {
        let path = state_path("unknown-appearance");
        std::fs::write(
            &path,
            r#"{"theme":"neon","accent":"purple","applications":[{"app_id":"ABCDEFGH12","api_key":"k"}]}"#,
        )
        .unwrap();
        let state = load_from(&path);
        assert_eq!(state.theme, None);
        assert_eq!(state.accent, Some(Accent::Purple));
        assert_eq!(state.applications.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn the_state_file_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = state_path("permissions");
        save_to(&path, &Persisted::default());
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn saved_state_round_trips() {
        let path = state_path("round-trip");
        let state = Persisted {
            applications: vec![SavedApplication {
                app_id: "ABCD1234".into(),
                api_key: "search-key".into(),
                label: Some("Production".into()),
                last_agent_id: Some("alg_agt_1".into()),
                last_agent_name: Some("Shopping".into()),
                colour: Some("teal".into()),
            }],
            active_app: Some("ABCD1234".into()),
            agent_id: None,
            app_id: None,
            api_key: None,
            theme: Some(ThemePref::Dark),
            accent: Some(Accent::Purple),
            width: Some(ChatWidth::Wide),
            tint_window: Some(false),
            open_at_launch: Some(true),
            notify_replies: Some(false),
            hidden: vec!["alg_cnv_old".into()],
            keybinds: BTreeMap::from([("new-session".into(), None)]),
        };
        save_to(&path, &state);
        assert_eq!(load_from(&path), state);
    }

    #[test]
    fn a_single_application_file_from_before_migrates_into_the_list() {
        let path = state_path("legacy");
        std::fs::write(
            &path,
            r#"{"agent_id":"alg_agt_1","app_id":"ABCD1234","api_key":"search-key","theme":"dark"}"#,
        )
        .unwrap();
        let state = load_from(&path);
        assert_eq!(state.applications.len(), 1);
        assert_eq!(state.applications[0].app_id, "ABCD1234");
        assert_eq!(state.applications[0].api_key, "search-key");
        assert_eq!(
            state.applications[0].last_agent_id.as_deref(),
            Some("alg_agt_1")
        );
        assert_eq!(state.active_app.as_deref(), Some("ABCD1234"));
        assert_eq!(state.theme, Some(ThemePref::Dark), "the rest is untouched");
        assert!(state.app_id.is_none() && state.api_key.is_none() && state.agent_id.is_none());

        save_to(&path, &state);
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            !written.contains("\"agent_id\""),
            "the old fields are not written again"
        );
        assert_eq!(load_from(&path), state);
    }

    #[test]
    fn a_file_with_applications_ignores_stale_single_application_fields() {
        let path = state_path("both");
        std::fs::write(
            &path,
            r#"{"applications":[{"app_id":"NEW","api_key":"k"}],"app_id":"OLD","api_key":"old"}"#,
        )
        .unwrap();
        let state = load_from(&path);
        assert_eq!(state.applications.len(), 1);
        assert_eq!(state.applications[0].app_id, "NEW");
        assert!(state.app_id.is_none());
    }

    #[test]
    fn appearance_settings_are_written_as_lowercase_names() {
        let json = serde_json::to_value(Persisted {
            theme: Some(ThemePref::Dark),
            accent: Some(Accent::Graphite),
            width: Some(ChatWidth::Wide),
            ..Persisted::default()
        })
        .unwrap();
        assert_eq!(json["theme"], "dark");
        assert_eq!(json["accent"], "graphite");
        assert_eq!(json["width"], "wide");
    }
}
