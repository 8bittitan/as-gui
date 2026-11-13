//! Instrumented frame benchmarks, in the style of Rune's `pinch_bench` example. Built only
//! with the `bench` feature (which turns on Rune's `frame-profile` phase clocks and pulls in
//! `procwatch`'s counting allocator), and driven by `examples/frame_bench.rs` via `just bench`.
//!
//! Everything runs headless and deterministic: scenarios never stream through the mock agent
//! (it sleeps on a real thread), they feed turn events through the `begin_turn` sender the way
//! `src/tests/mod.rs` feeds its loads. The glow compositor (`glow::Glow`) is not in the draw
//! path, so its shader cost is not measured: these numbers cover the runtime's update, view
//! and layout, and the offscreen draw's encode and submit.

use std::cell::RefCell;
use std::sync::Arc;

use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer};
use rune::{Env, Key, KeyEvent, Mods, Runtime};

use crate::agent::{
    AgentClient, AgentDirectory, AgentInfo, AgentStatus, ChatMessage, MockAgent, MockDirectory,
    StreamEvent,
};
use crate::chat::{self, Application, Chat, Session};
use crate::loading::Load;

const SIZE: (f32, f32) = (1100.0, 720.0);
const SCALE: f32 = 2.0;
const SETTLE_FRAMES: usize = 240;

/// The per-frame means `sample` records, in this order, one `METRIC` line each.
pub const METRICS: &[&str] = &[
    "commits",
    "cpu_ms",
    "frame_ms",
    "encode_ms",
    "view_ms",
    "layout_ms",
    "allocations",
    "live_only_events",
];

/// One shared headless GPU for a benchmark run: every scenario samples against it, so the
/// device and pipeline setup stay out of the numbers.
pub struct Bench {
    gpu: Gpu,
    renderer: Renderer,
    out: Offscreen,
}

impl Default for Bench {
    fn default() -> Self {
        Self::new()
    }
}

impl Bench {
    pub fn new() -> Bench {
        let gpu = Gpu::headless();
        let renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
        let out = Offscreen::new(
            &gpu.device,
            (SIZE.0 * SCALE) as u32,
            (SIZE.1 * SCALE) as u32,
        );
        Bench { gpu, renderer, out }
    }

    fn draw(&mut self, rt: &mut Runtime<Chat>) {
        self.renderer
            .upload_images(&self.gpu.device, &self.gpu.queue, &mut rt.images);
        self.renderer.prepare(
            &self.gpu.device,
            &self.gpu.queue,
            &mut rt.text.atlas,
            &rt.instances,
            (self.out.width, self.out.height),
        );
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.renderer
            .draw(&mut encoder, &self.out.view, rt.clear_color());
        self.renderer.submit(&self.gpu.queue, encoder);
    }

    fn wait(&self) {
        self.gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .ok();
    }
}

type Build = Box<dyn Fn() -> Runtime<Chat>>;
type Step = Box<dyn Fn(&mut Runtime<Chat>, usize)>;

pub struct Scenario {
    name: String,
    build: Build,
    step: Step,
}

impl Scenario {
    pub(crate) fn new(
        name: &str,
        build: impl Fn() -> Runtime<Chat> + 'static,
        step: impl Fn(&mut Runtime<Chat>, usize) + 'static,
    ) -> Scenario {
        Scenario {
            name: name.into(),
            build: Box::new(build),
            step: Box::new(step),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Warm-up pass first (dropped, like `pinch_bench`'s), then `samples` passes; each pass
    /// runs a fresh runtime for `frames` frames and returns the per-frame mean of every
    /// metric, in [`METRICS`] order.
    pub fn sample(&self, bench: &mut Bench, samples: usize, frames: usize) -> Vec<Vec<f64>> {
        let mut rows = Vec::new();
        for pass in 0..=samples {
            let mut rt = (self.build)();
            for _ in 0..SETTLE_FRAMES {
                if !rt.frame(1.0 / 60.0) {
                    break;
                }
            }
            bench.draw(&mut rt);
            bench.wait();
            rune::take_frame_phases();
            let (commits0, live0) = (rt.stats.commits, rt.live_only_events());
            let (mut cpu, mut frame, mut encode, mut allocations) = (0.0, 0.0, 0.0, 0.0);
            for i in 0..frames {
                let (alloc0, cpu0) = (
                    procwatch::alloc::thread_allocations(),
                    rune::thread_cpu_ms(),
                );
                (self.step)(&mut rt, i);
                rt.frame(1.0 / 60.0);
                let cpu1 = rune::thread_cpu_ms();
                bench.draw(&mut rt);
                let cpu2 = rune::thread_cpu_ms();
                allocations += (procwatch::alloc::thread_allocations() - alloc0) as f64;
                bench.wait();
                frame += cpu1 - cpu0;
                encode += cpu2 - cpu1;
                cpu += cpu2 - cpu0;
            }
            let phases = rune::take_frame_phases();
            let p = rune::PHASES;
            let n = frames as f64;
            let row = vec![
                (rt.stats.commits - commits0) as f64 / n,
                cpu / n,
                frame / n,
                encode / n,
                phases[p] / n,
                phases[p + 2] / n,
                allocations / n,
                (rt.live_only_events() - live0) as f64 / n,
            ];
            if pass > 0 {
                rows.push(row);
            }
        }
        rows
    }
}

/// The middle value once sorted; the two middle values averaged when the count is even.
pub fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    match values.len() {
        0 => 0.0,
        n if n % 2 == 0 => (values[n / 2 - 1] + values[n / 2]) / 2.0,
        n => values[n / 2],
    }
}

/// The 95th percentile once sorted, nearest rank.
pub fn p95(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * 0.95).round() as usize]
}

