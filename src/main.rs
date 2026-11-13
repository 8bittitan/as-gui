//! as-gui: a chat front end for an Algolia Agent Studio agent, built on Rune and rune-kit.
//!
//! `cargo run` opens the window. On first launch a dialog asks for the Algolia credentials
//! (an Application ID and a search API key with `search`, `settings` and `logs` ACLs); they're stored
//! with the rest of the state in `state/as-gui-state.json`, and the app opens to the last
//! selected agent. `AS_GUI_MOCK=1` runs the mock agent without asking; `ALGOLIA_AGENT_ID`
//! binds an agent directly. `LOG_LEVEL` controls logging (stderr and
//! `logs/as-gui.log.YYYY-MM-DD`; default `warn`). Headless: `cargo run -- --tree tree.json`.

mod agent;
mod brand;
mod chat;
mod command_bar;
mod conversation;
mod dotenv;
mod glow;
mod keymap;
mod knobs;
mod loading;
mod logging;
mod menubar;
mod notify;
mod palette;
mod paths;
mod playground;
mod quick_ask;
mod satellite;
mod scenes;
mod search_panel;
mod selection;
mod shell;
mod store;
#[cfg(test)]
mod tests;
mod timeline;

use std::sync::Arc;

use rune::{App, AppSpec, RunOptions, Waker, WindowOptions, WindowSpec};

use agent::{AgentClient, AgentDirectory, AgentInfo, AgentSnapshot, AgentStatus};
use chat::Chat;
use quick_ask::QuickAsk;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn mock_parts() -> (Arc<dyn AgentClient>, Arc<dyn AgentDirectory>) {
    (agent::mock_client(), agent::mock_directory())
}

/// The deterministic state `AppSpec::init` hands to playground scenes (and `--scene` fallbacks);
/// the real app's state comes from the `App::init` closure instead.
fn mock_chat() -> Chat {
    let (client, directory) = mock_parts();
    Chat::new(client, directory)
}

type SceneFn = fn(&mut Chat);

/// The launch size: 70% of the screen's available width and 90% of its height (the screen
/// minus its menu bar and Dock), so the chat opens roomy on any display. The fixed size is
/// the fallback where the screen can't be read.
fn launch_size() -> (f32, f32) {
    const FALLBACK: (f32, f32) = (860.0, 600.0);
    match available_screen() {
        Some((width, height)) => (width * 0.7, height * 0.9),
        None => FALLBACK,
    }
}

/// The main screen's available width and height in points, outside the menu bar and Dock.
#[cfg(target_os = "macos")]
fn available_screen() -> Option<(f32, f32)> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;
    let screen = NSScreen::mainScreen(MainThreadMarker::new()?)?;
    let frame = screen.visibleFrame();
    Some((frame.size.width as f32, frame.size.height as f32))
}

#[cfg(not(target_os = "macos"))]
fn available_screen() -> Option<(f32, f32)> {
    None
}

const SCENES: &[(&str, SceneFn)] = &[
    ("agents", scenes::agents),
    ("conversation", scenes::conversation),
    ("empty", scenes::empty),
    ("welcome", scenes::welcome),
    ("knobs", scenes::knobs),
    ("reader", scenes::reader),
    ("splash", scenes::splash),
    ("loading-sessions", scenes::loading_sessions),
    ("loading-conversation", scenes::loading_conversation),
    ("sessions-failed", scenes::sessions_failed),
    ("streaming", scenes::streaming),
    ("glow", scenes::glow),
    ("tools", scenes::tools),
    ("timeline", scenes::timeline),
    ("tool-error", scenes::tool_error),
    ("search", scenes::search),
    ("settings", scenes::settings),
    ("settings-wide", scenes::settings_wide),
    ("settings-styled", scenes::settings_styled),
    ("applications", scenes::applications),
    ("application-menu", scenes::application_menu),
    ("command-bar", scenes::command_bar),
    ("command-bar-query", scenes::command_bar_query),
    ("application-colours", scenes::application_colours),
    (
        "application-colours-light",
        scenes::application_colours_light,
    ),
];

