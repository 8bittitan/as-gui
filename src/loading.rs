//! The loading state machine: one status per background load, and the rules that keep the UI
//! from flashing between states while data arrives.
//!
//! A load shows nothing for [`SHOW_AFTER`] seconds, so fast loads never flash a placeholder; once
//! a placeholder has appeared it stays for at least [`HOLD`] seconds, so it never blinks.

pub const SHOW_AFTER: f64 = 0.15;
pub const HOLD: f64 = 0.30;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Load {
    #[default]
    Idle,
    Loading {
        since: f64,
    },
    Ready {
        since: f64,
        done: f64,
    },
    Failed(String),
}

/// What a load asks the view to show right now.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Not started: the view shows what it has.
    Idle,
    /// Loading, but not for long enough to show a placeholder: show nothing.
    Blank,
    Skeleton,
    Done,
    Failed(String),
}

impl Load {
    pub fn started(now: f64) -> Load {
        Load::Loading { since: now }
    }

    /// This load, finished at `now`. A load that never started settles at once.
    pub fn finished(&self, now: f64) -> Load {
        match self {
            Load::Loading { since } | Load::Ready { since, .. } => Load::Ready {
                since: *since,
                done: now,
            },
            _ => Load::Ready {
                since: now,
                done: now,
            },
        }
    }

    pub fn phase(&self, now: f64) -> Phase {
        match self {
            Load::Idle => Phase::Idle,
            Load::Loading { since } if now - since < SHOW_AFTER => Phase::Blank,
            Load::Loading { .. } => Phase::Skeleton,
            Load::Ready { since, done } => {
                let shown = done - since > SHOW_AFTER;
                let held_until = done.max(since + SHOW_AFTER + HOLD);
                if shown && now < held_until {
                    Phase::Skeleton
                } else {
                    Phase::Done
                }
            }
            Load::Failed(e) => Phase::Failed(e.clone()),
        }
    }

    /// Still loading, or finished but its placeholder is still held on screen.
    pub fn is_pending(&self, now: f64) -> bool {
        matches!(self.phase(now), Phase::Blank | Phase::Skeleton)
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Load::Idle)
    }

    /// Finished, either way.
    #[cfg(test)]
    pub fn is_settled(&self) -> bool {
        matches!(self, Load::Ready { .. } | Load::Failed(_))
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Load::Failed(e) => Some(e),
            _ => None,
        }
    }
}

use std::time::Duration;

use rune::Tx;

use crate::chat::Chat;

/// A gentle 0.45..0.85 opacity breathing for skeleton blocks, from the frame clock.
pub fn pulse(now: f64) -> f32 {
    0.65 + 0.2 * (now * std::f64::consts::TAU / 1.4).sin() as f32
}

/// What the whole window shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    /// Working out where to open: a centred spinner, once it has been a moment.
    Splash {
        spinner: bool,
    },
    AgentsFailed(String),
    Picker,
    Chat,
}

/// What the sidebar's session list shows, apart from the chat section.
#[derive(Clone, Debug, PartialEq)]
pub enum SidebarView {
    Blank,
    Skeleton,
    Sessions,
    Failed(String),
}

/// What the chat section shows.
#[derive(Clone, Debug, PartialEq)]
pub enum PaneView {
    Blank,
    Skeleton,
    /// A new session: the agent's starter prompts.
    Empty,
    History,
    Failed(String),
}

type Views = (Screen, SidebarView, PaneView);

impl Chat {
    pub fn screen(&self, now: f64) -> Screen {
        if let Some(error) = self.agents_load.error() {
            return Screen::AgentsFailed(error.to_string());
        }
        match self.launch.phase(now) {
            Phase::Blank => return Screen::Splash { spinner: false },
            Phase::Skeleton => return Screen::Splash { spinner: true },
            _ => {}
        }
        let about_to_load =
            self.launch.is_idle() && self.agents_load.is_idle() && self.loads_from_directory();
        if self.selected_agent.is_none() {
            if about_to_load {
                Screen::Splash { spinner: false }
            } else {
                Screen::Picker
            }
        } else {
            Screen::Chat
        }
    }

    pub fn sidebar_view(&self, now: f64) -> SidebarView {
        match self.sessions_load.phase(now) {
            Phase::Blank => SidebarView::Blank,
            Phase::Skeleton => SidebarView::Skeleton,
            Phase::Failed(e) => SidebarView::Failed(e),
            _ => SidebarView::Sessions,
        }
    }

