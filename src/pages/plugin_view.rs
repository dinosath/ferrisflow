use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::Value;

use crate::components::data_table::{DataTable, DataTableCell, DataTableRow};

use crate::{
    components::workflow_run::WorkflowRunDialog,
    pages::{
        api::list_workflows,
        models::Workflow,
        shared::{ErrorPanel, LoadingPanel, Shell},
    },
};

fn api_url(path: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let origin = web_sys::window()
            .and_then(|window| window.location().origin().ok())
            .unwrap_or_default();
        format!("{origin}{path}")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        format!(
            "{}{path}",
            std::env::var("FERRIS_WORKFLOW_API_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".into())
        )
    }
}

#[derive(Deserialize)]
struct PluginPayload {
    name: String,
    manifest: Value,
}

async fn load_plugin_view(
    plugin_id: String, view_id: String,
) -> Result<(String, Value, Vec<Workflow>), String> {
    let response = reqwest::get(api_url(&format!("/api/plugins/{plugin_id}")))
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Could not load plugin view ({status}): {}",
            response.text().await.unwrap_or_default()
        ));
    }
    let plugin: PluginPayload = response.json().await.map_err(|error| error.to_string())?;
    let view = plugin
        .manifest
        .get("views")
        .and_then(Value::as_array)
        .and_then(|views| {
            views.iter().find(|view| view.get("id").and_then(Value::as_str) == Some(&view_id))
        })
        .cloned()
        .ok_or_else(|| format!("Plugin view '{view_id}' was not found"))?;
    let workflows = list_workflows().await?;
    let names = view
        .get("config")
        .and_then(|config| config.get("workflowNames"))
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let workflows = workflows
        .into_iter()
        .filter(|workflow| {
            serde_json::from_str::<Value>(&workflow.definition_json)
                .ok()
                .and_then(|definition| {
                    definition.get("document")?.get("name")?.as_str().map(str::to_owned)
                })
                .is_some_and(|name| names.contains(&name.as_str()))
        })
        .collect();
    Ok((plugin.name, view, workflows))
}

async fn load_plugin_page(plugin_id: String, page_id: String) -> Result<(String, String), String> {
    let response = reqwest::get(api_url(&format!("/api/plugins/{plugin_id}")))
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Could not load plugin page ({status}): {}",
            response.text().await.unwrap_or_default()
        ));
    }
    let plugin: PluginPayload = response.json().await.map_err(|error| error.to_string())?;
    let page = plugin
        .manifest
        .get("pages")
        .and_then(Value::as_array)
        .and_then(|pages| {
            pages.iter().find(|page| page.get("id").and_then(Value::as_str) == Some(&page_id))
        })
        .ok_or_else(|| format!("Plugin page '{page_id}' was not found"))?;
    let view_id = page
        .get("view")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Plugin page '{page_id}' does not reference a view"))?;
    Ok((plugin.name, view_id.to_owned()))
}

#[component]
pub(crate) fn PluginViewPage(
    plugin_id: String, view_id: String, #[props(default = false)] embedded: bool,
) -> Element {
    let plugin_id_for_load = plugin_id.clone();
    let view_id_for_load = view_id.clone();
    let view = use_server_future(move || {
        load_plugin_view(plugin_id_for_load.clone(), view_id_for_load.clone())
    })?;
    match view() {
        Some(Ok((plugin_name, definition, workflows))) => {
            match definition.get("type").and_then(Value::as_str).unwrap_or("workflow-list") {
                "datatable" => {
                    rsx! { PluginDataTableView { plugin_name, definition, workflows, embedded } }
                }
                _ => {
                    rsx! { PluginWorkflowView { plugin_id, view_id, plugin_name, definition, workflows } }
                }
            }
        }
        Some(Err(message)) => {
            rsx! { Shell { active: "plugins", header: "Plugin view", intro: "Could not load the plugin view.", actions: rsx! {}, ErrorPanel { message } } }
        }
        None => {
            rsx! { Shell { active: "plugins", header: "Plugin view", intro: "Loading plugin view…", actions: rsx! {}, LoadingPanel {} } }
        }
    }
}

