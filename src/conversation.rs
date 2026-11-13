//! The conversation pane: the streaming message list, message bubbles, the tool-call group
//! and the composer.

use crate::satellite::{self, Fit, icons, typography};
use rune::{
    Color, El, RichStyle, ScrollSpec, Span, Transition, Tx, auto_grow, col, effect, el, icon, rich,
    row, spacer, text, text_input,
};
use rune_kit::{IconButton, IconButtonVariant, Markdown, Spinner, State};

use crate::agent::{ChatMessage, Role, SearchHit, ToolCall};
use crate::chat::Chat;
use crate::loading::PaneView;

/// The empty transcript's starter prompts, filling the composer when pressed.
const STARTER_PROMPTS: [&str; 3] = [
    "What can you help me with?",
    "Show me which tools you can use",
    "Summarize something interesting from your data",
];

/// The whole conversation screen: the transcript scrolling under a frosted composer pinned
/// to its bottom.
pub fn pane(s: &Chat, k: &rune_kit::Kit) -> El {
    col()
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .child(transcript(s, k))
        .child(
            el().key("composer-overlay")
                .pin(Some(16.0), None, Some(16.0), Some(8.0))
                .child(composer(s, k)),
        )
}

fn transcript(s: &Chat, k: &rune_kit::Kit) -> El {
    let session = &s.sessions[s.current];
    let (fraction, min_w, max_w) = s.width_spec();
    let list = col()
        .key("messages")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .scroll(ScrollSpec { stick_bottom: true })
        .role("list")
        .label("Conversation");
    match s.pane_view(s.now) {
        PaneView::Blank => return list,
        PaneView::Empty => {
            return list
                .items_center()
                .child(col().grow(1.0).center().child(empty_state(s, k)));
        }
        PaneView::Skeleton => {
            return list.items_center().child(
                col()
                    .key("messages-skeleton")
                    .w_full()
                    .padding(24.0)
                    .items_center()
                    .transition(Transition::opacity())
                    .child(
                        col()
                            .key("thread")
                            .w_frac(fraction)
                            .min_w(min_w)
                            .max_w(max_w)
                            .child(skeleton_thread(k, s.now)),
                    ),
            );
        }
        PaneView::Failed(error) => {
            return list.items_center().child(
                col()
                    .grow(1.0)
                    .center()
                    .gap(8.0)
                    .child(text("Couldn't load this conversation").color(k.c.text_base))
                    .child(text(error).color(k.c.text_muted))
                    .child(
                        satellite::button("Retry")
                            .on_press(|s: &mut Chat| s.retry_history())
                            .sized(k, satellite::Size::Medium)
                            .key("history-retry"),
                    ),
            );
        }
        PaneView::History => {}
    }
    // The padding lives on the scrolled content, so its bottom clears the frosted composer
    // overlaying the viewport.
    let mut content = col()
        .key("messages-content")
        .w_full()
        .padding(24.0)
        .pb(152.0)
        .items_center();
    let mut thread = col()
        .key("thread")
        .w_frac(fraction)
        .min_w(min_w)
        .max_w(max_w)
        .gap(MESSAGE_GAP);
    let latest = session
        .messages
        .iter()
        .rposition(|m| m.role == Role::Assistant);
    for (i, m) in session.messages.iter().enumerate() {
        thread = thread.child(match m.role {
            Role::User => user_bubble(k, m),
            Role::Assistant => assistant_bubble(s, k, m, Some(i) == latest),
        });
    }
    content = content.child(thread);
    list.child(content)
}

/// Satellite's ChatMessages: 24 points between messages.
const MESSAGE_GAP: f32 = 24.0;

/// A bar standing in for text or a title while it loads: Satellite's ChatMessageLoader line.
pub fn skeleton_bar(k: &rune_kit::Kit, width: f32, height: f32, now: f64) -> El {
    el().w(width)
        .h(height)
        .radius(satellite::RADIUS_CONTROL)
        .fill(k.c.layer_03.alpha(0.8))
        .opacity(crate::loading::pulse(now))
}

