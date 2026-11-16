//! The per-turn waterfall: where an answer's time went. `bars` and `summary` are the layout
//! maths; the view builds on them.

use rune::{El, Tx, col, el, row, text};
use rune_kit::ButtonVariant;

use crate::agent::{SearchHit, TraceSpan, TurnTrace};
use crate::chat::Chat;
use crate::conversation::json_card;
use crate::satellite::{self, Fit, RADIUS_CONTROL};

const LABEL_WIDTH: f32 = 150.0;
const TRACK_WIDTH: f32 = 260.0;
const INDENT: f32 = 12.0;
const BAR_HEIGHT: f32 = 10.0;

pub const MARKER_WIDTH: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Request,
    Tool(usize),
    Reply,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub kind: RowKind,
    pub label: String,
    pub depth: usize,
    pub x: f32,
    pub width: f32,
    pub marker: bool,
    /// A tool call that errored.
    pub failed: bool,
    /// Seconds the bar covers: for a call run on this Mac, the time it ran.
    pub seconds: f64,
    /// For a call run on this Mac, where its wait for the user is drawn: x and width.
    pub wait: Option<(f32, f32)>,
    /// How long that wait lasted.
    pub waited: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub total: f64,
    pub first_token: Option<f64>,
    pub tools: f64,
    /// Time spent waiting for the user's approval.
    pub waiting: f64,
    pub reply: Option<f64>,
}

fn span_end(span: &TraceSpan, now: f64) -> f64 {
    span.end.unwrap_or(now).max(span.start)
}

fn turn_end(trace: &TurnTrace, now: f64) -> f64 {
    let latest = trace
        .spans
        .iter()
        .map(|s| span_end(s, now))
        .chain(trace.last_token)
        .fold(trace.finished.unwrap_or(now), f64::max);
    latest.max(trace.sent)
}

/// When a client call stopped waiting and began running, else when it ended.
fn run_from(span: &TraceSpan, now: f64) -> Option<f64> {
    let timing = span.client?;
    Some(
        timing
            .run_start
            .unwrap_or_else(|| span_end(span, now))
            .max(timing.pending_at),
    )
}

/// The time between `from` and `to` that went on waiting for the user or running a client
/// call, which isn't the model's: a reply continued after a tool holds that gap.
fn paused_between(trace: &TurnTrace, from: f64, to: f64, now: f64) -> f64 {
    let mut intervals: Vec<(f64, f64)> = trace
        .spans
        .iter()
        .filter_map(|span| {
            let timing = span.client?;
            Some((timing.pending_at.max(from), span_end(span, now).min(to)))
        })
        .filter(|(start, end)| end > start)
        .collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut total = 0.0;
    let mut covered_to = f64::NEG_INFINITY;
    for (start, end) in intervals {
        let begin = start.max(covered_to);
        if end > begin {
            total += end - begin;
        }
        covered_to = covered_to.max(end);
    }
    total
}

fn reply_end(trace: &TurnTrace, now: f64) -> Option<f64> {
    let last = trace.last_token?;
    Some(if trace.finished.is_some() {
        last
    } else {
        now.max(last)
    })
}

