use dioxus::prelude::*;

use crate::{
    Route,
    components::data_table::{DataTable, DataTableCell, DataTableRow},
    components::icons::Icon,
    components::workflow_run::WorkflowRunDialog,
    components::workflow_transfer::{
        ImportedWorkflow, WorkflowImportDialog, export_data_url, export_yaml_data_url,
    },
    pages::shared::{ErrorPanel, LoadingPanel, Shell, TableStatus, format_date},
    pages::{
        api::{list_workflows, save_workflow, validate_workflow},
        models::{Workflow, WorkflowPayload},
    },
};

#[component]
pub(crate) fn WorkflowList() -> Element {
    let mut workflows = use_server_future(list_workflows)?;
    let mut query = use_signal(String::new);
    let mut import_open = use_signal(|| false);
    let mut import_error = use_signal(|| None::<String>);
    let mut run_workflow_id = use_signal(|| None::<i32>);
    let mut settings_workflow_id = use_signal(|| None::<i32>);
    let mut settings_error = use_signal(|| None::<String>);
    let navigator = use_navigator();
    let on_import = move |workflow: Result<ImportedWorkflow, String>| {
        import_open.set(false);
        import_error.set(None);
        let workflow = match workflow {
            Ok(workflow) => workflow,
            Err(message) => {
                import_error.set(Some(message));
                return;
            }
        };
        spawn(async move {
            let result = async {
                let validation = validate_workflow(workflow.definition_json.clone()).await?;
                if !validation.is_empty() {
                    return Err(validation.join("; "));
                }
                save_workflow(
                    WorkflowPayload {
                        active: true,
                        definition_json: workflow.definition_json,
                        description: workflow.description,
                        name: workflow.name,
                        version: workflow.version,
                    },
                    None,
                )
                .await
            }
            .await;
            match result {
                Ok(saved) => {
                    navigator.push(Route::WorkflowDetail { id: saved.id });
                }
                Err(message) => import_error.set(Some(message)),
            }
        });
    };

    rsx! {
        Shell { active: "workflows",
            header: "Workflows",
            intro: "A clear view of the documents that define your work.",
            actions: rsx! { Link { class: "button button-primary", to: Route::NewWorkflow {}, "+ New workflow" } },
            section { class: "workflows-list",
                if let Some(message) = import_error() { ErrorPanel { message } }
                match workflows() {
                    Some(Ok(items)) => {
                        let all_workflows = items.clone();
                        let needle = query().to_lowercase();
                        let filtered: Vec<(Workflow, String, String)> = items.into_iter()
                            .filter(|item| {
                                needle.is_empty()
                                    || item.name.to_lowercase().contains(&needle)
                                    || item.id.to_string().contains(&needle)
                            })
                            .map(|item| {
                                let description = item.description.clone().unwrap_or_else(|| "No description".into());
                                let updated = format_date(&item.last_updated);
                                (item, description, updated)
                            })
                            .collect();
                        let row_ids = filtered.iter().map(|(workflow, _, _)| workflow.id.to_string()).collect();
                        rsx! {
                            DataTable {
                                columns: vec!["Name".to_string(), "Version".to_string(), "Status".to_string(), "Description".to_string(), "Updated".to_string()],
                                row_ids,
                                toolbar: rsx! {
                                    label { class: "workflow-list-search",
                                        Icon { name: "search" }
                                        input {
                                            r#type: "search",
                                            aria_label: "Filter workflows by name or ID",
                                            placeholder: "Filter by name or ID...",
                                            value: "{query}",
                                            oninput: move |event| query.set(event.value()),
                                        }
                                    }
                                    button {
                                        class: "button button-quiet workflow-list-import",
                                        r#type: "button",
                                        onclick: move |_| { import_error.set(None); import_open.set(true); },
                                        Icon { name: "upload" }
                                        "Import"
                                    }
                                },
                                empty_message: if query().is_empty() { "No workflows yet. Create a workflow to get started.".to_string() } else { "No workflows match your search.".to_string() },
                                for (index, (workflow, description, updated)) in filtered.into_iter().enumerate() {
                                    DataTableRow { id: workflow.id.to_string(), index,
                                        DataTableCell { index: 0, class: "name-cell",
                                            Link { to: Route::EditWorkflow { id: workflow.id }, "{workflow.name}" }
                                            small { "#{workflow.id}" }
                                            button {
                                                class: "workflow-row-run",
                                                r#type: "button",
                                                title: "Run {workflow.name}",
                                                aria_label: "Run {workflow.name}",
                                                onclick: move |_| run_workflow_id.set(Some(workflow.id)),
                                                Icon { name: "play" }
                                            }
                                            button {
                                                class: "workflow-row-run workflow-row-settings",
                                                r#type: "button",
                                                title: "REST API settings for {workflow.name}",
                                                aria_label: "REST API settings for {workflow.name}",
                                                onclick: move |_| { settings_error.set(None); settings_workflow_id.set(Some(workflow.id)); },
                                                Icon { name: "settings" }
                                            }
                                            a {
                                                class: "workflow-row-export",
                                                href: "{export_data_url(&workflow.definition_json)}",
                                                download: "{workflow.name}.ows.json",
                                                title: "Export {workflow.name} as OWS JSON",
                                                aria_label: "Export {workflow.name} as OWS JSON",
                                                Icon { name: "download" }
                                            }
                                            a {
                                                class: "workflow-row-export workflow-row-export--yaml",
                                                href: "{export_yaml_data_url(&workflow.definition_json)}",
                                                download: "{workflow.name}.ows.yaml",
                                                title: "Export {workflow.name} as OWS YAML",
                                                aria_label: "Export {workflow.name} as OWS YAML",
                                                Icon { name: "download" }
                                                span { "YAML" }
                                            }
                                        }
                                        DataTableCell { index: 1, "v{workflow.version}" }
                                        DataTableCell { index: 2, TableStatus { active: workflow.active } }
                                        DataTableCell { index: 3, class: "description-cell", "{description}" }
                                        DataTableCell { index: 4, class: "date-cell", "{updated}" }
                                    }
                                }
                            }
                            if let Some(workflow_id) = run_workflow_id() {
                                WorkflowRunDialog {
                                    workflows: all_workflows.clone(),
                                    selected_workflow: Some(workflow_id),
                                    on_close: move |_| run_workflow_id.set(None),
                                    on_complete: move |_| {},
                                }
                            }
                            if let Some(workflow_id) = settings_workflow_id() {
                                if let Some(workflow) = all_workflows.iter().find(|item| item.id == workflow_id).cloned() {
                                    {
                                    let workflow_for_save = workflow.clone();
                                    rsx! { WorkflowRestApiDialog {
                                        workflow,
                                        error: settings_error(),
                                        on_close: move |_| { settings_workflow_id.set(None); settings_error.set(None); },
                                        on_save: move |(enabled, method, path): (bool, String, String)| {
                                            let workflow = workflow_for_save.clone();
                                            spawn(async move {
                                                let result = async {
                                                    let mut definition: serde_json::Value = serde_json::from_str(&workflow.definition_json)
                                                        .map_err(|error| error.to_string())?;
                                                    if !definition["document"]["metadata"].is_object() {
                                                        definition["document"]["metadata"] = serde_json::json!({});
                                                    }
                                                    definition["document"]["metadata"]["rest_api"] = serde_json::json!({
                                                        "enabled": enabled,
                                                        "method": method,
                                                        "path": path
                                                    });
                                                    let definition_json = serde_json::to_string_pretty(&definition).map_err(|error| error.to_string())?;
                                                    let errors = validate_workflow(definition_json.clone()).await?;
                                                    if !errors.is_empty() { return Err(errors.join("; ")); }
                                                    save_workflow(WorkflowPayload {
                                                        active: workflow.active,
                                                        definition_json,
                                                        description: workflow.description,
                                                        name: workflow.name,
                                                        version: workflow.version,
                                                    }, Some(workflow.id)).await
                                                }.await;
                                                match result {
                                                    Ok(_) => { settings_error.set(None); settings_workflow_id.set(None); workflows.restart(); }
                                                    Err(message) => settings_error.set(Some(message)),
                                                }
                                            });
                                        },
                                    } }
                                    }
                                }
                            }
                        }
                    },
                    Some(Err(error)) => rsx! { ErrorPanel { message: error } },
                    None => rsx! { LoadingPanel {} },
                }
                if import_open() {
                    WorkflowImportDialog {
                        on_import,
                        on_close: move |_| import_open.set(false),
                    }
                }
            }
        }
    }
}

