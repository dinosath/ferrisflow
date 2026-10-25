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
    on_complete: EventHandler<()>,
) -> Element {
    let first_id = workflows.first().map(|workflow| workflow.id).unwrap_or_default();
    let mut selected_id = use_signal(|| selected_workflow.unwrap_or(first_id));
    let mut input_json = use_signal(|| "{}".to_string());
    let mut running = use_signal(|| false);
    let mut result = use_signal(|| None::<ExecutionResult>);
    let mut error = use_signal(|| None::<String>);

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
                    label { class: "workflow-run-field",
                        span { "Input data" }
                        textarea {
                            class: "workflow-run-input",
                            rows: "6",
                            value: "{input_json}",
                            disabled: running(),
                            spellcheck: "false",
                            oninput: move |event| input_json.set(event.value()),
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
