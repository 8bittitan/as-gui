//! Where the app keeps its files. Debug builds stay inside the project (`state/`, `logs/`), as
//! `cargo run` always has; release builds, shipped as an app bundle, use the macOS homes.

use std::path::{Path, PathBuf};

pub const BUNDLE_ID: &str = "com.8bittitan.as-gui";

fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn state_path() -> PathBuf {
    state_path_in(home().as_deref(), cfg!(debug_assertions))
}

pub fn log_dir() -> PathBuf {
    log_dir_in(home().as_deref(), cfg!(debug_assertions))
}

fn state_path_in(home: Option<&Path>, dev: bool) -> PathBuf {
    match home {
        Some(home) if !dev => home
            .join("Library/Application Support")
            .join(BUNDLE_ID)
            .join("state.json"),
        _ => project_root().join("state/as-gui-state.json"),
    }
}

fn log_dir_in(home: Option<&Path>, dev: bool) -> PathBuf {
    match home {
        Some(home) if !dev => home.join("Library/Logs").join(BUNDLE_ID),
        _ => project_root().join("logs"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_builds_use_the_macos_application_support_and_logs_folders() {
        let home = Path::new("/Users/me");
        assert_eq!(
            state_path_in(Some(home), false),
            Path::new("/Users/me/Library/Application Support/com.8bittitan.as-gui/state.json")
        );
        assert_eq!(
            log_dir_in(Some(home), false),
            Path::new("/Users/me/Library/Logs/com.8bittitan.as-gui")
        );
    }

    #[test]
    fn debug_builds_stay_in_the_project() {
        let home = Path::new("/Users/me");
        assert!(state_path_in(Some(home), true).ends_with("state/as-gui-state.json"));
        assert!(log_dir_in(Some(home), true).ends_with("logs"));
        assert!(state_path_in(Some(home), true).starts_with(project_root()));
    }

    #[test]
    fn without_a_home_even_release_falls_back_to_the_project() {
        assert!(state_path_in(None, false).starts_with(project_root()));
        assert!(log_dir_in(None, false).starts_with(project_root()));
    }
}
