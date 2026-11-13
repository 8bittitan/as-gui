//! The app chrome: the SplitView with the session sidebar, the toolbar with the agent
//! title, the agent-picker screen, the global keybinds and the error toast.

use crate::satellite::typography::{BODY, HEADING, SUBHEADING};
use crate::satellite::{
    self, Fit, Hue, Level, MODAL_MEDIUM, MODAL_SMALL, Planet, RADIUS_CARD, RADIUS_CONTROL,
    RADIUS_MODAL, icons,
};
use rune::{
    Color, Cx, El, Key, KeyEvent, ScrollSpec, SpringPair, Transition, Tx, col, el, icon, row,
    spacer, text, text_input,
};
use rune_kit::shell::SidebarToggle;
use rune_kit::{
    Badge, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Markdown, SegmentedControl,
    Select, ShortcutRecorder, Sidebar, SidebarRow, SidebarSection, Spinner, SplitView, Switch,
    Toolbar, shortcut_error_message,
};
use tracing::warn;

use crate::agent::{AgentInfo, AgentStatus};
use crate::chat::{Accent, Chat, ChatWidth, ThemePref};
use crate::conversation;
use crate::keymap::Action;
use crate::loading::{Phase, Screen, SidebarView};
use crate::palette::AppColour;
use crate::selection::Access;

/// The rebindable shortcuts (`Keymap`), from anywhere including the focused composer.
pub fn on_key(s: &mut Chat, ev: &KeyEvent, tx: &mut Tx) -> bool {
    // ⌘⇧K opens the tuning knobs (dev builds), Esc closes them; the page slides over.
    if rune_knobs::key(&mut s.knobs, ev) {
        tx.motion(s.knobs.slide());
        return true;
    }
    if ev.key == Key::Escape && s.search.open && !s.has_modal_open() {
        s.close_search();
        return true;
    }
    if s.command_bar.open && crate::command_bar::key(s, ev, tx) {
        return true;
    }
    let action = s.keymap.action_for(ev, tx.physical_key());
    // Behind the open bar only its own chord and the system-wide one still answer.
    if s.command_bar.open
        && !matches!(action, Some(Action::CommandBar) | None)
        && !action.is_some_and(Action::is_global)
    {
        return true;
    }
    match action {
        Some(Action::ToggleSidebar) => s.sidebar.toggle(),
        Some(Action::CommandBar) => crate::command_bar::toggled(s, tx),
        Some(Action::Settings) => s.settings_open = !s.settings_open,
        Some(Action::NewSession) => s.new_session(),
        // The global shortcuts handle these, whichever app is in front.
        Some(Action::QuickAsk | Action::AskSelection) | None => return false,
    }
    true
}

pub fn view(s: &Chat, cx: &mut Cx<Chat>) -> El {
    let theme_kit = kit_for(s.appearance.theme, s.appearance.accent, cx.system_dark());
    let k = &theme_kit;
    crate::satellite::install(k, cx);
    let mut layer: Vec<El> = Vec::new();

    let screen = s.screen(s.now);
    if let Screen::Splash { spinner } = screen {
        return rune_kit::root(splash(k, spinner), layer);
    }

    let sidebar = session_sidebar(s, k);

    let picking = s.selected_agent.is_none();
    let session = &s.sessions[s.current];

    let main = if picking {
        picker_main(s, k, &screen)
    } else {
        conversation::pane(s, k)
    };
    // With the sidebar toggled away, a narrow rail of icons keeps sessions one click deep.
    let main = if s.sidebar.is_open() {
        main
    } else {
        row()
            .key("rail-row")
            .w_full()
            .h_full()
            .child(sidebar_rail(s, k))
            .child(
                el().key("rail-divider")
                    .w(1.0)
                    .h_full()
                    .fill(k.c.border_muted),
            )
            .child(el().grow(1.0).min_w(0.0).h_full().child(main))
    };

    let mut toolbar = Toolbar::new().leading(if picking {
        breadcrumb(k, "as-gui", "Choose an agent")
    } else if s.sessions_load.is_pending(s.now) {
        row().key("session-header").h(18.0).child(
            conversation::skeleton_bar(k, 140.0, 14.0, s.now).opacity(
                if s.sessions_load.phase(s.now) == Phase::Skeleton {
                    crate::loading::pulse(s.now)
                } else {
                    0.0
                },
            ),
        )
    } else {
        // The session title leads under a chip derived from it, matching the sidebar's
        // row; the agent's name is the centred title beside its own chip.
        row()
            .key("session-header")
            .gap(8.0)
            .center_y()
            .child(letter_chip(&session.title, k, 18.0))
            .child(
                text(&session.title)
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
            )
    });
    // The agent's name sits centred in the header, beside its chip. With a choice of agents
    // it opens the command bar on them.
    if !picking {
        let name = s
            .selected_agent
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_default();
        let mut title = row()
            .key("agent-title")
            .gap(8.0)
            .center_y()
            .px(8.0)
            .py(4.0)
            .radius(RADIUS_CONTROL)
            .child(letter_chip(&name, k, 18.0))
            .child(text(&name).font(k.ty.ui_medium).color(k.c.text_base));
        let published = s
            .agents
            .iter()
            .filter(|a| a.status == AgentStatus::Published)
            .count();
        if published > 1 {
            title = title
                .role("button")
                .label(format!("{name}, switch agent"))
                .focusable(true)
                .hover(k.c.hover)
                .on_press_tx(|s: &mut Chat, tx: &mut Tx| crate::command_bar::open_agents(s, tx));
        }
        toolbar = toolbar.center(title);
    }
    let mut trailing = row().gap(12.0).center_y();
    let mut has_trailing = false;
    if s.pending {
        trailing = trailing.child(Spinner::new().build(k));
        has_trailing = true;
    }
    if let Some(current) = s.active_option() {
        let switcher = Select::new("application-switcher", &s.application_options())
            .current(Some(&current))
            .label("Application")
            .open(s.application_menu_open)
            .on_toggle(|s: &mut Chat| s.application_menu_open = !s.application_menu_open)
            .on_close(|s: &mut Chat| s.application_menu_open = false)
            .on_choose(|s: &mut Chat, v: &str, _: &mut Tx| s.application_menu_chosen(v))
            .build(k, &mut layer);
        let mut application = row().gap(4.0).center_y();
        if let Some(colour) = s.active_colour() {
            application = application
                .child(colour_dot(colour, k, 8.0).key(format!("application-dot-{}", colour.id())));
        }
        trailing = trailing.child(application.child(switcher));
        has_trailing = true;
    }
    if has_trailing {
        toolbar = toolbar.trailing(trailing);
    }

    let sidebar_hint = interned(s.keymap.hint(Action::ToggleSidebar));
    let split = SplitView::new(main)
        .sidebar(&s.sidebar, |s: &mut Chat| &mut s.sidebar, sidebar)
        .toolbar(toolbar)
        .titlebar_inset(cx.titlebar_inset())
        .sidebar_toggle(
            SidebarToggle::new()
                .size(ControlSize::Large)
                .past_lights(16.0)
                .edge(18.0)
                .gap(4.0)
                .shortcut(sidebar_hint),
        )
        .sidebar_padding(18.0, 10.0)
        .build(k);

    if let Some(error) = &s.error {
        layer.push(
            rune_kit::Toast::new("error", "Request failed")
                .description(error)
                .on_close(|s: &mut Chat| s.error = None)
                .build(k),
        );
    }

    let mut portal = Vec::new();
    reader_dialog(s, k, &mut layer);
    crate::command_bar::overlay(s, k, &mut layer);
    settings_dialog(s, k, &mut layer, &mut portal);
    // The select's listbox follows the dialog in node order, so it anchors to the trigger
    // inside the panel and draws above it.
    layer.extend(portal);
    // The welcome dialog sits above everything: until credentials exist, they come first.
    credentials_dialog(s, k, &mut layer);

    let mut page = row().w_full().h_full().child(split.grow(1.0).min_w(0.0));
    if s.search.open {
        page = page.child(crate::search_panel::panel(s, k));
    }
    if let Some(colour) = s.window_tint() {
        page = page.child(
            el().key("window-tint")
                .pin(Some(0.0), Some(0.0), Some(0.0), None)
                .h(2.0)
                .fill(colour.color(k.dark))
                .a11y_hidden(),
        );
    }
    let tuned = rune_knobs::side(
        page,
        &s.knobs,
        crate::knobs::GROUPS,
        if k.dark {
            rune_knobs::Style::dark()
        } else {
            rune_knobs::Style::light()
        },
        |s: &mut Chat| &mut s.knobs,
    );
    rune_kit::root(tuned, layer)
}