/// The transcript's placeholder: an exchange or two of grey bars in the thread's shape.
fn skeleton_thread(k: &rune_kit::Kit, now: f64) -> El {
    let reply = |widths: [f32; 3]| {
        col()
            .w_full()
            .gap(4.0)
            .children(widths.map(|w| skeleton_bar(k, w, 16.0, now)))
    };
    let question = |width: f32| {
        row()
            .w_full()
            .child(spacer())
            .child(skeleton_bar(k, width, 44.0, now).radius(satellite::RADIUS_CARD))
    };
    col()
        .w_full()
        .gap(MESSAGE_GAP)
        .child(question(220.0))
        .child(reply([420.0, 360.0, 240.0]))
        .child(question(160.0))
        .child(reply([380.0, 300.0, 180.0]))
}

/// The empty transcript: the agent introduces itself, with prompts that fill the composer.
fn empty_state(s: &Chat, k: &rune_kit::Kit) -> El {
    let (name, description) = s
        .selected_agent
        .as_ref()
        .map(|a| (a.name.clone(), a.description.clone()))
        .unwrap_or_default();
    let mut state = col()
        .key("empty-state")
        .max_w(420.0)
        .gap(12.0)
        .items_center();
    state = state.child(super::shell::letter_chip(&name, k, 40.0));
    state = state.child(
        text(&name)
            .font(typography::DISPLAY_SMALL)
            .color(k.c.text_base),
    );
    if let Some(description) = &description {
        state = state.child(text(description).wrap().color(k.c.text_muted));
    }
    state = state.child(col().key("starter-prompts").gap(8.0).children(
        STARTER_PROMPTS.iter().enumerate().map(|(i, prompt)| {
            let prompt = prompt.to_string();
            satellite::button(&prompt)
                .on_press(move |s: &mut Chat| {
                    s.composer.clear();
                    s.composer.insert(&prompt);
                })
                .sized(k, satellite::Size::Medium)
                .key(format!("starter-{i}"))
        }),
    ));
    state
}

/// Satellite's neutral ChatMessage: a grey-100 card on the right, at most 70% of the thread.
fn user_bubble(k: &rune_kit::Kit, m: &ChatMessage) -> El {
    row()
        .key(format!("m-{}", m.id))
        .w_full()
        .role("listitem")
        .child(spacer())
        .child(
            row().w_frac(0.7).child(spacer()).child(
                el().key(format!("bubble-{}", m.id))
                    .shrink()
                    .min_w(0.0)
                    .padding(12.0)
                    .radius(satellite::RADIUS_CARD)
                    .fill(k.c.layer_02)
                    .child(
                        text(&m.text)
                            .wrap()
                            .font(typography::BODY)
                            .color(k.c.text_base),
                    ),
            ),
        )
}

/// Satellite's subtle ChatMessage: no card, the thread's full width. Its actions show on
/// hover or keyboard focus, except on the latest reply, where they stay.
fn assistant_bubble(s: &Chat, k: &rune_kit::Kit, m: &ChatMessage, latest: bool) -> El {
    let mut b = col()
        .key(format!("m-{}", m.id))
        .w_full()
        .role("listitem")
        .hover_group_focus()
        .gap(12.0);
    if let Some(secs) = m.thought {
        b = b.child(
            row()
                .key(format!("thought-{}", m.id))
                .gap(4.0)
                .center_y()
                .child(icon(&icons::BRAIN, 12.0).color(k.c.icon_muted))
                .child(
                    text(format!("Thought for {secs:.0}s"))
                        .font(typography::CAPTION)
                        .color(k.c.text_muted),
                ),
        );
    }
    if !m.tools.is_empty() {
        b = b.child(tools_section(s, k, m));
    }
    if m.text.is_empty() && m.streaming {
        b = b.child(
            row()
                .gap(8.0)
                .center_y()
                .child(Spinner::new().build(k))
                .child(text("Thinking…").color(k.c.text_muted)),
        );
    } else {
        let mut reply = Markdown::new(m.text.clone())
            .style(crate::satellite::markdown(k))
            .streaming(m.streaming)
            .selectable(true);
        // Words that arrive mid-stream fade in; a settled reply shows whole.
        if m.streaming {
            reply = reply.reveal(s.reveal_seconds());
        }
        b = b.child(reply.build(k));
        if !m.streaming {
            let actions = message_actions(k, m);
            b = b.child(if latest {
                actions
            } else {
                actions.group_opacity(0.0, 1.0)
            });
        }
        if let Some(trace) = m.trace.as_ref().filter(|t| t.open) {
            b = b.child(crate::timeline::timeline(k, &m.id, trace, s.now));
        }
        if let Some(suggestions) = &m.suggestions {
            b = b.child(follow_ups(k, &m.id, suggestions));
        }
    }
    b
}