/// All five scenarios, in the order the example reports them.
pub fn scenarios() -> Vec<Scenario> {
    vec![
        idle_scenario(),
        stream_scenario(),
        tool_card_scenario(),
        scroll_scenario(),
        command_bar_scenario(),
    ]
}

pub fn scenario(name: &str) -> Option<Scenario> {
    scenarios().into_iter().find(|s| s.name() == name)
}

/// A settled conversation screen with nothing happening per frame: the floor every other
/// scenario stands above.
fn idle_scenario() -> Scenario {
    Scenario::new("idle", || runtime(|s| fill_conversation(s, 6)), |_, _| {})
}

/// One text delta per frame through the `begin_turn` sender, so the transcript re-renders
/// markdown and the composer glows every frame, over a conversation that makes the reply wrap.
fn stream_scenario() -> Scenario {
    let sender = RefCell::new(None);
    Scenario::new(
        "stream",
        || runtime(|s| fill_conversation(s, 6)),
        move |rt, i| {
            let mut sender = sender.borrow_mut();
            if i == 0 {
                *sender = Some(rt.state.begin_turn("Which shoes fit wide feet?"));
            } else if let Some(sender) = sender.as_ref() {
                let _ = sender.send(StreamEvent::TextDelta("shoe ".into()));
            }
        },
    )
}

/// The search tool's call and its hits go up front through the sender; after that the card
/// header toggles open and closed every 30 frames, the way `scripts/chat.json` taps it.
fn tool_card_scenario() -> Scenario {
    let sender = RefCell::new(None);
    Scenario::new(
        "tool-card",
        || runtime(|s| fill_conversation(s, 6)),
        move |rt, i| {
            let mut sender = sender.borrow_mut();
            if i == 0 {
                *sender = Some(rt.state.begin_turn("Which shoes fit wide feet?"));
                let sender = sender.as_ref().unwrap();
                let _ = sender.send(StreamEvent::ToolStarted {
                    call_id: "bench_search".into(),
                    name: "search_tool".into(),
                });
                let _ = sender.send(StreamEvent::ToolFinished {
                    call_id: "bench_search".into(),
                    name: "search_tool".into(),
                    output: Some(hits_json()),
                });
                let _ = sender.send(StreamEvent::Finished);
            } else if i % 30 == 0 {
                tap(rt, "tool-search_tool");
            }
        },
    )
}

/// A wheel step per frame through a 200-message transcript: up for the first half, back down
/// for the second, so the offset never runs out of range.
fn scroll_scenario() -> Scenario {
    Scenario::new(
        "scroll",
        || runtime(|s| fill_conversation(s, 200)),
        |rt, i| {
            let dy = if i < 60 { 40.0 } else { -40.0 };
            if let Some((x, y)) = rt.find("messages") {
                rt.wheel(x, y, dy);
            }
        },
    )
}

/// ⌘K opens the bar, a query types one character per frame over a large entry set, Escape
/// closes; the cycle repeats for the whole run so every measured frame carries work.
fn command_bar_scenario() -> Scenario {
    const QUERY: [char; 10] = ['s', 'h', 'o', 'e', 's', ' ', 'w', 'i', 'd', 'e'];
    Scenario::new(
        "command-bar",
        || runtime(many_entries),
        |rt, i| {
            let beat = i % 15;
            let key = if beat == 0 {
                Key::Command('k')
            } else if (1..=8).contains(&beat) {
                Key::Text(QUERY[(i - 1) % QUERY.len()].to_string())
            } else if beat == 9 {
                Key::Escape
            } else {
                return;
            };
            rt.key(KeyEvent {
                key,
                mods: Mods::default(),
            });
        },
    )
}

fn tap(rt: &mut Runtime<Chat>, key: &str) {
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("no element keyed {key}"));
    rt.pointer_move(x, y);
    rt.frame(0.0);
    rt.pointer_down(x, y);
    rt.frame(0.0);
    rt.pointer_up(x, y);
    rt.frame(0.0);
}