/// The Satellite kit for the settings' appearance and accent.
pub fn kit_for(theme: ThemePref, accent: Accent, system_dark: bool) -> rune_kit::Kit {
    crate::satellite::kit(theme.resolve(system_dark), accent).clone()
}

/// A search hit zoomed out of its card: the full markdown, scrollable, flown in from the
/// grid card that shares its `matched` id.
fn reader_dialog(s: &Chat, k: &rune_kit::Kit, layer: &mut Vec<El>) {
    let (Some((tool, index)), Some(hit)) = (&s.reader, s.reader_hit()) else {
        return;
    };
    let close = |s: &mut Chat| s.close_reader();
    let mut header = modal_header(k, "hit-reader-header", dialog_title(k, &hit.title), 32.0);
    header = header.child(
        satellite::button("Search it yourself")
            .variant(ButtonVariant::Ghost)
            .disabled(!s.reader_can_search())
            .on_press(|s: &mut Chat| s.open_search_from_reader())
            .sized(k, satellite::Size::Medium)
            .key("hit-reader-search"),
    );
    if let Some(url) = hit.url.clone() {
        header = header.child(
            satellite::button("Open source")
                .variant(ButtonVariant::Outline)
                .on_press_tx(move |_: &mut Chat, tx: &mut Tx| {
                    if let Err(error) = tx.open_url(&url) {
                        warn!("could not open {url}: {error:?}");
                    }
                })
                .sized(k, satellite::Size::Medium)
                .key("hit-reader-open"),
        );
    }
    header = header.child(
        IconButton::new(k.glyph.close)
            .variant(IconButtonVariant::Ghost)
            .label("Close")
            .on_press(close)
            .sized(k, satellite::Size::Medium)
            .key("hit-reader-close"),
    );
    let body = col()
        .key("hit-reader-body")
        .grow(1.0)
        .min_h(0.0)
        .px(32.0)
        .py(24.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            Markdown::new(hit.snippet.unwrap_or_default())
                .style(crate::satellite::markdown(k))
                .build(k),
        );
    let panel = modal_panel(k, "hit-reader", &hit.title, 760.0, header, body)
        .h_full()
        .max_h(680.0)
        .matched(format!("hit-{tool}-{index}"))
        .modal(true, close)
        .modal_click_outside(true);
    push_modal_layer(k, layer, "hit-reader", panel);
}