pub fn bars(trace: &TurnTrace, width: f32, now: f64) -> Vec<Bar> {
    let total = (turn_end(trace, now) - trace.sent).max(0.001);
    let place = |start: f64, end: f64| {
        let x = ((start - trace.sent) / total) as f32 * width;
        let w = ((end - start) / total) as f32 * width;
        (x, w.max(MARKER_WIDTH), end <= start)
    };
    let mut rows = Vec::new();

    let first_activity = trace
        .spans
        .iter()
        .map(|s| s.start)
        .chain(trace.first_token)
        .fold(f64::INFINITY, f64::min);
    let request_end = if first_activity.is_finite() {
        first_activity
    } else {
        turn_end(trace, now)
    };
    let (x, w, marker) = place(trace.sent, request_end);
    rows.push(Bar {
        kind: RowKind::Request,
        label: "request".into(),
        depth: 0,
        x,
        width: w,
        marker,
        failed: false,
        seconds: request_end - trace.sent,
        wait: None,
        waited: 0.0,
    });

    for (i, span) in trace.spans.iter().enumerate() {
        let end = span_end(span, now);
        let depth = trace.spans[..i]
            .iter()
            .filter(|earlier| span_end(earlier, now) > span.start)
            .count();
        let ran_from = run_from(span, now).unwrap_or(span.start);
        let (x, w, marker) = place(ran_from, end);
        let wait = span.client.map(|timing| {
            let (wait_x, wait_w, _) = place(timing.pending_at, ran_from);
            (wait_x, wait_w - MARKER_WIDTH.min(wait_w))
        });
        rows.push(Bar {
            kind: RowKind::Tool(i),
            label: span.name.clone(),
            depth,
            x,
            width: w,
            marker,
            failed: span.error.is_some(),
            seconds: end - ran_from,
            wait,
            waited: span
                .client
                .map_or(0.0, |timing| ran_from - timing.pending_at),
        });
    }

    if let (Some(first), Some(end)) = (trace.first_token, reply_end(trace, now)) {
        let (x, w, marker) = place(first, end);
        rows.push(Bar {
            kind: RowKind::Reply,
            label: "reply".into(),
            depth: 0,
            x,
            width: w,
            marker,
            failed: false,
            seconds: end - first - paused_between(trace, first, end, now),
            wait: None,
            waited: 0.0,
        });
    }
    rows
}

pub fn summary(trace: &TurnTrace, now: f64) -> Summary {
    let mut intervals: Vec<(f64, f64)> = trace
        .spans
        .iter()
        .map(|s| (run_from(s, now).unwrap_or(s.start), span_end(s, now)))
        .collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut tools = 0.0;
    let mut covered_to = f64::NEG_INFINITY;
    for (start, end) in intervals {
        let from = start.max(covered_to);
        if end > from {
            tools += end - from;
        }
        covered_to = covered_to.max(end);
    }
    Summary {
        total: turn_end(trace, now) - trace.sent,
        first_token: trace.first_token.map(|t| t - trace.sent),
        tools,
        waiting: trace
            .spans
            .iter()
            .filter_map(|s| Some(run_from(s, now)? - s.client?.pending_at))
            .sum(),
        reply: trace
            .first_token
            .zip(reply_end(trace, now))
            .map(|(first, end)| end - first - paused_between(trace, first, end, now)),
    }
}

/// Tokens per second, estimated as four characters to a token between the first and last
/// text delta.
pub fn token_rate(trace: &TurnTrace) -> Option<f64> {
    let (first, last) = (trace.first_token?, trace.last_token?);
    let span = last - first - paused_between(trace, first, last, last);
    (span > 0.0).then(|| trace.chars as f64 / 4.0 / span)
}

fn seconds(secs: f64) -> String {
    format!("{secs:.1}s")
}

fn summary_line(summary: &Summary) -> String {
    let mut parts = vec![format!("{} total", seconds(summary.total))];
    if let Some(first) = summary.first_token {
        parts.push(format!("first token {}", seconds(first)));
    }
    parts.push(format!("tools {}", seconds(summary.tools)));
    if summary.waiting > 0.0 {
        parts.push(format!("waiting for you {}", seconds(summary.waiting)));
    }
    if let Some(reply) = summary.reply {
        parts.push(format!("reply {}", seconds(reply)));
    }
    parts.join(" · ")
}

/// The small "⏱ 4.2s" button under a settled reply that opens its timeline.
pub fn affordance(k: &rune_kit::Kit, message_id: &str, trace: &TurnTrace) -> El {
    let total = seconds(summary(trace, trace.finished.unwrap_or(trace.sent)).total);
    let id = message_id.to_string();
    satellite::button(total)
        .icon(&crate::satellite::icons::TIMER)
        .variant(ButtonVariant::GhostMuted)
        .expanded(trace.open)
        .help("Where this answer's time went")
        .on_press(move |s: &mut Chat| s.toggle_timeline(&id))
        .sized(k, satellite::Size::Small)
        .key(format!("timing-{message_id}"))
}