#[component]
fn WorkflowRestApiDialog(
    workflow: Workflow, error: Option<String>, on_close: EventHandler<()>,
    on_save: EventHandler<(bool, String, String)>,
) -> Element {
    let definition =
        serde_json::from_str::<serde_json::Value>(&workflow.definition_json).unwrap_or_default();
    let rest_api = definition["document"]["metadata"]["rest_api"].clone();
    let legacy_trigger = definition["document"]["metadata"]["triggers"]
        .as_array()
        .and_then(|triggers| triggers.iter().find(|trigger| trigger["type"] == "http"));
    let mut enabled =
        use_signal(|| rest_api["enabled"].as_bool().unwrap_or(legacy_trigger.is_some()));
    let mut method = use_signal(|| {
        rest_api["method"]
            .as_str()
            .or_else(|| legacy_trigger.and_then(|trigger| trigger["method"].as_str()))
            .unwrap_or("POST")
            .to_string()
    });
    let mut path = use_signal(|| {
        rest_api["path"]
            .as_str()
            .or_else(|| legacy_trigger.and_then(|trigger| trigger["path"].as_str()))
            .map(str::to_string)
            .unwrap_or_else(|| format!("/api/workflows/{}/run", workflow.id))
    });
    rsx! {
        div { class: "workflow-settings-overlay", role: "dialog", aria_modal: "true", aria_label: "REST API settings",
            section { class: "workflow-settings-dialog panel",
                header { class: "workflow-settings-dialog__header",
                    div { p { class: "workflow-editor__eyebrow", "WORKFLOW SETTINGS" } h2 { "REST API" } }
                    button { class: "workflow-editor__icon-action", r#type: "button", aria_label: "Close REST API settings", onclick: move |_| on_close.call(()), Icon { name: "close" } }
                }
                p { class: "workflow-settings-dialog__description", "Expose this workflow as a REST endpoint. The request is available to workflow tasks under request, including its method, path, query, headers, and body." }
                label { class: "workflow-settings-dialog__toggle",
                    input { r#type: "checkbox", checked: enabled(), onchange: move |event| enabled.set(event.checked()) }
                    span { "Expose via REST API" }
                }
                label { class: "field",
                    span { "HTTP method" }
                    select { value: "{method}", onchange: move |event| method.set(event.value()),
                        for option in ["POST", "GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] { option { value: "{option}", selected: method() == option, "{option}" } }
                    }
                }
                label { class: "field",
                    span { "Path" }
                    input { value: "{path}", placeholder: "/api/workflows/my-workflow", oninput: move |event| path.set(event.value()) }
                }
                p { class: "workflow-settings-dialog__hint", "Endpoint: {method} {path}" }
                if !workflow.active { p { class: "workflow-settings-dialog__hint", "This workflow must be active to receive requests." } }
                if let Some(message) = error { div { class: "error-panel", "{message}" } }
                footer { class: "workflow-settings-dialog__actions",
                    button { class: "button button-quiet", r#type: "button", onclick: move |_| on_close.call(()), "Cancel" }
                    button { class: "button button-primary", r#type: "button", onclick: move |_| on_save.call((enabled(), method(), path())), "Save settings" }
                }
            }
        }
    }
}
