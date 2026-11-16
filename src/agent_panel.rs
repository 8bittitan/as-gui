//! The agent sheet: a modal panel pinned to the window's right edge with the open agent's
//! description, how each of its client tools is handled, and the tools it no longer declares.

use rune::{
    Edge, El, ScrollSpec, SpringPair, Transition, Tx, auto_grow, col, el, row, spacer, text,
    text_input,
};
use rune_kit::{ButtonVariant, IconButton, IconButtonVariant, Select, SelectAppearance};

use crate::agent::{AgentConfig, AgentStatus, ClientToolDecl, is_builtin};
use crate::chat::Chat;
use crate::chat::agent_panel::{ASK_LABEL, COMMAND_LABEL, handler_key};
use crate::conversation::skeleton_bar;
use crate::loading::Phase;
use crate::satellite::typography::{BODY, CODE};
use crate::satellite::{self, Fit, RADIUS_CONTROL, RADIUS_MODAL};
use crate::shell::{
    card, dialog_title, field_box, modal_header, modal_panel, push_scrim, row_separator, section,
};
use crate::store::ToolBinding;

const WIDTH: f32 = 400.0;
const PREVIEW_HEIGHT: f32 = 140.0;

pub fn sheet(s: &Chat, k: &rune_kit::Kit, layer: &mut Vec<El>, portal: &mut Vec<El>) {
    if s.agent_panel.is_none() {
        return;
    }
    let close = |s: &mut Chat| s.close_agent_panel();
    let dismiss = IconButton::new(k.glyph.close)
        .variant(IconButtonVariant::Ghost)
        .label("Close")
        .on_press(close)
        .sized(k, satellite::Size::Small)
        .key("agent-panel-close");
    let header = modal_header(k, "agent-panel-header", title(s, k), 16.0).child(dismiss);
    let name = s
        .selected_agent
        .as_ref()
        .map_or("Agent", |agent| agent.name.as_str());
    let body = col()
        .key("agent-panel-body")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(16.0)
        .gap(20.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(contents(s, k, portal));
    let sheet = modal_panel(
        k,
        "agent-panel",
        &format!("{name} settings"),
        WIDTH,
        header,
        body,
    )
    .w(WIDTH)
    .h_full()
    .corners([RADIUS_MODAL, 0.0, 0.0, RADIUS_MODAL])
    .transition(
        Transition::move_edge(Edge::Trailing)
            .springs(SpringPair::with_exit_ratio(k.motion.overlay, 0.8)),
    )
    .modal(true, close)
    .modal_click_outside(true);
    push_scrim(k, layer, "agent-panel");
    layer.push(
        row()
            .key("agent-panel-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .child(spacer())
            .child(sheet),
    );
}

fn title(s: &Chat, k: &rune_kit::Kit) -> El {
    let Some(agent) = &s.selected_agent else {
        return dialog_title(k, "Agent settings");
    };
    let status = match agent.status {
        AgentStatus::Published => "Published",
        AgentStatus::Draft => "Draft",
    };
    let dot = || {
        text("·")
            .font(k.ty.small)
            .color(k.c.text_faint)
            .a11y_hidden()
    };
    let mut facts = row().w_full().gap(8.0).center_y().wrap();
    facts = facts.child(text(status).font(k.ty.small).color(k.c.text_muted));
    if let Some(model) = &agent.model {
        facts = facts
            .child(dot())
            .child(text(model).font(k.ty.small).color(k.c.text_muted));
    }
    if let Some(colour) = s.active_colour() {
        let application = s.application_label().unwrap_or_default();
        facts = facts.child(dot()).child(
            row()
                .key("agent-panel-application")
                .gap(4.0)
                .center_y()
                .child(crate::shell::colour_dot(colour, k, 8.0))
                .child(text(application).font(k.ty.small).color(k.c.text_muted)),
        );
    }
    col()
        .gap(2.0)
        .child(dialog_title(k, &agent.name))
        .child(facts)
}

fn contents(s: &Chat, k: &rune_kit::Kit, portal: &mut Vec<El>) -> El {
    let mut body = col().w_full().gap(20.0);
    match s.agent_config.load.phase(s.now) {
        Phase::Blank => {}
        Phase::Skeleton => {
            body = body.child(skeleton(s, k));
        }
        Phase::Failed(error) => {
            body = body.child(failure(k, &error));
        }
        Phase::Idle | Phase::Done => {
            if let Some(config) = &s.agent_config.config {
                body = body
                    .child(about(k, config))
                    .child(client_tools(s, k, config, portal))
                    .children(dropped_tools(s, k))
                    .child(server_tools(k, config));
            }
        }
    }
    body
}

fn skeleton(s: &Chat, k: &rune_kit::Kit) -> El {
    col()
        .key("agent-panel-skeleton")
        .w_full()
        .gap(12.0)
        .a11y_hidden()
        .child(skeleton_bar(k, 120.0, 14.0, s.now))
        .child(skeleton_bar(k, 320.0, 48.0, s.now))
        .child(skeleton_bar(k, 320.0, 96.0, s.now))
}

fn failure(k: &rune_kit::Kit, error: &str) -> El {
    col()
        .key("agent-panel-error")
        .w_full()
        .gap(8.0)
        .child(
            text(format!("Couldn't load the agent: {error}"))
                .wrap()
                .font(k.ty.small)
                .color(k.c.danger.fg),
        )
        .child(
            satellite::button("Try again")
                .variant(ButtonVariant::Outline)
                .on_press(|s: &mut Chat| s.retry_agent_config())
                .sized(k, satellite::Size::Small)
                .key("agent-panel-retry"),
        )
}

fn about(k: &rune_kit::Kit, config: &AgentConfig) -> El {
    let description = config
        .description
        .clone()
        .unwrap_or_else(|| "No description.".into());
    section(
        k,
        "About",
        col()
            .w_full()
            .px(4.0)
            .child(text(description).wrap().font(BODY).color(k.c.text_base)),
    )
}

fn client_tools(s: &Chat, k: &rune_kit::Kit, config: &AgentConfig, portal: &mut Vec<El>) -> El {
    let mut tools = card(k).key("agent-client-tools");
    if config.client_tools.is_empty() {
        tools = tools.child(
            row().w_full().padding(16.0).child(
                text("This agent declares no client tools.")
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            ),
        );
    }
    for (i, tool) in config.client_tools.iter().enumerate() {
        if i > 0 {
            tools = tools.child(row_separator(k));
        }
        tools = tools.child(tool_row(s, k, tool, portal));
    }
    section(k, "Client tools", tools)
}

fn tool_row(s: &Chat, k: &rune_kit::Kit, tool: &ClientToolDecl, portal: &mut Vec<El>) -> El {
    let name = tool.name.clone();
    let mut item = col()
        .key(format!("agent-tool-{name}"))
        .role("group")
        .label(name.clone())
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .child(text(&name).font(CODE).color(k.c.text_base).wrap_anywhere());
    if let Some(description) = &tool.description {
        item = item.child(
            text(description)
                .wrap()
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    }
    if let Some(schema) = &tool.input_schema {
        item = item.child(schema_preview(k, &name, schema));
    }
    let binding = s.tool_bindings().get(&name).cloned();
    let editing = s
        .agent_panel
        .as_ref()
        .is_some_and(|panel| panel.editing.as_deref() == Some(name.as_str()));
    item = item.child(handler_row(s, k, &name, binding.as_ref(), editing, portal));
    if let (Some(ToolBinding::Command { program, args, .. }), false) = (&binding, editing) {
        item = item.child(command_line(k, &name, program, args));
    }
    if editing {
        item = item.child(command_form(s, k));
    }
    item
}

fn schema_preview(k: &rune_kit::Kit, name: &str, schema: &serde_json::Value) -> El {
    let pretty = serde_json::to_string_pretty(schema).unwrap_or_default();
    el().key(format!("agent-tool-schema-{name}"))
        .role("region")
        .label(format!("Input schema of {name}"))
        .focusable(true)
        .w_full()
        .max_h(PREVIEW_HEIGHT)
        .radius(RADIUS_CONTROL)
        .fill(k.c.layer_02)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col().w_full().px(12.0).py(8.0).child(
                text(pretty)
                    .font(CODE)
                    .wrap_anywhere()
                    .color(k.c.text_muted),
            ),
        )
}

fn handler_row(
    s: &Chat,
    k: &rune_kit::Kit,
    name: &str,
    binding: Option<&ToolBinding>,
    editing: bool,
    portal: &mut Vec<El>,
) -> El {
    let mut handler = row().w_full().gap(12.0).center_y();
    if is_builtin(name) {
        return handler.child(
            text("Handled by the app (built-in)")
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    }
    let current = if editing || matches!(binding, Some(ToolBinding::Command { .. })) {
        COMMAND_LABEL
    } else {
        ASK_LABEL
    };
    let open = s
        .agent_panel
        .as_ref()
        .is_some_and(|panel| panel.select_open.as_deref() == Some(name));
    let select = Select::new(handler_key(name), &[ASK_LABEL, COMMAND_LABEL])
        .appearance(SelectAppearance::Field)
        .width(140.0)
        .placement(rune_kit::Placement::BottomStart)
        .label(format!("Handler for {name}"))
        .current(Some(current))
        .open(open)
        .on_toggle({
            let name = name.to_string();
            move |s: &mut Chat| s.toggle_handler_select(&name)
        })
        .on_close(|s: &mut Chat| s.close_handler_select())
        .on_choose({
            let name = name.to_string();
            move |s: &mut Chat, choice: &str, _: &mut Tx| s.choose_handler(&name, choice)
        })
        .build(k, portal);
    handler = handler.child(select);
    if matches!(binding, Some(ToolBinding::Command { .. })) && !editing {
        let target = name.to_string();
        handler = handler.child(spacer()).child(
            satellite::button("Edit")
                .variant(ButtonVariant::Ghost)
                .on_press(move |s: &mut Chat| s.edit_binding(&target))
                .sized(k, satellite::Size::Small)
                .label(format!("Edit the command for {name}"))
                .key(format!("agent-tool-edit-{name}")),
        );
    } else if binding.is_none() {
        handler = handler.child(
            text("Asks you each time")
                .key(format!("agent-tool-summary-{name}"))
                .grow(1.0)
                .min_w(0.0)
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    }
    handler
}

/// The command a tool runs, as a monospace block of its own so a long path or argument
/// wraps inside it instead of squeezing the select beside it.
fn command_line(k: &rune_kit::Kit, name: &str, program: &str, args: &[String]) -> El {
    row()
        .key(format!("agent-tool-command-{name}"))
        .role("group")
        .label(format!("Command for {name}"))
        .w_full()
        .min_w(0.0)
        .px(12.0)
        .py(8.0)
        .gap(8.0)
        .radius(RADIUS_CONTROL)
        .fill(k.c.layer_02)
        .child(text("$").font(CODE).color(k.c.text_faint).a11y_hidden())
        .child(
            text(crate::conversation::display_command(program, args))
                .key(format!("agent-tool-summary-{name}"))
                .grow(1.0)
                .min_w(0.0)
                .wrap_anywhere()
                .font(CODE)
                .color(k.c.text_base),
        )
}

fn command_form(s: &Chat, k: &rune_kit::Kit) -> El {
    let draft = &s.binding_draft;
    let program = text_input(&draft.program, k.ty.ui)
        .key("agent-binding-program")
        .w_full()
        .color(k.c.text_base)
        .placeholder("/usr/local/bin/tool", k.c.text_faint)
        .label("Program")
        .on_edit(|s: &mut Chat| &mut s.binding_draft.program)
        .on_submit(|s: &mut Chat, _: &mut Tx| s.save_binding_edit());
    let args = auto_grow(
        text_input(&draft.args, k.ty.ui)
            .key("agent-binding-args")
            .w_full()
            .color(k.c.text_base)
            .placeholder("One argument per line", k.c.text_faint)
            .label("Arguments, one per line")
            .on_edit(|s: &mut Chat| &mut s.binding_draft.args),
        1,
        5,
    );
    let timeout = text_input(&draft.timeout, k.ty.ui)
        .key("agent-binding-timeout")
        .w_full()
        .color(k.c.text_base)
        .label("Timeout in seconds")
        .on_edit(|s: &mut Chat| &mut s.binding_draft.timeout)
        .on_submit(|s: &mut Chat, _: &mut Tx| s.save_binding_edit());
    let mut form = col()
        .key("agent-binding-form")
        .role("group")
        .label("Command")
        .w_full()
        .gap(8.0)
        .child(
            rune_kit::Field::new()
                .label("Program")
                .control(field_box(k, program))
                .build(k),
        )
        .child(
            rune_kit::Field::new()
                .label("Arguments")
                .control(field_box(k, args))
                .build(k),
        )
        .child(
            rune_kit::Field::new()
                .label("Timeout (seconds)")
                .control(field_box(k, timeout))
                .build(k),
        );
    if let Some(error) = &draft.error {
        form = form.child(
            text(error)
                .key("agent-binding-error")
                .wrap()
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .live_region(rune::LiveRegion::Polite),
        );
    }
    form.child(
        row()
            .w_full()
            .gap(8.0)
            .child(spacer())
            .child(
                satellite::button("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut Chat| s.cancel_binding_edit())
                    .sized(k, satellite::Size::Small)
                    .key("agent-binding-cancel"),
            )
            .child(
                satellite::button("Save")
                    .variant(ButtonVariant::Contrast)
                    .on_press(|s: &mut Chat| s.save_binding_edit())
                    .sized(k, satellite::Size::Small)
                    .key("agent-binding-save"),
            ),
    )
}

fn dropped_tools(s: &Chat, k: &rune_kit::Kit) -> Option<El> {
    let dropped = s.dropped_tools();
    if dropped.is_empty() {
        return None;
    }
    let mut list = card(k).key("agent-dropped-tools");
    for (i, name) in dropped.iter().enumerate() {
        if i > 0 {
            list = list.child(row_separator(k));
        }
        let remove = {
            let target = name.clone();
            satellite::button("Remove")
                .variant(ButtonVariant::Outline)
                .on_press(move |s: &mut Chat| s.remove_binding(&target))
                .sized(k, satellite::Size::Small)
                .label(format!("Remove the handler for {name}"))
                .key(format!("agent-dropped-remove-{name}"))
        };
        list = list.child(
            row()
                .key(format!("agent-dropped-{name}"))
                .w_full()
                .px(16.0)
                .py(12.0)
                .gap(12.0)
                .center_y()
                .child(
                    text(name)
                        .grow(1.0)
                        .min_w(0.0)
                        .wrap_anywhere()
                        .font(CODE)
                        .color(k.c.text_base),
                )
                .child(remove),
        );
    }
    Some(section(k, "No longer declared", list))
}

fn server_tools(k: &rune_kit::Kit, config: &AgentConfig) -> El {
    let names = if config.other_tools.is_empty() {
        "None".to_string()
    } else {
        config.other_tools.join(", ")
    };
    section(
        k,
        "Server tools",
        col().w_full().px(4.0).child(
            text(names)
                .key("agent-server-tools")
                .wrap_anywhere()
                .font(k.ty.small)
                .color(k.c.text_muted),
        ),
    )
}