/// The agent's suggested follow-ups, under a heading as one list of full-width rows: pressing
/// one sends it as the next message.
fn follow_ups(k: &rune_kit::Kit, id: &str, suggestions: &[String]) -> El {
    let mut list = col()
        .key(format!("suggestions-{id}"))
        .role("list")
        .label("Follow up")
        .w_full()
        .radius(satellite::RADIUS_CARD)
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_base)
        .border_layout()
        .clip_rounded(satellite::RADIUS_CARD);
    for (i, suggestion) in suggestions.iter().enumerate() {
        let prompt = suggestion.clone();
        let mut r = row()
            .key(format!("suggestion-{i}"))
            .role("button")
            .label(suggestion)
            .focusable(true)
            .w_full()
            .min_h(40.0)
            .px(12.0)
            .py(10.0)
            .gap(12.0)
            .center_y()
            .hover(k.c.hover)
            .on_press(move |s: &mut Chat| s.send_prompt(&prompt))
            .child(icon(&icons::CORNER_DOWN_RIGHT, 14.0).color(k.c.icon_muted))
            .child(
                text(suggestion)
                    .wrap()
                    .grow(1.0)
                    .min_w(0.0)
                    .font(k.ty.ui)
                    .color(k.c.text_base),
            );
        if i > 0 {
            r = r.border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_muted);
        }
        list = list.child(r);
    }
    col()
        .key(format!("follow-ups-{id}"))
        .w_full()
        .gap(8.0)
        .mt(8.0)
        .child(
            text("Follow up")
                .font(typography::CAPTION)
                .color(k.c.text_muted),
        )
        .child(list)
}

/// The quiet action row under a settled assistant reply: copy, retry, and a vote. Votes go
/// to Agent Studio only for messages the server persisted.
fn message_actions(k: &rune_kit::Kit, m: &ChatMessage) -> El {
    let id = m.id.clone();
    let text = m.text.clone();
    let voted = m.vote;
    let mut actions = row()
        .key(format!("actions-{}", m.id))
        .gap(4.0)
        .child(
            IconButton::new(k.glyph.copy)
                .variant(IconButtonVariant::GhostMuted)
                .label("Copy reply")
                .on_press_tx(move |_: &mut Chat, tx: &mut Tx| tx.copy(text.clone()))
                .sized(k, satellite::Size::Small)
                .key(format!("copy-{}", m.id)),
        )
        .child({
            let id = id.clone();
            IconButton::new(&icons::ROTATE_CW)
                .variant(IconButtonVariant::GhostMuted)
                .label("Retry")
                .on_press(move |s: &mut Chat| s.retry(&id))
                .sized(k, satellite::Size::Small)
                .key(format!("retry-{}", m.id))
        });
    if m.server_id.is_some() {
        actions = actions.child(
            IconButton::new(&icons::THUMBS_UP)
                .variant(IconButtonVariant::GhostMuted)
                .label("Good reply")
                .disabled(voted.is_some())
                .state(if voted == Some(1) {
                    State::Pressed
                } else {
                    State::Rest
                })
                .on_press({
                    let id = id.clone();
                    move |s: &mut Chat| s.vote(&id, true)
                })
                .sized(k, satellite::Size::Small)
                .key(format!("up-{}", m.id)),
        );
        actions = actions.child(
            IconButton::new(&icons::THUMBS_DOWN)
                .variant(IconButtonVariant::GhostMuted)
                .label("Bad reply")
                .disabled(voted.is_some())
                .state(if voted == Some(0) {
                    State::Pressed
                } else {
                    State::Rest
                })
                .on_press({
                    let id = id.clone();
                    move |s: &mut Chat| s.vote(&id, false)
                })
                .sized(k, satellite::Size::Small)
                .key(format!("down-{}", m.id)),
        );
    }
    if let Some(trace) = m.trace.as_ref().filter(|t| t.finished.is_some()) {
        actions = actions.child(crate::timeline::affordance(k, &m.id, trace));
    }

    actions
}

