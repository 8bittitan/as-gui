//! The Quick Ask panel: a launcher-style window over any app (⌘⇧Space) that streams one
//! question to the agent in use and can promote the exchange into the main window.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rune::{
    AppHandle, Cx, El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, Waker, col, el, icon, row, spacer,
    text, text_input,
};
use rune_kit::{IconButton, IconButtonVariant, Markdown, Spinner};
use rune_theme_opencode::icons;
use tracing::warn;

use crate::agent::{AgentSnapshot, ChatMessage, Role, StreamEvent, spawn_stream};
use crate::chat::Chat;
use crate::palette::AppColour;
use crate::selection::{Capture, NoSelection, Quote, SelectionSource, Source};

pub struct QuickAsk {
    pub composer: TextEdit,
    pub messages: Vec<ChatMessage>,
    pub pending: bool,
    pub error: Option<String>,
    /// The main window's agent in use, published on every change.
    pub snapshot: Arc<Mutex<AgentSnapshot>>,
    /// The main window, for promoting the exchange. `None` in unit tests.
    pub chat: Option<AppHandle<Chat>>,
    /// This panel, for the shortcut's show/hide. `None` in unit tests.
    pub me: Option<AppHandle<QuickAsk>>,
    turn_started: Option<f64>,
    events: Option<std::sync::mpsc::Receiver<StreamEvent>>,
    pub waker: Option<Waker>,
    next_id: usize,
    /// The text the next question is about.
    pub quote: Option<Quote>,
    pub selection: Arc<dyn SelectionSource>,
    /// The instruction and quote each quoted question was built from, by message id.
    asked_about: HashMap<String, (String, Quote)>,
}

impl QuickAsk {
    pub fn new(
        snapshot: Arc<Mutex<AgentSnapshot>>,
        chat: Option<AppHandle<Chat>>,
        me: Option<AppHandle<QuickAsk>>,
    ) -> QuickAsk {
        QuickAsk {
            composer: TextEdit::new(""),
            messages: Vec::new(),
            pending: false,
            error: None,
            snapshot,
            chat,
            me,
            turn_started: None,
            events: None,
            waker: None,
            next_id: 0,
            quote: None,
            selection: Arc::new(NoSelection),
            asked_about: HashMap::new(),
        }
    }

    fn fresh_id(&mut self) -> String {
        self.next_id += 1;
        format!("qa{}", self.next_id)
    }

    /// Sends the composer's text as a question to the agent in use; with a quote and nothing
    /// typed, it asks to explain the quote.
    pub fn submit(&mut self) {
        let typed = self.composer.text.trim().to_string();
        let instruction = match (&self.quote, typed.is_empty()) {
            (None, true) => return,
            (Some(_), true) => DEFAULT_INSTRUCTION.to_string(),
            (_, false) => typed,
        };
        self.send(instruction);
    }

    fn send(&mut self, instruction: String) {
        if self.pending {
            return;
        }
        let client = {
            let snapshot = self.snapshot.lock().unwrap_or_else(|p| p.into_inner());
            if snapshot.agent_id.is_empty() {
                return;
            }
            Arc::clone(&snapshot.client)
        };
        self.composer.clear();
        self.error = None;
        let user = ChatMessage::user(self.fresh_id(), message(&instruction, self.quote.as_ref()));
        if let Some(quote) = self.quote.take() {
            self.asked_about
                .insert(user.id.clone(), (instruction, quote));
        }
        let assistant = ChatMessage::assistant(self.fresh_id());
        let history = vec![user.clone()];
        self.messages.push(user);
        self.messages.push(assistant);
        self.pending = true;
        let waker = self.waker.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        self.events = Some(receiver);
        spawn_stream(
            client,
            history,
            None,
            move |event| {
                let _ = sender.send(event);
            },
            waker,
        );
    }

    /// Carries the exchange into the main window and closes the panel.
    pub fn promote(&mut self, tx: &mut Tx) {
        let Some(chat) = &self.chat else { return };
        if self.pending || self.messages.len() < 2 {
            return;
        }
        let prompt = self.messages[0].text.clone();
        let reply = self.messages[1].text.clone();
        let conversation_id = self.messages[1].server_id.clone();
        let chat = chat.clone();
        chat.update(move |c, _| c.receive_quick_ask(prompt, reply, conversation_id));
        self.messages.clear();
        self.asked_about.clear();
        self.error = None;
        tx.close_window();
    }
}

