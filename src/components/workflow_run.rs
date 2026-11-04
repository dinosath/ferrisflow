use dioxus::prelude::*;

use crate::{
    components::icons::Icon,
    pages::{
        api::execute_saved_workflow,
        models::{ExecutionResult, Workflow},
    },
};

#[component]
pub fn WorkflowRunDialog(
    workflows: Vec<Workflow>, selected_workflow: Option<i32>, on_close: EventHandler<()>,
    on_complete: EventHandler<()>, #[props(default)] initial_input: String,
) -> Element {
    let first_id = workflows.first().map(|workflow| workflow.id).unwrap_or_default();
    let mut selected_id = use_signal(|| selected_workflow.unwrap_or(first_id));
    let mut input_json =
        use_signal(
            || if initial_input.is_empty() { "{}".to_string() } else { initial_input.clone() },
        );
    let mut running = use_signal(|| false);
    let mut result = use_signal(|| None::<ExecutionResult>);
    let mut error = use_signal(|| None::<String>);
    let selected_workflow = workflows.iter().find(|workflow| workflow.id == selected_id());
    let ui = selected_workflow
        .and_then(|workflow| {
            serde_json::from_str::<serde_json::Value>(&workflow.definition_json).ok()
        })
        .and_then(|definition| definition.get("document")?.get("metadata")?.get("ui").cloned())
        .unwrap_or_default();
    let view_type = ui
        .get("view")
        .and_then(|view| view.get("type"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("json");
    let view_config =
        ui.get("view").and_then(|view| view.get("config")).cloned().unwrap_or_default();
    let schema_properties = ui
        .get("input_schema")
        .and_then(|schema| schema.get("properties"))
        .and_then(serde_json::Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(name, schema)| (name.clone(), schema.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let input_properties = if view_type == "custom" {
        view_config
            .get("fields")
            .and_then(serde_json::Value::as_array)
            .map(|fields| {
                fields
                    .iter()
                    .filter_map(|field| {
                        let name = field.get("name")?.as_str()?;
                        let mut schema =
                            schema_properties.iter().find(|(key, _)| key == name)?.1.clone();
                        if let Some(label) = field.get("label").and_then(serde_json::Value::as_str)
                        {
                            schema["title"] = serde_json::Value::String(label.to_owned());
                        }
                        if let Some(placeholder) =
                            field.get("placeholder").and_then(serde_json::Value::as_str)
                        {
                            schema["placeholder"] =
                                serde_json::Value::String(placeholder.to_owned());
                        }
                        Some((name.to_owned(), schema))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or(schema_properties)
    } else {
        schema_properties
    };

    rsx! {
        div { class: "workflow-run-scrim", role: "presentation",
            section { class: "workflow-run-dialog", role: "dialog", aria_modal: "true", aria_labelledby: "workflow-run-title",
                header { class: "workflow-run-dialog__header",
                    div {
                        h2 { id: "workflow-run-title", "Run workflow" }
                        p { "Choose a workflow and provide its starting input." }
                    }
                    button { class: "workflow-run-dialog__close", r#type: "button", title: "Close", aria_label: "Close run dialog", onclick: move |_| on_close.call(()), Icon { name: "close" } }
                }
                if workflows.is_empty() {
                    div { class: "workflow-run-dialog__empty", "Create a workflow before running it." }
                } else {
                    label { class: "workflow-run-field",
                        span { "Workflow" }
                        select {
                            value: "{selected_id}",
                            disabled: running(),
                            onchange: move |event| {
                                selected_id.set(event.value().parse().unwrap_or_default());
                                result.set(None);
                                error.set(None);
                            },
                            for workflow in workflows.iter() {
                                option { value: "{workflow.id}", "{workflow.name}" }
                            }
                        }
                    }
                    if (view_type == "form" || view_type == "custom") && !input_properties.is_empty() {
                        div { class: "workflow-run-form",
                            if let Some(title) = view_config.get("title").and_then(serde_json::Value::as_str) { h3 { "{title}" } }
                            if let Some(description) = view_config.get("description").and_then(serde_json::Value::as_str) { p { "{description}" } }
                            for (field_name, field_schema) in input_properties.iter() {
                                {
                                    let key = field_name.clone();
                                    let label = field_schema.get("title").and_then(serde_json::Value::as_str).unwrap_or(field_name).to_owned();
                                    let kind = field_schema.get("type").and_then(serde_json::Value::as_str).unwrap_or("string").to_owned();
                                    let required = selected_workflow
                                        .and_then(|workflow| serde_json::from_str::<serde_json::Value>(&workflow.definition_json).ok())
                                        .and_then(|definition| definition.get("document")?.get("metadata")?.get("ui")?.get("input_schema")?.get("required")?.as_array().cloned())
                                        .is_some_and(|required| required.iter().any(|value| value.as_str() == Some(&key)));
                                    let value = input_field_value(&input_json(), &key);
                                    let placeholder = field_schema.get("placeholder").and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
                                    rsx! {
                                        label { class: "workflow-run-field",
                                            span { "{label}" if required { " *" } }
                                            if kind == "boolean" {
                                                input { r#type: "checkbox", checked: value == "true", disabled: running(), oninput: move |event| input_json.set(set_input_field(&input_json(), &key, event.checked().to_string(), &kind)) }
                                            } else if let Some(options) = field_schema.get("enum").and_then(serde_json::Value::as_array) {
                                                select { value: "{value}", disabled: running(), onchange: move |event| input_json.set(set_input_field(&input_json(), &key, event.value(), &kind)),
                                                    option { value: "", "Choose…" }
                                                    for item in options { if let Some(choice) = item.as_str() { option { value: "{choice}", "{choice}" } } }
                                                }
                                            } else {
                                                input { r#type: if kind == "number" || kind == "integer" { "number" } else { "text" }, value: "{value}", placeholder: "{placeholder}", disabled: running(), oninput: move |event| input_json.set(set_input_field(&input_json(), &key, event.value(), &kind)) }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        label { class: "workflow-run-field",
                            span { if view_type == "custom" { "Custom view input (JSON)" } else { "Input data" } }
                            textarea { class: "workflow-run-input", rows: "6", value: "{input_json}", disabled: running(), spellcheck: "false", oninput: move |event| input_json.set(event.value()) }
                        }
                    }
                    if let Some(message) = error() {
                        p { class: "workflow-run-result workflow-run-result--error", "{message}" }
                    }
                    if let Some(result) = result() {
                        if result.success {
                            div { class: "workflow-run-result workflow-run-result--success",
                                strong { "Workflow completed" }
                                if let Some(output) = result.output.as_ref() {
                                    pre { "{serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string())}" }
                                }
                            }
                        } else {
                            p { class: "workflow-run-result workflow-run-result--error",
                                if let Some(message) = result.error.as_ref() { "{message}" } else { "Workflow failed" }
                            }
                        }
                    }
                }
                footer { class: "workflow-run-dialog__footer",
                    button { class: "button button-quiet", r#type: "button", disabled: running(), onclick: move |_| on_close.call(()), "Close" }
                    if !workflows.is_empty() {
                        button {
                            class: "button button-primary",
                            r#type: "button",
                            disabled: running(),
                            onclick: move |_| async move {
                                running.set(true);
                                error.set(None);
                                result.set(None);
                                match execute_saved_workflow(selected_id(), input_json()).await {
                                    Ok(run) => {
                                        result.set(Some(run));
                                        on_complete.call(());
                                    }
                                    Err(message) => error.set(Some(message)),
                                }
                                running.set(false);
                            },
                            if running() { "Running…" } else if result().is_some() { "Run again" } else { "Run workflow" }
                        }
                    }
                }
            }
        }
    }
}

fn input_field_value(raw: &str, field: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|input| input.get(field).cloned())
        .map(|value| value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()))
        .unwrap_or_default()
}

fn set_input_field(raw: &str, field: &str, value: String, kind: &str) -> String {
    let mut input = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    input[field] = match kind {
        "number" | "integer" => value
            .parse::<serde_json::Number>()
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        "boolean" => serde_json::Value::Bool(value == "true"),
        _ => serde_json::Value::String(value),
    };
    serde_json::to_string(&input).unwrap_or_else(|_| "{}".into())
}
