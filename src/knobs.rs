//! The live tuning knobs (⌘⇧K, dev builds): the design constants as `rune-knobs` knobs. A
//! knob's default is the shipped value, so views read `Chat::results_shown()` and friends
//! and a build without the `knobs` feature runs on exactly the constants below.

use rune_knobs::{Group, Knob};

use crate::chat::{Chat, ChatWidth};

const FRACTION_NOTE: &str = "How much of the pane the thread and composer take";

pub const COMFORTABLE_FRACTION: Knob = Knob::new("Comfortable fraction", 0.3, 1.0, 0.6)
    .step(0.01)
    .about(FRACTION_NOTE);
pub const COMFORTABLE_MIN: Knob = Knob::new("Comfortable min", 200.0, 900.0, 500.0).step(10.0);
pub const COMFORTABLE_MAX: Knob = Knob::new("Comfortable max", 400.0, 1600.0, 860.0).step(10.0);
pub const WIDE_FRACTION: Knob = Knob::new("Wide fraction", 0.3, 1.0, 0.8)
    .step(0.01)
    .about(FRACTION_NOTE);
pub const WIDE_MIN: Knob = Knob::new("Wide min", 200.0, 900.0, 620.0).step(10.0);
pub const WIDE_MAX: Knob = Knob::new("Wide max", 400.0, 1800.0, 1160.0).step(10.0);

pub const RESULTS_SHOWN: Knob = Knob::new("Results shown", 1.0, 10.0, 3.0)
    .step(1.0)
    .about("How many search hits a tool call lists before Show all");

pub const FROST_BLUR: Knob = Knob::new("Frost blur", 0.0, 60.0, 24.0)
    .step(1.0)
    .about("How far the transcript blurs behind the composer");
pub const FROST_ALPHA: Knob = Knob::new("Frost opacity", 0.0, 1.0, 0.72)
    .step(0.01)
    .about("How much of the composer's fill covers the blurred transcript");

pub const REVEAL_SECONDS: Knob = Knob::new("Word fade", 0.0, 1.0, 0.18)
    .step(0.01)
    .about("Seconds a streaming word takes to fade in");

pub const GROUPS: &[Group] = &[
    Group::new(
        "Comfortable width",
        "thread",
        &[COMFORTABLE_FRACTION, COMFORTABLE_MIN, COMFORTABLE_MAX],
    )
    .under("Conversation"),
    Group::new("Wide width", "thread", &[WIDE_FRACTION, WIDE_MIN, WIDE_MAX]).under("Conversation"),
    Group::new("Search results", "messages", &[RESULTS_SHOWN]).under("Conversation"),
    Group::new("Streaming", "messages", &[REVEAL_SECONDS]).under("Conversation"),
    Group::new("Composer frost", "composer", &[FROST_BLUR, FROST_ALPHA]),
];

impl Chat {
    /// The thread and composer's width as `(fraction of the pane, min, max)` points,
    /// by the chat width setting.
    pub fn width_spec(&self) -> (f32, f32, f32) {
        let k = &self.knobs;
        match self.appearance.width {
            ChatWidth::Comfortable => (
                k.get(&COMFORTABLE_FRACTION),
                k.get(&COMFORTABLE_MIN),
                k.get(&COMFORTABLE_MAX),
            ),
            ChatWidth::Wide => (k.get(&WIDE_FRACTION), k.get(&WIDE_MIN), k.get(&WIDE_MAX)),
        }
    }

    pub fn results_shown(&self) -> usize {
        self.knobs.get(&RESULTS_SHOWN).round().max(1.0) as usize
    }

    pub fn frost_blur(&self) -> f32 {
        self.knobs.get(&FROST_BLUR)
    }

    pub fn frost_alpha(&self) -> f32 {
        self.knobs.get(&FROST_ALPHA)
    }

    pub fn reveal_seconds(&self) -> f32 {
        self.knobs.get(&REVEAL_SECONDS)
    }
}