const DEFAULT_INSTRUCTION: &str = "Explain this";

/// The canned instructions under a quote: key, label, instruction.
const CHIPS: [(&str, &str, &str); 4] = [
    ("explain", "Explain", DEFAULT_INSTRUCTION),
    ("summarize", "Summarize", "Summarize this"),
    ("translate", "Translate", "Translate this into English"),
    ("fix", "Fix", "Fix this"),
];

/// The question as the agent reads it: the instruction, then the quote in a code fence longer
/// than any backtick run inside it.
fn message(instruction: &str, quote: Option<&Quote>) -> String {
    let Some(quote) = quote else {
        return instruction.to_string();
    };
    let longest = quote
        .text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    format!("{instruction}\n\n{fence}\n{}\n{fence}", quote.text)
}

const PANEL_WIDTH: f32 = 480.0;
const COMPACT_HEIGHT: f32 = 160.0;
const EXPANDED_HEIGHT: f32 = 480.0;
/// The quote block and the chips under the field.
const QUOTE_HEIGHT: f32 = 150.0;

impl QuickAsk {
    /// The window is only as tall as the panel needs: the field alone, or room for an exchange.
    pub fn wanted_size(&self) -> (f32, f32) {
        let height = if self.messages.is_empty() {
            COMPACT_HEIGHT
        } else {
            EXPANDED_HEIGHT
        };
        let quote = if self.quote.is_some() {
            QUOTE_HEIGHT
        } else {
            0.0
        };
        (PANEL_WIDTH, height + quote)
    }
}

/// The panel's first size. A headless recording can't follow the panel growing, so the demo
/// starts it tall enough for the whole exchange.
pub fn launch_size() -> (f32, f32) {
    let height = if crate::agent::demo_mode() {
        EXPANDED_HEIGHT
    } else {
        COMPACT_HEIGHT
    };
    (PANEL_WIDTH, height)
}

fn fit(s: &QuickAsk, tx: &mut Tx) {
    tx.resize_window(s.wanted_size());
}

/// Sends the composer's question and makes room for its answer.
pub fn ask(s: &mut QuickAsk, tx: &mut Tx) {
    s.submit();
    fit(s, tx);
}

/// The panel is on screen: put the caret in the field, by the model (so it holds across
/// frames) and by key (so it wins focus a click on a panel button took).
pub fn shown(s: &mut QuickAsk, tx: &mut Tx) {
    fit(s, tx);
    s.composer.focused = true;
    tx.focus("qa-field", false);
}

/// The window's focus callback: the panel just became the key window.
pub fn focus_changed(s: &mut QuickAsk, focused: bool, tx: &mut Tx) {
    if focused {
        shown(s, tx);
    }
}

/// Sends a chip's canned instruction about the quote.
fn ask_with(s: &mut QuickAsk, instruction: &str, tx: &mut Tx) {
    s.send(instruction.to_string());
    fit(s, tx);
}

fn remove_quote(s: &mut QuickAsk, tx: &mut Tx) {
    s.quote = None;
    fit(s, tx);
    s.composer.focused = true;
    tx.focus("qa-field", false);
}

/// Places the panel near the top of the main screen, shows it and focuses the field.
fn present(s: &mut QuickAsk, tx: &mut Tx) {
    if let Some(me) = &s.me {
        me.place(
            rune::DisplayTarget::Primary,
            rune::OverlayPlacement::Edge {
                edge: rune::ScreenEdge::Top,
                margin: 150.0,
                align: rune::Align::Center,
                inset: 0.0,
            },
        );
        me.show();
    }
    shown(s, tx);
}

/// The Quick Ask shortcut: show the panel for a plain question, or hide it when it's up.
pub fn toggled(s: &mut QuickAsk, tx: &mut Tx) {
    s.quote = None;
    let Some(me) = &s.me else { return };
    if me.is_visible() {
        me.hide();
    } else {
        present(s, tx);
    }
}

/// How long the hidden panel gives up the keyboard before ⌘C is sent past it.
const HIDE_BEAT: std::time::Duration = std::time::Duration::from_millis(60);

