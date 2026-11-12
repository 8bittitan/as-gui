//! The conversation pane: the streaming message list, message bubbles, the tool-call group
//! and the composer.

use rune::{
    El, Icon, RichStyle, ScrollSpec, Span, TextStyle, Transition, Tx, auto_grow, col, effect, el,
    icon, rich, row, spacer, text, text_input,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Markdown, Spinner, State,
};
use rune_theme_opencode::icons;

use crate::agent::{ChatMessage, Role, SearchHit, ToolCall};
use crate::chat::Chat;
use crate::loading::PaneView;

/// The glyph the tool-call UI draws for tools; the theme has no wrench of its own.
static WRENCH: Icon = Icon {
    name: "tool-wrench",
    view: 24.0,
    body: r#"<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// The vote glyphs; the theme has no thumbs of its own.
static THUMBS_UP: Icon = Icon {
    name: "thumb-up",
    view: 24.0,
    body: r#"<path d="M14 9V5a3 3 0 0 0-3-3l-4 9v11h11.28a2 2 0 0 0 2-1.7l1.38-9a2 2 0 0 0-2-2.3zM7 22H4a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2h3" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

static THUMBS_DOWN: Icon = Icon {
    name: "thumb-down",
    view: 24.0,
    body: r#"<path d="M10 15v4a3 3 0 0 0 3 3l4-9V2H5.72a2 2 0 0 0-2 1.7l-1.38 9a2 2 0 0 0 2 2.3zm7-13h2.67A2.31 2.31 0 0 1 22 4v7a2.31 2.31 0 0 1-2.33 2H17" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

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
                    .padding(20.0)
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
                    .gap(10.0)
                    .child(text("Couldn't load this conversation").color(k.c.text_base))
                    .child(text(error).color(k.c.text_muted))
                    .child(
                        Button::new("Retry")
                            .on_press(|s: &mut Chat| s.retry_history())
                            .build(k)
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
        .padding(20.0)
        .pb(150.0)
        .gap(12.0)
        .items_center();
    let mut thread = col()
        .key("thread")
        .w_frac(fraction)
        .min_w(min_w)
        .max_w(max_w)
        .gap(12.0);
    for m in &session.messages {
        thread = thread.child(match m.role {
            Role::User => user_bubble(k, m),
            Role::Assistant => assistant_bubble(s, k, m),
        });
    }
    content = content.child(thread);
    list.child(content)
}

/// A bar standing in for text or a title while it loads.
pub fn skeleton_bar(k: &rune_kit::Kit, width: f32, height: f32, now: f64) -> El {
    el().w(width)
        .h(height)
        .radius(6.0)
        .fill(k.c.layer_02)
        .opacity(crate::loading::pulse(now))
}

/// The transcript's placeholder: an exchange or two of grey bars in the thread's shape.
fn skeleton_thread(k: &rune_kit::Kit, now: f64) -> El {
    let reply = |widths: [f32; 3]| {
        col()
            .w_full()
            .gap(8.0)
            .children(widths.map(|w| skeleton_bar(k, w, 14.0, now)))
    };
    col()
        .w_full()
        .gap(24.0)
        .child(
            row()
                .w_full()
                .child(spacer())
                .child(skeleton_bar(k, 220.0, 34.0, now)),
        )
        .child(reply([420.0, 360.0, 240.0]))
        .child(
            row()
                .w_full()
                .child(spacer())
                .child(skeleton_bar(k, 160.0, 34.0, now)),
        )
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
            .font(TextStyle {
                size: 15.0,
                weight: 590.0,
                tracking: -0.13,
                line_height: 20.0,
                ..k.ty.ui_medium
            })
            .color(k.c.text_base),
    );
    if let Some(description) = &description {
        state = state.child(text(description).wrap().color(k.c.text_muted));
    }
    state = state.child(col().key("starter-prompts").gap(8.0).children(
        STARTER_PROMPTS.iter().enumerate().map(|(i, prompt)| {
            let prompt = prompt.to_string();
            Button::new(&prompt)
                .variant(ButtonVariant::Outline)
                .on_press(move |s: &mut Chat| {
                    s.composer.clear();
                    s.composer.insert(&prompt);
                })
                .build(k)
                .key(format!("starter-{i}"))
        }),
    ));
    state
}

fn user_bubble(k: &rune_kit::Kit, m: &ChatMessage) -> El {
    row()
        .key(format!("m-{}", m.id))
        .w_full()
        .role("listitem")
        .child(spacer())
        .child(
            el().max_w(440.0)
                .padding(10.0)
                .px(14.0)
                .radius(12.0)
                .fill(k.c.layer_02)
                .child(text(&m.text).wrap().color(k.c.text_base)),
        )
}

fn assistant_bubble(s: &Chat, k: &rune_kit::Kit, m: &ChatMessage) -> El {
    let mut b = col()
        .key(format!("m-{}", m.id))
        .w_full()
        .role("listitem")
        .gap(6.0);
    if let Some(secs) = m.thought {
        b = b.child(
            row()
                .key(format!("thought-{}", m.id))
                .gap(6.0)
                .center_y()
                .child(icon(&icons::BRAIN, 12.0).color(k.c.icon_muted))
                .child(
                    text(format!("Thought for {secs:.0}s"))
                        .font(k.ty.ui_compact)
                        .color(k.c.text_faint),
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
            .streaming(m.streaming)
            .selectable(true);
        // Words that arrive mid-stream fade in; a settled reply shows whole.
        if m.streaming {
            reply = reply.reveal(s.reveal_seconds());
        }
        b = b.child(reply.build(k));
        if !m.streaming {
            b = b.child(message_actions(k, m));
        }
        if let Some(trace) = m.trace.as_ref().filter(|t| t.open) {
            b = b.child(crate::timeline::timeline(k, &m.id, trace, s.now));
        }
        if let Some(suggestions) = &m.suggestions {
            b = b.child(suggestion_chips(k, m.id.clone(), suggestions));
        }
    }
    b
}

/// The agent's suggested follow-ups as quick actions: tapping one sends it as the next
/// message.
fn suggestion_chips(k: &rune_kit::Kit, id: String, suggestions: &[String]) -> El {
    let mut chips = row().key(format!("suggestions-{id}")).flex_wrap().gap(6.0);
    for (i, suggestion) in suggestions.iter().enumerate() {
        let prompt = suggestion.clone();
        chips = chips.child(
            Button::new(suggestion)
                .variant(ButtonVariant::Outline)
                .on_press(move |s: &mut Chat| s.send_prompt(&prompt))
                .build(k)
                .key(format!("suggestion-{i}")),
        );
    }
    chips
}

/// The quiet action row under a settled assistant reply: copy, retry, and a vote. Votes go
/// to Agent Studio only for messages the server persisted.
fn message_actions(k: &rune_kit::Kit, m: &ChatMessage) -> El {
    let id = m.id.clone();
    let text = m.text.clone();
    let voted = m.vote;
    let mut actions = row()
        .key(format!("actions-{}", m.id))
        .gap(2.0)
        .child(
            IconButton::new(k.glyph.copy)
                .control_size(ControlSize::Small)
                .variant(IconButtonVariant::GhostMuted)
                .label("Copy reply")
                .on_press_tx(move |_: &mut Chat, tx: &mut Tx| tx.copy(text.clone()))
                .build(k)
                .key(format!("copy-{}", m.id)),
        )
        .child({
            let id = id.clone();
            IconButton::new(&icons::OUTLINE_ROTATE_CLOCKWISE)
                .control_size(ControlSize::Small)
                .variant(IconButtonVariant::GhostMuted)
                .label("Retry")
                .on_press(move |s: &mut Chat| s.retry(&id))
                .build(k)
                .key(format!("retry-{}", m.id))
        });
    if m.server_id.is_some() {
        actions = actions.child(
            IconButton::new(&THUMBS_UP)
                .control_size(ControlSize::Small)
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
                .build(k)
                .key(format!("up-{}", m.id)),
        );
        actions = actions.child(
            IconButton::new(&THUMBS_DOWN)
                .control_size(ControlSize::Small)
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
                .build(k)
                .key(format!("down-{}", m.id)),
        );
    }
    if let Some(trace) = m.trace.as_ref().filter(|t| t.finished.is_some()) {
        actions = actions.child(crate::timeline::affordance(k, &m.id, trace));
    }

    actions
}

fn tool_badge(k: &rune_kit::Kit) -> El {
    el().size(18.0, 18.0)
        .radius(9.0)
        .fill(k.c.layer_02)
        .center()
        .child(icon(&WRENCH, 11.0).color(k.c.icon_muted))
}

fn tools_section(s: &Chat, k: &rune_kit::Kit, m: &ChatMessage) -> El {
    let id = m.id.clone();
    let count = m.tools.len();
    let mut right = col().grow(1.0).min_w(0.0).gap(4.0).child(
        row()
            .key("tools-header")
            .gap(6.0)
            .center_y()
            .on_press_tx(move |s: &mut Chat, tx: &mut Tx| {
                s.toggle_tools_open(&id);
                tx.keep_on_screen("tools-header");
            })
            .child(
                text(if count == 1 {
                    "Used 1 tool".to_string()
                } else {
                    format!("Used {count} tools")
                })
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
            )
            .child(
                icon(
                    if m.tools_open {
                        k.glyph.chevron_up
                    } else {
                        k.glyph.chevron_down
                    },
                    14.0,
                )
                .color(k.c.text_muted),
            ),
    );
    if m.tools_open {
        for (i, tool) in m.tools.iter().enumerate() {
            right = right.child(tool_row(k, &m.id, i, tool));
            if tool.search_request().is_some() {
                right = right.child(search_button(k, &m.id, i));
            }
            if tool.open {
                if let Some(input) = &tool.input {
                    match SearchHit::from_json(input) {
                        Some(hits) => right = right.child(hit_cards(s, k, &tool.name, &hits)),
                        None => {
                            right =
                                right.child(json_card(k, format!("tool-json-{}", tool.name), input))
                        }
                    }
                }
                if let Some(error) = &tool.error {
                    right = right.child(error_card(k, &tool.name, error));
                }
            }
        }
    }
    row()
        .gap(14.0)
        .child(
            col()
                .w(18.0)
                .gap(4.0)
                .child(tool_badge(k))
                .child(el().w(1.0).grow(1.0).fill(k.c.border_muted)),
        )
        .child(right)
        .key("tools")
}

fn search_button(k: &rune_kit::Kit, message_id: &str, index: usize) -> El {
    let id = message_id.to_string();
    Button::new("Search it yourself")
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut Chat| s.open_search(&id, index))
        .build(k)
        .key(format!("search-yourself-{message_id}-{index}"))
        .ml(18.0)
}

fn tool_row(k: &rune_kit::Kit, message_id: &str, index: usize, tool: &ToolCall) -> El {
    let id = message_id.to_string();
    let name = tool.name.clone();
    let mut r = row()
        .gap(6.0)
        .center_y()
        .child(icon(&WRENCH, 12.0).color(k.c.icon_muted))
        .child(
            text(format!("Called tool {}", tool.name))
                .font(k.ty.ui_compact)
                .color(k.c.text_muted),
        );
    if tool.error.is_some() {
        r = r.child(
            text("failed")
                .key(format!("tool-failed-{}", tool.name))
                .font(k.ty.ui_compact)
                .color(k.c.danger.fg),
        );
    }
    if tool.input.is_some() || tool.error.is_some() {
        r = r.child(
            icon(
                if tool.open {
                    k.glyph.chevron_up
                } else {
                    k.glyph.chevron_down
                },
                12.0,
            )
            .color(k.c.text_faint),
        );
    }
    r.key(format!("tool-{}", tool.name))
        .on_press_tx(move |s: &mut Chat, tx: &mut Tx| {
            s.toggle_tool_open(&id, index);
            tx.keep_on_screen(format!("tool-{}", name));
        })
}

/// Why a tool call failed, under its row: the message as the stream gave it.
pub(crate) fn error_card(k: &rune_kit::Kit, name: &str, error: &str) -> El {
    col()
        .key(format!("tool-error-{name}"))
        .ml(28.0)
        .w_full()
        .max_w(440.0)
        .padding(12.0)
        .radius(8.0)
        .fill(k.c.danger.bg)
        .border(1.0, k.c.danger.border)
        .child(
            text(error)
                .wrap()
                .font(k.ty.ui_compact)
                .color(k.c.danger.fg),
        )
}

/// Algolia search hits as a grid of cards: each renders its markdown content clamped to the
/// card; pressing one opens it in the reader. Wide chats fit three columns, comfortable
/// ones two.
fn hit_cards(s: &Chat, k: &rune_kit::Kit, name: &str, hits: &[SearchHit]) -> El {
    let card_height = s.hit_card_height();
    let mut grid = col()
        .key(format!("hits-{name}"))
        .w_full()
        .equal_columns(s.hit_columns())
        .gap(8.0);
    for (i, hit) in hits.iter().enumerate() {
        let is_reading = s.reader.as_ref() == Some(&(name.to_string(), i));
        let mut card = col()
            .key(format!("hit-{name}-{i}"))
            .min_w(0.0)
            .h(card_height)
            .padding(12.0)
            .gap(6.0)
            .radius(10.0)
            .fill(k.c.layer_02)
            .border(1.0, k.c.border_muted)
            .clip_rounded(10.0);
        let tool = name.to_string();
        card = card
            .role("button")
            .label(&hit.title)
            .hover(k.c.hover)
            .on_press(move |s: &mut Chat| s.open_reader(&tool, i));
        if !is_reading {
            card = card.matched(format!("hit-{name}-{i}"));
        }
        card = card.child(
            text(&hit.title)
                .font(k.ty.ui_medium)
                .color(k.c.text_accent)
                .truncate(),
        );
        if let Some(snippet) = &hit.snippet {
            card = card.child(
                col()
                    .grow(1.0)
                    .min_h(0.0)
                    // A long URL in the markdown mustn't inflate the column's min-content
                    // width, or the grid's shared columns stop being equal.
                    .min_w(0.0)
                    .child(Markdown::new(snippet.clone()).build(k)),
            );
        }
        grid = grid.child(card);
    }
    grid
}

pub(crate) fn json_card(k: &rune_kit::Kit, key: String, input: &str) -> El {
    col()
        .ml(28.0)
        .w_full()
        .max_w(440.0)
        .padding(12.0)
        .radius(8.0)
        .fill(k.c.layer_01)
        .key(key)
        .child(
            rich(input, json_spans(input, k))
                .font(k.ty.mono)
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

fn json_spans(json: &str, k: &rune_kit::Kit) -> Vec<Span> {
    json_tokens(json)
        .into_iter()
        .map(|(start, end, token)| {
            let color = match token {
                JsonToken::Key => k.c.text_accent,
                JsonToken::Str => k.c.success.fg,
                JsonToken::Number => k.c.warning.fg,
                JsonToken::Keyword => k.c.text_faint,
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

const COMPOSER_RADIUS: f32 = 14.0;

fn composer(s: &Chat, k: &rune_kit::Kit) -> El {
    let field = text_input(&s.composer, k.ty.ui)
        .key("input")
        .w_full()
        .color(k.c.text_base)
        .rich_style(rich_style(k))
        .placeholder(
            "Ask anything…  \"Fix a TODO in the codebase\"",
            k.c.text_faint,
        )
        .caret(k.c.text_base, k.selection())
        .label("Message")
        .on_edit(|s: &mut Chat| &mut s.composer)
        .on_submit(|s: &mut Chat, _: &mut Tx| s.send());
    let (fraction, min_w, max_w) = s.width_spec();
    let card = col()
        .key("composer")
        .w_frac(fraction)
        .min_w(min_w)
        .max_w(max_w)
        .padding(12.0)
        .gap(10.0)
        .radius(COMPOSER_RADIUS)
        // Frosted: the transcript blurs through the card as it scrolls behind.
        .backdrop_blur(s.frost_blur())
        .fill(k.c.bg_base.alpha(s.frost_alpha()))
        .border(1.0, k.c.border_muted)
        .children(s.pending.then(|| glow_slot(k)))
        .child(auto_grow(field, 2, 8))
        .child(
            row().gap(8.0).center_y().child(status_line(s, k)).child(
                IconButton::new(&icons::ARROW_UP)
                    .variant(IconButtonVariant::Submit)
                    .label("Send")
                    .disabled(s.pending)
                    .on_press(|s: &mut Chat| s.send())
                    .build(k)
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
        .font(k.ty.ui_compact)
        .color(k.c.text_faint)
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