fn tools_section(s: &Chat, k: &rune_kit::Kit, m: &ChatMessage) -> El {
    let mut cards = col().key("tools").w_full().min_w(0.0).gap(8.0);
    for (i, tool) in m.tools.iter().enumerate() {
        cards = cards.child(tool_step(s, k, &m.id, i, tool));
    }
    cards
}

fn disclosure(k: &rune_kit::Kit, open: bool) -> El {
    icon(
        if open {
            k.glyph.chevron_down
        } else {
            k.glyph.caret_right
        },
        14.0,
    )
    .color(k.c.icon_muted)
}

/// A call's leading icon: the tool wrench, or the alert when it failed. The magnifying glass
/// is left to the header's Search it yourself.
pub(crate) fn step_glyph(tool: &ToolCall) -> &'static rune::Icon {
    if tool.error.is_some() {
        &icons::CIRCLE_ALERT
    } else {
        &icons::WRENCH
    }
}

/// What a call did, in words: the index it searched, or the tool it ran.
pub(crate) fn step_label(tool: &ToolCall) -> String {
    match crate::agent::index_from_tool_name(&tool.name) {
        Some(index) => format!("Searched {index}"),
        None => format!("Ran {}", tool.name),
    }
}

fn search_button(k: &rune_kit::Kit, message_id: &str, index: usize) -> El {
    let id = message_id.to_string();
    IconButton::new(&icons::SEARCH)
        .variant(IconButtonVariant::GhostMuted)
        .label("Search it yourself")
        .on_press(move |s: &mut Chat| s.open_search(&id, index))
        .sized(k, satellite::Size::Small)
        .help("Search it yourself")
        .key(format!("search-yourself-{message_id}-{index}"))
}

/// One call as one card: a header saying what it did and found, and, opened, its results,
/// payload or failure beneath on the same surface.
fn tool_step(s: &Chat, k: &rune_kit::Kit, message_id: &str, index: usize, tool: &ToolCall) -> El {
    let hits = tool.input.as_deref().and_then(SearchHit::from_json);
    let mut card = col()
        .key(format!("tool-step-{}", tool.name))
        .w_full()
        .min_w(0.0)
        .radius(satellite::RADIUS_CARD)
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_base)
        .border_layout()
        .clip_rounded(satellite::RADIUS_CARD)
        .child(tool_header(k, message_id, index, tool, hits.as_deref()));
    if tool.open {
        match (&hits, &tool.input) {
            (Some(hits), _) => {
                card = card.child(hit_list(s, k, message_id, index, &tool.name, hits))
            }
            (None, Some(input)) => card = card.child(step_json(k, &tool.name, input)),
            (None, None) => {}
        }
        if let Some(error) = &tool.error {
            card = card.child(step_error(k, &tool.name, error));
        }
    }
    card
}