/// The Ask about selection shortcut: capture the selection off the UI thread, then show the
/// panel quoting it. Without a handle to post back through, it captures in place.
pub fn ask_about_selection(s: &mut QuickAsk, tx: &mut Tx) {
    let source = Arc::clone(&s.selection);
    let Some(me) = s.me.clone() else {
        let capture = source.capture();
        quote_and_show(s, capture, tx);
        return;
    };
    // While the panel is up it takes keys, so ⌘C would copy from the panel itself.
    let was_visible = me.is_visible();
    if was_visible {
        me.hide();
    }
    std::thread::spawn(move || {
        if was_visible {
            std::thread::sleep(HIDE_BEAT);
        }
        let capture = source.capture();
        me.update(move |s, tx| quote_and_show(s, capture, tx));
    });
}

/// Quotes what was captured (nothing: a plain Quick Ask) and shows the panel.
pub fn quote_and_show(s: &mut QuickAsk, capture: Option<Capture>, tx: &mut Tx) {
    if !s.pending {
        s.quote = capture.and_then(Quote::from_capture);
    }
    present(s, tx);
}

/// Drains the reply stream; true when state changed.
pub fn poll(s: &mut QuickAsk, t: f64, _: &mut Tx) -> bool {
    let mut changed = false;
    if s.pending && s.turn_started.is_none() {
        s.turn_started = Some(t);
        changed = true;
    }
    let mut dead = false;
    while let Some(receiver) = s.events.as_ref() {
        match receiver.try_recv() {
            Ok(ev) => {
                changed = true;
                match ev {
                    StreamEvent::Start { message_id } => {
                        if let Some(m) = s.messages.last_mut() {
                            m.server_id = Some(message_id);
                        }
                    }
                    StreamEvent::TextDelta(t) => {
                        if let Some(m) = s.messages.last_mut() {
                            m.text.push_str(&t);
                        }
                    }
                    StreamEvent::Finished => {
                        if let Some(m) = s.messages.last_mut() {
                            m.streaming = false;
                        }
                        s.pending = false;
                    }
                    StreamEvent::Failed(e) => {
                        if let Some(m) = s.messages.last_mut() {
                            m.streaming = false;
                        }
                        s.pending = false;
                        s.error = Some(e);
                    }
                    // The server created the conversation for this exchange: promoting it
                    // carries the id, so the main window can continue it.
                    StreamEvent::ConversationStarted(id) => {
                        if let Some(m) = s.messages.last_mut() {
                            m.server_id = Some(id);
                        }
                    }
                    _ => {}
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                dead = true;
                break;
            }
        }
    }
    if dead {
        s.events = None;
        if s.pending {
            s.pending = false;
            if let Some(m) = s.messages.last_mut() {
                m.streaming = false;
            }
            s.error = Some("the request thread ended early".into());
        }
    }
    changed
}

/// The panel: the agent in use, the exchange, and the composer.
pub fn view(s: &QuickAsk, cx: &mut Cx<QuickAsk>) -> El {
    let (agent_name, agent_known, theme, accent, application) = {
        let snapshot = s.snapshot.lock().unwrap_or_else(|p| p.into_inner());
        (
            snapshot.agent_name.clone(),
            !snapshot.agent_id.is_empty(),
            snapshot.theme,
            snapshot.accent,
            snapshot.application.clone(),
        )
    };
    let themed = crate::shell::kit_for(theme, accent, cx.system_dark());
    let k = &themed;
    let has_exchange = !s.messages.is_empty();
    let mut header = row()
        .key("qa-header")
        .w_full()
        .px(14.0)
        .py(10.0)
        .gap(8.0)
        .center_y()
        .child(crate::shell::letter_chip(&agent_name, k, 18.0))
        .child(
            text(if agent_known {
                format!("Ask {agent_name}")
            } else {
                "Pick an agent in as-gui first".into()
            })
            .font(k.ty.ui_medium)
            .color(k.c.text_base)
            .truncate()
            .grow(1.0)
            .min_w(0.0),
        );
    if let Some((name, colour)) = application {
        header = header.child(application_chip(k, &name, colour));
    }
    if has_exchange {
        header = header.child(promote_button(s, k));
    }
    let mut panel = col()
        .key("quick-ask")
        .w_full()
        .radius(14.0)
        .clip_rounded(14.0)
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_muted)
        .child(header);
    if has_exchange {
        panel = panel
            .child(divider(k))
            .child(transcript(s, k))
            .child(divider(k));
    }
    if let Some(quote) = &s.quote {
        panel = panel.child(quote_block(k, quote));
    }
    let field = text_input(&s.composer, k.ty.ui)
        .key("qa-field")
        .grow(1.0)
        .color(k.c.text_base)
        .placeholder(
            match (agent_known, s.quote.is_some()) {
                (false, _) => "No agent selected yet",
                (true, true) => "Ask about it, or press Return to explain it",
                (true, false) => "Ask anything…",
            },
            k.c.text_faint,
        )
        .label("Quick ask")
        .on_edit(|s: &mut QuickAsk| &mut s.composer)
        .on_submit(|s: &mut QuickAsk, tx| ask(s, tx))
        .on_input_key(|_s: &mut QuickAsk, ev: &KeyEvent, tx: &mut Tx| {
            if ev.key == Key::Escape {
                tx.close_window();
                return true;
            }
            false
        });
    panel = panel.child(
        row()
            .key("qa-composer")
            .w_full()
            .px(14.0)
            .py(10.0)
            .gap(8.0)
            .center_y()
            .child(field)
            .child(
                IconButton::new(&icons::ARROW_UP)
                    .variant(IconButtonVariant::Submit)
                    .label("Ask")
                    .disabled(s.pending || !agent_known)
                    .on_press_tx(ask)
                    .build(k)
                    .key("qa-send"),
            ),
    );
    if s.quote.is_some() {
        panel = panel.child(chips(k, s.pending || !agent_known));
    }
    panel = panel.child(footer(k));
    col().w_full().h_full().child(panel)
}

