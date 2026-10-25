use dioxus::prelude::*;

use crate::{
    Route,
    components::icons::Icon,
    pages::{
        OwsEditor,
        api::{read_workflow, save_workflow},
        models::{DEFAULT_OWS, Workflow, WorkflowPayload},
        shared::{ErrorPanel, LoadingPanel, Shell, pretty_json},
    },
};

#[component]
pub(crate) fn NewWorkflow() -> Element {
    rsx! { WorkflowPage { workflow: None } }
}

#[component]
pub(crate) fn EditWorkflow(id: i32) -> Element {
    let workflow = use_resource(move || read_workflow(id));
    match workflow() {
        Some(Ok(workflow)) => rsx! { WorkflowPage { workflow: Some(workflow) } },
        Some(Err(message)) => {
            rsx! { Shell { active: "workflows", header: "Edit workflow", intro: "Update the saved OWS document.", actions: rsx! {}, ErrorPanel { message } } }
        }
        None => {
            rsx! { Shell { active: "workflows", header: "Edit workflow", intro: "Update the saved OWS document.", actions: rsx! {}, LoadingPanel {} } }
        }
    }
}

#[component]
fn WorkflowPage(workflow: Option<Workflow>) -> Element {
    let navigator = use_navigator();
    let workflow_id = workflow.as_ref().map(|item| item.id);
    let editing = workflow_id.is_some();
    let page_title = if editing { "Edit workflow" } else { "New workflow" };
    let intro = if editing {
        "Update the saved OWS document."
    } else {
        "Create an OWS document and add it to the registry."
    };
    let mut name =
        use_signal(|| workflow.as_ref().map(|item| item.name.clone()).unwrap_or_default());
    let mut description = use_signal(|| {
        workflow.as_ref().and_then(|item| item.description.clone()).unwrap_or_default()
    });
    let mut version = use_signal(|| {
        workflow.as_ref().map(|item| item.version.to_string()).unwrap_or_else(|| "1".to_string())
    });
    let mut definition = use_signal(|| {
        workflow
            .as_ref()
            .map(|item| pretty_json(&item.definition_json))
            .unwrap_or_else(|| DEFAULT_OWS.to_string())
    });
    let mut active = use_signal(|| workflow.as_ref().map(|item| item.active).unwrap_or(true));
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut settings_open = use_signal(|| false);
    let mut action_notice = use_signal(|| None::<String>);
    let breadcrumb_title = if name().trim().is_empty() { page_title.to_string() } else { name() };
    let on_document_action = move |action: String| match action.as_str() {
        "rename" => {},
        "settings" => settings_open.set(true),
        "version-history" => action_notice.set(Some("Version history is not available yet.".into())),
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
            disabled: saving(),
            title: if saving() { "Saving workflow" } else { "Save workflow" },
            aria_label: if saving() { "Saving workflow" } else { "Save workflow" },
            onclick: move |_| async move {
                saving.set(true);
                error.set(None);
                let payload = WorkflowPayload {
                    active: active(),
                    definition_json: definition(),
                    description: if description().trim().is_empty() { None } else { Some(description()) },
                    name: name(),
                    version: version().parse().unwrap_or(1),
                };
                match save_workflow(payload, workflow_id).await {
                    Ok(workflow) => {
                        navigator.push(Route::WorkflowDetail { id: workflow.id });
                    }
                    Err(message) => error.set(Some(message)),
                }
                saving.set(false);
            },
            if saving() { span { class: "breadcrumb-save__spinner" } } else { Icon { name: "save" } }
        }
    };
    let actions = rsx! {};

    rsx! {
        Shell { active: "workflows", header: page_title, intro, actions, breadcrumb_title, on_document_action, breadcrumb_back, breadcrumb_save, on_breadcrumb_rename,
            section { class: "workflow-form",
                if let Some(message) = error() { ErrorPanel { message } }
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
                div { class: "workflow-form__editor",
                    OwsEditor {
                        definition: definition(),
                        on_change: move |value| definition.set(value),
                    }
                }
            }
        }
    }
}