/// A step's header: icon, what it did, the query and what came back, then the search
/// action and the disclosure, all on one line.
fn tool_header(
    k: &rune_kit::Kit,
    message_id: &str,
    index: usize,
    tool: &ToolCall,
    hits: Option<&[SearchHit]>,
) -> El {
    let id = message_id.to_string();
    let name = tool.name.clone();
    let failed = tool.error.is_some();
    let expandable = tool.input.is_some() || failed;
    let search = tool.search_request();
    let glyph = step_glyph(tool);
    let label = step_label(tool);
    let mut facts: Vec<El> = Vec::new();
    if let Some(query) = search.as_ref().filter(|q| !q.query.is_empty()) {
        facts.push(
            text(format!("\u{201c}{}\u{201d}", query.query))
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .truncate(),
        );
    }
    if let Some(hits) = hits {
        let n = hits.len();
        facts.push(
            text(if n == 1 {
                "1 result".to_string()
            } else {
                format!("{n} results")
            })
            .font(k.ty.ui)
            .color(k.c.text_muted),
        );
    }
    if failed {
        facts.push(
            text("Failed")
                .key(format!("tool-failed-{}", tool.name))
                .font(k.ty.ui)
                .color(k.c.danger.fg),
        );
    }
    let mut header = row()
        .key(format!("tool-{}", tool.name))
        .role("button")
        .label(&label)
        .w_full()
        .min_w(0.0)
        .min_h(40.0)
        .pl(12.0)
        .pr(8.0)
        .gap(8.0)
        .center_y()
        .child(icon(glyph, 14.0).color(if failed {
            k.c.danger.icon
        } else {
            k.c.icon_muted
        }))
        .child(text(&label).font(k.ty.ui_medium).color(k.c.text_base));
    for fact in facts {
        header = header
            .child(text("·").font(k.ty.ui).color(k.c.text_faint).a11y_hidden())
            .child(fact);
    }
    header = header.child(spacer());
    if search.is_some() {
        header = header.child(search_button(k, message_id, index));
    }
    if expandable {
        header = header
            .child(disclosure(k, tool.open))
            .hover(k.c.hover)
            .on_press_tx(move |s: &mut Chat, tx: &mut Tx| {
                s.toggle_tool_open(&id, index);
                tx.keep_on_screen(format!("tool-{}", name));
            });
    }
    header
}

/// A section of a step's card below its header: a hairline above, then `body`.
fn step_section(k: &rune_kit::Kit, key: String, fill: Color, body: El) -> El {
    col()
        .key(key)
        .w_full()
        .min_w(0.0)
        .fill(fill)
        .border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_base)
        .child(body)
}

