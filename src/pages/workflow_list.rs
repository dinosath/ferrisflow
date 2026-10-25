use dioxus::prelude::*;

use crate::{
    Route,
    components::data_table::{DataTable, DataTableCell, DataTableRow},
    components::icons::Icon,
    pages::shared::{ErrorPanel, LoadingPanel, Shell, TableStatus, format_date},
    pages::{api::list_workflows, models::Workflow},
};

#[component]
pub(crate) fn WorkflowList() -> Element {
    let workflows = use_resource(list_workflows);
    let mut query = use_signal(String::new);

    rsx! {
        Shell { active: "workflows",
            header: "Workflows",
            intro: "A clear view of the documents that define your work.",
            actions: rsx! { Link { class: "button button-primary", to: Route::NewWorkflow {}, "+ New workflow" } },
            section { class: "workflows-list",
                match workflows() {
                    Some(Ok(items)) => {
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
                                },
                                empty_message: if query().is_empty() { "No workflows yet. Create a workflow to get started.".to_string() } else { "No workflows match your search.".to_string() },
                                for (index, (workflow, description, updated)) in filtered.into_iter().enumerate() {
                                    DataTableRow { id: workflow.id.to_string(), index,
                                        DataTableCell { index: 0, class: "name-cell",
                                            Link { to: Route::WorkflowDetail { id: workflow.id }, "{workflow.name}" }
                                            small { "#{workflow.id}" }
                                        }
                                        DataTableCell { index: 1, "v{workflow.version}" }
                                        DataTableCell { index: 2, TableStatus { active: workflow.active } }
                                        DataTableCell { index: 3, class: "description-cell", "{description}" }
                                        DataTableCell { index: 4, class: "date-cell", "{updated}" }
                                    }
                                }
                            }
                        }
                    },
                    Some(Err(error)) => rsx! { ErrorPanel { message: error } },
                    None => rsx! { LoadingPanel {} },
                }
            }
        }
    }
}