/// The settings dialog, hand-raised from the kit Dialog's parts so the header reads like
/// macOS System Settings: a centred title over grouped cards of rows.
fn settings_dialog(s: &Chat, k: &rune_kit::Kit, layer: &mut Vec<El>, portal: &mut Vec<El>) {
    if !s.settings_open {
        return;
    }
    let close = |s: &mut Chat| {
        s.settings_open = false;
        s.width_select_open = false;
    };
    let dismiss = IconButton::new(k.glyph.close)
        .variant(IconButtonVariant::Ghost)
        .label("Close")
        .on_press(close)
        .sized(k, satellite::Size::Small)
        .key("close");
    let header =
        modal_header(k, "settings-header", dialog_title(k, "Settings"), 32.0).child(dismiss);
    let panel = modal_panel(
        k,
        "settings-dialog",
        "Settings",
        MODAL_MEDIUM,
        header,
        // The body scrolls only when the window is too short for it: the panel shrinks to the
        // window and the body takes what's left.
        col()
            .key("settings-body")
            .w_full()
            .shrink()
            .min_h(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(settings_body(s, k, portal)),
    )
    .shrink()
    .min_h(0.0)
    .modal(true, close)
    .modal_click_outside(true)
    .token_motion(k.motion.overlay);
    push_modal_layer(k, layer, "settings", panel);
}

/// The first-run dialog: credentials for the Agent Studio API, asked once, stored with the
/// rest of the state. It only goes away by saving or choosing the mock agent.
fn credentials_dialog(s: &Chat, k: &rune_kit::Kit, layer: &mut Vec<El>) {
    let first_run = s.needs_credentials();
    if !first_run && !s.add_form.open {
        return;
    }
    let title = if first_run {
        "Connect to Algolia"
    } else {
        "Add application"
    };
    let app_field = text_input(&s.add_form.app_id, k.ty.ui)
        .key("app-id-field")
        .w_full()
        .color(k.c.text_base)
        .placeholder("e.g. ABC123XYZ", k.c.text_faint)
        .label("Application ID")
        .on_edit(|s: &mut Chat| &mut s.add_form.app_id);
    let key_field = text_input(&s.add_form.api_key, k.ty.ui)
        .key("api-key-field")
        .w_full()
        .secure()
        .color(k.c.text_base)
        .placeholder("••••••••••••", k.c.text_faint)
        .label("Search API key")
        .on_edit(|s: &mut Chat| &mut s.add_form.api_key)
        .on_submit(|s: &mut Chat, _: &mut Tx| s.submit_credentials());
    let name_field = text_input(&s.add_form.label, k.ty.ui)
        .key("application-label-field")
        .w_full()
        .color(k.c.text_base)
        .placeholder("Defaults to the application ID", k.c.text_faint)
        .label("Name")
        .on_edit(|s: &mut Chat| &mut s.add_form.label)
        .on_submit(|s: &mut Chat, _: &mut Tx| s.submit_credentials());
    let blank =
        s.add_form.app_id.text.trim().is_empty() || s.add_form.api_key.text.trim().is_empty();
    let mut buttons = row()
        .key("credentials-buttons")
        .w_full()
        .gap(8.0)
        .center_y();
    if first_run && crate::chat::MOCK_AVAILABLE {
        buttons = buttons.child(
            satellite::button("Use mock agent")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Chat| s.use_mock())
                .sized(k, satellite::Size::Medium)
                .key("credentials-mock"),
        );
    }
    buttons = buttons.child(spacer());
    if !first_run {
        buttons = buttons.child(
            satellite::button("Cancel")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Chat| s.cancel_add_application())
                .sized(k, satellite::Size::Medium)
                .key("credentials-cancel"),
        );
    }
    buttons = buttons.child(
        satellite::button("Save")
            .variant(ButtonVariant::Submit)
            .disabled(blank)
            .on_press(|s: &mut Chat| s.submit_credentials())
            .sized(k, satellite::Size::Medium)
            .key("credentials-save"),
    );
    let mut body = col()
        .w_full()
        .padding(24.0)
        .gap(16.0)
        .child(
            text("Connect as-gui to an Algolia application. The key needs the search, settings and logs ACLs; both are stored on this machine only.")
                .wrap()
                .font(k.ty.ui)
                .color(k.c.text_muted),
        )
        .child(
            rune_kit::Field::new()
                .label("Application ID")
                .control(field_box(k, app_field))
                .build(k),
        )
        .child(
            rune_kit::Field::new()
                .label("Search API key")
                .control(field_box(k, key_field))
                .build(k),
        )
        .child(
            rune_kit::Field::new()
                .label("Name (optional)")
                .control(field_box(k, name_field))
                .build(k),
        );
    if let Some(error) = &s.add_form.error {
        body = body.child(
            text(error)
                .key("credentials-error")
                .font(k.ty.small)
                .color(k.c.danger.fg),
        );
    }
    let body = body.child(buttons);
    let header = modal_header(k, "credentials-header", dialog_title(k, title), 24.0);
    let panel = modal_panel(k, "credentials-dialog", title, MODAL_SMALL, header, body)
        .modal(true, move |s: &mut Chat| {
            if !first_run {
                s.cancel_add_application();
            }
        })
        .modal_click_outside(false)
        .token_motion(k.motion.overlay);
    push_modal_layer(k, layer, "credentials", panel);
}

/// A modal panel with the overlay shadow; the shell's dialogs are hand-raised so their
/// headers can differ from the kit Dialog's.
pub(crate) fn modal_panel(
    k: &rune_kit::Kit,
    key: &str,
    label: &str,
    width: f32,
    header: El,
    body: El,
) -> El {
    let mut panel = col()
        .key(key.to_string())
        .role("dialog")
        .label(label.to_string())
        .w_full()
        .max_w(width)
        .radius(RADIUS_MODAL)
        .fill(k.c.layer_01)
        .transition(modal_entrance(k))
        .child(header)
        .child(body);
    for shadow in crate::satellite::shadow(k, Level::Z500) {
        panel = panel.box_shadow(shadow);
    }
    panel
}

/// Satellite's modal entrance: up 8 points from 94 % scale as it fades in.
fn modal_entrance(k: &rune_kit::Kit) -> Transition {
    Transition::scale(0.94)
        .combined(Transition::offset(0.0, 8.0))
        .springs(SpringPair::with_exit_ratio(k.motion.overlay, 0.8))
}

/// A Satellite modal header: the title on the leading edge, optional controls trailing, over
/// a hairline.
fn modal_header(k: &rune_kit::Kit, key: &str, title: El, px: f32) -> El {
    row()
        .key(key.to_string())
        .w_full()
        .min_h(56.0)
        .px(px)
        .py(8.0)
        .gap(8.0)
        .center_y()
        .border_sides([0.0, 0.0, 1.0, 0.0], k.c.border_muted)
        .child(title.grow(1.0).min_w(0.0))
}

