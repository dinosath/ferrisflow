use dioxus::prelude::*;

use crate::{
    Route,
    pages::{
        api::{delete_workflow, read_workflow},
        shared::{ErrorPanel, LoadingPanel, Shell, StatusPill, format_date, pretty_json},
    },
};

#[component]
pub(crate) fn WorkflowDetail(id: i32) -> Element {
    let workflow = use_resource(move || read_workflow(id));
    let navigator = use_navigator();
    let mut deleting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    rsx! {
        Shell { active: "workflows", header: "Workflow details", intro: "Inspect the saved definition and registry metadata.",
            actions: rsx! { Link { class: "button button-quiet", to: Route::WorkflowList {}, "Back to workflows" } },
            match workflow() {
                Some(Ok(item)) => {
                    let description = item.description.as_deref().unwrap_or("No description provided.").to_string();
                    rsx! {
                    if let Some(message) = error() { ErrorPanel { message } }
                    section { class: "detail-header",
                        div { class: "detail-title",
                            Link { class: "back-link", to: Route::WorkflowList {}, "‹ Workflows" }
                            h2 { "{item.name}" }
                            p { "{description}" }
                        }
                        div { class: "detail-actions",
                            StatusPill { active: item.active }
                            Link { class: "button button-quiet", to: Route::EditWorkflow { id }, "Edit workflow" }
                            button {
                                class: "button button-danger-quiet",
                                disabled: deleting(),
                                onclick: move |_| async move {
                                    deleting.set(true);
                                    match delete_workflow(id).await {
                                        Ok(()) => {
                                            navigator.push(Route::WorkflowList {});
                                        }
                                        Err(message) => error.set(Some(message)),
                                    }
                                    deleting.set(false);
                                },
                                if deleting() { "Deleting…" } else { "Delete workflow" }
                            }
                        }
                    }
                    section { class: "detail-grid",
                        div { class: "detail-meta panel",
                            h3 { "Registry record" }
                            dl {
                                dt { "Workflow ID" } dd { "#{item.id}" }
                                dt { "Version" } dd { "v{item.version}" }
                                dt { "Created" } dd { "{format_date(&item.created_at)}" }
                                dt { "Last updated" } dd { "{format_date(&item.last_updated)}" }
                                dt { "Status" } dd { StatusPill { active: item.active } }
                            }
                        }
                        div { class: "definition-panel panel",
                            div { class: "definition-heading",
                                div { h3 { "OWS definition" } p { "Normalized document stored by the REST API" } }
                                span { class: "language-tag", "JSON" }
                            }
                            pre { class: "definition-code", code { "{pretty_json(&item.definition_json)}" } }
                        }
                    }
                }
                },
                Some(Err(message)) => rsx! { ErrorPanel { message } },
                None => rsx! { LoadingPanel {} },
            }
        }
    }
}