fn hit_count(span: &TraceSpan) -> Option<usize> {
    let json = span.output.as_deref().or(span.input.as_deref())?;
    SearchHit::from_json(json).map(|hits| hits.len())
}

fn stats(trace: &TurnTrace, bar: &Bar) -> String {
    let mut stats = seconds(bar.seconds);
    match bar.kind {
        RowKind::Tool(i) => {
            if bar.waited > 0.0 {
                stats.push_str(&format!(" · waited {}", seconds(bar.waited)));
            }
            if bar.failed {
                stats.push_str(" · failed");
            } else if let Some(n) = hit_count(&trace.spans[i]) {
                stats.push_str(&format!("  {n} hits"));
            }
        }
        RowKind::Reply => {
            if let Some(rate) = token_rate(trace) {
                stats.push_str(&format!(" · ≈{rate:.0} tok/s"));
            }
        }
        RowKind::Request => {}
    }
    stats
}

fn row_key(kind: RowKind, id: &str) -> String {
    match kind {
        RowKind::Request => format!("timeline-request-{id}"),
        RowKind::Tool(i) => format!("timeline-tool-{id}-{i}"),
        RowKind::Reply => format!("timeline-reply-{id}"),
    }
}

fn bar_row(k: &rune_kit::Kit, id: &str, trace: &TurnTrace, bar: &Bar) -> El {
    let fill = match bar.kind {
        RowKind::Request => k.c.icon_muted,
        RowKind::Tool(_) if bar.failed => k.c.danger.icon,
        RowKind::Tool(_) => k.c.bg_accent,
        RowKind::Reply => k.c.icon_muted,
    };
    let mut track = row()
        .w(TRACK_WIDTH)
        .h(BAR_HEIGHT + 6.0)
        .center_y()
        .radius(RADIUS_CONTROL)
        .fill(k.c.layer_02);
    let mut cursor = 0.0;
    if let Some((wait_x, wait_width)) = bar.wait.filter(|(_, width)| *width > 0.0) {
        track = track
            .child(el().w(wait_x.min(TRACK_WIDTH - MARKER_WIDTH)))
            .child(
                el().w(wait_width)
                    .h(BAR_HEIGHT)
                    .radius(2.0)
                    .fill(k.c.bg_accent.alpha(0.35)),
            );
        cursor = wait_x + wait_width;
    }
    let track = track
        .child(el().w((bar.x - cursor).clamp(0.0, TRACK_WIDTH - MARKER_WIDTH)))
        .child(
            el().w(bar.width)
                .h(if bar.marker {
                    BAR_HEIGHT + 6.0
                } else {
                    BAR_HEIGHT
                })
                .radius(if bar.marker { 1.0 } else { 2.0 })
                .fill(fill),
        );
    let mut r = row()
        .key(row_key(bar.kind, id))
        .gap(8.0)
        .center_y()
        .child(
            col().w(LABEL_WIDTH).child(
                text(&bar.label)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .truncate()
                    .ml(bar.depth as f32 * INDENT),
            ),
        )
        .child(track)
        .child(
            text(stats(trace, bar))
                .font(k.ty.small)
                .color(k.c.text_faint),
        );
    if let RowKind::Tool(i) = bar.kind {
        let message_id = id.to_string();
        let key = row_key(bar.kind, id);
        r = r.role("button").expanded(trace.spans[i].open).on_press_tx(
            move |s: &mut Chat, tx: &mut Tx| {
                s.toggle_span_open(&message_id, i);
                tx.keep_on_screen(key.clone());
            },
        );
    }
    r
}