#[component]
fn PluginWorkflowView(
    plugin_id: String, view_id: String, plugin_name: String, definition: Value,
    workflows: Vec<Workflow>,
) -> Element {
    let initial = definition
        .get("config")
        .and_then(|config| config.get("listView"))
        .cloned()
        .unwrap_or_else(default_list_view);
    let mut list_view = use_signal(|| initial.clone());
    let mut settings_open = use_signal(|| false);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut query = use_signal(String::new);
    let mut status_filter = use_signal(|| "all".to_string());
    let mut page = use_signal(|| 1_usize);
    let mut selected = use_signal(Vec::<i32>::new);
    let mut run_id = use_signal(|| None::<i32>);

    let settings = list_view().get("settings").cloned().unwrap_or_default();
    let enable_search = settings.get("enableSearch").and_then(Value::as_bool).unwrap_or(true);
    let enable_filters = settings.get("enableFilters").and_then(Value::as_bool).unwrap_or(true);
    let enable_bulk = settings.get("enableBulkActions").and_then(Value::as_bool).unwrap_or(false);
    let page_size = settings.get("entriesPerPage").and_then(Value::as_u64).unwrap_or(20) as usize;
    let mut rows = workflows.clone();
    if enable_search && !query().trim().is_empty() {
        let needle = query().trim().to_lowercase();
        rows.retain(|item| {
            item.name.to_lowercase().contains(&needle)
                || item
                    .description
                    .as_deref()
                    .is_some_and(|value| value.to_lowercase().contains(&needle))
        });
    }
    if enable_filters {
        match status_filter().as_str() {
            "active" => rows.retain(|workflow| workflow.active),
            "inactive" => rows.retain(|workflow| !workflow.active),
            _ => {}
        }
    }
    let sort_field = settings.get("defaultSortAttribute").and_then(Value::as_str).unwrap_or("name");
    rows.sort_by(|a, b| {
        let ordering = match sort_field {
            "description" => a.description.cmp(&b.description),
            "active" => a.active.cmp(&b.active),
            _ => a.name.cmp(&b.name),
        };
        if settings.get("defaultSortOrder").and_then(Value::as_str) == Some("desc") {
            ordering.reverse()
        } else {
            ordering
        }
    });
    let page_count = rows.len().div_ceil(page_size.max(1)).max(1);
    let current_page = page().clamp(1, page_count);
    let visible_rows = rows
        .iter()
        .skip((current_page - 1) * page_size)
        .take(page_size)
        .cloned()
        .collect::<Vec<_>>();
    let fields = list_view().get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
    let columns = fields
        .iter()
        .filter_map(|field| field.get("label").and_then(Value::as_str).map(str::to_owned))
        .collect::<Vec<_>>();
    let settings_page_size = list_view()
        .get("settings")
        .and_then(|s| s.get("entriesPerPage"))
        .and_then(Value::as_u64)
        .unwrap_or(20);
    let settings_sort_attribute = list_view()
        .get("settings")
        .and_then(|s| s.get("defaultSortAttribute"))
        .and_then(Value::as_str)
        .unwrap_or("name")
        .to_owned();
    let settings_sort_order = list_view()
        .get("settings")
        .and_then(|s| s.get("defaultSortOrder"))
        .and_then(Value::as_str)
        .unwrap_or("asc")
        .to_owned();

    rsx! {
        Shell { active: "plugins", header: "Plugin view", intro: "Browse and configure the workflow list provided by this plugin.", actions: rsx! {},
            section { class: "panel plugin-view-workflows",
                div { class: "plugins-heading",
                    div { span { class: "data-eyebrow", "{plugin_name} / LIST VIEW" }
                        h2 { if let Some(name) = definition.get("name").and_then(Value::as_str) { "{name}" } else { "Plugin workflows" } }
                        if let Some(description) = definition.get("description").and_then(Value::as_str) { p { "{description}" } }
                    }
                    button { class: "button button-ghost", r#type: "button", onclick: move |_| settings_open.set(true), "Configure view" }
                }
                div { class: "shared-table-toolbar",
                    div { class: "shared-table-toolbar__filters",
                        if enable_search { input { r#type: "search", class: "workflow-list-search", placeholder: "Search workflows…", value: "{query}", oninput: move |event| { query.set(event.value()); page.set(1); } } }
                        if enable_filters {
                            select { class: "execution-filter", value: "{status_filter}", onchange: move |event| { status_filter.set(event.value()); page.set(1); },
                                option { value: "all", "All workflows" }
                                option { value: "active", "Active" }
                                option { value: "inactive", "Inactive" }
                            }
                        }
                    }
                    span { class: "data-eyebrow", "{rows.len()} WORKFLOWS" }
                }
                if workflows.is_empty() {
                    p { class: "plugin-empty-note", "This view does not declare executable workflows." }
                } else {
                    div { class: "shared-table-shell shared-table-scroll",
                        table { class: "shared-data-table",
                            thead { tr {
                                if enable_bulk { th { "Select" } }
                                for label in columns.iter() { th { class: "shared-table-heading", "{label}" } }
                                th { "Status" }
                                th { "Actions" }
                            } }
                            tbody {
                                for workflow in visible_rows.iter() {
                                    tr { class: "shared-table-row",
                                        if enable_bulk { td { input { r#type: "checkbox", checked: selected().contains(&workflow.id), onchange: { let id = workflow.id; move |event| { let mut ids = selected(); if event.checked() { if !ids.contains(&id) { ids.push(id); } } else { ids.retain(|value| *value != id); } selected.set(ids); } } } } }
                                        for field in fields.iter() {
                                            { let key = field.get("name").and_then(Value::as_str).unwrap_or("name"); let label = field.get("label").and_then(Value::as_str).unwrap_or(key); rsx! { td { "{workflow_field(workflow, key, label)}" } } }
                                        }
                                        td { span { class: if workflow.active { "execution-status execution-status--success" } else { "execution-status" }, if workflow.active { "Active" } else { "Inactive" } } }
                                        td { button { class: "button button-quiet", r#type: "button", onclick: { let id = workflow.id; move |_| run_id.set(Some(id)) }, "Run" } }
                                    }
                                }
                                if visible_rows.is_empty() { tr { td { colspan: "6", class: "plugin-empty-note", "No workflows match these settings." } } }
                            }
                        }
                    }
                    div { class: "shared-table-pagination",
                        label { "Entries per page " select { value: "{page_size}", onchange: move |event| { let value = event.value().parse::<usize>().unwrap_or(20); let mut config = list_view(); config["settings"]["entriesPerPage"] = Value::from(value); list_view.set(config); page.set(1); }, option { value: "10", "10" } option { value: "20", "20" } option { value: "50", "50" } option { value: "100", "100" } } }
                        span { "Page {current_page} of {page_count}" }
                        button { class: "button button-quiet", disabled: current_page <= 1, onclick: move |_| page.set(current_page.saturating_sub(1)), "Previous" }
                        button { class: "button button-quiet", disabled: current_page >= page_count, onclick: move |_| page.set((current_page + 1).min(page_count)), "Next" }
                    }
                }
                if enable_bulk && !selected().is_empty() { p { class: "data-eyebrow", "{selected().len()} SELECTED" } }
                if let Some(message) = error() { div { class: "error-panel", "{message}" } }
            }
        }
        if settings_open() {
            div { class: "dialog-backdrop", onclick: move |_| settings_open.set(false),
                section { class: "plugin-create-dialog panel", role: "dialog", aria_modal: "true", onclick: |event| event.stop_propagation(),
                    div { class: "builder-modal__header", div { span { class: "data-eyebrow", "LIST VIEW SETTINGS" } h2 { "Configure view" } p { "Choose the workflow list controls and displayed fields." } } button { class: "icon-button", r#type: "button", onclick: move |_| settings_open.set(false), "×" } }
                    div { class: "plugin-view-settings-section", h3 { "Settings" }
                        for (key,label) in [("enableSearch","Enable search"),("enableFilters","Enable filters"),("enableBulkActions","Enable bulk actions")] {
                            label { class: "plugin-view-setting-row", span { "{label}" } input { r#type: "checkbox", checked: list_view().get("settings").and_then(|settings| settings.get(key)).and_then(Value::as_bool).unwrap_or(false), onchange: move |event| { let mut config=list_view(); config["settings"][key]=Value::Bool(event.checked()); list_view.set(config); } } }
                        }
                        label { class: "field", span { "Entries per page" } select { value: "{settings_page_size}", onchange: move |event| { let mut config=list_view(); config["settings"]["entriesPerPage"]=Value::from(event.value().parse::<u64>().unwrap_or(20)); list_view.set(config); }, option { value:"10","10" } option {value:"20","20"} option {value:"50","50"} option {value:"100","100"} } }
                        label { class: "field", span { "Default sort attribute" } select { value: "{settings_sort_attribute}", onchange: move |event| { let mut config=list_view(); config["settings"]["defaultSortAttribute"]=Value::String(event.value()); list_view.set(config); }, option {value:"name","Workflow"} option {value:"description","Description"} option {value:"active","Status"} } }
                        label { class: "field", span { "Default sort order" } select { value: "{settings_sort_order}", onchange: move |event| { let mut config=list_view(); config["settings"]["defaultSortOrder"]=Value::String(event.value()); list_view.set(config); }, option {value:"asc","Ascending"} option {value:"desc","Descending"} } }
                    }
                    div { class: "plugin-view-settings-section", h3 { "View fields" }
                        for (index,field) in fields.iter().enumerate() {
                            { let field_name=field.get("name").and_then(Value::as_str).unwrap_or("").to_owned(); let field_label=field.get("label").and_then(Value::as_str).unwrap_or("").to_owned(); let sortable=field.get("sortable").and_then(Value::as_bool).unwrap_or(false); let field_count=fields.len(); rsx! {
                                div { class: "plugin-view-setting-row", span { "{field_name}" }
                                    input { value: "{field_label}", aria_label: "Field label", oninput: move |event| { let mut config=list_view(); if let Some(item)=config.get_mut("fields").and_then(Value::as_array_mut).and_then(|items|items.get_mut(index)) { item["label"]=Value::String(event.value()); } list_view.set(config); } }
                                    label { class: "plugin-view-setting-row__sort", span { "Enable sort" } input { r#type:"checkbox", checked:sortable, onchange:move |event|{let mut config=list_view();if let Some(item)=config.get_mut("fields").and_then(Value::as_array_mut).and_then(|items|items.get_mut(index)){item["sortable"]=Value::Bool(event.checked());}list_view.set(config);} } }
                                    button { class: "button button-quiet", disabled: index == 0, onclick: move |_| { let mut config=list_view(); if let Some(items)=config.get_mut("fields").and_then(Value::as_array_mut) { if index>0 { items.swap(index,index-1); } } list_view.set(config); }, "↑" }
                                    button { class: "button button-quiet", disabled: index+1 >= field_count, onclick: move |_| { let mut config=list_view(); if let Some(items)=config.get_mut("fields").and_then(Value::as_array_mut) { if index+1<items.len() { items.swap(index,index+1); } } list_view.set(config); }, "↓" }
                                    button { class: "button button-quiet", onclick: move |_| { let mut config=list_view(); if let Some(items)=config.get_mut("fields").and_then(Value::as_array_mut) { if index<items.len() { items.remove(index); } } list_view.set(config); }, "Remove" }
                                }
                            } }
                        }
                        button { class: "button button-quiet", onclick: move |_| { let mut config=list_view(); if let Some(fields)=config.get_mut("fields").and_then(Value::as_array_mut) { if !fields.iter().any(|field|field.get("name").and_then(Value::as_str)==Some("active")){fields.push(serde_json::json!({"name":"active","label":"Status","sortable":true}));} } list_view.set(config); }, "+ Add field" }
                    }
                    div { class: "content-type-dialog__actions",
                        button { class: "button button-quiet", onclick: move |_| settings_open.set(false), "Cancel" }
                        button { class: "button button-primary", disabled: saving(), onclick: move |_| {
                            saving.set(true); let uid=plugin_id.clone(); let id=view_id.clone(); let config=serde_json::json!({"listView":list_view()});
                            spawn(async move {
                                let result=async { let response=reqwest::Client::new().put(api_url(&format!("/api/plugins/{uid}/views/{id}/config"))).json(&config).send().await.map_err(|e|e.to_string())?; let status=response.status(); if status.is_success(){Ok(())}else{Err(format!("Save failed ({status}): {}",response.text().await.unwrap_or_default()))} }.await;
                                match result { Ok(())=>{settings_open.set(false);error.set(None)},Err(message)=>error.set(Some(message)) } saving.set(false);
                            });
                        }, if saving() { "Saving…" } else { "Save" } }
                    }
                }
            }
        }
        if let Some(id) = run_id() {
            WorkflowRunDialog { workflows: workflows.clone(), selected_workflow: Some(id), on_close: move |_| run_id.set(None), on_complete: move |_| {} }
        }
    }
}

fn workflow_field(workflow: &Workflow, name: &str, label: &str) -> String {
    match name {
        "name" => workflow.name.clone(),
        "description" => workflow.description.clone().unwrap_or_else(|| "—".to_owned()),
        "active" => if workflow.active { "Active" } else { "Inactive" }.to_owned(),
        _ => label.to_owned(),
    }
}

fn default_list_view() -> Value {
    serde_json::json!({
        "settings":{"enableSearch":true,"enableFilters":true,"enableBulkActions":false,"entriesPerPage":20,"defaultSortAttribute":"name","defaultSortOrder":"asc"},
        "fields":[{"name":"name","label":"Workflow","sortable":true},{"name":"description","label":"Description","sortable":false}]
    })
}

#[component]
pub(crate) fn PluginPagePage(plugin_id: String, page_id: String) -> Element {
    let plugin_id_for_load = plugin_id.clone();
    let page_id_for_load = page_id.clone();
    let page = use_server_future(move || {
        load_plugin_page(plugin_id_for_load.clone(), page_id_for_load.clone())
    })?;
    match page() {
        Some(Ok((_plugin_name, view_id))) => rsx! { PluginViewPage { plugin_id, view_id } },
        Some(Err(message)) => {
            rsx! { Shell { active: "plugins", header: "Plugin page", intro: "Could not load the plugin page.", actions: rsx! {}, ErrorPanel { message } } }
        }
        None => {
            rsx! { Shell { active: "plugins", header: "Plugin page", intro: "Loading plugin page…", actions: rsx! {}, LoadingPanel {} } }
        }
    }
}

async fn load_datatable_rows(path: String) -> Result<Vec<Value>, String> {
    let response = reqwest::get(api_url(&path)).await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Could not load data ({status}): {}",
            response.text().await.unwrap_or_default()
        ));
    }
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    value
        .get("output")
        .or_else(|| value.get("data"))
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .cloned()
        .ok_or_else(|| {
            "Datatable source must return an array or an object containing output/data array"
                .to_owned()
        })
}

#[component]
fn PluginDataTableView(
    plugin_name: String, definition: Value, workflows: Vec<Workflow>, embedded: bool,
) -> Element {
    let source_path = definition
        .get("dataSource")
        .and_then(|source| source.get("api"))
        .and_then(|api| api.get("path"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let source_path_for_load = source_path.clone();
    let mut rows = use_server_future(move || load_datatable_rows(source_path_for_load.clone()))?;
    let columns = definition
        .get("config")
        .and_then(|config| config.get("columns"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let create_action =
        definition.get("config").and_then(|config| config.get("createAction")).cloned();
    let row_action = definition.get("config").and_then(|config| config.get("rowAction")).cloned();
    let mut selected = use_signal(|| None::<(i32, String)>);
    let mut initial_input = use_signal(|| "{}".to_owned());
    let view_name = definition.get("name").and_then(Value::as_str).unwrap_or("Data table");
    let content = rsx! {
            section { class: "panel plugin-view-workflows",
                div { class: "plugins-heading",
                    div { span { class: "data-eyebrow", "{plugin_name} / DATA TABLE" } h2 { "{view_name}" } }
                    if let Some(action) = create_action.as_ref() {
                        if let Some(workflow) = action.get("workflow").and_then(Value::as_str).and_then(|name| workflow_named(&workflows, name)) {
                            button { class: "button button-primary", onclick: { let input = action.get("input").cloned().unwrap_or_else(|| serde_json::json!({})); move |_| { initial_input.set(input.to_string()); selected.set(Some((workflow.id, "Create".to_owned()))); } }, "+ Create" }
                        }
                    }
                }
                match rows() {
                    Some(Ok(rows)) => rsx! {
                        DataTable {
                            toolbar: rsx! {},
                            columns: { let mut labels=columns.iter().map(|column|column["label"].as_str().unwrap_or_default().to_owned()).collect::<Vec<_>>(); if row_action.is_some(){labels.push("Actions".into());} labels },
                            row_ids: rows.iter().enumerate().map(|(index,row)|row.get("id").or_else(||row.get("uid")).map(ToString::to_string).unwrap_or(index.to_string())).collect::<Vec<_>>(),
                            empty_message: "No entries yet.".to_owned(),
                            for (index,row) in rows.iter().enumerate() {
                                DataTableRow { id: row.get("id").or_else(||row.get("uid")).map(ToString::to_string).unwrap_or(index.to_string()), index,
                                    for (column_index,column) in columns.iter().enumerate() {
                                        DataTableCell { index: column_index, {mapped_value(row, column.get("mapping").and_then(Value::as_str).unwrap_or_default())} }
                                    }
                                    if let Some(action) = row_action.as_ref() {
                                        DataTableCell { index: columns.len(), if let Some(workflow) = action.get("workflow").and_then(Value::as_str).and_then(|name| workflow_named(&workflows, name)) {
                                            button { class: "button button-quiet", onclick: { let row = row.clone(); move |_| { initial_input.set(row.to_string()); selected.set(Some((workflow.id, "Edit".to_owned()))); } }, "Edit" }
                                        } }
                                    }
                                }
                            }
                        }
                    },
                    Some(Err(message)) => rsx! { ErrorPanel { message } },
                    None => rsx! { LoadingPanel {} },
                }
            }
    };
    rsx! {
        if embedded { {content} } else {
            Shell { active: "plugins", header: "Plugin data table", intro: "Plugin-provided data table", actions: rsx! {}, {content} }
        }
        if let Some((workflow_id, _label)) = selected() {
            WorkflowRunDialog { workflows: workflows.clone(), selected_workflow: Some(workflow_id), initial_input: initial_input(), on_close: move |_| selected.set(None), on_complete: move |_| { rows.restart(); selected.set(None); } }
        }
    }
}

fn workflow_named(workflows: &[Workflow], name: &str) -> Option<Workflow> {
    workflows
        .iter()
        .find(|workflow| {
            serde_json::from_str::<Value>(&workflow.definition_json)
                .ok()
                .and_then(|definition| {
                    definition.get("document")?.get("name")?.as_str().map(str::to_owned)
                })
                .as_deref()
                == Some(name)
        })
        .cloned()
}

fn mapped_value(row: &Value, mapping: &str) -> String {
    let mut value = row;
    for part in mapping
        .trim_start_matches('$')
        .trim_start_matches('.')
        .split('.')
        .filter(|part| !part.is_empty())
    {
        let Some(next) = value.get(part) else { return String::new() };
        value = next;
    }
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}
