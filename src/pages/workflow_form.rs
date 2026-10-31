use dioxus::prelude::*;
use ows_workflow_editor::ValidationState;

use crate::{
    Route,
    components::icons::Icon,
    components::workflow_transfer::{
        ImportedWorkflow, WorkflowImportDialog, export_data_url, export_yaml_data_url,
    },
    pages::{
        OwsEditor,
        WorkflowChatMessage,
        api::{read_workflow, save_workflow, validate_workflow, workflow_ai_chat},
        models::{DEFAULT_OWS, WorkflowPayload},
        shared::{Shell, pretty_json},
    },
};

#[component]
pub(crate) fn NewWorkflow() -> Element {
    rsx! { WorkflowPage { workflow_id: None, initial_execution_id: None } }
}

fn rest_api_settings(definition_json: &str, workflow_id: Option<i32>) -> (bool, String, String) {
    let definition = serde_json::from_str::<serde_json::Value>(definition_json).unwrap_or_default();
    let rest_api = &definition["document"]["metadata"]["rest_api"];
    let legacy_trigger = definition["document"]["metadata"]["triggers"]
        .as_array()
        .and_then(|triggers| triggers.iter().find(|trigger| trigger["type"] == "http"));
    let enabled = rest_api["enabled"].as_bool().unwrap_or(legacy_trigger.is_some());
    let method = rest_api["method"]
        .as_str()
        .or_else(|| legacy_trigger.and_then(|trigger| trigger["method"].as_str()))
        .unwrap_or("POST")
        .to_string();
    let path = rest_api["path"]
        .as_str()
        .or_else(|| legacy_trigger.and_then(|trigger| trigger["path"].as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| match workflow_id {
            Some(id) => format!("/api/workflows/{id}/run"),
            None => "/api/workflows/run".into(),
        });
    (enabled, method, path)
}

fn apply_rest_api_settings(
    definition_json: &str,
    enabled: bool,
    method: &str,
    path: &str,
) -> String {
    let mut definition = serde_json::from_str::<serde_json::Value>(definition_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    if !definition["document"]["metadata"].is_object() {
        definition["document"]["metadata"] = serde_json::json!({});
    }
    definition["document"]["metadata"]["rest_api"] = serde_json::json!({
        "enabled": enabled,
        "method": method,
        "path": path,
    });
    serde_json::to_string_pretty(&definition).unwrap_or_else(|_| definition_json.to_string())
}

fn workflow_display_settings(definition_json: &str) -> (bool, bool, String, String, String) {
    let definition = serde_json::from_str::<serde_json::Value>(definition_json).unwrap_or_default();
    let settings = &definition["document"]["metadata"]["workflow_settings"];
    let show_text = settings["show_text"].as_bool().unwrap_or(true);
    let show_icon = settings["show_icon"].as_bool().unwrap_or(true);
    let icon_mode = settings["icon_mode"].as_str().unwrap_or("default").to_string();
    let icon = settings["icon"].as_str().unwrap_or("workflow").to_string();
    let hardware_button = settings["hardware_button"].as_str().unwrap_or("").to_string();
    (show_text, show_icon, icon_mode, icon, hardware_button)
}

fn apply_workflow_settings(
    definition_json: &str,
    show_text: bool,
    show_icon: bool,
    icon_mode: &str,
    icon: &str,
    hardware_button: &str,
) -> String {
    let mut definition = serde_json::from_str::<serde_json::Value>(definition_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    if !definition["document"]["metadata"].is_object() {
        definition["document"]["metadata"] = serde_json::json!({});
    }
    definition["document"]["metadata"]["workflow_settings"] = serde_json::json!({
        "show_text": show_text,
        "show_icon": show_icon,
        "icon_mode": icon_mode,
        "icon": icon,
        "hardware_button": if hardware_button.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(hardware_button.to_string()) },
    });
    serde_json::to_string_pretty(&definition).unwrap_or_else(|_| definition_json.to_string())
}

#[component]
pub(crate) fn EditWorkflow(id: i32) -> Element {
    rsx! { WorkflowPage { workflow_id: Some(id), initial_execution_id: None } }
}

#[component]
pub(crate) fn ViewWorkflowExecution(id: i32, execution_id: String) -> Element {
    rsx! { WorkflowPage { workflow_id: Some(id), initial_execution_id: Some(execution_id) } }
}

#[component]
fn WorkflowPage(workflow_id: Option<i32>, initial_execution_id: Option<String>) -> Element {
    let workflow = use_resource(move || async move {
        match workflow_id {
            Some(id) => read_workflow(id).await.map(Some),
            None => Ok(None),
        }
    });
    let loaded_workflow = workflow().and_then(Result::ok).flatten();
    let navigator = use_navigator();
    let editing = workflow_id.is_some();
    let page_title = if editing { "Edit workflow" } else { "New workflow" };
    let intro = if editing {
        "Update the saved OWS document."
    } else {
        "Create an OWS document and add it to the registry."
    };
    let mut name =
        use_signal(|| loaded_workflow.as_ref().map(|item| item.name.clone()).unwrap_or_default());
    let mut description = use_signal(|| {
        loaded_workflow.as_ref().and_then(|item| item.description.clone()).unwrap_or_default()
    });
    let mut version = use_signal(|| {
        loaded_workflow
            .as_ref()
            .map(|item| item.version.to_string())
            .unwrap_or_else(|| "1".to_string())
    });
    let mut definition = use_signal(|| {
        loaded_workflow
            .as_ref()
            .map(|item| pretty_json(&item.definition_json))
            .unwrap_or_else(|| DEFAULT_OWS.to_string())
    });
    let mut active =
        use_signal(|| loaded_workflow.as_ref().map(|item| item.active).unwrap_or(true));
    let initial_rest_api = loaded_workflow
        .as_ref()
        .map(|item| rest_api_settings(&item.definition_json, Some(item.id)))
        .unwrap_or_else(|| (false, "POST".into(), "/api/workflows/run".into()));
    let mut rest_api_enabled = use_signal(|| initial_rest_api.0);
    let mut rest_api_method = use_signal(|| initial_rest_api.1.clone());
    let mut rest_api_path = use_signal(|| initial_rest_api.2.clone());
    let initial_display = loaded_workflow
        .as_ref()
        .map(|item| workflow_display_settings(&item.definition_json))
        .unwrap_or_else(|| (true, true, "default".into(), "workflow".into(), String::new()));
    let mut show_text = use_signal(|| initial_display.0);
    let mut show_icon = use_signal(|| initial_display.1);
    let mut icon_mode = use_signal(|| initial_display.2.clone());
    let mut workflow_icon = use_signal(|| initial_display.3.clone());
    let mut hardware_button = use_signal(|| initial_display.4.clone());
    let mut settings_tab = use_signal(|| "settings".to_string());
    let mut hydrated_workflow = use_signal(|| None::<i32>);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut settings_open = use_signal(|| false);
    let mut import_open = use_signal(|| false);
    let mut action_notice = use_signal(|| None::<String>);
    let mut workflow_validation = use_signal(ValidationState::default);
    let mut ai_messages = use_signal(Vec::<WorkflowChatMessage>::new);
    let mut ai_busy = use_signal(|| false);
    use_effect(move || {
        if let Some(Ok(Some(item))) = workflow() {
            if hydrated_workflow() != Some(item.id) {
                name.set(item.name.clone());
                description.set(item.description.clone().unwrap_or_default());
                version.set(item.version.to_string());
                definition.set(pretty_json(&item.definition_json));
                active.set(item.active);
                let (enabled, method, path) = rest_api_settings(&item.definition_json, Some(item.id));
                rest_api_enabled.set(enabled);
                rest_api_method.set(method);
                rest_api_path.set(path);
                let (show, icon_visible, mode, icon, button) = workflow_display_settings(&item.definition_json);
                show_text.set(show);
                show_icon.set(icon_visible);
                icon_mode.set(mode);
                workflow_icon.set(icon);
                hardware_button.set(button);
                hydrated_workflow.set(Some(item.id));
            }
        }
    });
    let loading = editing && workflow().is_none();
    let load_error = workflow().and_then(Result::err);
    let editor_notice = if loading {
        Some("Loading workflow…".to_string())
    } else {
        error().or_else(|| load_error.clone())
    };
    let breadcrumb_title = if name().trim().is_empty() { page_title.to_string() } else { name() };
    let on_document_action = move |action: String| match action.as_str() {
        "rename" => {}
        "settings" => settings_open.set(true),
        "version-history" => {
            action_notice.set(Some("Version history is not available yet.".into()))
        }
        "archive" => action_notice.set(Some("Archiving is not available yet.".into())),
        _ => {}
    };
    let on_breadcrumb_rename = move |value: String| name.set(value);
    let on_ai_chat = move |prompt: String| {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() || ai_busy() {
            return;
        }
        let mut conversation = ai_messages();
        let history = conversation
            .iter()
            .map(|message| (message.role.clone(), message.content.clone()))
            .collect::<Vec<_>>();
        conversation.push(WorkflowChatMessage {
            role: "user".into(),
            content: prompt.clone(),
            definition_json: None,
        });
        ai_messages.set(conversation);
        ai_busy.set(true);
        let current_definition = definition();
        spawn(async move {
            let result = workflow_ai_chat(prompt, current_definition, history).await;
            let mut conversation = ai_messages();
            match result {
                Ok(response) => conversation.push(WorkflowChatMessage {
                    role: "assistant".into(),
                    content: response.reply,
                    definition_json: response.definition_json,
                }),
                Err(message) => conversation.push(WorkflowChatMessage {
                    role: "assistant".into(),
                    content: message,
                    definition_json: None,
                }),
            }
            ai_messages.set(conversation);
            ai_busy.set(false);
        });
    };
    let on_ai_apply = move |next_definition: String| definition.set(next_definition);

    let breadcrumb_back = if let Some(id) = workflow_id {
        rsx! { Link { class: "breadcrumb-back", to: Route::WorkflowDetail { id }, title: "Back to workflow", aria_label: "Back to workflow", Icon { name: "arrow-left" } } }
    } else {
        rsx! { Link { class: "breadcrumb-back", to: Route::WorkflowList {}, title: "Back to workflows", aria_label: "Back to workflows", Icon { name: "arrow-left" } } }
    };
    let breadcrumb_save = rsx! {
        button {
            class: "breadcrumb-save",
            disabled: saving() || loading || load_error.is_some() || workflow_validation().checking || !workflow_validation().valid,
            title: if loading { "Loading workflow" } else if saving() { "Saving workflow" } else if workflow_validation().checking { "Validating workflow" } else if !workflow_validation().valid { "Fix workflow validation errors before saving" } else { "Save workflow" },
            aria_label: if loading { "Loading workflow" } else if saving() { "Saving workflow" } else if workflow_validation().checking { "Validating workflow" } else if !workflow_validation().valid { "Fix workflow validation errors before saving" } else { "Save workflow" },
            onclick: move |_| async move {
                saving.set(true);
                error.set(None);
                let definition_json = apply_workflow_settings(
                    &apply_rest_api_settings(
                    &definition(),
                    rest_api_enabled(),
                    &rest_api_method(),
                    &rest_api_path(),
                    ),
                    show_text(),
                    show_icon(),
                    &icon_mode(),
                    &workflow_icon(),
                    &hardware_button(),
                );
                match validate_workflow(definition_json.clone()).await {
                    Ok(errors) if errors.is_empty() => {}
                    Ok(errors) => {
                        error.set(Some(errors.join("; ")));
                        saving.set(false);
                        return;
                    }
                    Err(message) => {
                        error.set(Some(message));
                        saving.set(false);
                        return;
                    }
                }
                definition.set(definition_json.clone());
                let payload = WorkflowPayload {
                    active: active(),
                    definition_json,
                    description: if description().trim().is_empty() { None } else { Some(description()) },
                    name: name(),
                    version: version().parse().unwrap_or(1),
                };
                match save_workflow(payload, workflow_id).await {
                    Ok(workflow) => {
                        if !editing {
                            navigator.push(Route::WorkflowDetail { id: workflow.id });
                        }
                    }
                    Err(message) => error.set(Some(message)),
                }
                saving.set(false);
            },
            if saving() { span { class: "breadcrumb-save__spinner" } } else { Icon { name: "save" } }
        }
    };
    let export_name = if name().trim().is_empty() { "workflow".to_string() } else { name() };
    let export_href = export_data_url(&definition());
    let export_yaml_href = export_yaml_data_url(&definition());
    let actions = rsx! {
        div { class: "workflow-form__transfer-actions",
            a {
                class: "button button-quiet",
                href: "{export_href}",
                download: "{export_name}.ows.json",
                title: "Export workflow JSON",
                Icon { name: "download" }
                "JSON"
            }
            a {
                class: "button button-quiet",
                href: "{export_yaml_href}",
                download: "{export_name}.ows.yaml",
                title: "Export workflow YAML",
                Icon { name: "download" }
                "YAML"
            }
        }
    };
    let on_import = move |imported: ImportedWorkflow| {
        error.set(None);
        spawn(async move {
            match validate_workflow(imported.definition_json.clone()).await {
                Ok(errors) if errors.is_empty() => {
                    workflow_validation.set(ValidationState { valid: true, ..Default::default() });
                    name.set(imported.name);
                    description.set(imported.description.unwrap_or_default());
                    version.set(imported.version.to_string());
                    let (show, icon_visible, mode, icon, button) =
                        workflow_display_settings(&imported.definition_json);
                    show_text.set(show);
                    show_icon.set(icon_visible);
                    icon_mode.set(mode);
                    workflow_icon.set(icon);
                    hardware_button.set(button);
                    definition.set(imported.definition_json);
                    import_open.set(false);
                }
                Ok(errors) => error
                    .set(Some(format!("Imported OWS workflow is invalid: {}", errors.join("; ")))),
                Err(message) => {
                    error.set(Some(format!("Could not validate imported workflow: {message}")))
                }
            }
        });
    };

    rsx! {
        Shell { active: "workflows", header: page_title, intro, actions, breadcrumb_title: breadcrumb_title.clone(), on_document_action, breadcrumb_back, breadcrumb_save, on_breadcrumb_rename,
            section { class: "workflow-form",
                if settings_open() {
                    div { class: "workflow-settings-scrim",
                        section { class: "workflow-settings-dialog", role: "dialog", aria_modal: "true", aria_labelledby: "workflow-settings-title",
                            header { class: "workflow-settings-dialog__header",
                                h2 { id: "workflow-settings-title", "Workflow settings" }
                                button { class: "button button-quiet", r#type: "button", onclick: move |_| settings_open.set(false), "Done" }
                            }
                            nav { class: "workflow-settings-tabs", aria_label: "Workflow settings tabs",
                                button { class: if settings_tab() == "settings" { "workflow-settings-tab selected" } else { "workflow-settings-tab" }, r#type: "button", onclick: move |_| settings_tab.set("settings".into()), "Settings" }
                                button { class: if settings_tab() == "hardware" { "workflow-settings-tab selected" } else { "workflow-settings-tab" }, r#type: "button", onclick: move |_| settings_tab.set("hardware".into()), "Hardware buttons" }
                            }
                            if settings_tab() == "settings" {
                                label { class: "field",
                                    span { "Name" }
                                    input { value: "{name}", oninput: move |event| name.set(event.value()) }
                                }
                                label { class: "field",
                                    span { "Description" }
                                    textarea { rows: "3", placeholder: "What does this workflow do?", value: "{description}", oninput: move |event| description.set(event.value()) }
                                }
                                label { class: "field",
                                    span { "Version" }
                                    input { r#type: "number", min: "1", value: "{version}", oninput: move |event| version.set(event.value()) }
                                }
                                label { class: "toggle-field",
                                    input { r#type: "checkbox", checked: active(), oninput: move |event| active.set(event.checked()) }
                                    span { "Active workflow" }
                                }
                                label { class: "toggle-field",
                                    input { r#type: "checkbox", checked: show_text(), oninput: move |event| show_text.set(event.checked()) }
                                    span { "Show text" }
                                }
                                label { class: "toggle-field",
                                    input { r#type: "checkbox", checked: show_icon(), oninput: move |event| show_icon.set(event.checked()) }
                                    span { "Show icon" }
                                }
                                label { class: "field",
                                    span { "Set icon" }
                                    select { value: "{icon_mode}", onchange: move |event| icon_mode.set(event.value()),
                                        option { value: "default", selected: icon_mode() == "default", "Default icon" }
                                        option { value: "custom", selected: icon_mode() == "custom", "Custom icon" }
                                    }
                                }
                                if icon_mode() == "custom" {
                                    label { class: "field",
                                        span { "Custom icon name" }
                                        input { value: "{workflow_icon}", placeholder: "workflow, zap, sparkles…", oninput: move |event| workflow_icon.set(event.value()) }
                                    }
                                }
                                label { class: "toggle-field",
                                    input { r#type: "checkbox", checked: rest_api_enabled(), oninput: move |event| rest_api_enabled.set(event.checked()) }
                                    span { "Expose via HTTP API" }
                                }
                                if rest_api_enabled() {
                                    label { class: "field",
                                        span { "HTTP method" }
                                        select { value: "{rest_api_method}", onchange: move |event| rest_api_method.set(event.value()),
                                            for method in ["POST", "GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
                                                option { value: "{method}", selected: rest_api_method() == method, "{method}" }
                                            }
                                        }
                                    }
                                    label { class: "field",
                                        span { "HTTP path" }
                                        input { value: "{rest_api_path}", placeholder: "/api/workflows/my-workflow", oninput: move |event| rest_api_path.set(event.value()) }
                                    }
                                    p { class: "workflow-settings-dialog__hint", "Endpoint: {rest_api_method()} {rest_api_path()}" }
                                }
                            } else {
                                label { class: "field",
                                    span { "Assign hardware button" }
                                    select { value: "{hardware_button}", onchange: move |event| hardware_button.set(event.value()),
                                        option { value: "", selected: hardware_button().is_empty(), "No button assigned" }
                                        for (value, label) in [("button-1", "Button 1"), ("button-2", "Button 2"), ("button-3", "Button 3"), ("button-4", "Button 4")] {
                                            option { value: "{value}", selected: hardware_button() == value, "{label}" }
                                        }
                                    }
                                }
                                p { class: "workflow-settings-dialog__hint", "Choose a physical button to launch this workflow." }
                            }
                        }
                    }
                }
                if let Some(message) = action_notice() {
                    div { class: "workflow-form__action-notice",
                        span { "{message}" }
                        button { r#type: "button", onclick: move |_| action_notice.set(None), "Dismiss" }
                    }
                }
                if import_open() {
                    WorkflowImportDialog {
                        on_import,
                        on_close: move |_| import_open.set(false),
                    }
                }
                div { class: "workflow-form__editor",
                    OwsEditor {
                        definition: definition(),
                        on_change: move |value| definition.set(value),
                        validation: workflow_validation(),
                        on_validation: move |state: ValidationState| workflow_validation.set(state),
                        notice: editor_notice,
                        workflow_id,
                        workflow_name: breadcrumb_title.clone(),
                        initial_execution_id: initial_execution_id.clone(),
                        on_open_settings: move |_| settings_open.set(true),
                        on_import: move |_| import_open.set(true),
                        ai_messages: ai_messages(),
                        ai_busy: ai_busy(),
                        on_ai_chat,
                        on_ai_apply,
                    }
                }
            }
        }
    }
}
