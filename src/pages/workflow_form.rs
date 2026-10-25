use dioxus::prelude::*;
use ferris_workflow_editor::ValidationState;

use crate::{
    Route,
    components::icons::Icon,
    components::workflow_transfer::{
        ImportedWorkflow, WorkflowImportDialog, export_data_url, export_yaml_data_url,
    },
    pages::{
        OwsEditor,
        api::{read_workflow, save_workflow, validate_workflow},
        models::{DEFAULT_OWS, WorkflowPayload},
        shared::{Shell, pretty_json},
    },
};

#[component]
pub(crate) fn NewWorkflow() -> Element {
    rsx! { WorkflowPage { workflow_id: None } }
}

#[component]
pub(crate) fn EditWorkflow(id: i32) -> Element {
    rsx! { WorkflowPage { workflow_id: Some(id) } }
}

#[component]
fn WorkflowPage(workflow_id: Option<i32>) -> Element {
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
    let mut hydrated_workflow = use_signal(|| None::<i32>);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut settings_open = use_signal(|| false);
    let mut import_open = use_signal(|| false);
    let mut action_notice = use_signal(|| None::<String>);
    let mut workflow_validation = use_signal(ValidationState::default);
    use_effect(move || {
        if let Some(Ok(Some(item))) = workflow() {
            if hydrated_workflow() != Some(item.id) {
                name.set(item.name.clone());
                description.set(item.description.clone().unwrap_or_default());
                version.set(item.version.to_string());
                definition.set(pretty_json(&item.definition_json));
                active.set(item.active);
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
                let definition_json = definition();
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
            button {
                class: "button button-quiet",
                r#type: "button",
                onclick: move |_| import_open.set(true),
                Icon { name: "upload" }
                "Import"
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
                    }
                }
            }
        }
    }
}
