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
        api::{api_url, list_database_tables, list_workflows, save_workflow},
        models::{Workflow, WorkflowPayload},
        shared::{ErrorPanel, LoadingPanel, Shell, TableStatus, format_date},
    },
};

async fn load_plugin_tables() -> Result<Vec<(String, String, String)>, String> {
    let response =
        reqwest::get(api_url("/api/plugins")).await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(response.text().await.unwrap_or_default());
    }
    let plugins =
        response.json::<Vec<serde_json::Value>>().await.map_err(|error| error.to_string())?;
    let mut views = Vec::new();
    for plugin in plugins {
        if plugin["enabled"] != true {
            continue;
        }
        if let Some(items) = plugin["manifest"]["views"].as_array() {
            for view in items {
                if view["type"] == "datatable"
                    && let (Some(plugin_id), Some(view_id), Some(name)) =
                        (plugin["uid"].as_str(), view["id"].as_str(), view["name"].as_str())
                {
                    views.push((plugin_id.to_owned(), view_id.to_owned(), name.to_owned()));
                }
            }
        }
    }
    Ok(views)
}

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
    let workflows = use_server_future(list_workflows)?;
    let tables = use_server_future(list_database_tables)?;
    let plugin_tables = use_server_future(load_plugin_tables)?;
    let mut selected_plugin = use_signal(|| None::<(String, String)>);
    let mut selected_table = use_signal(|| "workflows".to_owned());
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
                        span { class: "database-kind", "{tables().and_then(Result::ok).map(|value| value.backend).unwrap_or_default()}" }
                    }
                    button { class: if selected_plugin().is_none() && selected_table() == "workflows" { "data-table-source selected" } else { "data-table-source" }, r#type: "button", onclick: move |_| { selected_plugin.set(None); selected_table.set("workflows".into()); },
                        span { class: "data-table-source__icon", Icon { name: "table" } }
                        span { "Workflows" }
                        span { class: "data-table-source__count", match workflows() { Some(Ok(ref rows)) => rows.len().to_string(), _ => "…".to_string() } }
                    }
                    if let Some(Ok(database)) = tables() {
                        for table in database.tables.into_iter().filter(|table| table != "workflows" && table != "public.workflows") {
                            button { class: if selected_plugin().is_none() && selected_table() == table { "data-table-source selected" } else { "data-table-source" }, r#type: "button", onclick: { let table=table.clone(); move |_| { selected_plugin.set(None); selected_table.set(table.clone()); } }, span { Icon { name: "table" } } "{table}" }
                        }
                    }
                    if let Some(Err(message)) = tables() { ErrorPanel { message } }
                    if let Some(Ok(views)) = plugin_tables() {
                        for (plugin_id,view_id,name) in views {
                            button { class: if selected_plugin() == Some((plugin_id.clone(),view_id.clone())) { "data-table-source selected" } else { "data-table-source" }, r#type: "button", onclick: { let plugin_id=plugin_id.clone(); let view_id=view_id.clone(); move |_| selected_plugin.set(Some((plugin_id.clone(),view_id.clone()))) }, span { Icon { name: "table" } } "{name}" }
                        }
                    }
                    if let Some(Err(message)) = plugin_tables() { ErrorPanel { message } }
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

                if let Some((plugin_id,view_id)) = selected_plugin() {
                    super::plugin_view::PluginViewPage { key: "{plugin_id}-{view_id}", plugin_id, view_id, embedded: true }
                } else if selected_table() != "workflows" {
                    DatabaseTable { key: "{selected_table()}", table: selected_table() }
                } else {
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
}

#[component]
fn DatabaseTable(table: String) -> Element {
    let selected = table.clone();
    let records = use_server_future(move || {
        let table = selected.clone();
        async move {
            let response = reqwest::get(api_url(&format!("/api/database/tables/{table}/records")))
                .await
                .map_err(|error| error.to_string())?;
            if !response.status().is_success() {
                return Err(response.text().await.unwrap_or_default());
            }
            response.json::<serde_json::Value>().await.map_err(|error| error.to_string())
        }
    })?;
    rsx! { section { class: "data-main panel",
        div { class: "data-table-heading", h2 { "{table}" } }
        match records() {
            Some(Ok(data)) => {
                let columns=data["columns"].as_array().map(|columns|columns.iter().filter_map(|column|column.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
                let rows=data["rows"].as_array().cloned().unwrap_or_default();
                rsx! { DataTable { toolbar: rsx! {}, columns: columns.clone(), row_ids: rows.iter().enumerate().map(|(index,row)|row.get("id").map(ToString::to_string).unwrap_or(index.to_string())).collect::<Vec<_>>(), empty_message: "This table has no records yet.".to_owned(),
                    for (index,row) in rows.into_iter().enumerate() {
                        DataTableRow { id: row.get("id").map(ToString::to_string).unwrap_or(index.to_string()), index,
                            for (column_index,column) in columns.iter().enumerate() { DataTableCell { index: column_index, "{row[column].as_str().map(str::to_owned).unwrap_or_else(|| row[column].to_string())}" } }
                        }
                    }
                } }
            },
            Some(Err(message)) => rsx! { ErrorPanel { message } },
            None => rsx! { LoadingPanel {} },
        }
    } }
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