fn push_scrim(k: &rune_kit::Kit, layer: &mut Vec<El>, key: &str) {
    layer.push(
        el().key(format!("{key}-scrim"))
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(k.c.scrim)
            .token_motion(k.motion.overlay)
            .a11y_hidden(),
    );
}

/// The scrim and centring layer a modal panel draws in.
fn push_modal_layer(k: &rune_kit::Kit, layer: &mut Vec<El>, key: &str, panel: El) {
    push_scrim(k, layer, key);
    layer.push(
        col()
            .key(format!("{key}-layer"))
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(16.0)
            .center()
            .child(panel),
    );
}

/// [`push_modal_layer`] with the panel `top` points below the window's top edge instead of
/// centred, as a launcher sits.
pub(crate) fn push_top_modal_layer(
    k: &rune_kit::Kit,
    layer: &mut Vec<El>,
    key: &str,
    panel: El,
    top: f32,
) {
    push_scrim(k, layer, key);
    layer.push(
        col()
            .key(format!("{key}-layer"))
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(16.0)
            .pt(top)
            .items_center()
            .child(panel),
    );
}

/// Satellite's text field box around a bare `text_input`: page fill, the kit's field border,
/// radius 4, and the focus colour as its border while it has focus.
pub(crate) fn field_box(k: &rune_kit::Kit, input: El) -> El {
    let mut field = col()
        .w_full()
        .min_h(32.0)
        .px(12.0)
        .py(6.0)
        .justify_center()
        .radius(RADIUS_CONTROL)
        .fill(k.c.bg_base)
        .focus_within_ring(1.0, -1.0, k.c.border_focus)
        .focus_within_flat();
    for shadow in k.shadow_layers(k.elevation.input()) {
        field = field.box_shadow(shadow);
    }
    field.child(input)
}

fn dialog_title(k: &rune_kit::Kit, title: &str) -> El {
    text(title).font(HEADING).color(k.c.text_base).truncate()
}

const WIDTH_OPTIONS: [&str; 2] = ["Comfortable", "Wide"];

fn width_from_choice(choice: &str) -> ChatWidth {
    if choice == "Wide" {
        ChatWidth::Wide
    } else {
        ChatWidth::Comfortable
    }
}

fn settings_body(s: &Chat, k: &rune_kit::Kit, portal: &mut Vec<El>) -> El {
    let general = cfg!(target_os = "macos").then(|| {
        section(
            k,
            "General",
            card(k).child(settings_row(
                k,
                OPEN_AT_LAUNCH_LABEL,
                open_at_launch_control(s, k),
            )),
        )
    });
    col()
        .w_full()
        .padding(32.0)
        .pt(24.0)
        .gap(24.0)
        .child(section(
            k,
            "Appearance",
            card(k)
                .child(settings_row(k, "Theme", theme_control(s, k)))
                .child(row_separator(k))
                .child(settings_row(k, "Accent colour", accent_swatches(s, k)))
                .child(row_separator(k))
                .child(settings_row(k, "Chat width", width_control(s, k, portal)))
                .child(row_separator(k))
                .child(settings_row(k, TINT_LABEL, tint_control(s, k))),
        ))
        .children(general)
        .child(section(k, "Applications", applications_card(s, k)))
        .child(section(
            k,
            "Notifications",
            card(k).child(settings_row(k, NOTIFY_LABEL, notify_control(s, k))),
        ))
        .child(section(k, "Keyboard shortcuts", keybinds_card(s, k)))
        .child(
            row().w_full().child(spacer()).child(
                satellite::button("Restore defaults")
                    .disabled(!s.keybinds_customised())
                    .on_press(|s: &mut Chat| s.restore_default_keybinds())
                    .sized(k, satellite::Size::Small)
                    .key("keybinds-restore"),
            ),
        )
}

const TINT_LABEL: &str = "Tint the window with the application's colour";

fn tint_control(s: &Chat, k: &rune_kit::Kit) -> El {
    Switch::new(s.appearance.tint_window)
        .label(TINT_LABEL)
        .hide_label()
        .on_toggle(|s: &mut Chat| s.set_tint_window(!s.appearance.tint_window))
        .build(k)
        .key("tint-window")
}

const OPEN_AT_LAUNCH_LABEL: &str = "Open the window at launch";

fn open_at_launch_control(s: &Chat, k: &rune_kit::Kit) -> El {
    Switch::new(s.appearance.open_at_launch)
        .label(OPEN_AT_LAUNCH_LABEL)
        .hide_label()
        .on_toggle(|s: &mut Chat| s.set_open_at_launch(!s.appearance.open_at_launch))
        .build(k)
        .key("open-at-launch")
}

const NOTIFY_LABEL: &str = "Notify when a reply finishes in the background";

fn notify_control(s: &Chat, k: &rune_kit::Kit) -> El {
    Switch::new(s.notify_replies)
        .label(NOTIFY_LABEL)
        .hide_label()
        .on_toggle(|s: &mut Chat| s.set_notify_replies(!s.notify_replies))
        .build(k)
        .key("notify-replies")
}

/// An application's colour as a small decorative dot; its name always shows beside it.
pub(crate) fn colour_dot(colour: AppColour, k: &rune_kit::Kit, size: f32) -> El {
    el().size(size, size)
        .radius(size / 2.0)
        .fill(colour.color(k.dark))
        .a11y_hidden()
}

/// The application's dot in settings: a button that opens its swatch row.
fn colour_button(s: &Chat, k: &rune_kit::Kit, i: usize, colour: AppColour) -> El {
    let open = s.colour_picker == Some(i);
    el().key(format!("application-colour-{i}"))
        .role("button")
        .label(format!("Colour: {}", colour.label()))
        .expanded(open)
        .focusable(true)
        .size(22.0, 22.0)
        .radius(11.0)
        .center()
        .on_press(move |s: &mut Chat| s.toggle_colour_picker(i))
        .child(colour_dot(colour, k, 12.0))
}

