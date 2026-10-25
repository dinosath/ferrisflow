use dioxus::prelude::*;
use dioxus_icons::lucide::{
    arrow_down::ArrowDown, arrow_right::ArrowRight, arrow_up::ArrowUp, chevron_down::ChevronDown,
    chevron_up::ChevronUp, clock::Clock, file::File, file_pen_line::FilePenLine,
    git_branch::GitBranch, layout_dashboard::LayoutDashboard, list::List, maximize::Maximize,
    panel_right::PanelRight, plus::Plus, radio::Radio, repeat::Repeat, search::Search,
    shield_alert::ShieldAlert, sparkles::Sparkles, table::Table, trash_2::Trash2, wand::Wand,
    workflow::Workflow, x::X, zap::Zap, zoom_in::ZoomIn, zoom_out::ZoomOut,
};
use serde_json::{Map, Value, json};

const TASK_TYPES: [&str; 8] = ["set", "call", "wait", "emit", "switch", "for", "do", "try"];

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
    let mut zoom_percent = use_signal(|| 100_i32);
    let mut editor_tab = use_signal(|| "editor".to_string());
    let mut logs_open = use_signal(|| false);
    let mut json_draft = use_signal(|| definition.clone());
    let mut task_draft = use_signal(|| "{}".to_string());
    let mut trigger_input_draft = use_signal(|| "{}".to_string());
    let mut edit_error = use_signal(|| None::<String>);

    let document = parse_document(&definition).unwrap_or_else(|_| starter_document());
    let tasks = document["do"].as_array().cloned().unwrap_or_default();
    let has_tasks = !tasks.is_empty();
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
                                        p { "Choose a node to append it to the workflow." }
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
                                                move |_| {
                                                    let mut next_tasks = next["do"].as_array().cloned().unwrap_or_default();
                                                    let new_task = create_task(node_type, &next_tasks);
                                                    next_tasks.push(new_task);
                                                    selected_index.set(next_tasks.len() - 1);
                                                    trigger_selected.set(false);
                                                    inspector_open.set(true);
                                                    palette_open.set(false);
                                                    next["do"] = Value::Array(next_tasks);
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
                        div { class: "workflow-editor__canvas", style: "zoom: {zoom_percent()}%;",
                            div { class: "workflow-node-group workflow-node-group--start",
                                button {
                                    class: if trigger_selected() { "workflow-editor__start-pill is-selected" } else { "workflow-editor__start-pill" },
                                    r#type: "button",
                                    aria_label: "Edit manual trigger",
                                    title: "Edit manual trigger",
                                    onclick: {
                                        let input = trigger_input.clone();
                                        move |_| {
                                            trigger_selected.set(true);
                                            inspector_open.set(true);
                                            trigger_input_draft.set(format_json(&input));
                                            edit_error.set(None);
                                        }
                                    },
                                    span { EditorIcon { name: "zap", size: 30 } }
                                }
                                div { class: "workflow-node-label", strong { "{trigger_name}" } small { "Trigger" } }
                            }
                            div { class: "workflow-editor__connector workflow-editor__connector--short" }
                            if tasks.is_empty() {
                                div { class: "workflow-editor__canvas-empty",
                                    strong { "Your workflow starts here" }
                                    span { "Choose a node from the library to build the execution path." }
                                }
                            }
                            for (index, task) in tasks.iter().enumerate() {
                                {
                                    let name = task_name(task).unwrap_or_else(|| "Unnamed task".into());
                                    let kind = task_definition(task)
                                        .and_then(|value| value.as_object().and_then(|object| object.keys().next().cloned()))
                                        .unwrap_or_else(|| "task".into());
                                    let icon = node_icon(&kind);
                                    rsx! {
                                        div { class: "workflow-editor__connector", div { class: "workflow-editor__connector-line" } span { EditorIcon { name: "arrow-right", size: 14 } } }
                                        div { class: "workflow-node-group",
                                            button {
                                                class: if index == selected { "workflow-node is-selected" } else { "workflow-node" },
                                                r#type: "button",
                                                aria_label: "Edit workflow step",
                                                onclick: {
                                                    let task_value = task_definition(task).unwrap_or_else(|| json!({}));
                                                    move |_| { trigger_selected.set(false); selected_index.set(index); inspector_open.set(true); task_draft.set(format_json(&task_value)); edit_error.set(None); }
                                                },
                                                span { class: "workflow-node__port workflow-node__port--in" }
                                                span { class: "workflow-node__icon", EditorIcon { name: icon, size: 30 } }
                                                span { class: "workflow-node__port workflow-node__port--out" }
                                            }
                                            div { class: "workflow-node-label", strong { "{name}" } small { "{pretty_type(&kind)}" } }
                                        }
                                    }
                                }
                            }
                            div { class: "workflow-editor__connector workflow-editor__connector--short", div { class: "workflow-editor__connector-line" } span { EditorIcon { name: "arrow-right", size: 14 } } }
                            button { class: "workflow-flow-add", title: "Add a workflow node", aria_label: "Add a workflow node", r#type: "button", onclick: move |_| palette_open.set(true), EditorIcon { name: "plus", size: 18 } }
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
                                button { class: "workflow-editor__tool-button", title: "Add node", aria_label: "Add node", r#type: "button", onclick: move |_| palette_open.set(!palette_open()), EditorIcon { name: "plus", size: 18 } }
                                button { class: "workflow-editor__tool-button", title: "Search nodes", aria_label: "Search nodes", r#type: "button", onclick: move |_| palette_open.set(true), EditorIcon { name: "search", size: 17 } }
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
                                    let node_count = tasks.len();
                                    move |_| {
                                        let fit = (1100.0 / (230.0 + node_count as f64 * 210.0) * 100.0).round() as i32;
                                        zoom_percent.set(fit.clamp(45, 100));
                                    }
                                }, EditorIcon { name: "maximize", size: 16 } }
                                button { class: "workflow-editor__tool-button", title: "Zoom in", aria_label: "Zoom in", r#type: "button", onclick: move |_| zoom_percent.set((zoom_percent() + 10).min(160)), EditorIcon { name: "zoom-in", size: 16 } }
                                button { class: "workflow-editor__tool-button", title: "Zoom out", aria_label: "Zoom out", r#type: "button", onclick: move |_| zoom_percent.set((zoom_percent() - 10).max(40)), EditorIcon { name: "zoom-out", size: 16 } }
                                button { class: "workflow-editor__tool-button", title: "Reset layout", aria_label: "Reset layout", r#type: "button", onclick: move |_| zoom_percent.set(100), EditorIcon { name: "wand", size: 16 } }
                            }
                        }
                        if editor_tab() == "editor" && assistant_open() {
                            aside { class: "workflow-editor__assistant",
                                div { class: "workflow-editor__assistant-heading",
                                    strong { "Workflow help" }
                                    button { class: "workflow-editor__icon-action", title: "Close workflow help", aria_label: "Close workflow help", r#type: "button", onclick: move |_| assistant_open.set(false), EditorIcon { name: "x", size: 16 } }
                                }
                                p { "Add steps from the node library, select a node to edit it, and use Move up or Move down to change execution order." }
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
                    aside { class: "workflow-editor__inspector panel",
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
                                        move |event| {
                                            let mut task = Map::new();
                                            task.insert(event.value(), value.clone());
                                            if let Some(nodes) = next["do"].as_array_mut() { if selected < nodes.len() { nodes[selected] = Value::Object(task); } }
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
        "git-branch" => rsx! { GitBranch { size, class } },
        "layout" => rsx! { LayoutDashboard { size, class } },
        "list" => rsx! { List { size, class } },
        "maximize" => rsx! { Maximize { size, class } },
        "panel" => rsx! { PanelRight { size, class } },
        "plus" => rsx! { Plus { size, class } },
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
