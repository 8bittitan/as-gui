//! `tracing` setup: the same events go to stderr and to `logs/as-gui.log.YYYY-MM-DD`, at the
//! level `LOG_LEVEL` chooses (default `warn`). The filter applies to this crate only, so
//! `LOG_LEVEL=debug` does not also enable `ureq`'s or `rune`'s debug output.

use std::fmt;
use std::sync::Mutex;

use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::{Layer as _, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_LEVEL: Level = Level::Warn;

enum Level {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    /// The level `LOG_LEVEL` asks for, plus a notice when the value was invalid or unset.
    fn parse(raw: Option<&str>) -> (LevelFilter, Option<String>) {
        let Some(raw) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
            return (DEFAULT_LEVEL.filter(), None);
        };
        match raw.to_ascii_lowercase().as_str() {
            "off" => (Level::Off.filter(), None),
            "error" => (Level::Error.filter(), None),
            "warn" | "warning" => (Level::Warn.filter(), None),
            "info" => (Level::Info.filter(), None),
            "debug" => (Level::Debug.filter(), None),
            "trace" => (Level::Trace.filter(), None),
            other => (
                DEFAULT_LEVEL.filter(),
                Some(format!(
                    "unknown LOG_LEVEL {other:?}, using {DEFAULT_LEVEL}"
                )),
            ),
        }
    }

    fn filter(self) -> LevelFilter {
        match self {
            Level::Off => LevelFilter::OFF,
            Level::Error => LevelFilter::ERROR,
            Level::Warn => LevelFilter::WARN,
            Level::Info => LevelFilter::INFO,
            Level::Debug => LevelFilter::DEBUG,
            Level::Trace => LevelFilter::TRACE,
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::Off => "off",
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        })
    }
}

pub fn init() {
    let (filter, notice) = Level::parse(std::env::var("LOG_LEVEL").ok().as_deref());
    let filter = Targets::new().with_target("as_gui", filter);

    let dir = crate::paths::log_dir();
    if let Err(error) = crate::store::create_private_dir(&dir) {
        eprintln!("[as-gui] could not create {}: {error}", dir.display());
    }
    // A plain Mutex writer rather than `non_blocking`, so each line is on disk immediately
    // and nothing is lost if the app exits without `run()` returning.
    let file = Mutex::new(tracing_appender::rolling::daily(dir, "as-gui.log"));

    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false)
                .with_filter(filter.clone()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file)
                .with_target(false)
                .with_ansi(false)
                .with_filter(filter),
        )
        .try_init();

    if let Some(notice) = notice {
        eprintln!("[as-gui] {notice}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_comes_from_log_level_case_insensitively() {
        for (raw, expected) in [
            ("off", LevelFilter::OFF),
            ("ERROR", LevelFilter::ERROR),
            ("warn", LevelFilter::WARN),
            ("Warning", LevelFilter::WARN),
            ("info", LevelFilter::INFO),
            ("debug", LevelFilter::DEBUG),
            ("trace", LevelFilter::TRACE),
        ] {
            let (filter, notice) = Level::parse(Some(raw));
            assert_eq!(filter, expected, "LOG_LEVEL={raw}");
            assert!(notice.is_none());
        }
    }

    #[test]
    fn an_unset_or_blank_log_level_defaults_to_warn() {
        for raw in [None, Some(""), Some("   ")] {
            let (filter, notice) = Level::parse(raw);
            assert_eq!(filter, LevelFilter::WARN);
            assert!(notice.is_none());
        }
    }

    #[test]
    fn an_unknown_log_level_falls_back_to_warn_with_a_notice() {
        let (filter, notice) = Level::parse(Some("loud"));
        assert_eq!(filter, LevelFilter::WARN);
        assert_eq!(
            notice.as_deref(),
            Some("unknown LOG_LEVEL \"loud\", using warn")
        );
    }
}