/// The colours application `i` can take, as the accent picker shows its own.
fn colour_swatches(s: &Chat, k: &rune_kit::Kit, i: usize) -> El {
    let current = s.applications.get(i).map(|a| a.colour);
    row()
        .key(format!("application-colours-{i}"))
        .role("radiogroup")
        .label("Application colour")
        .gap(8.0)
        .pt(4.0)
        .center_y()
        .children(AppColour::ALL.map(|colour| {
            let selected = current == Some(colour);
            let fill = colour.color(k.dark);
            let mut swatch = el()
                .key(format!("application-colour-{i}-{}", colour.id()))
                .role("radio")
                .label(colour.label())
                .focusable(true)
                .focus_selected(selected)
                .size(24.0, 24.0)
                .radius(12.0)
                .center()
                .on_press(move |s: &mut Chat| s.set_application_colour(i, colour));
            if selected {
                swatch = swatch.border(2.0, fill);
            }
            swatch.child(el().size(16.0, 16.0).radius(8.0).fill(fill))
        }))
}

/// One row per saved application: its colour, name (editable), which is active, and removal.
fn applications_card(s: &Chat, k: &rune_kit::Kit) -> El {
    let mut card = card(k);
    for (i, application) in s.applications.iter().enumerate() {
        if i > 0 {
            card = card.child(row_separator(k));
        }
        let active = s.active == Some(i);
        let mut head =
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(colour_button(s, k, i, application.colour));
        if let Some(edit) = s.label_edits.get(i) {
            head = head.child(
                text_input(edit, k.ty.ui)
                    .key(format!("application-name-{i}"))
                    .grow(1.0)
                    .min_w(0.0)
                    .color(k.c.text_base)
                    .placeholder(&application.app_id, k.c.text_faint)
                    .label("Application name")
                    .on_edit(move |s: &mut Chat| &mut s.label_edits[i]),
            );
        }
        head = head.child(if active {
            Badge::new("Active").build(k)
        } else {
            satellite::button("Switch")
                .variant(ButtonVariant::Ghost)
                .on_press(move |s: &mut Chat| s.switch_application(i))
                .sized(k, satellite::Size::Small)
                .key(format!("application-switch-{i}"))
        });
        head = head.child(
            satellite::button("Remove")
                .variant(ButtonVariant::Ghost)
                .on_press(move |s: &mut Chat| s.remove_application(i))
                .sized(k, satellite::Size::Small)
                .key(format!("application-remove-{i}")),
        );
        let mut details = col()
            .key(format!("application-row-{i}"))
            .w_full()
            .px(16.0)
            .py(12.0)
            .gap(4.0)
            .child(head)
            .child(
                text(format!(
                    "{} · {}",
                    application.app_id,
                    application.masked_key()
                ))
                .font(k.ty.small)
                .color(k.c.text_muted),
            );
        if s.colour_picker == Some(i) {
            details = details.child(colour_swatches(s, k, i));
        }
        card = card.child(details);
    }
    if !s.applications.is_empty() {
        card = card.child(row_separator(k));
    }
    card.child(
        row().w_full().px(16.0).py(8.0).child(spacer()).child(
            satellite::button("Add application…")
                .on_press(|s: &mut Chat| {
                    s.add_form.error = None;
                    s.add_form.open = true;
                })
                .sized(k, satellite::Size::Small)
                .key("applications-add"),
        ),
    )
}

/// One row per rebindable action: its recorder, and a reset when it isn't the default.
fn keybinds_card(s: &Chat, k: &rune_kit::Kit) -> El {
    let mut card = card(k);
    for (n, action) in Action::ALL.into_iter().enumerate() {
        if n > 0 {
            card = card.child(row_separator(k));
        }
        card = card.child(settings_row(
            k,
            action.label(),
            keybind_control(s, k, action),
        ));
        if let Some(warning) = s.shortcut_warning(action) {
            card = card.child(
                col()
                    .key(format!("keybind-{}-warning", action.id()))
                    .w_full()
                    .px(16.0)
                    .pb(12.0)
                    .child(text(warning).font(k.ty.small).color(k.c.danger.fg).wrap()),
            );
        }
        if action == Action::AskSelection
            && let Some(access) = accessibility_row(s, k)
        {
            card = card.child(row_separator(k)).child(access);
        }
    }
    card
}

const ACCESSIBILITY_HELP: &str = "Ask about selection copies the selected text with ⌘C, which \
macOS allows only with Accessibility access. Without it, it quotes the clipboard instead.";

/// Whether macOS lets Ask about selection copy from other apps, and a way to ask for it.
fn accessibility_row(s: &Chat, k: &rune_kit::Kit) -> Option<El> {
    let granted = match s.selection.access() {
        Access::NotNeeded => return None,
        Access::Granted => true,
        Access::NotGranted => false,
    };
    let mut control = row().gap(8.0).center_y();
    if !granted {
        control = control.child(
            satellite::button("Grant access…")
                .on_press(|s: &mut Chat| s.selection.request_access())
                .sized(k, satellite::Size::Small)
                .key("accessibility-grant"),
        );
    }
    control = control.child(Badge::new(if granted { "Granted" } else { "Not granted" }).build(k));
    let mut status = col()
        .key("accessibility-status")
        .w_full()
        .child(settings_row(k, "Accessibility access", control));
    if !granted {
        status = status.child(
            col().w_full().px(16.0).pb(12.0).child(
                text(ACCESSIBILITY_HELP)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap(),
            ),
        );
    }
    Some(status)
}