fn step_json(k: &rune_kit::Kit, name: &str, input: &str) -> El {
    step_section(
        k,
        format!("tool-json-{name}"),
        k.c.layer_02,
        col()
            .w_full()
            .max_h(320.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(
                col().w_full().px(12.0).py(8.0).child(
                    rich(input, json_spans(input, k))
                        .font(typography::CODE)
                        .wrap_anywhere()
                        .color(k.c.text_muted),
                ),
            ),
    )
}

fn step_error(k: &rune_kit::Kit, name: &str, error: &str) -> El {
    step_section(
        k,
        format!("tool-error-{name}"),
        k.c.danger.bg,
        row()
            .w_full()
            .px(12.0)
            .py(8.0)
            .gap(8.0)
            .child(
                icon(&icons::CIRCLE_ALERT, 14.0)
                    .color(k.c.danger.icon)
                    .mt(3.0),
            )
            .child(
                text(error)
                    .wrap()
                    .grow(1.0)
                    .min_w(0.0)
                    .font(k.ty.text_small)
                    .color(k.c.danger.fg),
            ),
    )
}

/// Where a step's payload starts: past its row's 14 point icon and 8 point gap.
const TOOL_INDENT: f32 = 22.0;

/// The look of Satellite's ChatContextContainer: neutral, or the danger tint for a failure.
struct Context {
    border: Color,
    body: Color,
    ink: Color,
    icon: Color,
}

impl Context {
    fn neutral(k: &rune_kit::Kit) -> Context {
        Context {
            border: k.c.border_base,
            body: k.c.layer_02,
            ink: k.c.text_muted,
            icon: k.c.icon_muted,
        }
    }

    fn danger(k: &rune_kit::Kit) -> Context {
        Context {
            border: k.c.danger.border,
            body: k.c.danger.bg,
            ink: k.c.danger.fg,
            icon: k.c.danger.icon,
        }
    }
}

/// Satellite's ChatContextContainer: a white header naming the payload over a grey body that
/// scrolls past 320 points.
fn context_card(
    k: &rune_kit::Kit,
    key: String,
    glyph: &'static rune::Icon,
    title: &str,
    look: Context,
    body: El,
) -> El {
    let card = col()
        .key(key)
        .w_full()
        .min_w(0.0)
        .radius(satellite::RADIUS_CONTROL)
        .border(1.0, look.border)
        .border_layout()
        .clip_rounded(satellite::RADIUS_CONTROL)
        .child(
            row()
                .w_full()
                .min_h(32.0)
                .pl(12.0)
                .pr(4.0)
                .gap(8.0)
                .center_y()
                .fill(k.c.bg_base)
                .border_sides([0.0, 0.0, 1.0, 0.0], look.border)
                .child(icon(glyph, 12.0).color(look.icon))
                .child(text(title).font(typography::CAPTION).color(k.c.text_muted)),
        )
        .child(
            col()
                .w_full()
                .max_h(320.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .fill(look.body)
                .child(col().w_full().px(12.0).py(8.0).child(body.color(look.ink))),
        );
    col().w_full().min_w(0.0).pl(TOOL_INDENT).child(card)
}

/// Why a tool call failed, under its row: the message as the stream gave it.
pub(crate) fn error_card(k: &rune_kit::Kit, name: &str, error: &str) -> El {
    context_card(
        k,
        format!("tool-error-{name}"),
        &icons::CIRCLE_ALERT,
        "Error",
        Context::danger(k),
        text(error).wrap().font(typography::CAPTION),
    )
}

/// Algolia search hits as one list: rank, title and a two-line plain excerpt per row, the
/// first few until Show all. Pressing a row opens the hit in the reader.
fn hit_list(
    s: &Chat,
    k: &rune_kit::Kit,
    message_id: &str,
    index: usize,
    name: &str,
    hits: &[SearchHit],
) -> El {
    let expanded = s.hits_expanded(message_id, index);
    let shown = if expanded {
        hits.len()
    } else {
        s.results_shown().min(hits.len())
    };
    let mut list = col()
        .key(format!("hits-{name}"))
        .w_full()
        .min_w(0.0)
        .border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_base);
    for (i, hit) in hits.iter().enumerate().take(shown) {
        list = list.child(hit_row(s, k, name, i, hit));
    }
    if hits.len() > s.results_shown() {
        let id = message_id.to_string();
        let label = if expanded {
            "Show fewer".to_string()
        } else {
            format!("Show all {} results", hits.len())
        };
        list = list.child(
            row()
                .key(format!("hits-more-{name}"))
                .role("button")
                .label(&label)
                .w_full()
                .px(12.0)
                .py(8.0)
                .border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_muted)
                .hover(k.c.hover)
                .on_press(move |s: &mut Chat| s.toggle_hits_expanded(&id, index))
                .child(text(label).font(k.ty.ui).color(k.c.text_accent)),
        );
    }
    list
}

fn hit_row(s: &Chat, k: &rune_kit::Kit, name: &str, i: usize, hit: &SearchHit) -> El {
    let is_reading = s.reader.as_ref() == Some(&(name.to_string(), i));
    let tool = name.to_string();
    let excerpt = hit.excerpt();
    let mut body = col().grow(1.0).min_w(0.0).gap(2.0).child(
        text(&hit.title)
            .font(k.ty.ui_medium)
            .color(k.c.text_accent)
            .truncate(),
    );
    if !excerpt.is_empty() {
        body = body.child(
            text(excerpt)
                .font(k.ty.text_small)
                .color(k.c.text_muted)
                .max_lines(2),
        );
    }
    let mut r = row()
        .key(format!("hit-{name}-{i}"))
        .role("button")
        .label(&hit.title)
        .w_full()
        .min_w(0.0)
        .px(12.0)
        .py(10.0)
        .gap(12.0)
        .hover(k.c.hover)
        .on_press(move |s: &mut Chat| s.open_reader(&tool, i))
        .child(
            text(format!("{}", i + 1))
                .font(typography::CAPTION)
                .color(k.c.text_muted)
                .w(16.0)
                .mt(2.0),
        )
        .child(body);
    if i > 0 {
        r = r.border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_muted);
    }
    if !is_reading {
        r = r.matched(format!("hit-{name}-{i}"));
    }
    r
}

