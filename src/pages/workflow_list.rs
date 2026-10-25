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
    let workflows = use_resource(list_workflows);
    let mut query = use_signal(String::new);
    let mut import_open = use_signal(|| false);
    let mut import_error = use_signal(|| None::<String>);
    let mut run_workflow_id = use_signal(|| None::<i32>);
    let navigator = use_navigator();
    let on_import = move |workflow: ImportedWorkflow| {
        import_open.set(false);
        import_error.set(None);
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
                                    workflows: all_workflows,
                                    selected_workflow: Some(workflow_id),
                                    on_close: move |_| run_workflow_id.set(None),
                                    on_complete: move |_| {},
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