fn main() {
    dotenv::load_env_file();
    logging::init();
    // Windows takes its toast identity from this; macOS reads it from the bundle. Posting
    // itself waits until a notification is actually due, which is when permission is asked.
    let _ = rune_notify::register(&rune_notify::Identity::new(paths::BUNDLE_ID, "as-gui"));
    let mock = cfg!(debug_assertions) && env("AS_GUI_MOCK").is_some_and(|m| m == "1");
    let preset = env("ALGOLIA_AGENT_ID").map(|agent_id| AgentInfo {
        name: agent_id.clone(),
        agent_id,
        description: None,
        status: AgentStatus::Published,
        model: None,
    });
    let demo = agent::demo_mode();
    let state = if demo {
        store::Persisted::default()
    } else {
        store::load()
    };
    let launch = menubar::launch_mode(menubar::LaunchInputs {
        macos: cfg!(target_os = "macos"),
        headless: rune::is_headless(),
        demo,
        needs_credentials: !mock && state.applications.is_empty(),
        preset_agent: preset.is_some(),
        open_at_launch: state.open_at_launch.unwrap_or(false),
        menu_bar_demo: demo && env("AS_GUI_DEMO_MENU_BAR").is_some_and(|v| v == "1"),
    });
    let options = RunOptions {
        accessory: launch.status_item,
    };

    rune::run_windows_with(options, move |_displays| {
        let snapshot = Arc::new(std::sync::Mutex::new(AgentSnapshot::default()));
        let selection = selection::system();

        let mut main = App::new(AppSpec::<Chat> {
            scenes: SCENES,
            fonts: satellite::FONTS,
            families: satellite::FAMILIES,
            setup: Some(chat::setup),
            poll: Some(chat::poll),
            on_key: Some(shell::on_key),
            ..AppSpec::new("as-gui", launch_size(), mock_chat, shell::view)
        })
        .playground(playground::playground())
        .compositor(glow::Glow::default())
        .init({
            let snapshot = snapshot.clone();
            let selection = selection.clone();
            move || {
                let (client, directory) = mock_parts();
                let mut chat = Chat::new(client, directory);
                // Headless runs stay on the mock: deterministic, and no network on a snapshot.
                chat.mock_forced = mock || rune::is_headless();
                chat.persist_path = (!demo).then(paths::state_path);
                chat.selected_agent = preset;
                chat.snapshot = Some(snapshot.clone());
                chat.selection = selection;
                chat.restore(&state);
                if !chat.mock_forced {
                    chat.connect_active();
                }
                chat
            }
        })
        .chrome(rune::Chrome {
            traffic_lights: Some((16.0, 17.0)),
            ..rune::Chrome::DEFAULT
        })
        .keep_running()
        .on_close(|s, _| s.window_closed())
        .on_focus(chat::focus_changed)
        .on_reopen(|s, _, _| {
            s.open_window();
            true
        });
        if !launch.show_window {
            main = main.windowless();
        }

        let chat = main.handle();
        // A notification click arrives on a background thread: bring the app forward and
        // open the conversation the reply belongs to. Installed once, like Quick Ask's.
        let notify_target = chat.clone();
        rune_notify::on_click(move |click| {
            rune_notify::bring_to_front();
            if let (Some(agent), Some(session)) = (click.get("agent"), click.get("session")) {
                let (agent, session) = (agent.to_string(), session.to_string());
                notify_target.update(move |chat, _| chat.open_session_by_id(&agent, &session));
            }
        });
        let panel = App::new(AppSpec::<QuickAsk> {
            scenes: &[
                ("quick-ask", quick_ask::scene),
                ("quick-ask-error", quick_ask::error_scene),
                ("quick-ask-selection", quick_ask::selection_scene),
            ],
            fonts: satellite::FONTS,
            families: satellite::FAMILIES,
            setup: Some(|s: &mut QuickAsk, waker: Waker| s.waker = Some(waker)),
            poll: Some(quick_ask::poll),
            ..AppSpec::new(
                "Quick Ask",
                quick_ask::launch_size(),
                || unreachable!(),
                quick_ask::view,
            )
        })
        .windowless()
        .keep_running();
        let me = panel.handle();
        let shortcuts = panel.handle();
        let shell = menubar::RuneShell {
            main: chat.clone(),
            panel: panel.handle(),
        };
        let menu_target = chat.clone();
        chat.update(move |chat, _| {
            chat.attach_registrar(std::rc::Rc::new(quick_ask::PanelShortcuts::new(shortcuts)));
            chat.attach_shell(std::rc::Rc::new(shell));
            if launch.status_item {
                menubar::attach_status_item(chat, menu_target, launch.show_window);
            }
        });
        let quick_ask = panel
            .init(move || {
                let mut panel = QuickAsk::new(snapshot, Some(chat), Some(me));
                panel.selection = selection;
                panel
            })
            .on_focus(quick_ask::focus_changed);

        let windows: Vec<rune::AnyWindowSpec> = vec![
            WindowSpec::from_app("as-gui", main).into(),
            WindowSpec::from_app("quick-ask", quick_ask)
                .configured_options(WindowOptions::launcher_panel())
                .into(),
        ];
        windows
    });
}