fn keybind_control(s: &Chat, k: &rune_kit::Kit, action: Action) -> El {
    let index = action.index();
    let note = s.keybind_note.clone();
    let recorder = ShortcutRecorder::new(
        format!("keybind-{}", action.id()),
        &s.recorders[index],
        move |s: &mut Chat| &mut s.recorders[index],
    )
    .label(action.label())
    .width(150.0)
    .on_change(move |s: &mut Chat, next, _: &mut Tx| s.rebind(action, next))
    .error_message(move |error, chord| {
        note.clone()
            .unwrap_or_else(|| shortcut_error_message(error, chord))
    })
    .build(k);
    let recorder = if s.shortcut_warning(action).is_some() {
        row()
            .radius(k.radius.md + 1.0)
            .border(1.0, k.c.danger.border)
            .child(recorder)
    } else {
        recorder
    };
    let mut control = row().gap(8.0).center_y();
    if !s.keymap.is_default(action) {
        control = control.child(
            satellite::button("Reset")
                .variant(ButtonVariant::Ghost)
                .on_press(move |s: &mut Chat| s.reset_keybind(action))
                .sized(k, satellite::Size::Small)
                .key(format!("keybind-reset-{}", action.id())),
        );
    }
    control.child(recorder)
}

/// A settings section: its name over a card of rows, as macOS System Settings groups them.
fn section(k: &rune_kit::Kit, name: &str, card: El) -> El {
    col()
        .key(format!("section-{name}"))
        .w_full()
        .gap(8.0)
        .child(
            text(name.to_uppercase())
                .label(name)
                .font(SUBHEADING)
                .color(k.c.text_muted),
        )
        .child(card)
}

fn card(k: &rune_kit::Kit) -> El {
    let mut card = col()
        .key("card")
        .w_full()
        .radius(RADIUS_CARD)
        .fill(k.c.layer_01);
    for shadow in crate::satellite::shadow(k, Level::Z100) {
        card = card.box_shadow(shadow);
    }
    card
}

