use dioxus::prelude::*;

use super::{
    api::{list_executions, list_workflows},
    models::ExecutionRecord,
    shared::{ErrorPanel, LoadingPanel, Shell, format_date},
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
    let on_run_complete = move |_| refresh_tick.set(refresh_tick() + 1);

    rsx! {
        Shell { active: "executions", header: "Executions", intro: "Review workflow runs and their results.", actions: rsx! {
            button { class: "button button-primary", r#type: "button", disabled: workflows().is_none(), onclick: move |_| run_dialog_open.set(true), Icon { name: "play" } "Run workflow" }
        },
            match executions() {
                Some(Ok(rows)) => {
                    let row_ids = rows.iter().map(|row| row.id.to_string()).collect();
                    rsx! {
                        DataTable {
                            columns: vec!["Execution".to_string(), "Workflow".to_string(), "Status".to_string(), "Started".to_string(), "Duration".to_string(), "Error".to_string()],
                            row_ids,
                            toolbar: rsx! {},
                            empty_message: "No executions yet. Run a workflow to see its history here.".to_string(),
                            for (index, row) in rows.into_iter().enumerate() {
                                ExecutionRow { row, index }
                            }
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
fn ExecutionRow(row: ExecutionRecord, index: usize) -> Element {
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
            DataTableCell { index: 0, class: "execution-id-cell", "#{row.id}" }
            DataTableCell { index: 1,
                if let Some(workflow_id) = row.workflow_id {
                    Link { to: Route::EditWorkflow { id: workflow_id }, "{row.workflow_name}" }
                } else { "{row.workflow_name}" }
            }
            DataTableCell { index: 2, span { class: "{status_class}", "{row.status}" } }
            DataTableCell { index: 3, class: "date-cell", "{format_date(&row.started_at)}" }
            DataTableCell { index: 4, "{duration}" }
            DataTableCell { index: 5, class: "execution-error-cell", if let Some(message) = row.error.as_ref() { "{message}" } else { "—" } }
        }
    }
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
