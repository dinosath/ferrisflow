use dioxus::prelude::*;
use dioxus_icons::lucide::{
    arrow_down::ArrowDown, arrow_right::ArrowRight, arrow_up::ArrowUp, chevron_down::ChevronDown,
    chevron_up::ChevronUp, clock::Clock, file::File, file_pen_line::FilePenLine,
    ellipsis::Ellipsis, git_branch::GitBranch, layout_dashboard::LayoutDashboard, list::List,
    maximize::Maximize, panel_right::PanelRight, play::Play, plus::Plus, power::Power,
    radio::Radio, repeat::Repeat, search::Search, shield_alert::ShieldAlert, sparkles::Sparkles,
    table::Table, trash_2::Trash2, wand::Wand, workflow::Workflow, x::X, zap::Zap,
    zoom_in::ZoomIn, zoom_out::ZoomOut,
};
use serde_json::{Map, Value, json};

const TASK_TYPES: [&str; 8] = ["set", "call", "wait", "emit", "switch", "for", "do", "try"];
const TRIGGER_CONNECTION_ID: &str = "__manual_trigger__";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidationState {
    pub checking: bool,
    pub valid: bool,
    pub errors: Vec<String>,
}

#[component]
pub fn WorkflowEditor(
    definition: String, on_change: EventHandler<String>, on_validate: EventHandler<String>,
    validation: ValidationState,
) -> Element {
    let mut selected_index = use_signal(|| 0_usize);
    let mut trigger_selected = use_signal(|| false);
    let mut view_json = use_signal(|| false);
    let mut inspector_open = use_signal(|| false);
    let mut palette_open = use_signal(|| false);
    let mut assistant_open = use_signal(|| false);
    let mut more_menu_index = use_signal(|| None::<usize>);
    let mut zoom_percent = use_signal(|| 100_i32);
    let mut canvas_pan = use_signal(|| (0.0_f64, 0.0_f64));
    let mut pan_origin = use_signal(|| (0.0_f64, 0.0_f64));
    let mut pointer_origin = use_signal(|| (0.0_f64, 0.0_f64));
    let mut panning = use_signal(|| false);
    let mut space_pressed = use_signal(|| false);
    let mut dragging_node = use_signal(|| None::<usize>);
    let mut trigger_position_override = use_signal(|| None::<(f64, f64)>);
    let mut node_start_position = use_signal(|| (0.0_f64, 0.0_f64));
    let mut node_was_moved = use_signal(|| false);
    let mut node_positions = use_signal(Vec::<(f64, f64)>::new);
    let mut pinch_origin = use_signal(|| None::<(f64, i32)>);
    let mut connecting_from = use_signal(|| None::<String>);
    let mut connecting_start = use_signal(|| (0.0_f64, 0.0_f64));
    let mut connecting_pointer = use_signal(|| (0.0_f64, 0.0_f64));
    let mut pending_add_source = use_signal(|| None::<String>);
    let mut pending_insert_edge = use_signal(|| None::<(String, String)>);
    let mut selected_connection = use_signal(|| None::<(String, String)>);
    let mut editor_tab = use_signal(|| "editor".to_string());
    let mut logs_open = use_signal(|| false);
    let mut json_draft = use_signal(|| definition.clone());
    let mut task_draft = use_signal(|| "{}".to_string());
    let mut trigger_input_draft = use_signal(|| "{}".to_string());
    let mut edit_error = use_signal(|| None::<String>);

    let document = parse_document(&definition).unwrap_or_else(|_| starter_document());
    let tasks = document["do"].as_array().cloned().unwrap_or_default();
    let has_tasks = !tasks.is_empty();
    let content_width = (1000.0_f64).max(500.0 + tasks.len() as f64 * 230.0);
    let default_start_x = (content_width - (350.0 + tasks.len() as f64 * 230.0)) / 2.0;
    let stored_positions = read_canvas_positions(&document, &tasks, default_start_x);
    let positions = if node_positions().len() == tasks.len() {
        node_positions()
    } else {
        stored_positions
    };
    let default_trigger_position = (default_start_x, 380.0_f64);
    let trigger_position = trigger_position_override().unwrap_or_else(|| read_trigger_position(&document, default_trigger_position));
    let content_width = positions.iter().map(|(x, _)| x + 300.0).fold(content_width.max(trigger_position.0 + 300.0), f64::max);
    let content_height = positions.iter().map(|(_, y)| y + 220.0).fold(900.0_f64.max(trigger_position.1 + 220.0), f64::max);
    let add_position = positions.last().map(|(x, y)| (x + 230.0, *y)).unwrap_or((default_start_x + 260.0, trigger_position.1));
    let start_positions = positions.clone();
    let connections = read_canvas_connections(&document, &tasks);
    let mut edge_paths = Vec::with_capacity(connections.len() + 1);
    let mut connection_controls = Vec::with_capacity(connections.len());
    for (edge_index, (from, to)) in connections.iter().enumerate() {
        let (source_node, source_port) = split_connection_source(from);
        let Some(mut from_position) = (if source_node == TRIGGER_CONNECTION_ID {
            Some(trigger_position)
        } else {
            tasks.iter().position(|task| task_name(task).as_deref() == Some(source_node)).and_then(|index| positions.get(index).copied())
        }) else { continue; };
        let source_count = tasks.iter().find(|task| task_name(task).as_deref() == Some(source_node)).map(node_output_count).unwrap_or(1);
        let source_offsets = output_port_offsets(source_count);
        from_position.1 += source_offsets.get(source_port).copied().unwrap_or(48.0) - 48.0;
        let Some(mut to_position) = tasks.iter().position(|task| task_name(task).as_deref() == Some(to.as_str())).and_then(|index| positions.get(index).copied()) else { continue; };
        let target_edges = connections.iter().enumerate().filter(|(_, (_, target))| target == to).map(|(index, _)| index).collect::<Vec<_>>();
        let port_index = target_edges.iter().position(|index| *index == edge_index).unwrap_or(0);
        let port_offsets = input_port_offsets(target_edges.len().max(1));
        to_position.1 += port_offsets[port_index] - 48.0;
        edge_paths.push(connector_path(from_position, to_position));
        let midpoint = ((from_position.0 + 116.0 + to_position.0 + 16.0) / 2.0, (from_position.1 + 48.0 + to_position.1 + 48.0) / 2.0);
        connection_controls.push((from.clone(), to.clone(), midpoint));
    }
    if let Some(source) = connecting_from() {
        let (source_node, source_port) = split_connection_source(&source);
        let source_position = if source_node == TRIGGER_CONNECTION_ID { Some(trigger_position) } else {
            tasks.iter().position(|task| task_name(task).as_deref() == Some(source_node)).and_then(|index| positions.get(index).copied())
        };
        if let Some(mut from_position) = source_position {
            let source_count = tasks.iter().find(|task| task_name(task).as_deref() == Some(source_node)).map(node_output_count).unwrap_or(1);
            let source_offsets = output_port_offsets(source_count);
            from_position.1 += source_offsets.get(source_port).copied().unwrap_or(48.0) - 48.0;
            edge_paths.push(connector_preview_path(from_position, connecting_pointer()));
        }
    }
    let selected = if tasks.is_empty() { 0 } else { selected_index().min(tasks.len() - 1) };
    let selected_task = tasks.get(selected).cloned();
    let selected_name = selected_task.as_ref().and_then(task_name).unwrap_or_default();
    let selected_definition =
        selected_task.as_ref().and_then(task_definition).unwrap_or_else(|| json!({}));
    let selected_type = selected_definition
        .as_object()
        .and_then(|object| object.keys().next())
        .map(String::as_str)
        .unwrap_or("task");
    let call_target = selected_definition["call"].as_str().unwrap_or_default();
    let trigger_config = manual_trigger_config(&document);
    let trigger_name = trigger_config["name"].as_str().unwrap_or("Manual Trigger");
    let trigger_description = trigger_config["description"].as_str().unwrap_or_default();
    let trigger_input = trigger_config.get("input").cloned().unwrap_or_else(|| json!({}));
    let dsl_version = document["document"]["dsl"].as_str().unwrap_or("1.0.3");

    rsx! {
        section { class: "workflow-editor",
            header { class: "workflow-editor__topbar",
                nav { class: "workflow-editor__tabs", "aria-label": "Workflow sections",
                    for (tab, label) in [("editor", "Editor"), ("executions", "Executions")] {
                        button {
                            class: if editor_tab() == tab { "workflow-editor__tab is-active" } else { "workflow-editor__tab" },
                            r#type: "button",
                            onclick: move |_| editor_tab.set(tab.to_string()),
                            "{label}"
                        }
                    }
                }
            }

            if view_json() {
                div { class: "workflow-editor__json panel",
                    div { class: "workflow-editor__section-heading",
                        div { strong { "Workflow definition" } small { "Edit the canonical OWS JSON." } }
                        button {
                            class: "button button-quiet",
                            r#type: "button",
                            onclick: move |_| view_json.set(false),
                            "Back to canvas"
                        }
                        button {
                            class: "button button-primary",
                            r#type: "button",
                            onclick: move |_| match parse_document(&json_draft()) {
                                Ok(next) => { on_change.call(format_json(&next)); edit_error.set(None); }
                                Err(message) => edit_error.set(Some(message)),
                            },
                            "Apply JSON"
                        }
                    }
                    textarea {
                        class: "code-editor workflow-editor__json-input",
                        value: "{json_draft}",
                        spellcheck: "false",
                        oninput: move |event| json_draft.set(event.value()),
                    }
                    if let Some(message) = edit_error() { p { class: "workflow-editor__inline-error", "{message}" } }
                }
            } else {
                div { class: "workflow-editor__workspace",
                    if palette_open() {
                        div { class: "workflow-node-picker__overlay", role: "dialog", aria_modal: "true", aria_label: "Add a workflow node",
                            section { class: "workflow-node-picker panel",
                                header { class: "workflow-node-picker__header",
                                    div {
                                        p { class: "workflow-editor__eyebrow", "NODE LIBRARY" }
                                        h2 { "Add a step" }
                                        if pending_insert_edge().is_some() {
                                            p { "Choose a node to insert into the selected connection." }
                                        } else if pending_add_source().is_some() {
                                            p { "Choose a node to connect after this output." }
                                        } else {
                                            p { "Choose a node to add without a connection." }
                                        }
                                    }
                                    button { class: "workflow-editor__icon-action", title: "Close node picker", aria_label: "Close node picker", r#type: "button", onclick: move |_| palette_open.set(false), EditorIcon { name: "x", size: 16 } }
                                }
                                div { class: "workflow-node-picker__list",
                                    for node_type in TASK_TYPES {
                                        button {
                                            class: "workflow-editor__palette-item",
                                            r#type: "button",
                                            onclick: {
                                                let mut next = document.clone();
                                                let existing_tasks = tasks.clone();
                                                let existing_positions = positions.clone();
                                                let current_trigger_position = trigger_position;
                                                move |_| {
                                                    let mut next_tasks = next["do"].as_array().cloned().unwrap_or_default();
                                                    let new_task = create_task(node_type, &next_tasks);
                                                    let new_name = task_name(&new_task).unwrap_or_default();
                                                    let insertion = pending_insert_edge();
                                                    let source = insertion.as_ref().map(|edge| edge.0.clone()).or_else(|| pending_add_source());
                                                    let new_position = if let Some((from, to)) = insertion.as_ref() {
                                                        let from_position = canvas_node_position(from, &existing_tasks, &existing_positions, current_trigger_position);
                                                        let to_position = canvas_node_position(to, &existing_tasks, &existing_positions, current_trigger_position);
                                                        from_position.zip(to_position).map(|(from_position, to_position)| (
                                                            (from_position.0 + to_position.0 + 132.0) / 2.0 - 66.0,
                                                            (from_position.1 + to_position.1) / 2.0,
                                                        ))
                                                    } else {
                                                        source.as_ref().and_then(|source| canvas_node_position(source, &existing_tasks, &existing_positions, current_trigger_position))
                                                            .map(|position| (position.0 + if source.as_deref() == Some(TRIGGER_CONNECTION_ID) { 260.0 } else { 230.0 }, position.1))
                                                    };
                                                    let existing_graph = read_canvas_connections(&next, &next_tasks);
                                                    write_canvas_connections(&mut next, &existing_graph);
                                                    next_tasks.push(new_task);
                                                    selected_index.set(next_tasks.len() - 1);
                                                    trigger_selected.set(false);
                                                    inspector_open.set(true);
                                                    palette_open.set(false);
                                                    next["do"] = Value::Array(next_tasks);
                                                    if let Some((from, to)) = insertion {
                                                        remove_specific_canvas_connection(&mut next, &from, &to);
                                                        add_canvas_connection(&mut next, &from, &new_name);
                                                        add_canvas_connection(&mut next, &new_name, &to);
                                                    } else if let Some(from) = source {
                                                        add_canvas_connection(&mut next, &from, &new_name);
                                                    }
                                                    if let Some(position) = new_position { write_canvas_node_position(&mut next, &new_name, position); }
                                                    pending_insert_edge.set(None);
                                                    pending_add_source.set(None);
                                                    task_draft.set("{}".into());
                                                    on_change.call(format_json(&next));
                                                }
                                            },
                                            span { class: "workflow-editor__palette-icon", EditorIcon { name: node_icon(node_type), size: 18 } }
                                            span { class: "workflow-node-picker__copy",
                                                strong { "{pretty_type(node_type)}" }
                                                small { "{node_description(node_type)}" }
                                            }
                                            span { class: "workflow-editor__palette-chevron", EditorIcon { name: "arrow-up-right", size: 14 } }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    main { class: "workflow-editor__canvas-wrap",
                        if editor_tab() == "editor" {
                            div { class: "workflow-editor__canvas-toolbar",
                                span { class: "workflow-editor__eyebrow", "OWS DSL {dsl_version}" }
                            }
                        }
                        div {
                            class: "workflow-editor__canvas",
                            tabindex: "0",
                            onwheel: move |event| {
                                let delta = event.data().delta().strip_units();
                                let modifiers = event.data().modifiers();
                                if modifiers.ctrl() || modifiers.meta() {
                                    event.prevent_default();
                                    let direction = if delta.y < 0.0 { 1 } else { -1 };
                                    let amount = ((delta.y.abs() / 35.0).ceil() as i32).clamp(1, 20) * direction;
                                    zoom_percent.set((zoom_percent() - amount).clamp(40, 160));
                                } else {
                                    event.prevent_default();
                                    let pan = canvas_pan();
                                    canvas_pan.set((pan.0 - delta.x, pan.1 - delta.y));
                                }
                            },
                            onkeydown: move |event| {
                                let key = event.data().key().to_string();
                                match key.as_str() {
                                    " " => { space_pressed.set(true); event.prevent_default(); }
                                    "+" | "=" => { zoom_percent.set((zoom_percent() + 10).min(160)); event.prevent_default(); }
                                    "-" | "_" => { zoom_percent.set((zoom_percent() - 10).max(40)); event.prevent_default(); }
                                    "0" => { zoom_percent.set(100); canvas_pan.set((0.0, 0.0)); event.prevent_default(); }
                                    "1" => {
                                        zoom_percent.set(fit_zoom(&positions, trigger_position, add_position));
                                        canvas_pan.set((0.0, 0.0));
                                        event.prevent_default();
                                    }
                                    _ => {}
                                }
                            },
                            onkeyup: move |event| {
                                if event.data().key().to_string() == " " { space_pressed.set(false); }
                            },
                            onpointermove: move |event| {
                                let point = event.data().client_coordinates();
                                if connecting_from().is_some() {
                                    let origin = pointer_origin();
                                    let start = connecting_start();
                                    let scale = zoom_percent() as f64 / 100.0;
                                    connecting_pointer.set((start.0 + (point.x - origin.0) / scale, start.1 + (point.y - origin.1) / scale));
                                } else if let Some(index) = dragging_node() {
                                    let origin = pointer_origin();
                                    let offset = node_start_position();
                                    let delta_x = point.x - origin.0;
                                    let delta_y = point.y - origin.1;
                                    if delta_x.abs() + delta_y.abs() > 3.0 { node_was_moved.set(true); }
                                    if node_was_moved() {
                                        let scale = zoom_percent() as f64 / 100.0;
                                        let moved = ((offset.0 + delta_x / scale).max(0.0), (offset.1 + delta_y / scale).max(0.0));
                                        if index == usize::MAX {
                                            trigger_position_override.set(Some(moved));
                                        } else {
                                            let mut next_positions = node_positions();
                                            if index < next_positions.len() {
                                                next_positions[index] = moved;
                                                node_positions.set(next_positions);
                                            }
                                        }
                                    }
                                } else if panning() {
                                    let origin = pointer_origin();
                                    let start = pan_origin();
                                    canvas_pan.set((start.0 + point.x - origin.0, start.1 + point.y - origin.1));
                                }
                            },
                            onpointerup: {
                                let mut next = document.clone();
                                let workflow_tasks = tasks.clone();
                                let current_trigger_position = trigger_position;
                                move |_| {
                                    if node_was_moved() {
                                        let trigger_position = trigger_position_override().unwrap_or(current_trigger_position);
                                        save_canvas_positions(&mut next, &workflow_tasks, &node_positions(), trigger_position);
                                        on_change.call(format_json(&next));
                                    }
                                    dragging_node.set(None);
                                    node_was_moved.set(false);
                                    panning.set(false);
                                    connecting_from.set(None);
                                }
                            },
                            onpointercancel: move |_| { dragging_node.set(None); node_was_moved.set(false); panning.set(false); connecting_from.set(None); },
                            ontouchstart: move |event| {
                                let touches = event.data().touches();
                                if touches.len() >= 2 {
                                    panning.set(false);
                                    dragging_node.set(None);
                                    node_was_moved.set(false);
                                    let first = touches[0].client_coordinates();
                                    let second = touches[1].client_coordinates();
                                    let distance = ((second.x - first.x).powi(2) + (second.y - first.y).powi(2)).sqrt();
                                    pinch_origin.set(Some((distance, zoom_percent())));
                                }
                            },
                            ontouchmove: move |event| {
                                let touches = event.data().touches();
                                if touches.len() >= 2 {
                                    event.prevent_default();
                                    panning.set(false);
                                    let first = touches[0].client_coordinates();
                                    let second = touches[1].client_coordinates();
                                    let distance = ((second.x - first.x).powi(2) + (second.y - first.y).powi(2)).sqrt();
                                    if let Some((start_distance, start_zoom)) = pinch_origin() {
                                        if start_distance > 0.0 && distance > 0.0 {
                                            let scale = start_distance / distance;
                                            zoom_percent.set(((start_zoom as f64 * scale).round() as i32).clamp(40, 160));
                                        }
                                    }
                                }
                            },
                            ontouchend: move |event| {
                                if event.data().touches().len() < 2 { pinch_origin.set(None); }
                            },
                            ontouchcancel: move |_| pinch_origin.set(None),
                            div { class: "workflow-editor__canvas-pan-surface", onpointerdown: move |event| {
                                let point = event.data().client_coordinates();
                                pointer_origin.set((point.x, point.y));
                                pan_origin.set(canvas_pan());
                                panning.set(true);
                            } }
                            div {
                                class: "workflow-editor__canvas-content",
                                style: "width: {content_width}px; height: {content_height}px; transform: translate(calc(-50% + {canvas_pan().0}px), calc(-50% + {canvas_pan().1}px)) scale({zoom_percent() as f64 / 100.0});",
                                svg { class: "workflow-editor__edges", width: "{content_width}", height: "{content_height}",
                                    for (index, (from, to, _)) in connection_controls.iter().enumerate() {
                                        {
                                            let source = from.clone();
                                            let target = to.clone();
                                            let is_selected = selected_connection() == Some((source.clone(), target.clone()));
                                            rsx! {
                                                path {
                                                    key: "edge-hit-{index}",
                                                    class: "workflow-editor__edge-hit",
                                                    d: "{edge_paths[index]}",
                                                    onclick: move |_| selected_connection.set(Some((source.clone(), target.clone())))
                                                }
                                                path {
                                                    class: if is_selected { "workflow-editor__edge is-selected" } else { "workflow-editor__edge" },
                                                    d: "{edge_paths[index]}"
                                                }
                                            }
                                        }
                                    }
                                    for (index, edge_path) in edge_paths.iter().enumerate().skip(connection_controls.len()) {
                                        path { key: "aux-edge-{index}", class: "workflow-editor__edge workflow-editor__edge--aux", d: "{edge_path}" }
                                    }
                                }
                                for (from, to, midpoint) in connection_controls.iter() {
                                    if selected_connection() == Some((from.clone(), to.clone())) {
                                        div { class: "workflow-connection-actions", style: "left: {midpoint.0}px; top: {midpoint.1}px;",
                                            button { class: "workflow-editor__node-action", title: "Insert node into connection", aria_label: "Insert node into connection", r#type: "button", onclick: {
                                                let source = from.clone();
                                                let target = to.clone();
                                                move |_| { pending_add_source.set(None); pending_insert_edge.set(Some((source.clone(), target.clone()))); palette_open.set(true); }
                                            }, EditorIcon { name: "plus", size: 15 } }
                                            button { class: "workflow-editor__node-action workflow-editor__node-action--danger", title: "Delete connection", aria_label: "Delete connection", r#type: "button", onclick: {
                                                let mut next = document.clone();
                                                let source = from.clone();
                                                let target = to.clone();
                                                move |_| { remove_specific_canvas_connection(&mut next, &source, &target); selected_connection.set(None); on_change.call(format_json(&next)); }
                                            }, EditorIcon { name: "trash", size: 15 } }
                                        }
                                    }
                                }
                                div { class: "workflow-node-group workflow-node-group--start", style: "left: {trigger_position.0}px; top: {trigger_position.1}px;",
                                    div { class: "workflow-node-actions",
                                        button { class: "workflow-editor__node-action", title: "Edit trigger", aria_label: "Edit trigger", r#type: "button", onclick: {
                                            let input = trigger_input.clone();
                                            move |_| { trigger_selected.set(true); inspector_open.set(true); trigger_input_draft.set(format_json(&input)); edit_error.set(None); }
                                        }, EditorIcon { name: "file-pen", size: 15 } }
                                    }
                                    button {
                                        class: if trigger_selected() { "workflow-editor__start-pill is-selected" } else { "workflow-editor__start-pill" },
                                        r#type: "button",
                                        aria_label: "Edit manual trigger",
                                        title: "Edit manual trigger",
                                        onpointerdown: move |event| {
                                            if connecting_from().is_some() {
                                                event.prevent_default();
                                            } else if space_pressed() || event.data().modifiers().ctrl() || event.data().modifiers().meta() {
                                                let point = event.data().client_coordinates();
                                                pointer_origin.set((point.x, point.y));
                                                pan_origin.set(canvas_pan());
                                                panning.set(true);
                                            } else {
                                                let point = event.data().client_coordinates();
                                                pointer_origin.set((point.x, point.y));
                                                node_start_position.set(trigger_position);
                                                node_positions.set(start_positions.clone());
                                                dragging_node.set(Some(usize::MAX));
                                                node_was_moved.set(false);
                                            }
                                        },
                                        onclick: move |_| { trigger_selected.set(true); more_menu_index.set(None); },
                                        ondoubleclick: {
                                            let input = trigger_input.clone();
                                            move |_| {
                                                trigger_selected.set(true);
                                                inspector_open.set(true);
                                                trigger_input_draft.set(format_json(&input));
                                                edit_error.set(None);
                                            }
                                        },
                                        span { class: "workflow-node__icon", EditorIcon { name: "zap", size: 30 } }
                                        span {
                                            class: "workflow-node__port workflow-node__port--out",
                                            title: "Drag to connect to a node input",
                                            onpointerdown: move |event| {
                                                event.prevent_default();
                                                let point = event.data().client_coordinates();
                                                connecting_from.set(Some(TRIGGER_CONNECTION_ID.to_string()));
                                                connecting_start.set(trigger_position);
                                                connecting_pointer.set(trigger_position);
                                                pointer_origin.set((point.x, point.y));
                                            }
                                        }
                                    }
                                    button { class: "workflow-node-add-output", title: "Add node connected to Manual Trigger", aria_label: "Add node connected to Manual Trigger", r#type: "button", onclick: move |_| { pending_add_source.set(Some(TRIGGER_CONNECTION_ID.to_string())); pending_insert_edge.set(None); palette_open.set(true); }, EditorIcon { name: "plus", size: 14 } }
                                    div { class: "workflow-node-label", strong { "{trigger_name}" } small { "Trigger" } }
                                }
                                for (index, task) in tasks.iter().enumerate() {
                                    {
                                        let name = task_name(task).unwrap_or_else(|| "Unnamed task".into());
                                        let kind = task_definition(task)
                                            .and_then(|value| value.as_object().and_then(|object| object.keys().next().cloned()))
                                            .unwrap_or_else(|| "task".into());
                                        let icon = node_icon(&kind);
                                        let disabled = node_is_disabled(&document, &name);
                                        let input_offsets = input_port_offsets(connections.iter().filter(|(_, target)| target == &name).count().max(1));
                                        let output_count = node_output_count(task);
                                        let output_offsets = output_port_offsets(output_count);
                                        let task_value = task_definition(task).unwrap_or_else(|| json!({}));
                                        let position = positions[index];
                                        let task_position = position;
                                        let positions_snapshot = positions.clone();
                                        rsx! {
                                            div { class: "workflow-node-group workflow-node-group--canvas", style: "left: {position.0}px; top: {position.1}px;",
                                                div { class: "workflow-node-actions",
                                                        button { class: "workflow-editor__node-action", title: "Validate workflow", aria_label: "Validate workflow", r#type: "button", onclick: { let current = definition.clone(); move |_| on_validate.call(current.clone()) }, EditorIcon { name: "play", size: 14 } }
                                                        button { class: if disabled { "workflow-editor__node-action is-muted" } else { "workflow-editor__node-action" }, title: if disabled { "Activate node" } else { "Deactivate node" }, aria_label: if disabled { "Activate node" } else { "Deactivate node" }, r#type: "button", onclick: {
                                                            let mut next = document.clone();
                                                            let node_name = name.clone();
                                                            move |_| { set_node_disabled(&mut next, &node_name, !disabled); on_change.call(format_json(&next)); }
                                                        }, EditorIcon { name: "power", size: 15 } }
                                                        button { class: "workflow-editor__node-action", title: "Edit node", aria_label: "Edit node", r#type: "button", onclick: {
                                                            let value = task_value.clone();
                                                            move |_| { trigger_selected.set(false); selected_index.set(index); inspector_open.set(true); task_draft.set(format_json(&value)); edit_error.set(None); more_menu_index.set(None); }
                                                        }, EditorIcon { name: "file-pen", size: 15 } }
                                                        button { class: "workflow-editor__node-action workflow-editor__node-action--danger", title: "Delete node", aria_label: "Delete node", r#type: "button", onclick: {
                                                            let mut next = document.clone();
                                                            move |_| {
                                                                let mut next_tasks = next["do"].as_array().cloned().unwrap_or_default();
                                                                if let Some(removed_name) = next_tasks.get(index).and_then(task_name) { remove_canvas_connections(&mut next, &removed_name, &next_tasks); }
                                                                if index < next_tasks.len() { next_tasks.remove(index); }
                                                                let empty = next_tasks.is_empty();
                                                                next["do"] = Value::Array(next_tasks);
                                                                selected_index.set(index.saturating_sub(1));
                                                                more_menu_index.set(None);
                                                                if empty { inspector_open.set(false); }
                                                                on_change.call(format_json(&next));
                                                            }
                                                        }, EditorIcon { name: "trash", size: 15 } }
                                                        button { class: "workflow-editor__node-action workflow-editor__node-action--accent", title: "Workflow help", aria_label: "Workflow help", r#type: "button", onclick: move |_| assistant_open.set(!assistant_open()), EditorIcon { name: "sparkles", size: 15 } }
                                                        button { class: "workflow-editor__node-action", title: "More node options", aria_label: "More node options", aria_expanded: more_menu_index() == Some(index), r#type: "button", onclick: move |_| more_menu_index.set(if more_menu_index() == Some(index) { None } else { Some(index) }), EditorIcon { name: "ellipsis", size: 15 } }
                                                        if more_menu_index() == Some(index) {
                                                            div { class: "workflow-node-more-menu",
                                                                button { r#type: "button", onclick: { let value = task_value.clone(); move |_| { trigger_selected.set(false); selected_index.set(index); inspector_open.set(true); task_draft.set(format_json(&value)); edit_error.set(None); more_menu_index.set(None); } }, "Open node settings" }
                                                                button { r#type: "button", onclick: {
                                                                    let mut next = document.clone();
                                                                    let source_name = name.clone();
                                                                    move |_| {
                                                                        if let Some(duplicate_index) = duplicate_task(&mut next, index) {
                                                                            let duplicate = next["do"][duplicate_index].clone();
                                                                            let duplicate_name = task_name(&duplicate).unwrap_or_default();
                                                                            add_canvas_connection(&mut next, &source_name, &duplicate_name);
                                                                            selected_index.set(duplicate_index);
                                                                            task_draft.set(format_json(&task_definition(&duplicate).unwrap_or_else(|| json!({}))));
                                                                            more_menu_index.set(None);
                                                                            on_change.call(format_json(&next));
                                                                        }
                                                                    }
                                                                }, "Duplicate" }
                                                                button { r#type: "button", onclick: {
                                                                    let mut next = document.clone();
                                                                    let node_name = name.clone();
                                                                    move |_| { set_node_disabled(&mut next, &node_name, !disabled); more_menu_index.set(None); on_change.call(format_json(&next)); }
                                                                }, if disabled { "Activate" } else { "Deactivate" } }
                                                                button { class: "is-danger", r#type: "button", onclick: {
                                                                    let mut next = document.clone();
                                                                    move |_| {
                                                                        let mut next_tasks = next["do"].as_array().cloned().unwrap_or_default();
                                                                        if let Some(removed_name) = next_tasks.get(index).and_then(task_name) { remove_canvas_connections(&mut next, &removed_name, &next_tasks); }
                                                                        if index < next_tasks.len() { next_tasks.remove(index); }
                                                                        let empty = next_tasks.is_empty();
                                                                        next["do"] = Value::Array(next_tasks);
                                                                        selected_index.set(index.saturating_sub(1));
                                                                        more_menu_index.set(None);
                                                                        if empty { inspector_open.set(false); }
                                                                        on_change.call(format_json(&next));
                                                                    }
                                                                }, "Delete" }
                                                            }
                                                        }
                                                }
                                                button {
                                                    class: if disabled && index == selected { "workflow-node is-disabled is-selected" } else if disabled { "workflow-node is-disabled" } else if index == selected { "workflow-node is-selected" } else { "workflow-node" },
                                                    r#type: "button",
                                                    aria_label: "Select workflow step",
                                                    title: "Drag to move · Click to select · Double-click to edit",
                                                    onpointerdown: move |event| {
                                                        if connecting_from().is_some() {
                                                            event.prevent_default();
                                                        } else if space_pressed() || event.data().modifiers().ctrl() || event.data().modifiers().meta() {
                                                            let point = event.data().client_coordinates();
                                                            pointer_origin.set((point.x, point.y));
                                                            pan_origin.set(canvas_pan());
                                                            panning.set(true);
                                                        } else {
                                                            let point = event.data().client_coordinates();
                                                            pointer_origin.set((point.x, point.y));
                                                            node_start_position.set(task_position);
                                                            node_positions.set(positions_snapshot.clone());
                                                            dragging_node.set(Some(index));
                                                            node_was_moved.set(false);
                                                        }
                                                    },
                                                    onclick: move |_| { trigger_selected.set(false); selected_index.set(index); more_menu_index.set(None); },
                                                    ondoubleclick: {
                                                        let value = task_value.clone();
                                                        move |_| { trigger_selected.set(false); selected_index.set(index); inspector_open.set(true); task_draft.set(format_json(&value)); edit_error.set(None); more_menu_index.set(None); }
                                                    },
                                                    for (port_index, port_offset) in input_offsets.iter().enumerate() {
                                                        span {
                                                            key: "input-{port_index}",
                                                            class: "workflow-node__port workflow-node__port--in",
                                                            style: "top: {port_offset - 6.0}px;",
                                                            title: "Connect workflow output here",
                                                            onpointerup: {
                                                                let mut next = document.clone();
                                                                let target_name = name.clone();
                                                                move |event| {
                                                                    event.prevent_default();
                                                                    if let Some(source_name) = connecting_from() {
                                                                        if add_canvas_connection(&mut next, &source_name, &target_name) {
                                                                            on_change.call(format_json(&next));
                                                                        }
                                                                        connecting_from.set(None);
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        if input_offsets.len() > 1 {
                                                            span { class: "workflow-node__port-label workflow-node__port-label--in", style: "top: {port_offset}px;", "{port_index + 1}" }
                                                        }
                                                    }
                                                    span { class: "workflow-node__icon", EditorIcon { name: icon, size: 30 } }
                                                    for (output_index, output_offset) in output_offsets.iter().enumerate() {
                                                        {
                                                            let source = connection_source_id(&name, output_index);
                                                            let output_start_y = task_position.1 + output_offset - 48.0;
                                                            let output_label = node_output_label(&kind, output_index, output_count);
                                                            rsx! {
                                                                span {
                                                                    key: "output-{output_index}",
                                                                    class: "workflow-node__port workflow-node__port--out",
                                                                    style: "top: {output_offset - 6.0}px;",
                                                                    title: if output_count > 1 { output_label.clone() } else { "Drag to connect to another node".to_owned() },
                                                                    onpointerdown: move |event| {
                                                                        event.prevent_default();
                                                                        let point = event.data().client_coordinates();
                                                                        connecting_from.set(Some(source.clone()));
                                                                        connecting_start.set((task_position.0, output_start_y));
                                                                        connecting_pointer.set((task_position.0, output_start_y));
                                                                        pointer_origin.set((point.x, point.y));
                                                                    }
                                                                }
                                                                if output_count > 1 {
                                                                    span { class: "workflow-node__port-label workflow-node__port-label--out", style: "top: {output_offset}px;", "{output_label}" }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                for (output_index, output_offset) in output_offsets.iter().enumerate() {
                                                    {
                                                        let source = connection_source_id(&name, output_index);
                                                        let label = node_output_label(&kind, output_index, output_count);
                                                        rsx! {
                                                            button {
                                                                key: "add-output-{output_index}",
                                                                class: "workflow-node-add-output",
                                                                style: "top: {output_offset}px;",
                                                                title: if output_count > 1 { format!("Add node from {label} output") } else { format!("Add node connected to {name}") },
                                                                aria_label: if output_count > 1 { format!("Add node from {label} output") } else { format!("Add node connected to {name}") },
                                                                r#type: "button",
                                                                onclick: move |_| { pending_add_source.set(Some(source.clone())); pending_insert_edge.set(None); palette_open.set(true); },
                                                                EditorIcon { name: "plus", size: 14 }
                                                            }
                                                        }
                                                    }
                                                }
                                                div { class: "workflow-node-label", strong { "{name}" } small { "{pretty_type(&kind)}" } }
                                            }
                                        }
                                    }
                                }
                            }
                            if editor_tab() != "editor" {
                                div { class: "workflow-editor__tab-empty",
                                    EditorIcon { name: "clock", size: 24 }
                                    strong { "No executions yet" }
                                    span { "Run the workflow to see execution history." }
                                }
                            }
                        }
                        if editor_tab() == "editor" {
                            div { class: "workflow-editor__canvas-tools",
                                button { class: "workflow-editor__tool-button", title: "Add standalone node", aria_label: "Add standalone node", r#type: "button", onclick: move |_| { pending_add_source.set(None); pending_insert_edge.set(None); palette_open.set(true); }, EditorIcon { name: "plus", size: 18 } }
                                button { class: "workflow-editor__tool-button", title: "Search nodes", aria_label: "Search nodes", r#type: "button", onclick: move |_| { pending_add_source.set(None); pending_insert_edge.set(None); palette_open.set(true); }, EditorIcon { name: "search", size: 17 } }
                                button { class: "workflow-editor__tool-button", title: "Edit definition", aria_label: "Edit definition", r#type: "button", onclick: { let current = definition.clone(); move |_| { json_draft.set(current.clone()); view_json.set(true); } }, EditorIcon { name: "file", size: 17 } }
                                button { class: "workflow-editor__tool-button", title: "Toggle node settings", aria_label: "Toggle node settings", r#type: "button", onclick: move |_| {
                                    if inspector_open() { inspector_open.set(false); }
                                    else { if !has_tasks { trigger_selected.set(true); } inspector_open.set(true); }
                                }, EditorIcon { name: "panel", size: 17 } }
                                button { class: "workflow-editor__tool-button workflow-editor__tool-button--accent", title: "Workflow help", aria_label: "Workflow help", r#type: "button", onclick: move |_| assistant_open.set(!assistant_open()), EditorIcon { name: "sparkles", size: 17 } }
                            }
                        }
                        if editor_tab() == "editor" {
                            div { class: "workflow-editor__zoom-controls",
                                button { class: "workflow-editor__tool-button", title: "Fit workflow", aria_label: "Fit workflow", r#type: "button", onclick: {
                                    let current_positions = positions.clone();
                                    move |_| {
                                        zoom_percent.set(fit_zoom(&current_positions, trigger_position, add_position));
                                        canvas_pan.set((0.0, 0.0));
                                    }
                                }, EditorIcon { name: "maximize", size: 16 } }
                                button { class: "workflow-editor__tool-button", title: "Zoom in", aria_label: "Zoom in", r#type: "button", onclick: move |_| zoom_percent.set((zoom_percent() + 10).min(160)), EditorIcon { name: "zoom-in", size: 16 } }
                                button { class: "workflow-editor__tool-button", title: "Zoom out", aria_label: "Zoom out", r#type: "button", onclick: move |_| zoom_percent.set((zoom_percent() - 10).max(40)), EditorIcon { name: "zoom-out", size: 16 } }
                                button { class: "workflow-editor__tool-button", title: "Reset view", aria_label: "Reset view", r#type: "button", onclick: move |_| { zoom_percent.set(100); canvas_pan.set((0.0, 0.0)); }, EditorIcon { name: "wand", size: 16 } }
                            }
                        }
                        if editor_tab() == "editor" && assistant_open() {
                            aside { class: "workflow-editor__assistant",
                                div { class: "workflow-editor__assistant-heading",
                                    strong { "Workflow help" }
                                    button { class: "workflow-editor__icon-action", title: "Close workflow help", aria_label: "Close workflow help", r#type: "button", onclick: move |_| assistant_open.set(false), EditorIcon { name: "x", size: 16 } }
                                }
                                p { "Drag nodes to reposition them and drag the blank canvas to pan. Use Ctrl/⌘ + wheel or pinch to zoom, +/− to zoom, 0 to reset, and 1 to fit." }
                                button { class: "button button-quiet", r#type: "button", onclick: move |_| { assistant_open.set(false); palette_open.set(true); }, EditorIcon { name: "plus", size: 14 } "Add a step" }
                            }
                        }
                        if editor_tab() == "editor" {
                            button {
                                class: "workflow-editor__execute",
                                r#type: "button",
                                disabled: validation.checking,
                                onclick: { let current = definition.clone(); move |_| on_validate.call(current.clone()) },
                                EditorIcon { name: "zap", size: 16 }
                                if validation.checking { "Validating…" } else { "Execute workflow" }
                            }
                        }
                        footer { class: "workflow-editor__logs",
                            button { class: "workflow-editor__logs-toggle", r#type: "button", onclick: move |_| logs_open.set(!logs_open()),
                                span { "Logs" }
                                if logs_open() { EditorIcon { name: "chevron-down", size: 14 } } else { EditorIcon { name: "chevron-up", size: 14 } }
                            }
                            if logs_open() { div { class: "workflow-editor__logs-content", "Run a workflow to see execution logs." } }
                        }
                    }

                    if inspector_open() {
                    div { class: "workflow-node-settings-overlay", role: "dialog", aria_modal: "true", aria_label: "Node settings",
                    aside { class: "workflow-editor__inspector workflow-node-settings-modal panel",
                        div { class: "workflow-editor__section-heading",
                            div { class: "workflow-editor__inspector-title",
                                p { class: "workflow-editor__eyebrow", "NODE SETTINGS" }
                                strong { if trigger_selected() { "{trigger_name}" } else if selected_task.is_some() { "{selected_name}" } else { "No node selected" } }
                            }
                            div { class: "workflow-editor__inspector-actions",
                                if selected_task.is_some() && !trigger_selected() {
                                    button {
                                        class: "workflow-editor__icon-action workflow-editor__icon-action--danger",
                                        title: "Delete node",
                                        aria_label: "Delete node",
                                        r#type: "button",
                                        onclick: {
                                        let mut next = document.clone();
                                        move |_| {
                                            let mut next_tasks = next["do"].as_array().cloned().unwrap_or_default();
                                            if let Some(removed_name) = next_tasks.get(selected).and_then(task_name) { remove_canvas_connections(&mut next, &removed_name, &next_tasks); }
                                            if selected < next_tasks.len() { next_tasks.remove(selected); }
                                            let is_empty = next_tasks.is_empty();
                                            next["do"] = Value::Array(next_tasks);
                                            selected_index.set(selected.saturating_sub(1));
                                            if is_empty { inspector_open.set(false); }
                                            on_change.call(format_json(&next));
                                        }
                                        },
                                        EditorIcon { name: "trash", size: 16 }
                                    }
                                }
                                button { class: "workflow-editor__icon-action", title: "Close node settings", aria_label: "Close node settings", r#type: "button", onclick: move |_| inspector_open.set(false), EditorIcon { name: "x", size: 16 } }
                            }
                        }
                        if trigger_selected() {
                            label { class: "field",
                                span { "Trigger name" }
                                input {
                                    value: "{trigger_name}",
                                    oninput: {
                                        let mut next = document.clone();
                                        move |event| {
                                            set_manual_trigger_field(&mut next, "name", Value::String(event.value()));
                                            on_change.call(format_json(&next));
                                        }
                                    }
                                }
                            }
                            label { class: "field",
                                span { "Description" }
                                input {
                                    placeholder: "Describe when this workflow should run",
                                    value: "{trigger_description}",
                                    oninput: {
                                        let mut next = document.clone();
                                        move |event| {
                                            set_manual_trigger_field(&mut next, "description", Value::String(event.value()));
                                            on_change.call(format_json(&next));
                                        }
                                    }
                                }
                            }
                            div { class: "workflow-editor__definition-field",
                                label { class: "field",
                                    span { "Sample input · JSON" }
                                    textarea {
                                        class: "code-editor workflow-editor__definition",
                                        value: "{trigger_input_draft}",
                                        spellcheck: "false",
                                        oninput: move |event| trigger_input_draft.set(event.value()),
                                    }
                                }
                                button {
                                    class: "button button-primary",
                                    r#type: "button",
                                    onclick: {
                                        let mut next = document.clone();
                                        move |_| match serde_json::from_str::<Value>(&trigger_input_draft()) {
                                            Ok(value) => {
                                                set_manual_trigger_field(&mut next, "input", value);
                                                edit_error.set(None);
                                                on_change.call(format_json(&next));
                                            }
                                            Err(_) => edit_error.set(Some("Sample input must be valid JSON".into())),
                                        }
                                    },
                                    "Apply sample input"
                                }
                            }
                        } else if selected_task.is_some() {
                            label { class: "field",
                                span { "Node name" }
                                input {
                                    value: "{selected_name}",
                                    oninput: {
                                        let mut next = document.clone();
                                        let value = selected_definition.clone();
                                        let old_name = selected_name.clone();
                                        move |event| {
                                            let new_name = event.value();
                                            let current_name = next["do"].as_array().and_then(|nodes| nodes.get(selected)).and_then(task_name).unwrap_or_else(|| old_name.clone());
                                            rename_canvas_node(&mut next, &current_name, &new_name);
                                            replace_task_definition(&mut next, selected, &new_name, value.clone());
                                            on_change.call(format_json(&next));
                                        }
                                    }
                                }
                            }
                            div { class: "workflow-editor__move-actions",
                                button {
                                    class: "button button-quiet",
                                    r#type: "button",
                                    disabled: selected == 0,
                                    onclick: {
                                        let mut next = document.clone();
                                        move |_| {
                                            let mut nodes = next["do"].as_array().cloned().unwrap_or_default();
                                            if selected > 0 && selected < nodes.len() { nodes.swap(selected, selected - 1); selected_index.set(selected - 1); next["do"] = Value::Array(nodes); on_change.call(format_json(&next)); }
                                        }
                                    },
                                    "↑ Move up"
                                }
                                button {
                                    class: "button button-quiet",
                                    r#type: "button",
                                    disabled: selected + 1 >= tasks.len(),
                                    onclick: {
                                        let mut next = document.clone();
                                        move |_| {
                                            let mut nodes = next["do"].as_array().cloned().unwrap_or_default();
                                            if selected + 1 < nodes.len() { nodes.swap(selected, selected + 1); selected_index.set(selected + 1); next["do"] = Value::Array(nodes); on_change.call(format_json(&next)); }
                                        }
                                    },
                                    "↓ Move down"
                                }
                            }
                            if selected_type == "call" {
                                div { class: "workflow-editor__definition-field",
                                    div { class: "workflow-editor__definition-heading",
                                        span { "Call settings" }
                                        small { "Configure the operation and its inputs." }
                                    }
                                    label { class: "field",
                                        span { "Call target" }
                                        input {
                                            placeholder: "core.noop",
                                            value: "{call_target}",
                                            oninput: {
                                                let mut next = document.clone();
                                                let mut definition = selected_definition.clone();
                                                let task_name = selected_name.clone();
                                                move |event| {
                                                    definition["call"] = Value::String(event.value());
                                                    replace_task_definition(&mut next, selected, &task_name, definition.clone());
                                                    on_change.call(format_json(&next));
                                                }
                                            }
                                        }
                                    }
                                    if let Some(inputs) = selected_definition["with"].as_object() {
                                        div { class: "workflow-editor__call-inputs",
                                            div { class: "workflow-editor__call-input-heading", strong { "Input values" } small { "Expressions can use the OWS expression syntax." } }
                                            for (key, value) in inputs.iter() {
                                                {
                                                    let key = key.clone();
                                                    let field_value = display_form_value(value);
                                                    rsx! {
                                                        label { class: "field",
                                                            span { "{key}" }
                                                            input {
                                                                value: "{field_value}",
                                                                oninput: {
                                                                    let mut next = document.clone();
                                                                    let mut definition = selected_definition.clone();
                                                                    let task_name = selected_name.clone();
                                                                    let key = key.clone();
                                                                    move |event| {
                                                                        definition["with"][key.clone()] = Value::String(event.value());
                                                                        replace_task_definition(&mut next, selected, &task_name, definition.clone());
                                                                        on_change.call(format_json(&next));
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    button {
                                        class: "button button-quiet workflow-editor__add-input",
                                        r#type: "button",
                                        onclick: {
                                            let mut next = document.clone();
                                            let mut definition = selected_definition.clone();
                                            let task_name = selected_name.clone();
                                            move |_| {
                                                if !definition["with"].is_object() { definition["with"] = json!({}); }
                                                if let Some(inputs) = definition["with"].as_object_mut() {
                                                    let mut index = inputs.len() + 1;
                                                    let mut key = format!("input_{index}");
                                                    while inputs.contains_key(&key) { index += 1; key = format!("input_{index}"); }
                                                    inputs.insert(key, Value::String(String::new()));
                                                }
                                                replace_task_definition(&mut next, selected, &task_name, definition.clone());
                                                on_change.call(format_json(&next));
                                            }
                                        },
                                        "+ Add input"
                                    }
                                }
                            } else {
                                label { class: "field workflow-editor__definition-field",
                                    span { "Definition · {pretty_type(selected_type)}" }
                                    textarea {
                                        class: "code-editor workflow-editor__definition",
                                        value: "{task_draft}",
                                        spellcheck: "false",
                                        oninput: move |event| task_draft.set(event.value()),
                                    }
                                    button {
                                        class: "button button-primary",
                                        r#type: "button",
                                        onclick: {
                                            let mut next = document.clone();
                                            let task_name = selected_name.clone();
                                            move |_| match serde_json::from_str::<Value>(&task_draft()) {
                                                Ok(value) if value.is_object() => {
                                                    replace_task_definition(&mut next, selected, &task_name, value);
                                                    edit_error.set(None);
                                                    on_change.call(format_json(&next));
                                                }
                                                Ok(_) => edit_error.set(Some("Node definition must be a JSON object".into())),
                                                Err(_) => edit_error.set(Some("Node definition must be valid JSON".into())),
                                            }
                                        },
                                        "Apply changes"
                                    }
                                }
                            }
                        } else {
                            p { class: "workflow-editor__hint", "Select a node on the canvas to edit its settings." }
                        }
                        if let Some(message) = edit_error() { p { class: "workflow-editor__inline-error", "{message}" } }
                    }
                    }
                    }
                }
            }

            if !validation.errors.is_empty() {
                div { class: "ows-validation ows-invalid",
                    strong { "Validation issues" }
                    ul { for (index, error) in validation.errors.iter().enumerate() { li { key: "{index}", "{error}" } } }
                }
            }
        }
    }
}

fn starter_document() -> Value {
    json!({
        "document": {
            "dsl":"1.0.3",
            "namespace":"default",
            "name":"new-workflow",
            "version":"1.0.0",
            "metadata": {
                "ferrisflow": {
                    "manual_trigger": {
                        "name": "Manual Trigger",
                        "description": "",
                        "input": {}
                    }
                }
            }
        },
        "do":[]
    })
}

fn manual_trigger_config(document: &Value) -> Value {
    document["document"]["metadata"]["ferrisflow"]["manual_trigger"]
        .as_object()
        .map(|config| Value::Object(config.clone()))
        .unwrap_or_else(|| json!({"name":"Manual Trigger","description":"","input":{}}))
}

fn set_manual_trigger_field(document: &mut Value, field: &str, value: Value) {
    if !document["document"]["metadata"].is_object() {
        document["document"]["metadata"] = json!({});
    }
    if !document["document"]["metadata"]["ferrisflow"].is_object() {
        document["document"]["metadata"]["ferrisflow"] = json!({});
    }
    if !document["document"]["metadata"]["ferrisflow"]["manual_trigger"].is_object() {
        document["document"]["metadata"]["ferrisflow"]["manual_trigger"] = json!({
            "name":"Manual Trigger",
            "description":"",
            "input":{}
        });
    }
    document["document"]["metadata"]["ferrisflow"]["manual_trigger"][field] = value;
}

fn read_canvas_positions(document: &Value, tasks: &[Value], start_x: f64) -> Vec<(f64, f64)> {
    tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let name = task_name(task).unwrap_or_default();
            let position = &document["document"]["metadata"]["ferrisflow"]["canvas"]["positions"][name];
            match (position["x"].as_f64(), position["y"].as_f64()) {
                (Some(x), Some(y)) => (x.max(0.0), y.max(0.0)),
                _ => (start_x + 260.0 + index as f64 * 230.0, 380.0),
            }
        })
        .collect()
}

fn read_trigger_position(document: &Value, fallback: (f64, f64)) -> (f64, f64) {
    let position = &document["document"]["metadata"]["ferrisflow"]["canvas"]["trigger"];
    match (position["x"].as_f64(), position["y"].as_f64()) {
        (Some(x), Some(y)) => (x.max(0.0), y.max(0.0)),
        _ => fallback,
    }
}

fn save_canvas_positions(
    document: &mut Value,
    tasks: &[Value],
    positions: &[(f64, f64)],
    trigger_position: (f64, f64),
) {
    if !document["document"]["metadata"].is_object() {
        document["document"]["metadata"] = json!({});
    }
    if !document["document"]["metadata"]["ferrisflow"].is_object() {
        document["document"]["metadata"]["ferrisflow"] = json!({});
    }
    if !document["document"]["metadata"]["ferrisflow"]["canvas"].is_object() {
        document["document"]["metadata"]["ferrisflow"]["canvas"] = json!({});
    }
    let node_positions = tasks
        .iter()
        .zip(positions.iter())
        .map(|(task, (x, y))| (task_name(task).unwrap_or_default(), json!({"x":x,"y":y})))
        .collect::<Map<_, _>>();
    document["document"]["metadata"]["ferrisflow"]["canvas"]["positions"] = Value::Object(node_positions);
    document["document"]["metadata"]["ferrisflow"]["canvas"]["trigger"] =
        json!({"x":trigger_position.0,"y":trigger_position.1});
}

fn read_canvas_connections(document: &Value, tasks: &[Value]) -> Vec<(String, String)> {
    if let Some(connections) = document["document"]["metadata"]["ferrisflow"]["canvas"]["connections"].as_array() {
        return connections
            .iter()
            .filter_map(|edge| Some((edge["from"].as_str()?.to_owned(), edge["to"].as_str()?.to_owned())))
            .collect();
    }
    let mut chain = Vec::with_capacity(tasks.len());
    let mut previous = TRIGGER_CONNECTION_ID.to_owned();
    for task in tasks {
        if let Some(name) = task_name(task) {
            chain.push((previous, name.clone()));
            previous = name;
        }
    }
    chain
}

fn input_port_offsets(count: usize) -> Vec<f64> {
    (0..count)
        .map(|index| 96.0 * (index + 1) as f64 / (count + 1) as f64)
        .collect()
}

fn output_port_offsets(count: usize) -> Vec<f64> {
    input_port_offsets(count)
}

fn connection_source_id(name: &str, output_index: usize) -> String {
    if output_index == 0 { name.to_owned() } else { format!("{name}::{output_index}") }
}

fn split_connection_source(source: &str) -> (&str, usize) {
    source.rsplit_once("::")
        .and_then(|(name, index)| index.parse::<usize>().ok().map(|index| (name, index)))
        .unwrap_or((source, 0))
}

fn connection_source_node(source: &str) -> &str {
    split_connection_source(source).0
}

fn node_output_count(task: &Value) -> usize {
    let definition = task_definition(task).unwrap_or(Value::Null);
    let kind = definition.as_object().and_then(|object| object.keys().next()).map(String::as_str).unwrap_or_default();
    match kind {
        "switch" => definition["switch"].as_array().map(|branches| branches.len() + 1).unwrap_or(2).max(2),
        "for" | "try" => 2,
        _ => 1,
    }
}

fn node_output_label(kind: &str, index: usize, count: usize) -> String {
    match kind {
        "switch" if count == 2 => if index == 0 { "true".into() } else { "false".into() },
        "switch" => if index + 1 == count { "default".into() } else { format!("case {}", index + 1) },
        "for" => if index == 0 { "loop".into() } else { "done".into() },
        "try" => if index == 0 { "success".into() } else { "error".into() },
        _ => String::new(),
    }
}

fn canvas_node_position(
    name: &str,
    tasks: &[Value],
    positions: &[(f64, f64)],
    trigger_position: (f64, f64),
) -> Option<(f64, f64)> {
    let name = connection_source_node(name);
    if name == TRIGGER_CONNECTION_ID {
        return Some(trigger_position);
    }
    tasks.iter().position(|task| task_name(task).as_deref() == Some(name)).and_then(|index| positions.get(index).copied())
}

fn write_canvas_node_position(document: &mut Value, name: &str, position: (f64, f64)) {
    if !document["document"]["metadata"].is_object() { document["document"]["metadata"] = json!({}); }
    if !document["document"]["metadata"]["ferrisflow"].is_object() { document["document"]["metadata"]["ferrisflow"] = json!({}); }
    if !document["document"]["metadata"]["ferrisflow"]["canvas"].is_object() { document["document"]["metadata"]["ferrisflow"]["canvas"] = json!({}); }
    if !document["document"]["metadata"]["ferrisflow"]["canvas"]["positions"].is_object() { document["document"]["metadata"]["ferrisflow"]["canvas"]["positions"] = json!({}); }
    document["document"]["metadata"]["ferrisflow"]["canvas"]["positions"][name] = json!({"x":position.0,"y":position.1});
}

fn add_canvas_connection(document: &mut Value, from: &str, to: &str) -> bool {
    if from == to || to == TRIGGER_CONNECTION_ID {
        return false;
    }
    let tasks = document["do"].as_array().cloned().unwrap_or_default();
    let source_node = connection_source_node(from);
    let is_valid_source = source_node == TRIGGER_CONNECTION_ID || tasks.iter().any(|task| task_name(task).as_deref() == Some(source_node));
    let is_valid_target = tasks.iter().any(|task| task_name(task).as_deref() == Some(to));
    if !is_valid_source || !is_valid_target {
        return false;
    }
    let mut connections = read_canvas_connections(document, &tasks);
    if connections.iter().any(|edge| edge.0 == from && edge.1 == to) {
        return false;
    }
    let mut pending = vec![to.to_owned()];
    let mut visited = std::collections::HashSet::new();
    while let Some(current) = pending.pop() {
        if current == source_node || !visited.insert(current.clone()) {
            if current == source_node { return false; }
            continue;
        }
        pending.extend(connections.iter().filter(|(source, _)| connection_source_node(source) == current).map(|(_, target)| target.clone()));
    }
    connections.push((from.to_owned(), to.to_owned()));
    write_canvas_connections(document, &connections);
    true
}

fn remove_specific_canvas_connection(document: &mut Value, from: &str, to: &str) {
    let tasks = document["do"].as_array().cloned().unwrap_or_default();
    let connections = read_canvas_connections(document, &tasks)
        .into_iter()
        .filter(|edge| edge.0 != from || edge.1 != to)
        .collect::<Vec<_>>();
    write_canvas_connections(document, &connections);
}

fn remove_canvas_connections(document: &mut Value, removed: &str, tasks: &[Value]) {
    let connections = read_canvas_connections(document, tasks);
    let incoming = connections.iter().filter(|(_, to)| to == removed).map(|(from, _)| from.clone()).collect::<Vec<_>>();
    let outgoing = connections.iter().filter(|(from, _)| connection_source_node(from) == removed).map(|(_, to)| to.clone()).collect::<Vec<_>>();
    let mut remaining = connections.into_iter().filter(|(from, to)| connection_source_node(from) != removed && to != removed).collect::<Vec<_>>();
    for from in &incoming {
        for to in &outgoing {
            if from != to && !remaining.iter().any(|edge| edge.0 == *from && edge.1 == *to) {
                remaining.push((from.clone(), to.clone()));
            }
        }
    }
    write_canvas_connections(document, &remaining);
    if let Some(positions) = document["document"]["metadata"]["ferrisflow"]["canvas"]["positions"].as_object_mut() {
        positions.remove(removed);
    }
}

fn rename_canvas_node(document: &mut Value, old: &str, new: &str) {
    if old == new {
        return;
    }
    let tasks = document["do"].as_array().cloned().unwrap_or_default();
    let mut connections = read_canvas_connections(document, &tasks);
    for (from, to) in &mut connections {
        let (source_node, output_index) = split_connection_source(from);
        if source_node == old { *from = connection_source_id(new, output_index); }
        if to == old { *to = new.to_owned(); }
    }
    write_canvas_connections(document, &connections);
    if let Some(positions) = document["document"]["metadata"]["ferrisflow"]["canvas"]["positions"].as_object_mut() {
        if let Some(position) = positions.remove(old) {
            positions.insert(new.to_owned(), position);
        }
    }
}

fn write_canvas_connections(document: &mut Value, connections: &[(String, String)]) {
    if !document["document"]["metadata"].is_object() {
        document["document"]["metadata"] = json!({});
    }
    if !document["document"]["metadata"]["ferrisflow"].is_object() {
        document["document"]["metadata"]["ferrisflow"] = json!({});
    }
    if !document["document"]["metadata"]["ferrisflow"]["canvas"].is_object() {
        document["document"]["metadata"]["ferrisflow"]["canvas"] = json!({});
    }
    document["document"]["metadata"]["ferrisflow"]["canvas"]["connections"] = Value::Array(
        connections.iter().map(|(from, to)| json!({"from":from,"to":to})).collect(),
    );
}

fn node_is_disabled(document: &Value, name: &str) -> bool {
    document["document"]["metadata"]["ferrisflow"]["canvas"]["disabled_nodes"][name]
        .as_bool()
        .unwrap_or(false)
}

fn set_node_disabled(document: &mut Value, name: &str, disabled: bool) {
    if !document["document"]["metadata"].is_object() {
        document["document"]["metadata"] = json!({});
    }
    if !document["document"]["metadata"]["ferrisflow"].is_object() {
        document["document"]["metadata"]["ferrisflow"] = json!({});
    }
    if !document["document"]["metadata"]["ferrisflow"]["canvas"].is_object() {
        document["document"]["metadata"]["ferrisflow"]["canvas"] = json!({});
    }
    if !document["document"]["metadata"]["ferrisflow"]["canvas"]["disabled_nodes"].is_object() {
        document["document"]["metadata"]["ferrisflow"]["canvas"]["disabled_nodes"] = json!({});
    }
    document["document"]["metadata"]["ferrisflow"]["canvas"]["disabled_nodes"][name] =
        Value::Bool(disabled);
}

fn duplicate_task(document: &mut Value, index: usize) -> Option<usize> {
    let mut tasks = document["do"].as_array().cloned()?;
    let original = tasks.get(index)?.clone();
    let name = task_name(&original)?;
    let definition = task_definition(&original)?;
    let existing = tasks
        .iter()
        .filter_map(task_name)
        .collect::<std::collections::HashSet<_>>();
    let base = format!("{name}-copy");
    let mut duplicate_name = base.clone();
    let mut suffix = 2;
    while existing.contains(&duplicate_name) {
        duplicate_name = format!("{base}-{suffix}");
        suffix += 1;
    }
    let mut duplicate = Map::new();
    duplicate.insert(duplicate_name, definition);
    let duplicate_index = index + 1;
    tasks.insert(duplicate_index, Value::Object(duplicate));
    document["do"] = Value::Array(tasks);
    Some(duplicate_index)
}

fn connector_path(from: (f64, f64), to: (f64, f64)) -> String {
    let from_x = from.0 + 116.0;
    let from_y = from.1 + 48.0;
    let to_x = to.0 + 16.0;
    let to_y = to.1 + 48.0;
    let control = ((to_x - from_x).abs() * 0.45).max(35.0);
    format!("M {from_x} {from_y} C {} {from_y}, {} {to_y}, {to_x} {to_y}", from_x + control, to_x - control)
}

fn connector_preview_path(from: (f64, f64), to: (f64, f64)) -> String {
    let start = (from.0 + 116.0, from.1 + 48.0);
    let end = (to.0 + 16.0, to.1 + 48.0);
    let control = ((end.0 - start.0).abs() * 0.45).max(35.0);
    format!("M {} {} C {} {}, {} {}, {} {}", start.0, start.1, start.0 + control, start.1, end.0 - control, end.1, end.0, end.1)
}

fn fit_zoom(positions: &[(f64, f64)], trigger: (f64, f64), add: (f64, f64)) -> i32 {
    let min_x = positions.iter().map(|(x, _)| *x).fold(trigger.0, f64::min);
    let max_x = positions.iter().map(|(x, _)| *x + 132.0).fold(add.0 + 30.0, f64::max);
    let min_y = positions.iter().map(|(_, y)| *y).fold(trigger.1, f64::min);
    let max_y = positions.iter().map(|(_, y)| *y + 140.0).fold(trigger.1 + 140.0, f64::max);
    let width = (max_x - min_x + 80.0).max(360.0);
    let height = (max_y - min_y + 80.0).max(240.0);
    let width_zoom = 1120.0 / width * 100.0;
    let height_zoom = 560.0 / height * 100.0;
    width_zoom.min(height_zoom).min(100.0).round().clamp(45.0, 100.0) as i32
}

fn parse_document(raw: &str) -> Result<Value, String> {
    let value: Value = if raw.trim().is_empty() {
        starter_document()
    } else {
        serde_json::from_str(raw).map_err(|error| error.to_string())?
    };
    if !value["document"].is_object() || !value["do"].is_array() {
        return Err("An OWS document needs document metadata and a do task list.".into());
    }
    Ok(value)
}

fn task_name(task: &Value) -> Option<String> {
    task.as_object()?.keys().next().cloned()
}

fn task_definition(task: &Value) -> Option<Value> {
    task.get(task_name(task)?).cloned()
}

fn replace_task_definition(document: &mut Value, index: usize, name: &str, definition: Value) {
    let mut task = Map::new();
    task.insert(name.to_owned(), definition);
    if let Some(tasks) = document["do"].as_array_mut() {
        if let Some(slot) = tasks.get_mut(index) {
            *slot = Value::Object(task);
        }
    }
}

fn display_form_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

fn create_task(kind: &str, tasks: &[Value]) -> Value {
    let mut index = tasks.len() + 1;
    let mut name = format!("{kind}-{index}");
    while tasks.iter().any(|task| task_name(task).as_deref() == Some(name.as_str())) {
        index += 1;
        name = format!("{kind}-{index}");
    }
    let definition = match kind {
        "set" => json!({"set":{"value":""}}),
        "call" => json!({"call":"core.noop"}),
        "wait" => json!({"wait":{"duration":"PT1S"}}),
        "emit" => json!({"emit":{"event":{"type":"workflow.event"}}}),
        "switch" => json!({"switch":[{"when":"${ true }","then":""}]}),
        "for" => json!({"for":{"each":"item","in":"${ .items }","do":[]}}),
        "do" => json!({"do":[]}),
        "try" => json!({"try":[]}),
        _ => json!({"set":{"value":""}}),
    };
    json!({(name):definition})
}

fn format_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into())
}

fn pretty_type(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn node_icon(kind: &str) -> &'static str {
    match kind {
        "call" => "workflow",
        "switch" => "git-branch",
        "wait" => "clock",
        "emit" => "radio",
        "for" => "repeat",
        "try" => "shield-alert",
        "do" => "list",
        _ => "file-pen",
    }
}

fn node_description(kind: &str) -> &'static str {
    match kind {
        "set" => "Set or transform workflow data",
        "call" => "Call an operation or service",
        "wait" => "Pause for a duration or event",
        "emit" => "Publish a workflow event",
        "switch" => "Choose a path based on conditions",
        "for" => "Repeat steps for each item",
        "do" => "Run a group of steps in sequence",
        "try" => "Run steps with error handling",
        _ => "Add a workflow step",
    }
}

#[component]
fn EditorIcon(name: &'static str, #[props(default = 16)] size: u32) -> Element {
    let _ = (name, size);
    let class = "ui-icon";
    match name {
        "arrow-down" => rsx! { ArrowDown { size, class } },
        "arrow-right" => rsx! { ArrowRight { size, class } },
        "arrow-up" => rsx! { ArrowUp { size, class } },
        "arrow-up-right" => rsx! { ArrowUp { size, class } },
        "chevron-down" => rsx! { ChevronDown { size, class } },
        "chevron-up" => rsx! { ChevronUp { size, class } },
        "clock" => rsx! { Clock { size, class } },
        "file" => rsx! { File { size, class } },
        "file-pen" => rsx! { FilePenLine { size, class } },
        "ellipsis" => rsx! { Ellipsis { size, class } },
        "git-branch" => rsx! { GitBranch { size, class } },
        "layout" => rsx! { LayoutDashboard { size, class } },
        "list" => rsx! { List { size, class } },
        "maximize" => rsx! { Maximize { size, class } },
        "panel" => rsx! { PanelRight { size, class } },
        "play" => rsx! { Play { size, class } },
        "plus" => rsx! { Plus { size, class } },
        "power" => rsx! { Power { size, class } },
        "radio" => rsx! { Radio { size, class } },
        "repeat" => rsx! { Repeat { size, class } },
        "search" => rsx! { Search { size, class } },
        "shield-alert" => rsx! { ShieldAlert { size, class } },
        "sparkles" => rsx! { Sparkles { size, class } },
        "table" => rsx! { Table { size, class } },
        "trash" => rsx! { Trash2 { size, class } },
        "wand" => rsx! { Wand { size, class } },
        "workflow" => rsx! { Workflow { size, class } },
        "zap" => rsx! { Zap { size, class } },
        "zoom-in" => rsx! { ZoomIn { size, class } },
        "zoom-out" => rsx! { ZoomOut { size, class } },
        "x" => rsx! { X { size, class } },
        _ => rsx! { Workflow { size, class } },
    }
}