pub(crate) fn json_card(k: &rune_kit::Kit, key: String, input: &str) -> El {
    context_card(
        k,
        key,
        &icons::WRENCH,
        "JSON",
        Context::neutral(k),
        rich(input, json_spans(input, k))
            .font(typography::CODE)
            .wrap_anywhere(),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsonToken {
    Key,
    Str,
    Number,
    Keyword,
}

/// Byte ranges of the JSON's tokens: object keys, string values, numbers and
/// `true`/`false`/`null`. Punctuation and whitespace stay unstyled.
fn json_tokens(json: &str) -> Vec<(usize, usize, JsonToken)> {
    let bytes = json.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                let rest = json[i..].trim_start();
                let kind = if rest.starts_with(':') {
                    JsonToken::Key
                } else {
                    JsonToken::Str
                };
                tokens.push((start, i, kind));
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    i += 1;
                }
                tokens.push((start, i, JsonToken::Number));
            }
            b't' | b'f' | b'n' => {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if matches!(&json[start..i], "true" | "false" | "null") {
                    tokens.push((start, i, JsonToken::Keyword));
                }
            }
            _ => i += 1,
        }
    }
    tokens
}

pub(crate) fn json_spans(json: &str, k: &rune_kit::Kit) -> Vec<Span> {
    json_tokens(json)
        .into_iter()
        .map(|(start, end, token)| {
            let color = match token {
                JsonToken::Key => k.c.text_accent,
                JsonToken::Str => k.c.success.fg,
                JsonToken::Number => k.c.warning.fg,
                JsonToken::Keyword => k.c.text_muted,
            };
            Span::new(start, end).color(color)
        })
        .collect()
}

/// The working glow while a reply streams: the slot covers the card, and `glow::Glow` paints
/// Palermo's beams behind its top edge. It fades in and out over 0.28s with the turn.
fn glow_slot(k: &rune_kit::Kit) -> El {
    let fade = rune::Spring::new(0.28, 0.0);
    effect(crate::glow::MARK)
        .key("composer-glow")
        .effect_data(crate::glow::slot_data(k))
        .effect_bleed(crate::glow::BLEED)
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .radius(COMPOSER_RADIUS)
        .a11y_hidden()
        .transition(Transition::opacity().springs(rune::SpringPair::new(fade, fade)))
}

const COMPOSER_RADIUS: f32 = satellite::RADIUS_CARD;

/// Satellite's ChatPrompt: a TextArea's grey-500 border, focus border and 12 point insets,
/// with the send row under the field, over the frosted fill the transcript blurs through.
fn composer(s: &Chat, k: &rune_kit::Kit) -> El {
    let field = text_input(&s.composer, typography::BODY)
        .key("input")
        .w_full()
        .color(k.c.text_base)
        .rich_style(rich_style(k))
        .placeholder("Ask anything…", k.c.text_muted)
        .caret(k.c.text_base, k.selection())
        .label("Message")
        .no_focus_ring()
        .on_edit(|s: &mut Chat| &mut s.composer)
        .on_submit(|s: &mut Chat, _: &mut Tx| s.send());
    let (fraction, min_w, max_w) = s.width_spec();
    let card = col()
        .key("composer")
        .w_frac(fraction)
        .min_w(min_w)
        .max_w(max_w)
        .radius(COMPOSER_RADIUS)
        // Frosted: the transcript blurs through the card as it scrolls behind.
        .backdrop_blur(s.frost_blur())
        .fill(k.c.bg_base.alpha(s.frost_alpha()))
        .border(1.0, satellite::elevation::field_border())
        .focus_within_ring(1.0, -1.0, k.c.border_focus)
        .children(s.pending.then(|| glow_slot(k)))
        .child(
            col()
                .w_full()
                .px(12.0)
                .pt(12.0)
                .child(auto_grow(field, 2, 8)),
        )
        .child(
            row()
                .w_full()
                .px(12.0)
                .pt(8.0)
                .pb(12.0)
                .gap(8.0)
                .center_y()
                .child(status_line(s, k))
                .child(
                    IconButton::new(&icons::ARROW_UP)
                        .variant(IconButtonVariant::Submit)
                        .label("Send")
                        .disabled(s.pending)
                        .on_press(|s: &mut Chat| s.send())
                        .sized(k, satellite::Size::Medium)
                        .key("send"),
                ),
        );
    col().w_full().center().child(card)
}

