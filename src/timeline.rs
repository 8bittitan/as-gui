//! The per-turn waterfall: where an answer's time went. `bars` and `summary` are the layout
//! maths; the view builds on them.

use rune::{El, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use crate::agent::{SearchHit, TraceSpan, TurnTrace};
use crate::chat::Chat;
use crate::conversation::json_card;

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
    pub seconds: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub total: f64,
    pub first_token: Option<f64>,
    pub tools: f64,
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
    });

    for (i, span) in trace.spans.iter().enumerate() {
        let end = span_end(span, now);
        let depth = trace.spans[..i]
            .iter()
            .filter(|earlier| span_end(earlier, now) > span.start)
            .count();
        let (x, w, marker) = place(span.start, end);
        rows.push(Bar {
            kind: RowKind::Tool(i),
            label: span.name.clone(),
            depth,
            x,
            width: w,
            marker,
            failed: span.error.is_some(),
            seconds: end - span.start,
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
            seconds: end - first,
        });
    }
    rows
}

pub fn summary(trace: &TurnTrace, now: f64) -> Summary {
    let mut intervals: Vec<(f64, f64)> = trace
        .spans
        .iter()
        .map(|s| (s.start, span_end(s, now)))
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
        reply: trace
            .first_token
            .zip(reply_end(trace, now))
            .map(|(first, end)| end - first),
    }
}

/// Tokens per second, estimated as four characters to a token between the first and last
/// text delta.
pub fn token_rate(trace: &TurnTrace) -> Option<f64> {
    let span = trace.last_token? - trace.first_token?;
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
    if let Some(reply) = summary.reply {
        parts.push(format!("reply {}", seconds(reply)));
    }
    parts.join(" · ")
}

/// The small "⏱ 4.2s" button under a settled reply that opens its timeline.
pub fn affordance(k: &rune_kit::Kit, message_id: &str, trace: &TurnTrace) -> El {
    let total = seconds(summary(trace, trace.finished.unwrap_or(trace.sent)).total);
    let id = message_id.to_string();
    Button::new(format!("⏱ {total}"))
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .expanded(trace.open)
        .help("Where this answer's time went")
        .on_press(move |s: &mut Chat| s.toggle_timeline(&id))
        .build(k)
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
        RowKind::Request => k.c.icon_faint,
        RowKind::Tool(_) if bar.failed => k.c.danger.icon,
        RowKind::Tool(_) => k.c.bg_accent,
        RowKind::Reply => k.c.icon_muted,
    };
    let track = row()
        .w(TRACK_WIDTH)
        .h(BAR_HEIGHT + 6.0)
        .center_y()
        .radius(4.0)
        .fill(k.c.layer_01)
        .child(el().w(bar.x.min(TRACK_WIDTH - MARKER_WIDTH)))
        .child(
            el().w(bar.width)
                .h(if bar.marker {
                    BAR_HEIGHT + 6.0
                } else {
                    BAR_HEIGHT
                })
                .radius(3.0)
                .fill(fill),
        );
    let mut r = row()
        .key(row_key(bar.kind, id))
        .gap(8.0)
        .center_y()
        .child(
            col().w(LABEL_WIDTH).child(
                text(&bar.label)
                    .font(k.ty.ui_compact)
                    .color(k.c.text_muted)
                    .truncate()
                    .ml(bar.depth as f32 * INDENT),
            ),
        )
        .child(track)
        .child(
            text(stats(trace, bar))
                .font(k.ty.ui_compact)
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
    let mut view = col().key(format!("timeline-{message_id}")).gap(6.0).child(
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
}
