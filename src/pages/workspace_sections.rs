use dioxus::prelude::*;

use super::{
    api::{list_executions, list_workflows},
    models::ExecutionRecord,
    shared::{ErrorPanel, LoadingPanel, Shell, format_datetime},
};
use crate::Route;
use crate::components::data_table::DataTable;
use crate::components::data_table::{DataTableCell, DataTableRow};
use crate::components::icons::Icon;
use crate::components::workflow_run::WorkflowRunDialog;

#[component]
pub(crate) fn Executions() -> Element {
    let mut refresh_tick = use_signal(|| 0_u64);
    let executions = use_resource(move || {
        let current = refresh_tick();
        async move {
            let _ = current;
            list_executions().await
        }
    });
    let workflows = use_resource(list_workflows);
    let mut run_dialog_open = use_signal(|| false);
    let mut query = use_signal(String::new);
    let mut status_filter = use_signal(|| "all".to_string());
    let mut workflow_filter = use_signal(|| "all".to_string());
    let mut started_after = use_signal(String::new);
    let mut selected_execution = use_signal(|| None::<ExecutionRecord>);
    let on_run_complete = move |_| refresh_tick.set(refresh_tick() + 1);

    rsx! {
        Shell { active: "executions", header: "Executions", intro: "Review workflow runs and their results.", actions: rsx! {
            button { class: "button button-primary", r#type: "button", disabled: workflows().is_none(), onclick: move |_| run_dialog_open.set(true), Icon { name: "play" } "Run workflow" }
        },
            match executions() {
                Some(Ok(rows)) => {
                    let workflow_items = workflows().and_then(Result::ok).unwrap_or_default();
                    let needle = query().trim().to_lowercase();
                    let filtered = rows.into_iter().filter(|row| {
                        let matches_query = needle.is_empty()
                            || row.execution_id.to_lowercase().contains(&needle)
                            || row.workflow_name.to_lowercase().contains(&needle)
                            || row.error.as_deref().is_some_and(|error| error.to_lowercase().contains(&needle));
                        let matches_status = status_filter() == "all" || row.status == status_filter();
                        let matches_workflow = workflow_filter() == "all"
                            || row.workflow_id.is_some_and(|id| id.to_string() == workflow_filter());
                        let matches_date = started_after().is_empty()
                            || row.started_at.get(..10).is_some_and(|date| date >= started_after().as_str());
                        matches_query && matches_status && matches_workflow && matches_date
                    }).collect::<Vec<_>>();
                    let row_ids = filtered.iter().map(|row| row.id.to_string()).collect();
                    rsx! {
                        DataTable {
                            columns: vec!["Execution".to_string(), "Workflow".to_string(), "Status".to_string(), "Date & time".to_string(), "Duration".to_string(), "Error".to_string()],
                            row_ids,
                            toolbar: rsx! {
                                label { class: "workflow-list-search execution-list-search",
                                    Icon { name: "search" }
                                    input { r#type: "search", aria_label: "Search executions", placeholder: "Search executions...", value: "{query}", oninput: move |event| query.set(event.value()) }
                                }
                                select { class: "execution-filter", aria_label: "Filter executions by workflow", value: "{workflow_filter}", onchange: move |event| workflow_filter.set(event.value()),
                                    option { value: "all", "All workflows" }
                                    for workflow in workflow_items.iter() { option { value: "{workflow.id}", "{workflow.name}" } }
                                }
                                select { class: "execution-filter", aria_label: "Filter executions by status", value: "{status_filter}", onchange: move |event| status_filter.set(event.value()),
                                    option { value: "all", "All statuses" }
                                    option { value: "succeeded", "Success" }
                                    option { value: "failed", "Failed" }
                                    option { value: "running", "Running" }
                                }
                                label { class: "execution-date-filter", span { "Started after" } input { r#type: "date", aria_label: "Filter executions by start date", value: "{started_after}", oninput: move |event| started_after.set(event.value()) } }
                            },
                            empty_message: if needle.is_empty() && status_filter() == "all" && workflow_filter() == "all" && started_after().is_empty() { "No executions yet. Run a workflow to see its history here.".to_string() } else { "No executions match these filters.".to_string() },
                            for (index, row) in filtered.into_iter().enumerate() {
                                ExecutionRow { row, index, on_select: move |row| selected_execution.set(Some(row)) }
                            }
                        }
                        if let Some(row) = selected_execution() {
                            ExecutionDetailsDialog { row, on_close: move |_| selected_execution.set(None) }
                        }
                    }
                },
                Some(Err(error)) => rsx! { ErrorPanel { message: error } },
                None => rsx! { LoadingPanel {} },
            }
            if run_dialog_open() {
                match workflows() {
                    Some(Ok(items)) => rsx! {
                        WorkflowRunDialog {
                            workflows: items,
                            selected_workflow: None,
                            on_close: move |_| run_dialog_open.set(false),
                            on_complete: on_run_complete,
                        }
                    },
                    Some(Err(error)) => rsx! { ErrorPanel { message: error } },
                    None => rsx! { LoadingPanel {} },
                }
            }
        }
    }
}

#[component]
fn ExecutionRow(
    row: ExecutionRecord, index: usize, on_select: EventHandler<ExecutionRecord>,
) -> Element {
    let duration = row
        .duration_ms
        .map(|milliseconds| {
            if milliseconds < 1_000 {
                format!("{milliseconds} ms")
            } else {
                format!("{:.2} s", milliseconds as f64 / 1_000.0)
            }
        })
        .unwrap_or_else(|| "—".to_string());
    let status_class = match row.status.as_str() {
        "succeeded" => "execution-status execution-status--success",
        "running" => "execution-status execution-status--running",
        _ => "execution-status execution-status--failed",
    };
    rsx! {
        DataTableRow { id: row.id.to_string(), index,
            DataTableCell { index: 0, class: "execution-id-cell",
                if let Some(workflow_id) = row.workflow_id {
                    Link {
                        class: "execution-details-link",
                        to: Route::ViewWorkflowExecution { id: workflow_id, execution_id: row.execution_id.clone() },
                        "#{row.execution_id}"
                    }
                } else {
                    button { class: "execution-details-link", r#type: "button", onclick: { let row = row.clone(); move |_| on_select.call(row.clone()) }, "#{row.execution_id}" }
                }
            }
            DataTableCell { index: 1,
                if let Some(workflow_id) = row.workflow_id {
                    Link { to: Route::EditWorkflow { id: workflow_id }, "{row.workflow_name}" }
                } else { "{row.workflow_name}" }
            }
            DataTableCell { index: 2, span { class: "{status_class}", "{row.status}" } }
            DataTableCell { index: 3, class: "date-cell", "{format_datetime(&row.started_at)}" }
            DataTableCell { index: 4, "{duration}" }
            DataTableCell { index: 5, class: "execution-error-cell", if let Some(message) = row.error.as_ref() { "{message}" } else { "—" } }
        }
    }
}

#[component]
fn ExecutionDetailsDialog(row: ExecutionRecord, on_close: EventHandler<()>) -> Element {
    let status_class = match row.status.as_str() {
        "succeeded" => "execution-status execution-status--success",
        "running" => "execution-status execution-status--running",
        _ => "execution-status execution-status--failed",
    };
    let duration = row
        .duration_ms
        .map(|milliseconds| {
            if milliseconds < 1_000 {
                format!("{milliseconds} ms")
            } else {
                format!("{:.2} s", milliseconds as f64 / 1_000.0)
            }
        })
        .unwrap_or_else(|| "—".to_string());
    rsx! {
        div { class: "workflow-run-scrim execution-detail-scrim", role: "presentation", onclick: move |_| on_close.call(()),
            section { class: "workflow-execution-detail", role: "dialog", aria_modal: "true", aria_label: "Execution details", onclick: move |event| event.stop_propagation(),
                header { class: "workflow-execution-detail__header",
                    div { h2 { "Execution #{row.execution_id}" } p { "{row.workflow_name}" } }
                    button { class: "workflow-run-dialog__close", r#type: "button", title: "Close", aria_label: "Close execution details", onclick: move |_| on_close.call(()), Icon { name: "close" } }
                }
                div { class: "workflow-execution-detail__summary",
                    span { class: "{status_class}", "{row.status}" }
                    div { strong { "Started" } span { "{format_datetime(&row.started_at)}" } }
                    div { strong { "Duration" } span { "{duration}" } }
                    if let Some(finished_at) = row.finished_at.as_ref() { div { strong { "Finished" } span { "{format_datetime(finished_at)}" } } }
                }
                if let Some(message) = row.error.as_ref() {
                    section { class: "workflow-execution-detail__error", strong { "Execution failed" } p { "{message}" } }
                }
                div { class: "workflow-execution-detail__data",
                    section { h3 { "Input" } pre { "{pretty_execution_json(&row.input_json)}" } }
                    section { h3 { "Output" }
                        if let Some(output) = row.output_json.as_ref() { pre { "{pretty_execution_json(output)}" } }
                        else { p { class: "workflow-execution-detail__muted", "No output was recorded." } }
                    }
                }
            }
        }
    }
}

fn pretty_execution_json(value: &str) -> String {
    serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| value.to_string())
}

#[component]
pub(crate) fn Credentials() -> Element {
    rsx! {
        Shell { active: "credentials", header: "Credentials", intro: "Manage the connections your workflows can use.", actions: rsx! {},
            DataTable {
                columns: vec!["Name".to_string(), "Type".to_string(), "Used by".to_string(), "Created".to_string(), "Updated".to_string()],
                row_ids: vec![],
                toolbar: rsx! {},
                empty_message: "No credentials configured yet.".to_string(),
            }
        }
    }
}

#[component]
pub(crate) fn Variables() -> Element {
    rsx! { WorkspaceSection { active: "variables", title: "Variables", description: "Manage values shared across your workspace.", icon: "variable", empty_title: "No variables defined", empty_copy: "Create workspace variables to reuse values across workflows." } }
}

#[component]
fn WorkspaceSection(
    active: &'static str, title: &'static str, description: &'static str, icon: &'static str,
    empty_title: &'static str, empty_copy: &'static str,
) -> Element {
    rsx! {
        Shell { active, header: title, intro: description, actions: rsx! {},
            section { class: "resource-empty panel",
                span { class: "resource-empty__icon", Icon { name: icon } }
                h2 { "{empty_title}" }
                p { "{empty_copy}" }
            }
        }
    }
}