/// A labelled setting with its control on the trailing edge.
fn settings_row(k: &rune_kit::Kit, label: &str, control: El) -> El {
    row()
        .key(format!("row-{label}"))
        .w_full()
        .min_h(44.0)
        .px(16.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .child(text(label).font(BODY).color(k.c.text_base))
        .child(spacer())
        .child(control)
}

fn row_separator(k: &rune_kit::Kit) -> El {
    col()
        .px(16.0)
        .child(el().w_full().h(1.0).fill(k.c.border_muted))
}

fn theme_control(s: &Chat, k: &rune_kit::Kit) -> El {
    let current = match s.appearance.theme {
        ThemePref::Auto => 0,
        ThemePref::Light => 1,
        ThemePref::Dark => 2,
    };
    SegmentedControl::new(&ThemePref::LABELS, Some(current))
        .label("Theme")
        .fit()
        .on_pick(|s: &mut Chat, i: usize, _: &mut Tx| {
            s.set_theme(match i {
                1 => ThemePref::Light,
                2 => ThemePref::Dark,
                _ => ThemePref::Auto,
            });
        })
        .build(k)
        .key("theme-segments")
}

fn accent_swatches(s: &Chat, k: &rune_kit::Kit) -> El {
    row()
        .key("accent-swatches")
        .role("radiogroup")
        .label("Accent colour")
        .gap(12.0)
        .center_y()
        .children(Accent::ALL.map(|accent| accent_swatch(s, accent, k)))
}

fn accent_swatch(s: &Chat, accent: Accent, k: &rune_kit::Kit) -> El {
    let selected = s.appearance.accent == accent;
    let fill = crate::satellite::tones(accent, k.dark).fill;
    let mut swatch = el()
        .key(format!("accent-{}", accent.label().to_ascii_lowercase()))
        .role("radio")
        .label(accent.label())
        .focusable(true)
        .focus_selected(selected)
        .size(26.0, 26.0)
        .radius(13.0)
        .center()
        .on_press(move |s: &mut Chat| s.set_accent(accent));
    if selected {
        swatch = swatch.border(2.0, fill);
    }
    swatch.child(el().size(18.0, 18.0).radius(9.0).fill(fill))
}

fn width_control(s: &Chat, k: &rune_kit::Kit, portal: &mut Vec<El>) -> El {
    let current = match s.appearance.width {
        ChatWidth::Comfortable => "Comfortable",
        ChatWidth::Wide => "Wide",
    };
    // The select's listbox is a portal: it mounts into `portal`, which the view appends after
    // the dialog so the trigger (inside the panel) precedes it and the anchor resolves.
    Select::new("chat-width", &WIDTH_OPTIONS)
        .appearance(rune_kit::SelectAppearance::Field)
        .width(170.0)
        .placement(rune_kit::Placement::BottomStart)
        .label("Chat width")
        .current(Some(current))
        .open(s.settings_open && s.width_select_open)
        .on_toggle(|s: &mut Chat| s.width_select_open = !s.width_select_open)
        .on_close(|s: &mut Chat| s.width_select_open = false)
        .on_choose(|s: &mut Chat, v: &str, _: &mut Tx| {
            s.set_width(width_from_choice(v));
            s.width_select_open = false;
        })
        .build(k, portal)
}

/// The collapsed sidebar: a narrow rail of icons — new session, then one chip per session —
/// shown when the sidebar toggles away, so sessions stay one click deep.
fn sidebar_rail(s: &Chat, k: &rune_kit::Kit) -> El {
    let mut sessions = col().key("rail-sessions").gap(8.0).items_center();
    let listed = match s.sidebar_view(s.now) {
        SidebarView::Blank | SidebarView::Skeleton => Vec::new(),
        _ => visible_session_indices(s),
    };
    for i in listed {
        let session = &s.sessions[i];
        let selected = i == s.current;
        let mut button = el()
            .key(format!("rail-session-{i}"))
            .role("button")
            .label(&session.title)
            .size(36.0, 36.0)
            .radius(RADIUS_CONTROL)
            .center()
            .help(&session.title)
            .on_press(move |s: &mut Chat| s.select_session(i));
        button = if selected {
            button.fill(k.c.hover).border(1.0, k.c.border_focus)
        } else {
            button.hover(k.c.hover)
        };
        sessions = sessions.child(button.child(letter_chip(&session.title, k, 18.0)));
    }
    col()
        .key("sidebar-rail")
        .w(56.0)
        .h_full()
        .fill(k.c.bg_deep)
        .items_center()
        .pt(12.0)
        .pb(12.0)
        .gap(12.0)
        .child(
            IconButton::new(&icons::PLUS)
                .label("New conversation")
                .on_press(|s: &mut Chat| s.new_session())
                .sized(k, satellite::Size::Medium)
                .key("rail-new-session")
                .help("New conversation"),
        )
        .child(
            col()
                .key("rail-scroll")
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(sessions),
        )
        .child(
            IconButton::new(&icons::SETTINGS)
                .label("Settings")
                .on_press(|s: &mut Chat| s.settings_open = true)
                .sized(k, satellite::Size::Medium)
                .key("rail-settings")
                .help(hinted("Settings", &s.keymap.hint(Action::Settings))),
        )
}

fn breadcrumb(k: &rune_kit::Kit, name: &str, title: &str) -> El {
    row()
        .key("breadcrumb")
        .gap(8.0)
        .center_y()
        .child(letter_chip(name, k, 18.0))
        .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
}

/// The colored square with a leading letter the sidebar rows and breadcrumb carry.
pub(crate) fn letter_chip(source: &str, k: &rune_kit::Kit, size: f32) -> El {
    let letter = source
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('•');
    el().size(size, size)
        .radius((size * 0.25).min(RADIUS_CARD))
        .fill(chip_fill(source))
        .center()
        .child(
            text(letter.to_string())
                .font(if size > 24.0 {
                    k.ty.ui_medium
                } else {
                    k.ty.ui_compact
                })
                .color(k.c.white),
        )
}

/// Satellite's planet colours, with Jupiter and Saturn swapped for the green and orange deep
/// enough to carry a white letter.
fn chip_fills() -> [Color; 8] {
    [
        Planet::Neptune.at(1),
        Planet::Mercury.at(1),
        Hue::Green.at(700),
        Hue::Orange.at(700),
        Planet::Mars.at(1),
        Planet::Venus.at(1),
        Hue::Xenon.at(600),
        Hue::Purple.at(600),
    ]
}

fn chip_fill(source: &str) -> Color {
    let fills = chip_fills();
    fills[source.bytes().map(|b| b as usize).sum::<usize>() % fills.len()]
}

/// How many conversations the collapsed sidebar and rail show.
const RECENT_SESSIONS: usize = 5;

/// Which sessions the sidebar and rail show: when expanded, everything; else the recent
/// non-hidden ones, always including the session in use.
fn visible_session_indices(s: &Chat) -> Vec<usize> {
    const RECENT: usize = RECENT_SESSIONS;
    let expanded = s.show_all_sessions || s.show_hidden;
    let mut visible: Vec<usize> = (0..s.sessions.len())
        .filter(|&i| expanded || !s.sessions[i].hidden)
        .collect();
    if !expanded && !visible.contains(&s.current) && s.sessions.get(s.current).is_some() {
        visible.push(s.current);
    }
    if visible.len() > RECENT && !expanded {
        visible.truncate(RECENT);
        if !visible.contains(&s.current) && s.sessions.get(s.current).is_some() {
            visible.push(s.current);
        }
    }
    visible
}

fn session_sidebar(s: &Chat, k: &rune_kit::Kit) -> El {
    let list = s.sidebar_view(s.now);
    let visible = if matches!(list, SidebarView::Blank | SidebarView::Skeleton) {
        Vec::new()
    } else {
        visible_session_indices(s)
    };
    let hidden_count = s.sessions.iter().filter(|s| s.hidden).count();
    let mut section = SidebarSection::new("Conversations");
    if matches!(list, SidebarView::Sessions | SidebarView::Failed(_)) {
        section = section.count(s.sessions.len() - hidden_count);
    }
    let mut section = section.rows(visible.into_iter().map(|i| {
        let sess = &s.sessions[i];
        let mut row = SidebarRow::new(&sess.title)
            .avatar(letter_chip(&sess.title, k, 18.0).key(format!("session-chip-{i}")))
            .selected(i == s.current)
            .on_select(move |s: &mut Chat, _| s.select_session(i));
        row = if sess.hidden {
            row.subtitle("hidden — select to restore")
        } else {
            // The hover-revealed × hides the conversation; its data stays.
            row.on_close(move |s: &mut Chat, _| s.hide_session(i))
        };
        row.build(k).key(format!("session-{i}"))
    }));
    if list == SidebarView::Skeleton {
        section = section.row(
            col()
                .key("sessions-skeleton")
                .w_full()
                .px(12.0)
                .py(8.0)
                .gap(12.0)
                .transition(Transition::opacity())
                .children(
                    [150.0, 120.0, 168.0, 104.0]
                        .map(|w| conversation::skeleton_bar(k, w, 14.0, s.now)),
                ),
        );
    }
    if let SidebarView::Failed(error) = &list {
        section = section.row(
            col()
                .key("sessions-failed")
                .w_full()
                .px(12.0)
                .py(8.0)
                .gap(8.0)
                .child(
                    text("Couldn't load conversations")
                        .font(k.ty.small)
                        .color(k.c.text_base),
                )
                .child(text(error).font(k.ty.small).color(k.c.text_muted).wrap())
                .child(
                    satellite::button("Retry")
                        .on_press(|s: &mut Chat| s.retry_sessions())
                        .sized(k, satellite::Size::Small)
                        .key("sessions-retry"),
                ),
        );
    }
    if s.sessions.len() > RECENT_SESSIONS {
        let label = if s.show_all_sessions {
            "Show recent only".to_string()
        } else {
            format!("View all {} conversations", s.sessions.len())
        };
        section = section.row(
            row()
                .key("sessions-toggle")
                .role("button")
                .label(&label)
                .w_full()
                .h(26.0)
                .px(12.0)
                .center_y()
                .radius(RADIUS_CONTROL)
                .hover(k.c.hover)
                .focusable(true)
                .on_press(|s: &mut Chat| s.show_all_sessions = !s.show_all_sessions)
                .child(text(label).font(k.ty.small).color(k.c.text_muted)),
        );
    }
    let mut footer = col().w_full();
    if hidden_count > 0 {
        let label = if s.show_hidden {
            "Hide hidden conversations".to_string()
        } else {
            format!("Show {} hidden", hidden_count)
        };
        footer = footer.child(
            row()
                .key("hidden-toggle")
                .role("button")
                .label(&label)
                .w_full()
                .h(26.0)
                .px(12.0)
                .center_y()
                .radius(RADIUS_CONTROL)
                .hover(k.c.hover)
                .focusable(true)
                .on_press(|s: &mut Chat| s.show_hidden = !s.show_hidden)
                .child(text(label).font(k.ty.small).color(k.c.text_muted)),
        );
    }
    Sidebar::new()
        .header(
            satellite::button("New conversation")
                .icon(&icons::PLUS)
                .on_press(|s: &mut Chat| s.new_session())
                .sized(k, satellite::Size::Medium)
                .key("new-session"),
        )
        .section(section)
        .footer(footer.child(settings_footer(k, &s.keymap.hint(Action::Settings))))
        .build(k)
}

/// The sidebar's last item: opens the settings dialog, with its keybind as the hint.
fn settings_footer(k: &rune_kit::Kit, hint: &str) -> El {
    row()
        .key("settings-footer")
        .role("button")
        .label("Settings")
        .w_full()
        .h(30.0)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .radius(RADIUS_CONTROL)
        .hover(k.c.hover)
        .focusable(true)
        .help(hinted("Settings", hint))
        .on_press(|s: &mut Chat| s.settings_open = true)
        .child(icon(&icons::SETTINGS, 15.0).color(k.c.icon_muted))
        .child(
            text("Settings")
                .font(k.ty.ui)
                .color(k.c.text_base)
                .grow(1.0),
        )
        .child(text(hint).font(k.ty.small).color(k.c.text_faint))
}

/// `SidebarToggle::shortcut` wants a `'static` hint; each distinct chord is leaked once.
fn interned(text: String) -> &'static str {
    static SEEN: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(known) = seen.iter().find(|k| **k == text) {
        return known;
    }
    let leaked: &'static str = Box::leak(text.into_boxed_str());
    seen.push(leaked);
    leaked
}

