//! The frame benchmark's cases, one file per scenario. Each file builds its `Scenario` from
//! the engine in `src/bench.rs`; `all()` lists them in the order the runner reports them.

mod command_bar;
mod idle;
mod scroll;
mod stream;
mod tool_card;

#[cfg(test)]
use rune::Runtime;

use crate::bench::Scenario;

#[cfg(test)]
use crate::chat::Chat;

pub(super) fn all() -> Vec<Scenario> {
    vec![
        idle::scenario(),
        stream::scenario(),
        tool_card::scenario(),
        scroll::scenario(),
        command_bar::scenario(),
    ]
}

/// Builds a case's runtime, runs its frame-0 step and one frame, for the case tests.
#[cfg(test)]
pub(super) fn one_frame(name: &str) -> Runtime<Chat> {
    let scenario =
        crate::bench::scenario(name).unwrap_or_else(|| panic!("no scenario named {name}"));
    let mut rt = (scenario.build)();
    (scenario.step)(&mut rt, 0);
    rt.frame(1.0 / 60.0);
    rt
}