/// The rich-mark palette the composer's plain field paints, matching the kit's boxed fields.
fn rich_style(k: &rune_kit::Kit) -> RichStyle {
    RichStyle {
        link: k.c.text_accent,
        code_fg: None,
        code_bg: k.c.layer_03,
        code_block_bg: k.c.layer_02,
        quote_bar: k.c.border_strong,
        marker: Some(k.c.text_muted),
    }
}

/// The client and model the composer talks to, in the card's send row.
fn status_line(s: &Chat, k: &rune_kit::Kit) -> El {
    let model = s
        .selected_agent
        .as_ref()
        .and_then(|a| a.model.clone())
        .unwrap_or_else(|| "—".into());
    text(format!("{} • {}", s.client.name(), model))
        .key("status-line")
        .font(typography::CAPTION)
        .color(k.c.text_muted)
        .grow(1.0)
}

#[cfg(test)]
mod json_tests {
    use super::*;

    fn kinds(json: &str) -> Vec<(String, JsonToken)> {
        json_tokens(json)
            .into_iter()
            .map(|(s, e, k)| (json[s..e].to_string(), k))
            .collect()
    }

    #[test]
    fn object_keys_and_string_values_are_told_apart() {
        assert_eq!(
            kinds("{\n  \"name\": \"inbox-check\",\n  \"cron\": \"0 9 * * 1-5\"\n}"),
            vec![
                ("\"name\"".into(), JsonToken::Key),
                ("\"inbox-check\"".into(), JsonToken::Str),
                ("\"cron\"".into(), JsonToken::Key),
                ("\"0 9 * * 1-5\"".into(), JsonToken::Str),
            ]
        );
    }

    #[test]
    fn numbers_and_keywords_are_tokenized() {
        assert_eq!(
            kinds(
                "{\"limit\": 7, \"facets\": [\"*\"], \"page\": 0, \"ok\": true, \"extra\": null, \"ratio\": -1.5e2}"
            ),
            vec![
                ("\"limit\"".into(), JsonToken::Key),
                ("7".into(), JsonToken::Number),
                ("\"facets\"".into(), JsonToken::Key),
                ("\"*\"".into(), JsonToken::Str),
                ("\"page\"".into(), JsonToken::Key),
                ("0".into(), JsonToken::Number),
                ("\"ok\"".into(), JsonToken::Key),
                ("true".into(), JsonToken::Keyword),
                ("\"extra\"".into(), JsonToken::Key),
                ("null".into(), JsonToken::Keyword),
                ("\"ratio\"".into(), JsonToken::Key),
                ("-1.5e2".into(), JsonToken::Number),
            ]
        );
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_string() {
        assert_eq!(
            kinds("{\"note\": \"said \\\"hi\\\" loudly\"}"),
            vec![
                ("\"note\"".into(), JsonToken::Key),
                ("\"said \\\"hi\\\" loudly\"".into(), JsonToken::Str),
            ]
        );
    }

    #[test]
    fn a_nested_search_payload_tokenizes_like_the_tools_show() {
        let json = "{\n  \"queries\": [\n    {\n      \"query\": \"wide running shoes\",\n      \"facet_lang\": [\n        \"en-US\"\n      ]\n    }\n  ],\n  \"sessionId\": \"7a1d\",\n  \"clickAnalytics\": true\n}";
        let tokens = kinds(json);
        assert!(tokens.contains(&("\"queries\"".into(), JsonToken::Key)));
        assert!(tokens.contains(&("\"wide running shoes\"".into(), JsonToken::Str)));
        assert!(tokens.contains(&("\"en-US\"".into(), JsonToken::Str)));
        assert!(tokens.contains(&("\"sessionId\"".into(), JsonToken::Key)));
        assert!(tokens.contains(&("\"7a1d\"".into(), JsonToken::Str)));
        assert!(tokens.contains(&("\"clickAnalytics\"".into(), JsonToken::Key)));
        assert!(tokens.contains(&("true".into(), JsonToken::Keyword)));
        assert_eq!(
            tokens.len(),
            9,
            "keys, strings and the keyword; no punctuation"
        );
    }
}
