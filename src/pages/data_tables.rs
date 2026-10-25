use crate::components::data_table::{DataTable, DataTableCell, DataTableRow};
use crate::components::icons::Icon;
use crate::components::ui::{
    button::{Button, ButtonSize, ButtonVariant},
    input::Input,
};
use dioxus::prelude::*;

use crate::{
    Route,
    pages::{
        api::{list_workflows, save_workflow},
        models::{Workflow, WorkflowPayload},
        shared::{ErrorPanel, LoadingPanel, Shell, TableStatus, format_date},
    },
};

#[derive(Clone, Copy, PartialEq)]
enum DataView {
    Table,
    Form,
    Kanban,
}

#[derive(Clone, Debug, PartialEq)]
struct CustomView {
    name: String,
    filter: String,
}

#[component]
pub(crate) fn DataTables() -> Element {
    let workflows = use_resource(list_workflows);
    let mut view = use_signal(|| DataView::Table);
    let selected_id = use_signal(|| None::<i32>);
    let mut search = use_signal(String::new);
    let mut custom_views = use_signal(Vec::<CustomView>::new);
    let mut view_name = use_signal(String::new);
    let mut creating_view = use_signal(|| false);
    let mut active_custom = use_signal(|| None::<usize>);
    let record_count = match workflows() {
        Some(Ok(ref rows)) => rows.len(),
        _ => 0,
    };

    rsx! {
        Shell { active: "datatables", header: "Data tables", intro: "Browse and organize records from your workspace database.", actions: rsx! {},
            div { class: "data-workspace",
                aside { class: "data-sidebar panel",
                    div { class: "data-sidebar__heading",
                        span { class: "data-eyebrow", "DATABASE" }
                        span { class: "database-kind", "SQLite" }
                    }
                    button { class: "data-table-source selected", r#type: "button",
                        span { class: "data-table-source__icon", Icon { name: "table" } }
                        span { "Workflows" }
                        span { class: "data-table-source__count", match workflows() { Some(Ok(ref rows)) => rows.len().to_string(), _ => "…".to_string() } }
                    }
                    div { class: "data-sidebar__divider" }
                    div { class: "data-sidebar__heading",
                        span { class: "data-eyebrow", "CUSTOM VIEWS" }
                        button { class: "icon-button", title: "Create custom view", r#type: "button", onclick: move |_| creating_view.set(true), Icon { name: "plus" } }
                    }
                    for (index, saved_view) in custom_views().iter().enumerate() {
                        button {
                            class: if active_custom() == Some(index) { "data-saved-view selected" } else { "data-saved-view" },
                            r#type: "button",
                            onclick: move |_| { active_custom.set(Some(index)); view.set(DataView::Table); },
                            span { Icon { name: "list" } } "{saved_view.name}"
                        }
                    }
                    if creating_view() {
                        div { class: "custom-view-create",
                            input { placeholder: "View name", value: "{view_name}", oninput: move |event| view_name.set(event.value()) }
                            Button { class: "custom-view-save", size: ButtonSize::Sm, variant: ButtonVariant::Primary, r#type: "button", disabled: view_name().trim().is_empty(), onclick: move |_| {
                                let name = view_name().trim().to_string();
                                if !name.is_empty() {
                                    let mut saved = custom_views();
                                    saved.push(CustomView { name, filter: search() });
                                    custom_views.set(saved);
                                    active_custom.set(Some(custom_views().len().saturating_sub(1)));
                                    view_name.set(String::new());
                                    creating_view.set(false);
                                }
                            }, "Save view" }
                        }
                    }
                    div { class: "data-sidebar__footer", span { class: "database-status-dot" } "Connected database" }
                }

                section { class: "data-main panel",
                    div { class: "data-table-heading",
                        div {
                            div { class: "data-breadcrumb", "Data tables" span { "/" } strong { "Workflows" } }
                            h2 { "Workflows" }
                            p { "SQLite table · {record_count} records" }
                        }
                        Link { class: "button button-primary", to: Route::NewWorkflow {}, "+ Add record" }
                    }
                    div { class: "data-view-toolbar",
                        div { class: "data-view-tabs",
                            button { class: if view() == DataView::Table && active_custom().is_none() { "data-view-tab selected" } else { "data-view-tab" }, r#type: "button", onclick: move |_| { view.set(DataView::Table); active_custom.set(None); }, span { Icon { name: "table" } } "Table" }
                            button { class: if view() == DataView::Form { "data-view-tab selected" } else { "data-view-tab" }, r#type: "button", onclick: move |_| { view.set(DataView::Form); active_custom.set(None); }, span { Icon { name: "form" } } "Form" }
                            button { class: if view() == DataView::Kanban { "data-view-tab selected" } else { "data-view-tab" }, r#type: "button", onclick: move |_| { view.set(DataView::Kanban); active_custom.set(None); }, span { Icon { name: "kanban" } } "Kanban" }
                            if let Some(index) = active_custom() {
                                if let Some(saved_view) = custom_views().get(index) {
                                    button { class: "data-view-tab selected custom-tab", r#type: "button", onclick: move |_| view.set(DataView::Table), span { Icon { name: "sparkles" } } "{saved_view.name}" }
                                }
                            }
                            button { class: "data-custom-view-button", r#type: "button", onclick: move |_| creating_view.set(true), "+ Custom view" }
                        }
                        if view() != DataView::Table {
                            label { class: "data-search",
                                span { Icon { name: "search" } }
                                Input { class: "data-search__input", placeholder: "Search records…", value: search(), on_value_change: move |value| search.set(value) }
                            }
                        }
                    }
                    match workflows() {
                        Some(Ok(rows)) => {
                            let filter = active_custom()
                                .and_then(|index| custom_views().get(index).map(|saved| saved.filter.clone()))
                                .unwrap_or_else(|| search());
                            match view() {
                                DataView::Table => rsx! { TableView { rows, filter, search_value: search(), on_search: move |value| search.set(value) } },
                                DataView::Form => rsx! { FormView { rows, selected_id } },
                                DataView::Kanban => rsx! { KanbanView { rows, filter } },
                            }
                        },
                        Some(Err(error)) => rsx! { ErrorPanel { message: error } },
                        None => rsx! { LoadingPanel {} },
                    }
                }
            }
        }
    }
}

#[component]
fn TableView(
    rows: Vec<Workflow>, filter: String, search_value: String, on_search: EventHandler<String>,
) -> Element {
    let query = filter.trim().to_lowercase();
    let rows: Vec<Workflow> = rows.into_iter().filter(|row| matches_filter(row, &query)).collect();
    let row_ids = rows.iter().map(|row| row.id.to_string()).collect();
    rsx! {
        DataTable {
            class: "data-record-table-shell",
            columns: vec!["ID".to_string(), "Name".to_string(), "Description".to_string(), "Status".to_string(), "Version".to_string(), "Last updated".to_string()],
            row_ids,
            toolbar: rsx! {
                label { class: "data-search",
                    span { Icon { name: "search" } }
                    Input { class: "data-search__input", placeholder: "Search records…", value: search_value, on_value_change: move |value| on_search.call(value) }
                }
            },
            empty_message: "No records match this view.".to_string(),
            for (index, row) in rows.into_iter().enumerate() {
                {
                    let description = row.description.clone().unwrap_or_else(|| "—".into());
                    rsx! { DataTableRow { id: row.id.to_string(), index,
                        DataTableCell { index: 0, class: "data-id-cell", "{row.id}" }
                        DataTableCell { index: 1, class: "data-name-cell", Link { to: Route::WorkflowDetail { id: row.id }, "{row.name}" } }
                        DataTableCell { index: 2, class: "data-description-cell", "{description}" }
                        DataTableCell { index: 3, TableStatus { active: row.active } }
                        DataTableCell { index: 4, "v{row.version}" }
                        DataTableCell { index: 5, class: "date-cell", "{format_date(&row.last_updated)}" }
                    }
                    }
                }
            }
        }
    }
}

#[component]
fn FormView(rows: Vec<Workflow>, selected_id: Signal<Option<i32>>) -> Element {
    let selected = rows.iter().find(|row| Some(row.id) == selected_id()).or_else(|| rows.first());
    rsx! {
        div { class: "data-form-view",
            nav { class: "data-record-list",
                for row in rows.iter() {
                    {
                        let row_id = row.id;
                        rsx! { button { class: if selected_id() == Some(row_id) { "data-record-option selected" } else { "data-record-option" }, r#type: "button", onclick: move |_| selected_id.set(Some(row_id)),
                        span { class: "data-record-option__avatar", "{row.name.chars().next().unwrap_or('W').to_ascii_uppercase()}" }
                        span { strong { "{row.name}" } small { "Record #{row.id}" } }
                    } }
                    }
                }
            }
            if let Some(row) = selected {
                WorkflowRecordForm { key: "{row.id}", workflow: row.clone() }
            } else {
                div { class: "data-empty", "No records to display." }
            }
        }
    }
}

#[component]
fn WorkflowRecordForm(workflow: Workflow) -> Element {
    let id = workflow.id;
    let definition_json = workflow.definition_json.clone();
    let mut name = use_signal(|| workflow.name.clone());
    let mut description = use_signal(|| workflow.description.clone().unwrap_or_default());
    let mut version = use_signal(|| workflow.version.to_string());
    let mut active = use_signal(|| workflow.active);
    let mut saving = use_signal(|| false);
    let mut result = use_signal(|| None::<Result<(), String>>);
    let save_definition_json = definition_json.clone();

    rsx! {
        article { class: "data-record-form",
            div { class: "data-form-title",
                div { span { class: "data-eyebrow", "EDIT RECORD · #{id}" } h3 { "{name}" } }
                        Button { class: "record-save-button", size: ButtonSize::Sm, variant: ButtonVariant::Primary, r#type: "button", disabled: saving(), onclick: move |_| {
                            let definition_json = save_definition_json.clone();
                            spawn(async move {
                        saving.set(true);
                        let payload = WorkflowPayload {
                            active: active(),
                            definition_json,
                            description: if description().trim().is_empty() { None } else { Some(description()) },
                            name: name(),
                            version: version().parse().unwrap_or(1),
                        };
                        result.set(Some(match save_workflow(payload, Some(id)).await { Ok(_) => Ok(()), Err(error) => Err(error) }));
                        saving.set(false);
                            });
                }, if saving() { "Saving…" } else { "Save record" } }
            }
            if let Some(Err(message)) = result() { div { class: "error-panel", "{message}" } }
            if let Some(Ok(())) = result() { p { class: "record-saved", "Changes saved to SQLite." } }
            label { class: "field", span { "Name" } input { value: "{name}", oninput: move |event| { name.set(event.value()); result.set(None); } } }
            label { class: "field", span { "Description" } textarea { rows: "3", value: "{description}", oninput: move |event| { description.set(event.value()); result.set(None); } } }
            div { class: "field-row data-record-fields",
                label { class: "field", span { "Version" } input { r#type: "number", min: "1", value: "{version}", oninput: move |event| { version.set(event.value()); result.set(None); } } }
                label { class: "toggle-field", input { r#type: "checkbox", checked: active(), oninput: move |event| { active.set(event.checked()); result.set(None); } } span { "Active" } }
            }
            label { class: "field", span { "OWS definition" } pre { class: "data-definition-preview", "{definition_json}" } }
        }
    }
}

#[component]
fn KanbanView(rows: Vec<Workflow>, filter: String) -> Element {
    let query = filter.trim().to_lowercase();
    let rows: Vec<Workflow> = rows.into_iter().filter(|row| matches_filter(row, &query)).collect();
    let active_rows: Vec<Workflow> = rows.iter().filter(|row| row.active).cloned().collect();
    let paused_rows: Vec<Workflow> = rows.into_iter().filter(|row| !row.active).collect();
    rsx! {
        div { class: "data-kanban",
            KanbanColumn { title: "Active", active: true, rows: active_rows }
            KanbanColumn { title: "Paused", active: false, rows: paused_rows }
        }
    }
}

#[component]
fn KanbanColumn(title: &'static str, active: bool, rows: Vec<Workflow>) -> Element {
    rsx! {
        section { class: "kanban-column",
            div { class: "kanban-column__heading", span { class: if active { "kanban-dot active-dot" } else { "kanban-dot paused-dot" } } strong { "{title}" } span { "{rows.len()}" } }
            for row in rows {
                {
                    let description = row.description.clone().unwrap_or_else(|| "No description".into());
                    rsx! { Link { class: "kanban-card", to: Route::WorkflowDetail { id: row.id },
                        span { class: "data-eyebrow", "WORKFLOW · #{row.id}" }
                        strong { "{row.name}" }
                        p { "{description}" }
                        div { class: "kanban-card__footer", span { "v{row.version}" } span { "Updated {format_date(&row.last_updated)}" } }
                    } }
                }
            }
        }
    }
}

fn matches_filter(row: &Workflow, query: &str) -> bool {
    query.is_empty()
        || row.name.to_lowercase().contains(query)
        || row.description.as_deref().unwrap_or_default().to_lowercase().contains(query)
}