/// What the next question is about: where it came from, the start of it, and an × that
/// takes it away.
fn quote_block(k: &rune_kit::Kit, quote: &Quote) -> El {
    let origin = match quote.source {
        Source::Selection => "Selected text",
        Source::Clipboard => "From clipboard",
    };
    let mut head = row()
        .w_full()
        .gap(6.0)
        .center_y()
        .child(icon(&icons::OUTLINE_QUOTE, 12.0).color(k.c.text_muted))
        .child(text(origin).font(k.ty.small).color(k.c.text_muted));
    if quote.trimmed {
        head = head.child(text("(trimmed)").font(k.ty.small).color(k.c.text_faint));
    }
    head = head.child(spacer()).child(
        IconButton::new(&icons::CLOSE_SMALL)
            .control_size(rune_kit::ControlSize::Small)
            .variant(IconButtonVariant::GhostMuted)
            .label("Remove quote")
            .help("Remove quote")
            .on_press_tx(remove_quote)
            .build(k)
            .key("qa-quote-remove"),
    );
    col().key("qa-quote").w_full().px(14.0).pt(10.0).child(
        col()
            .w_full()
            .px(10.0)
            .py(6.0)
            .gap(2.0)
            .radius(8.0)
            .fill(k.c.layer_02)
            .border(1.0, k.c.border_muted)
            .child(head)
            .child(
                text(preview(&quote.text))
                    .wrap()
                    .font(k.ty.ui_compact)
                    .color(k.c.text_base)
                    .min_w(0.0),
            ),
    )
}

const PREVIEW_LINES: usize = 3;
const PREVIEW_CHARS: usize = 240;