fn span_json(k: &rune_kit::Kit, id: &str, index: usize, span: &TraceSpan) -> El {
    let mut payloads = col()
        .key(format!("timeline-json-{id}-{index}"))
        .gap(4.0)
        .ml(28.0);
    for (label, json) in [("Input", &span.input), ("Output", &span.output)] {
        if let Some(json) = json {
            payloads = payloads
                .child(text(label).font(k.ty.ui_compact).color(k.c.text_faint))
                .child(json_card(
                    k,
                    format!("timeline-{}-{id}-{index}", label.to_lowercase()),
                    json,
                ));
        }
    }
    if let Some(error) = &span.error {
        payloads = payloads
            .child(text("Error").font(k.ty.ui_compact).color(k.c.text_faint))
            .child(crate::conversation::error_card(
                k,
                &format!("timeline-{id}-{index}"),
                error,
            ));
    }
    payloads
}

/// The waterfall for one settled turn: a summary line, then a row per request, tool call and
/// reply on a shared time axis. Pressing a tool row opens its input and output.
pub fn timeline(k: &rune_kit::Kit, message_id: &str, trace: &TurnTrace, now: f64) -> El {
    let mut view = col().key(format!("timeline-{message_id}")).gap(8.0).child(
        text(summary_line(&summary(trace, now)))
            .key(format!("timeline-summary-{message_id}"))
            .font(k.ty.ui_compact)
            .color(k.c.text_muted),
    );
    for bar in bars(trace, TRACK_WIDTH, now) {
        view = view.child(bar_row(k, message_id, trace, &bar));
        if let RowKind::Tool(i) = bar.kind
            && trace.spans[i].open
        {
            view = view.child(span_json(k, message_id, i, &trace.spans[i]));
        }
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ClientTiming;

    fn span(name: &str, start: f64, end: Option<f64>) -> TraceSpan {
        TraceSpan {
            call_id: name.into(),
            name: name.into(),
            start,
            end,
            input: None,
            output: None,
            error: None,
            open: false,
            client: None,
        }
    }

    fn trace(finished: Option<f64>, spans: Vec<TraceSpan>) -> TurnTrace {
        TurnTrace {
            sent: 0.0,
            finished,
            spans,
            ..TurnTrace::default()
        }
    }

    fn tool_bars(trace: &TurnTrace, width: f32, now: f64) -> Vec<Bar> {
        bars(trace, width, now)
            .into_iter()
            .filter(|b| matches!(b.kind, RowKind::Tool(_)))
            .collect()
    }

    #[test]
    fn spans_map_onto_the_axis_from_the_moment_the_turn_was_sent() {
        let t = trace(Some(4.0), vec![span("search", 1.0, Some(2.0))]);
        let b = &tool_bars(&t, 100.0, 9.0)[0];
        assert_eq!((b.x, b.width), (25.0, 25.0));
        assert_eq!(b.seconds, 1.0);
        assert_eq!(b.label, "search");
    }

    #[test]
    fn rows_run_request_then_tools_then_reply() {
        let mut t = trace(Some(4.0), vec![span("search", 1.0, Some(2.0))]);
        t.first_token = Some(2.0);
        t.last_token = Some(4.0);
        let kinds: Vec<RowKind> = bars(&t, 100.0, 9.0).iter().map(|b| b.kind).collect();
        assert_eq!(kinds, [RowKind::Request, RowKind::Tool(0), RowKind::Reply]);
    }

    #[test]
    fn the_request_row_runs_until_the_first_activity() {
        let mut t = trace(Some(4.0), vec![span("search", 1.0, Some(2.0))]);
        t.first_token = Some(2.0);
        let request = &bars(&t, 100.0, 9.0)[0];
        assert_eq!((request.x, request.width), (0.0, 25.0));
    }

    #[test]
    fn the_reply_row_spans_the_first_to_the_last_token() {
        let mut t = trace(Some(4.0), vec![]);
        t.first_token = Some(1.0);
        t.last_token = Some(3.0);
        let reply = bars(&t, 100.0, 9.0).pop().unwrap();
        assert_eq!(reply.kind, RowKind::Reply);
        assert_eq!((reply.x, reply.width), (25.0, 50.0));
    }

    #[test]
    fn overlapping_calls_stack_and_sequential_calls_do_not() {
        let overlapping = trace(
            Some(4.0),
            vec![span("outer", 1.0, Some(3.0)), span("inner", 2.0, Some(2.5))],
        );
        let depths: Vec<usize> = tool_bars(&overlapping, 100.0, 9.0)
            .iter()
            .map(|b| b.depth)
            .collect();
        assert_eq!(depths, [0, 1]);

        let sequential = trace(
            Some(4.0),
            vec![span("a", 1.0, Some(2.0)), span("b", 3.0, Some(4.0))],
        );
        let depths: Vec<usize> = tool_bars(&sequential, 100.0, 9.0)
            .iter()
            .map(|b| b.depth)
            .collect();
        assert_eq!(depths, [0, 0]);
    }

    #[test]
    fn an_open_span_extends_to_now() {
        let t = trace(None, vec![span("search", 1.0, None)]);
        let b = &tool_bars(&t, 90.0, 3.0)[0];
        assert_eq!((b.x, b.width), (30.0, 60.0));
        assert_eq!(b.seconds, 2.0);
    }

    #[test]
    fn a_zero_width_call_becomes_a_marker() {
        let t = trace(Some(4.0), vec![span("skill", 2.0, Some(2.0))]);
        let b = &tool_bars(&t, 100.0, 9.0)[0];
        assert!(b.marker);
        assert_eq!((b.x, b.width), (50.0, MARKER_WIDTH));
    }

    #[test]
    fn a_tool_that_outlasts_the_turn_still_fits_the_axis() {
        let t = trace(Some(2.0), vec![span("late", 1.0, Some(4.0))]);
        let b = &tool_bars(&t, 100.0, 9.0)[0];
        assert_eq!((b.x, b.width), (25.0, 75.0));
    }

    #[test]
    fn a_failed_span_makes_a_failed_bar() {
        let mut failed = span("facet", 1.0, Some(2.0));
        failed.error = Some("no".into());
        let t = trace(Some(4.0), vec![failed, span("ok", 2.0, Some(3.0))]);
        let flags: Vec<bool> = tool_bars(&t, 100.0, 9.0).iter().map(|b| b.failed).collect();
        assert_eq!(flags, [true, false]);
    }

    #[test]
    fn the_summary_splits_the_turn_into_wait_tools_and_reply() {
        let mut t = trace(
            Some(4.0),
            vec![span("a", 0.5, Some(1.5)), span("b", 1.0, Some(2.0))],
        );
        t.first_token = Some(2.5);
        t.last_token = Some(4.0);
        assert_eq!(
            summary(&t, 9.0),
            Summary {
                total: 4.0,
                first_token: Some(2.5),
                tools: 1.5,
                waiting: 0.0,
                reply: Some(1.5),
            }
        );
    }

    #[test]
    fn the_token_rate_estimates_four_characters_to_a_token() {
        let mut t = trace(Some(3.0), vec![]);
        t.first_token = Some(1.0);
        t.last_token = Some(3.0);
        t.chars = 400;
        assert_eq!(token_rate(&t), Some(50.0));
    }

    #[test]
    fn a_single_delta_has_no_token_rate() {
        let mut t = trace(Some(3.0), vec![]);
        t.first_token = Some(1.0);
        t.last_token = Some(1.0);
        t.chars = 40;
        assert_eq!(token_rate(&t), None);
    }

    fn client_span(pending: f64, run: Option<f64>, end: Option<f64>) -> TraceSpan {
        let mut span = span("export", 0.5, end);
        span.client = Some(ClientTiming {
            pending_at: pending,
            run_start: run,
        });
        span
    }

    #[test]
    fn a_client_call_draws_its_wait_apart_from_the_time_it_ran() {
        let t = trace(Some(5.0), vec![client_span(1.0, Some(3.0), Some(4.0))]);
        let bar = &tool_bars(&t, 100.0, 9.0)[0];
        assert_eq!(bar.seconds, 1.0, "the bar is the run");
        assert_eq!(bar.waited, 2.0);
        let (wait_x, wait_width) = bar.wait.unwrap();
        assert_eq!(wait_x, 20.0);
        assert!(wait_width > 0.0 && wait_x + wait_width <= bar.x + 0.001);
        let summary = summary(&t, 9.0);
        assert_eq!(summary.tools, 1.0);
        assert_eq!(summary.waiting, 2.0);
    }

    #[test]
    fn a_declined_call_is_all_wait_and_a_marker() {
        let t = trace(Some(5.0), vec![client_span(1.0, None, Some(3.0))]);
        let bar = &tool_bars(&t, 100.0, 9.0)[0];
        assert!(bar.marker);
        assert_eq!(bar.seconds, 0.0);
        assert_eq!(bar.waited, 2.0);
        assert_eq!(summary(&t, 9.0).tools, 0.0);
    }

    #[test]
    fn a_server_tool_has_no_wait() {
        let t = trace(Some(4.0), vec![span("search", 1.0, Some(2.0))]);
        let bar = &tool_bars(&t, 100.0, 9.0)[0];
        assert_eq!(bar.wait, None);
        assert_eq!(summary(&t, 9.0).waiting, 0.0);
    }

    #[test]
    fn a_reply_continued_after_a_client_call_leaves_the_pause_out_of_its_rate() {
        let mut t = trace(Some(8.0), vec![client_span(2.0, Some(3.0), Some(5.0))]);
        t.first_token = Some(1.0);
        t.last_token = Some(8.0);
        t.chars = 400;
        assert_eq!(
            token_rate(&t),
            Some(100.0 / 4.0),
            "100 tokens in 4 s of streaming"
        );
        assert_eq!(summary(&t, 9.0).reply, Some(4.0));
        let reply = bars(&t, 100.0, 9.0).pop().unwrap();
        assert_eq!(reply.seconds, 4.0);
    }

    #[test]
    fn the_trace_records_pending_run_and_result_of_a_client_call() {
        use crate::agent::{PendingCall, StreamEvent};
        let mut t = TurnTrace::new(0.0);
        t.record(
            &StreamEvent::ToolStarted {
                call_id: "c1".into(),
                name: "export".into(),
            },
            0.5,
        );
        t.record(
            &StreamEvent::ClientToolsPending(vec![PendingCall {
                call_id: "c1".into(),
                name: "export".into(),
                input: serde_json::json!({}),
            }]),
            1.0,
        );
        t.record(
            &StreamEvent::ClientToolRunning {
                call_id: "c1".into(),
            },
            3.0,
        );
        t.record(
            &StreamEvent::ToolFinished {
                call_id: "c1".into(),
                name: "export".into(),
                output: None,
            },
            4.0,
        );
        assert_eq!(t.spans.len(), 1);
        assert_eq!(
            t.spans[0].client,
            Some(ClientTiming {
                pending_at: 1.0,
                run_start: Some(3.0)
            })
        );
        assert_eq!(t.spans[0].end, Some(4.0));
    }

    #[test]
    fn a_continued_turn_keeps_one_trace_and_a_correct_total() {
        let mut t = trace(None, vec![client_span(1.0, Some(2.0), Some(3.0))]);
        t.finished = None;
        t.first_token = Some(0.2);
        t.last_token = Some(0.9);
        t.record(&crate::agent::StreamEvent::TextDelta("more".into()), 3.5);
        t.record(&crate::agent::StreamEvent::Finished, 4.0);
        assert_eq!(summary(&t, 9.0).total, 4.0);
        assert_eq!(t.spans.len(), 1);
    }
}