/// The test harness's runtime, sized like the `screenshots` recipe: an application registered,
/// the mock forced so `poll` never touches the network, and the conversation screen up.
fn runtime(fixtures: impl FnOnce(&mut Chat)) -> Runtime<Chat> {
    let env = Env {
        system_dark: true,
        scale: SCALE,
    };
    let client: Arc<dyn AgentClient> = Arc::new(MockAgent {
        response: "Hi!",
        chunk_chars: 2,
        tools: Vec::new(),
    });
    let directory: Arc<dyn AgentDirectory> = Arc::new(MockDirectory {
        agents: Vec::new(),
        sessions: Vec::new(),
        conversation: Vec::new(),
    });
    let mut rt = Runtime::new(Chat::new(client, directory), crate::shell::view, SIZE, env);
    rt.poll = Some(chat::poll);
    rt.on_key = Some(crate::shell::on_key);
    rt.update(|s, _| {
        s.applications = vec![Application::new("bench-app", "bench-key")];
        s.active = Some(0);
        s.mock_forced = true;
        s.selected_agent = Some(published("bench-agent", "Bench agent"));
        fixtures(s);
    });
    rt.frame(0.0);
    rt
}

fn published(id: &str, name: &str) -> AgentInfo {
    AgentInfo {
        agent_id: id.into(),
        name: name.into(),
        description: None,
        status: AgentStatus::Published,
        model: None,
    }
}

fn fill_conversation(s: &mut Chat, turns: usize) {
    for i in 0..turns {
        s.sessions[0]
            .messages
            .push(ChatMessage::user(format!("u{i}"), format!("question {i}")));
        let mut a = ChatMessage::assistant(format!("a{i}"));
        a.text = format!("answer {i} — a fairly long line of text to fill the transcript");
        a.streaming = false;
        s.sessions[0].messages.push(a);
    }
}

fn many_entries(s: &mut Chat) {
    s.agents = (0..50)
        .map(|i| published(&format!("agent{i}"), &format!("Agent number {i}")))
        .collect();
    s.sessions = (0..50)
        .map(|i| Session {
            id: None,
            hidden: false,
            title: format!("conversation {i} about shoes and insoles"),
            messages: Vec::new(),
            agent_message_id: None,
            history: Load::Idle,
        })
        .collect();
}

fn hits_json() -> String {
    serde_json::json!({ "hits": [
        { "title": "Fit guide: wide feet", "path": "/guides/wide-fit",
          "content": "The current best **shoe and insole** pairings for wide feet." },
        { "title": "Shoe tier list", "path": "/guides/shoes",
          "content": "Where every shoe lands **after the latest release**." },
        { "title": "Insole pairings", "path": "/guides/insoles",
          "content": "Pairings that cover **arch support** needs." },
    ] })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_an_odd_count_is_the_middle_value() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
    }

    #[test]
    fn the_median_of_an_even_count_averages_the_two_middle_values() {
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), 2.5);
    }

    #[test]
    fn the_median_of_two_samples_is_their_mean() {
        assert_eq!(median(&mut [10.0, 20.0]), 15.0);
    }

    #[test]
    fn p95_picks_the_nearest_rank_of_the_sorted_samples() {
        assert_eq!(p95(&mut [10.0, 20.0]), 20.0);
        // Twenty samples: Rune's formula rounds (n-1) * 0.95 to 18, the 19th of 20.
        let mut twenty: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(p95(&mut twenty), 19.0);
    }

    fn one_frame(name: &str) -> Runtime<Chat> {
        let scenario = scenario(name).unwrap_or_else(|| panic!("no scenario named {name}"));
        let mut rt = (scenario.build)();
        (scenario.step)(&mut rt, 0);
        rt.frame(1.0 / 60.0);
        rt
    }

    #[test]
    fn the_idle_scenario_sits_on_a_settled_conversation() {
        let rt = one_frame("idle");
        assert!(rt.find("composer").is_some());
        assert!(rt.find("thread").is_some());
    }

    #[test]
    fn the_stream_scenario_streams_its_first_delta() {
        let rt = one_frame("stream");
        assert!(rt.find("composer").is_some());
        assert!(rt.state.pending, "the turn is still streaming");
    }

    #[test]
    fn the_tool_card_scenario_renders_the_tool_card() {
        let rt = one_frame("tool-card");
        assert!(
            rt.find("tool-search_tool").is_some(),
            "the card header is on screen"
        );
    }

    #[test]
    fn the_scroll_scenario_builds_two_hundred_messages() {
        let rt = one_frame("scroll");
        assert!(rt.find("composer").is_some());
        assert_eq!(rt.state.sessions[0].messages.len(), 400);
    }

    #[test]
    fn the_command_bar_scenario_opens_the_bar_on_frame_one() {
        let rt = one_frame("command-bar");
        assert!(rt.state.command_bar.open, "⌘K opened the bar");
    }
}