    pub fn pane_view(&self, now: f64) -> PaneView {
        match self.sessions_load.phase(now) {
            Phase::Blank => return PaneView::Blank,
            Phase::Skeleton => return PaneView::Skeleton,
            _ => {}
        }
        let session = &self.sessions[self.current];
        let by_messages = || {
            if session.messages.is_empty() && session.id.is_none() {
                PaneView::Empty
            } else {
                PaneView::History
            }
        };
        if session.id.is_none() {
            return by_messages();
        }
        match session.history.phase(now) {
            Phase::Idle if self.loads_from_directory() => PaneView::Blank,
            Phase::Idle => by_messages(),
            Phase::Blank => PaneView::Blank,
            Phase::Skeleton => PaneView::Skeleton,
            Phase::Failed(e) => PaneView::Failed(e),
            Phase::Done => PaneView::History,
        }
    }

    /// Whether any load is still going, or holding its placeholder on screen.
    fn loading_visible_or_due(&self, now: f64) -> bool {
        self.launch.is_pending(now)
            || self.agents_load.is_pending(now)
            || self.sessions_load.is_pending(now)
            || self
                .sessions
                .get(self.current)
                .is_some_and(|s| s.history.is_pending(now))
    }

    fn views_at(&self, now: f64) -> Views {
        (
            self.screen(now),
            self.sidebar_view(now),
            self.pane_view(now),
        )
    }

    /// Called every frame: true when time alone changed what the views show (a placeholder
    /// came due or was released), and keeps frames coming while one could. State changes
    /// already rebuild the view by themselves.
    pub fn track_views(&mut self, now: f64, tx: &mut Tx) -> bool {
        let (before, after) = (self.views_at(self.last_poll), self.views_at(now));
        self.last_poll = now;
        let pulsing = after.1 == SidebarView::Skeleton
            || after.2 == PaneView::Skeleton
            || matches!(after.0, Screen::Splash { spinner: true });
        if self.loading_visible_or_due(now) {
            tx.wake_after(Duration::from_millis(if pulsing { 33 } else { 40 }));
        }
        before != after || pulsing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_load_stays_blank_for_the_first_150ms() {
        let load = Load::Loading { since: 1.0 };
        assert_eq!(load.phase(1.0), Phase::Blank);
        assert_eq!(load.phase(1.149), Phase::Blank);
        assert_eq!(load.phase(1.16), Phase::Skeleton);
        assert_eq!(load.phase(9.0), Phase::Skeleton);
    }

    #[test]
    fn a_fast_load_never_shows_a_skeleton() {
        let load = Load::Loading { since: 0.0 }.finished(0.1);
        assert_eq!(load.phase(0.1), Phase::Done);
        assert_eq!(load.phase(0.2), Phase::Done);
    }

    #[test]
    fn a_skeleton_that_appeared_stays_for_at_least_300ms() {
        let load = Load::Loading { since: 0.0 }.finished(0.2);
        assert_eq!(load.phase(0.2), Phase::Skeleton, "finished, but held");
        assert_eq!(load.phase(0.449), Phase::Skeleton);
        assert_eq!(load.phase(0.45), Phase::Done);
    }

    #[test]
    fn a_slow_load_ends_the_moment_it_finishes() {
        let load = Load::Loading { since: 0.0 }.finished(2.0);
        assert_eq!(load.phase(2.0), Phase::Done);
    }

    #[test]
    fn a_failed_load_shows_its_error_at_once() {
        let load = Load::Failed("HTTP 403".into());
        assert_eq!(load.phase(0.0), Phase::Failed("HTTP 403".into()));
    }

    #[test]
    fn an_idle_load_is_idle_and_not_pending() {
        assert_eq!(Load::Idle.phase(5.0), Phase::Idle);
        assert!(!Load::Idle.is_pending(5.0));
        assert!(Load::Loading { since: 0.0 }.is_pending(0.0));
        assert!(Load::Loading { since: 0.0 }.finished(0.2).is_pending(0.3));
        assert!(!Load::Loading { since: 0.0 }.finished(0.2).is_pending(0.5));
    }

    #[test]
    fn finishing_a_load_that_was_not_loading_settles_it_at_once() {
        let load = Load::Idle.finished(3.0);
        assert_eq!(load.phase(3.0), Phase::Done);
        assert!(load.is_settled());
    }
}
