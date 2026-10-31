use dioxus::prelude::*;
use ows_workflow_editor::{
    ValidationState, WorkflowEditor, WorkflowExecution, WorkflowExecutionRecord,
    WorkflowExecutionStep, WorkflowExample,
};
use std::{cell::Cell, rc::Rc};

use crate::pages::{
    api::{
        execute_workflow, list_database_tables, list_executions, listen_database_event,
        validate_workflow,
    },
    models::ExecutionRecord,
};

#[component]
pub(crate) fn OwsEditor(
    definition: String, on_change: EventHandler<String>,
    on_validation: EventHandler<ValidationState>, validation: ValidationState,
    notice: Option<String>, workflow_id: Option<i32>, workflow_name: String,
    initial_execution_id: Option<String>, on_open_settings: EventHandler<()>,
    ai_messages: Vec<WorkflowChatMessage>, ai_busy: bool,
    on_ai_chat: EventHandler<String>, on_ai_apply: EventHandler<String>,
    on_import: EventHandler<()>,
) -> Element {
    let mut last_definition = use_signal(String::new);
    let mut execution = use_signal(WorkflowExecution::default);
    let mut execution_refresh = use_signal(|| 0_u64);
    let database_tables = use_resource(list_database_tables);
    let mut database_listening = use_signal(|| false);
    let mut database_event = use_signal(|| None::<String>);
    let mut assistant_open = use_signal(|| false);
    let mut input_open = use_signal(|| false);
    let mut input_json = use_signal(|| "{}".to_string());
    let mut input_error = use_signal(|| None::<String>);
    let mut execution_input = use_signal(|| serde_json::json!({}));
    let execution_history = use_resource(move || {
        let refresh = execution_refresh();
        async move {
            let _ = refresh;
            list_executions().await
        }
    });
    let validation_generation = use_hook(|| Rc::new(Cell::new(0_u64)));
    let generation_for_effect = validation_generation.clone();

    use_effect(use_reactive((&definition,), {
        move |(current,)| {
            let current = current.clone();
            if last_definition() == current {
                return;
            }
            last_definition.set(current.clone());
            let generation = generation_for_effect.get() + 1;
            generation_for_effect.set(generation);
            let checking = ValidationState { checking: true, ..Default::default() };
            on_validation.call(checking);
            let validation_generation = generation_for_effect.clone();
            spawn(async move {
                let result = validate_workflow(current).await;
                if validation_generation.get() != generation {
                    return;
                }
                on_validation.call(validation_result(result));
            });
        }
    }));

    let generation_for_change = validation_generation.clone();
    let generation_for_change_async = validation_generation.clone();
    let on_change = move |next_definition: String| {
        let generation = generation_for_change.get() + 1;
        generation_for_change.set(generation);
        let checking = ValidationState { checking: true, ..Default::default() };
        on_validation.call(checking);
        on_change.call(next_definition.clone());
        let validation_generation = generation_for_change_async.clone();
        let on_validation = on_validation;
        spawn(async move {
            let result = validate_workflow(next_definition).await;
            if validation_generation.get() != generation {
                return;
            }
            on_validation.call(validation_result(result));
        });
    };
    let generation_for_validate = validation_generation.clone();
    let on_validate = move |definition: String| {
        let generation = generation_for_validate.get() + 1;
        generation_for_validate.set(generation);
        let checking = ValidationState { checking: true, ..Default::default() };
        on_validation.call(checking);
        let validation_generation = generation_for_validate.clone();
        spawn(async move {
            let result = validate_workflow(definition).await;
            if validation_generation.get() != generation {
                return;
            }
            let result = validation_result(result);
            on_validation.call(result);
        });
    };
    let definition_for_execution = definition.clone();
    let workflow_name_for_execution = workflow_name.clone();
    let on_execute = move |_| {
        let current = definition_for_execution.clone();
        let name = workflow_name_for_execution.clone();
        let input = execution_input();
        execution.set(WorkflowExecution { running: true, ..Default::default() });
        spawn(async move {
            let result = execute_workflow(current, workflow_id, Some(name), input).await;
            execution.set(match result {
                Ok(result) => WorkflowExecution {
                    execution_id: Some(result.execution_id),
                    status: Some(if result.success { "succeeded" } else { "failed" }.to_string()),
                    started_at: Some(result.started_at),
                    duration_ms: Some(result.duration_ms),
                    input_json: Some("{}".to_string()),
                    output: result.output.map(|output| {
                        serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string())
                    }),
                    error: if result.success {
                        None
                    } else {
                        result.error.or_else(|| Some("Workflow execution failed".into()))
                    },
                    steps: result.logs.into_iter().map(execution_step).collect(),
                    ..Default::default()
                },
                Err(error) => WorkflowExecution { error: Some(error), ..Default::default() },
            });
            execution_refresh.set(execution_refresh() + 1);
        });
    };

    let on_listen_database_event = move |table: String| {
        database_listening.set(true);
        database_event.set(None);
        spawn(async move {
            let result = listen_database_event(table).await;
            database_event.set(Some(match result {
                Ok(Some(event)) => {
                    serde_json::to_string_pretty(&event).unwrap_or_else(|_| event.to_string())
                }
                Ok(None) => "No event received before the listen window expired.".into(),
                Err(error) => format!("Could not listen for the database event: {error}"),
            }));
            database_listening.set(false);
        });
    };

    let database_tables = database_tables().and_then(Result::ok).unwrap_or_default();

    let execution_history = match execution_history() {
        Some(Ok(rows)) => rows
            .into_iter()
            .filter(|row| {
                row.workflow_id == workflow_id
                    || (workflow_id.is_none() && row.workflow_name == workflow_name)
            })
            .map(execution_record)
            .collect(),
        _ => Vec::new(),
    };

    rsx! {
        div { class: "workflow-editor-integration",
            div { class: "workflow-editor-project-actions",
                button { class: "button button-quiet", r#type: "button", onclick: move |_| assistant_open.set(!assistant_open()), "AI assistant" }
                button { class: "button button-quiet", r#type: "button", onclick: move |_| on_open_settings.call(()), "Workflow settings" }
            }
            div { class: "workflow-editor-integration__canvas",
                WorkflowEditor {
                    definition,
                    on_change,
                    on_validate,
                    validation,
                    execution: execution(),
                    on_execute,
                    on_import,
                    examples: Vec::<WorkflowExample>::new(),
                    on_select_example: move |_| {},
                    on_configure_input: move |_| { input_json.set(execution_input().to_string()); input_error.set(None); input_open.set(true); },
                    execution_history,
                    initial_execution_id,
                    notice,
                    database_backend: database_tables.backend,
                    database_tables: database_tables.tables,
                    database_listening: database_listening(),
                    database_event: database_event(),
                    on_listen_database_event,
                }
                if assistant_open() {
                    WorkflowChatOverlay {
                        messages: ai_messages,
                        busy: ai_busy,
                        on_send: on_ai_chat,
                        on_apply: on_ai_apply,
                        on_close: move |_| assistant_open.set(false),
                    }
                }
            }
            if input_open() {
                div { class: "workflow-settings-overlay", role: "presentation", onclick: move |_| input_open.set(false),
                    section { class: "workflow-settings-dialog", role: "dialog", aria_modal: "true", aria_label: "Workflow input", onclick: move |event| event.stop_propagation(),
                        header { class: "workflow-settings-dialog__header", h2 { "Execution input" }
                            button { class: "button button-quiet", r#type: "button", onclick: move |_| input_open.set(false), "Close" }
                        }
                        p { class: "workflow-settings-dialog__description", "Provide the JSON object passed to this workflow when it runs." }
                        textarea { class: "workflow-editor__definition code-editor", value: "{input_json}", rows: "10", spellcheck: "false", oninput: move |event| input_json.set(event.value()) }
                        if let Some(message) = input_error() { div { class: "error-panel", "{message}" } }
                        footer { class: "workflow-settings-dialog__actions",
                            button { class: "button button-quiet", r#type: "button", onclick: move |_| input_open.set(false), "Cancel" }
                            button { class: "button button-primary", r#type: "button", onclick: move |_| match serde_json::from_str::<serde_json::Value>(&input_json()) {
                                Ok(value) if value.is_object() => { execution_input.set(value); input_open.set(false); }
                                Ok(_) => input_error.set(Some("Workflow input must be a JSON object.".into())),
                                Err(error) => input_error.set(Some(format!("Invalid JSON: {error}"))),
                            }, "Use input" }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkflowChatMessage {
    pub role: String,
    pub content: String,
    pub definition_json: Option<String>,
}

#[component]
fn WorkflowChatOverlay(
    messages: Vec<WorkflowChatMessage>, busy: bool, on_send: EventHandler<String>,
    on_apply: EventHandler<String>, on_close: EventHandler<()>,
) -> Element {
    let mut draft = use_signal(String::new);
    rsx! {
        aside { class: "workflow-chat-overlay", aria_label: "AI workflow assistant",
            header { class: "workflow-chat-overlay__header",
                div { strong { "Workflow assistant" } small { "Describe a workflow or ask for a change" } }
                button { class: "workflow-editor__icon-action", title: "Close assistant", aria_label: "Close assistant", r#type: "button", onclick: move |_| on_close.call(()), "×" }
            }
            div { class: "workflow-chat-overlay__messages",
                if messages.is_empty() {
                    div { class: "workflow-chat-overlay__empty", strong { "What should this workflow do?" } p { "Ask for a new workflow or describe an edit. Generated changes stay here until you apply them." } }
                }
                for message in messages.iter() {
                    article { class: if message.role == "user" { "workflow-chat-overlay__message is-user" } else { "workflow-chat-overlay__message" },
                        p { "{message.content}" }
                        if let Some(definition_json) = message.definition_json.clone() {
                            button { class: "button button-primary workflow-chat-overlay__apply", r#type: "button", onclick: move |_| on_apply.call(definition_json.clone()), "Apply workflow changes" }
                        }
                    }
                }
                if busy { div { class: "workflow-chat-overlay__typing", span {} span {} span {} "Thinking" } }
            }
            form { class: "workflow-chat-overlay__composer", onsubmit: move |event| {
                event.prevent_default();
                let prompt = draft().trim().to_string();
                if !prompt.is_empty() && !busy { draft.set(String::new()); on_send.call(prompt); }
            },
                textarea { value: "{draft}", rows: "2", placeholder: "Ask the workflow assistant…", aria_label: "Message the workflow assistant", oninput: move |event| draft.set(event.value()) }
                div { class: "workflow-chat-overlay__composer-footer", small { "AI suggestions require review before use." }
                    button { class: "button button-primary", r#type: "submit", disabled: busy || draft().trim().is_empty(), "Send" }
                }
            }
        }
    }
}

fn execution_record(record: ExecutionRecord) -> WorkflowExecutionRecord {
    WorkflowExecutionRecord {
        execution_id: record.execution_id,
        status: record.status,
        started_at: record.started_at,
        duration_ms: record.duration_ms,
        input_json: record.input_json,
        output_json: record.output_json,
        error: record.error,
        steps: record.logs.into_iter().map(execution_step).collect(),
    }
}

fn execution_step(step: crate::pages::models::ExecutionStep) -> WorkflowExecutionStep {
    WorkflowExecutionStep {
        name: step.name,
        status: step.status,
        duration_ms: step.duration_ms,
        input_json: step.input_json,
        output_json: step.output_json,
    }
}

fn validation_result(result: Result<Vec<String>, String>) -> ValidationState {
    match result {
        Ok(errors) if errors.is_empty() => ValidationState { valid: true, ..Default::default() },
        Ok(errors) => ValidationState { errors, ..Default::default() },
        Err(error) => ValidationState { errors: vec![error], ..Default::default() },
    }
}