/// A tooltip: the name, then its shortcut when it has one.
fn hinted(name: &str, hint: &str) -> String {
    if hint.is_empty() {
        name.to_string()
    } else {
        format!("{name} {hint}")
    }
}

fn agent_row(k: &rune_kit::Kit, agent: &AgentInfo, index: usize) -> El {
    let published = agent.status == AgentStatus::Published;
    let mut info =
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(2.0)
            .child(text(&agent.name).font(k.ty.ui_medium).color(if published {
                k.c.text_base
            } else {
                k.c.text_faint
            }));
    if let Some(description) = &agent.description {
        info = info.child(text(description).wrap().color(k.c.text_muted));
    }
    if let Some(model) = &agent.model {
        info = info.child(text(model).color(k.c.text_faint));
    }
    let mut row = row()
        .key(format!("agent-{}", agent.agent_id))
        .w_full()
        .gap(12.0)
        .center_y()
        .padding(12.0)
        .px(16.0)
        .radius(RADIUS_CARD)
        .child(info)
        .child(Badge::new(if published { "published" } else { "draft" }).build(k));
    if published {
        row = row.fill(k.c.layer_01);
        for shadow in crate::satellite::shadow(k, Level::Z100) {
            row = row.box_shadow(shadow);
        }
        row = row
            .role("button")
            .on_press(move |s: &mut Chat| s.choose_agent(index));
    }
    row
}

/// The window while the app works out where to open: nothing, then a spinner once it has been
/// a moment, so a fast launch never flashes a screen on its way to the real one.
fn splash(k: &rune_kit::Kit, spinner: bool) -> El {
    let mut body = col()
        .key("splash")
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .center()
        .transition(Transition::opacity());
    if spinner {
        body = body.child(Spinner::new().build(k));
    }
    body
}

fn picker_main(s: &Chat, k: &rune_kit::Kit, screen: &Screen) -> El {
    let mut pane = col()
        .key("agent-picker")
        .w_full()
        .h_full()
        .fill(k.c.bg_base);
    if let Screen::AgentsFailed(error) = screen {
        pane = pane.child(
            col()
                .grow(1.0)
                .center()
                .gap(12.0)
                .child(text("Couldn't load your agents").color(k.c.text_base))
                .child(text(error).color(k.c.text_muted))
                .child(
                    satellite::button("Retry")
                        .on_press(|s: &mut Chat| s.load_agents())
                        .sized(k, satellite::Size::Medium)
                        .key("agents-retry"),
                ),
        );
    } else {
        let list = col()
            .key("agent-list")
            .max_w(560.0)
            .w_full()
            .gap(8.0)
            .children(s.agents.iter().enumerate().map(|(i, a)| agent_row(k, a, i)));
        pane = pane.child(
            col()
                .key("agent-picker-pane")
                .grow(1.0)
                .center()
                .child(el().w(560.0).py(24.0).child(list)),
        );
    }
    pane
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::satellite::contrast;

    #[test]
    fn white_letters_read_on_every_chip_fill() {
        for fill in chip_fills() {
            let ratio = contrast(Color::WHITE, fill);
            assert!(ratio >= 3.0, "{} gives {ratio:.2}", fill.hex());
        }
    }

    #[test]
    fn chips_take_satellites_brand_colours() {
        let fills = chip_fills();
        assert_eq!(fills.len(), 8);
        assert!(fills.contains(&Planet::Neptune.at(1)));
        assert!(fills.contains(&Hue::Xenon.at(600)));
        assert!(fills.contains(&Hue::Purple.at(600)));
    }

    #[test]
    fn a_title_always_gets_the_same_chip_colour() {
        assert_eq!(chip_fill("Rune layout"), chip_fill("Rune layout"));
        let distinct: std::collections::HashSet<String> = ["a", "b", "c", "d", "e"]
            .map(|t| chip_fill(t).hex())
            .into_iter()
            .collect();
        assert!(distinct.len() > 1);
    }
}