/// The start of the quote, as much as the block shows.
fn preview(text: &str) -> String {
    let mut lines = text.lines();
    let mut shown: String = lines
        .by_ref()
        .take(PREVIEW_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    let mut cut = lines.next().is_some();
    if let Some((at, _)) = shown.char_indices().nth(PREVIEW_CHARS) {
        shown.truncate(at);
        cut = true;
    }
    if cut {
        shown.push('…');
    }
    shown
}

/// One press asks a canned question about the quote.
fn chips(k: &rune_kit::Kit, disabled: bool) -> El {
    let mut chips = row().key("qa-chips").w_full().px(14.0).pb(10.0).gap(6.0);
    for (key, label, instruction) in CHIPS {
        chips = chips.child(
            rune_kit::Button::new(label)
                .variant(rune_kit::ButtonVariant::Outline)
                .control_size(rune_kit::ControlSize::Small)
                .disabled(disabled)
                .on_press_tx(move |s: &mut QuickAsk, tx: &mut Tx| ask_with(s, instruction, tx))
                .build(k)
                .key(format!("qa-chip-{key}")),
        );
    }
    chips
}

/// Which application the panel asks: its colour beside its name.
fn application_chip(k: &rune_kit::Kit, name: &str, colour: AppColour) -> El {
    row()
        .key("qa-application")
        .gap(6.0)
        .px(8.0)
        .py(3.0)
        .radius(10.0)
        .fill(k.c.layer_02)
        .center_y()
        .shrink()
        .min_w(0.0)
        .child(crate::shell::colour_dot(colour, k, 8.0))
        .child(text(name).font(k.ty.small).color(k.c.text_muted).truncate())
}

fn divider(k: &rune_kit::Kit) -> El {
    el().h(1.0).w_full().fill(k.c.border_muted)
}

/// Opens the exchange in the main window; there once there's something to open, live once the
/// reply has settled.
fn promote_button(s: &QuickAsk, k: &rune_kit::Kit) -> El {
    let ready = !s.pending && s.messages.len() >= 2;
    let mut promote = IconButton::new(&icons::OUTLINE_EXPAND)
        .control_size(rune_kit::ControlSize::Small)
        .variant(IconButtonVariant::GhostMuted)
        .label("Open in as-gui")
        .help("Open in as-gui");
    promote = if ready {
        promote.on_press_tx(|s: &mut QuickAsk, tx| s.promote(tx))
    } else {
        promote.disabled(true)
    };
    promote.build(k).key("qa-promote")
}

/// The question, then the reply as markdown (a single "Thinking…" until its first words),
/// then any error, scrolling within the panel.
fn transcript(s: &QuickAsk, k: &rune_kit::Kit) -> El {
    let mut transcript = col()
        .key("qa-transcript")
        .w_full()
        .max_h(320.0)
        .padding(14.0)
        .gap(12.0)
        .scroll(ScrollSpec { stick_bottom: true });
    for m in s
        .messages
        .iter()
        .filter(|m| m.role == Role::User || m.streaming || !m.text.is_empty())
    {
        transcript = transcript.child(match m.role {
            Role::User => row()
                .key(format!("qa-{}", m.id))
                .w_full()
                .child(spacer())
                .child(question_bubble(k, m, s.asked_about.get(&m.id))),
            Role::Assistant if m.streaming && m.text.is_empty() => row()
                .key("qa-thinking")
                .gap(8.0)
                .center_y()
                .child(Spinner::new().build(k))
                .child(text("Thinking…").color(k.c.text_muted)),
            Role::Assistant => col().key("qa-reply").w_full().min_w(0.0).child(
                Markdown::new(m.text.clone())
                    .streaming(m.streaming)
                    .selectable(true)
                    .build(k),
            ),
        });
    }
    if let Some(error) = &s.error {
        transcript = transcript.child(error_callout(k, error));
    }
    transcript
}

/// The question as typed; a quoted one shows its instruction, the quote folded to one line
/// beneath it.
fn question_bubble(k: &rune_kit::Kit, m: &ChatMessage, asked: Option<&(String, Quote)>) -> El {
    let bubble = col()
        .max_w(340.0)
        .padding(8.0)
        .px(12.0)
        .gap(4.0)
        .radius(10.0)
        .fill(k.c.layer_02);
    let Some((instruction, quote)) = asked else {
        return bubble.child(text(&m.text).wrap().color(k.c.text_base));
    };
    bubble
        .child(text(instruction).wrap().color(k.c.text_base))
        .child(
            row()
                .key(format!("qa-{}-quote", m.id))
                .gap(6.0)
                .center_y()
                .min_w(0.0)
                .child(icon(&icons::OUTLINE_QUOTE, 12.0).color(k.c.text_muted))
                .child(
                    text(quote.text.lines().next().unwrap_or_default())
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .truncate()
                        .min_w(0.0),
                ),
        )
}

/// A failed ask: an icon and the error as the API gave it, on the danger tint.
fn error_callout(k: &rune_kit::Kit, error: &str) -> El {
    row()
        .key("qa-error")
        .role("alert")
        .w_full()
        .padding(10.0)
        .gap(8.0)
        .radius(8.0)
        .fill(k.c.danger.bg)
        .border(1.0, k.c.danger.border)
        .child(icon(&icons::CIRCLE_EXCLAMATION, 14.0).color(k.c.danger.icon))
        .child(
            text(error)
                .wrap()
                .font(k.ty.ui_compact)
                .color(k.c.danger.fg)
                .grow(1.0)
                .min_w(0.0),
        )
}

/// "Powered by Algolia" under the composer; it links to algolia.com.
fn footer(k: &rune_kit::Kit) -> El {
    row()
        .key("qa-footer")
        .role("button")
        .label("Powered by Algolia")
        .w_full()
        .px(14.0)
        .py(9.0)
        .gap(8.0)
        .center_y()
        .fill(k.c.bg_deep)
        .hover(k.c.hover)
        .on_press_tx(|_: &mut QuickAsk, tx: &mut Tx| {
            if let Err(error) = tx.open_url("https://www.algolia.com") {
                warn!("could not open algolia.com: {error:?}");
            }
        })
        .child(spacer())
        .child(text("Powered by").font(k.ty.small).color(k.c.text_muted))
        .child(logo())
}

const LOGO_WIDTH: f32 = 68.0;
const LOGO_HEIGHT: f32 = LOGO_WIDTH * 500.0 / 2196.2;

/// The logo icon is a square view box with the artwork centred in it, so a window of the
/// artwork's own height clips the empty bands above and below.
fn logo() -> El {
    col()
        .key("qa-footer-logo")
        .w(LOGO_WIDTH)
        .h(LOGO_HEIGHT)
        .clip_rounded(0.0)
        .child(icon(&crate::brand::ALGOLIA_LOGO, LOGO_WIDTH).mt(-(LOGO_WIDTH - LOGO_HEIGHT) / 2.0))
}

fn with_scene_agent(s: &mut QuickAsk) {
    let mut snapshot = s.snapshot.lock().unwrap_or_else(|p| p.into_inner());
    snapshot.agent_id = "scene".into();
    snapshot.agent_name = "Shoe fit helper".into();
    snapshot.application = Some(("Production".into(), AppColour::Orange));
}

/// The debug scene's state: a question already asked and answered.
pub fn scene(s: &mut QuickAsk) {
    with_scene_agent(s);
    s.messages.push(ChatMessage::user("qa1", "What is new?"));
    let mut reply = ChatMessage::assistant("qa2");
    reply.text = "The new season adds the **Aero 2** and a wider fit:\n\n- Cushioned Series\n- Two trail options".into();
    reply.streaming = false;
    reply.server_id = Some("alg_cnv_demo".into());
    s.messages.push(reply);
}

/// A question whose request failed: the error callout.
pub fn error_scene(s: &mut QuickAsk) {
    with_scene_agent(s);
    s.messages.push(ChatMessage::user("qa1", "This is a test"));
    let mut reply = ChatMessage::assistant("qa2");
    reply.streaming = false;
    s.messages.push(reply);
    s.error = Some("agent studio returned HTTP 422 — messages: must not be empty".into());
}

/// Ask about selection, opened on an error message copied from another app.
pub fn selection_scene(s: &mut QuickAsk) {
    with_scene_agent(s);
    s.quote = Quote::from_capture(Capture {
        text: "TypeError: Cannot read properties of undefined (reading 'map')\n    at renderHits (search.js:42:18)\n    at InstantSearch.render (instantsearch.js:118:9)".into(),
        source: Source::Selection,
    });
}

/// Registers the panel's system-wide chords (Quick Ask, Ask about selection) through its app.
pub struct PanelShortcuts {
    app: AppHandle<QuickAsk>,
    held: std::cell::RefCell<std::collections::HashMap<u32, rune::ShortcutId>>,
    next: std::cell::Cell<u32>,
}

impl PanelShortcuts {
    pub fn new(app: AppHandle<QuickAsk>) -> Self {
        PanelShortcuts {
            app,
            held: Default::default(),
            next: std::cell::Cell::new(0),
        }
    }
}

impl crate::keymap::ShortcutRegistrar for PanelShortcuts {
    fn register(
        &self,
        action: crate::keymap::Action,
        shortcut: &rune::Shortcut,
    ) -> Result<crate::keymap::Registration, rune::ShortcutError> {
        let handler = match action {
            crate::keymap::Action::AskSelection => ask_about_selection,
            _ => toggled,
        };
        let id = self.app.register_shortcut(shortcut.clone(), handler)?;
        let n = self.next.get() + 1;
        self.next.set(n);
        self.held.borrow_mut().insert(n, id);
        Ok(crate::keymap::Registration(n))
    }

    fn unregister(
        &self,
        registration: crate::keymap::Registration,
    ) -> Result<(), rune::ShortcutError> {
        match self.held.borrow_mut().remove(&registration.0) {
            Some(id) => self.app.unregister_shortcut(id),
            None => Ok(()),
        }
    }

    fn owner(&self, shortcut: &rune::Shortcut) -> Option<String> {
        self.app.shortcut_owner(shortcut)
    }
}
