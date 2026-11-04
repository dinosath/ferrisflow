use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::Value;

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
) -> Result<(String, Value, Vec<Workflow>, Vec<Value>), String> {
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
    let content_types =
        if view.get("dataSource").and_then(|source| source.get("type")).and_then(Value::as_str)
            == Some("database.contentTypes")
        {
            list_content_types().await?
        } else {
            Vec::new()
        };
    Ok((plugin.name, view, workflows, content_types))
}

#[derive(Deserialize)]
struct WorkflowApiResult<T> {
    output: T,
}

async fn list_content_types() -> Result<Vec<Value>, String> {
    let response = reqwest::get(api_url("/api/content-type-registry"))
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Could not load content types ({status}): {}",
            response.text().await.unwrap_or_default()
        ));
    }
    let result: WorkflowApiResult<Vec<Value>> =
        response.json().await.map_err(|error| error.to_string())?;
    Ok(result.output)
}

async fn list_content_records(uid: String, path: String) -> Result<Vec<Value>, String> {
    let path = path.replace("{uid}", &uid);
    let response = reqwest::get(api_url(&path)).await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Could not load records ({status}): {}",
            response.text().await.unwrap_or_default()
        ));
    }
    let result: Value = response.json().await.map_err(|error| error.to_string())?;
    result
        .get("output")
        .or_else(|| result.get("data"))
        .and_then(Value::as_array)
        .or_else(|| result.as_array())
        .cloned()
        .ok_or_else(|| {
            "Configured list API must return an array, { output: [...] }, or { data: [...] }"
                .to_owned()
        })
}

#[component]
pub(crate) fn PluginViewPage(plugin_id: String, view_id: String) -> Element {
    let plugin_id_for_load = plugin_id.clone();
    let view_id_for_load = view_id.clone();
    let view = use_server_future(move || {
        load_plugin_view(plugin_id_for_load.clone(), view_id_for_load.clone())
    })?;
    match view() {
        Some(Ok((plugin_name, definition, workflows, content_types))) => {
            if definition
                .get("dataSource")
                .and_then(|source| source.get("type"))
                .and_then(Value::as_str)
                == Some("database.contentTypes")
            {
                rsx! { PluginCollectionView { plugin_id, view_id, plugin_name, definition, workflows, content_types } }
            } else {
                rsx! { PluginWorkflowView { plugin_id, view_id, plugin_name, definition, workflows } }
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

fn default_form_view() -> Value {
    serde_json::json!({
        "api": {
            "create": {"method":"POST","path":"/api/content/{uid}"},
            "update": {"method":"PUT","path":"/api/content/{uid}/{id}"}
        },
        "create": {"title":"Create entry","submitLabel":"Create","fields":[]},
        "update": {"title":"Edit entry","submitLabel":"Save changes","fields":[]},
        "byType": {}
    })
}

fn normalize_list_view(mut config: Value, schema_fields: &[Value]) -> Value {
    if !config.is_object() {
        config = serde_json::json!({});
    }
    let old_fields = config.get("fields").and_then(Value::as_array).cloned();
    if !config.get("columns").is_some_and(Value::is_array) {
        config["columns"] = old_fields
            .map(Value::Array)
            .unwrap_or_else(|| Value::Array(default_collection_columns(schema_fields)));
    }
    if config["columns"].as_array().is_some_and(Vec::is_empty) {
        config["columns"] = Value::Array(default_collection_columns(schema_fields));
    }
    if let Some(columns) = config["columns"].as_array_mut() {
        for column in columns {
            let name = column.get("name").and_then(Value::as_str).unwrap_or_default();
            if column.get("mapping").is_none() {
                let mapping = match name {
                    "id" | "published" => name.to_owned(),
                    "*" => "data.{name}".to_owned(),
                    _ => format!("data.{name}"),
                };
                column["mapping"] = Value::String(mapping);
            }
        }
    }
    if let Some(columns) = config["columns"].as_array().cloned() {
        config["columns"] = Value::Array(expand_collection_columns(columns, schema_fields));
    }
    if !config.get("settings").is_some_and(Value::is_object) {
        config["settings"] = serde_json::json!({
            "enableSearch":true,"enableFilters":true,"enableBulkActions":false,
            "entriesPerPage":20,"defaultSortAttribute":"id","defaultSortOrder":"desc"
        });
    }
    if !config.get("api").is_some_and(Value::is_object) {
        config["api"] = serde_json::json!({"method":"GET","path":"/api/content/{uid}"});
    }
    config
}

fn normalize_form_view(mut config: Value, schema_fields: &[Value]) -> Value {
    if !config.is_object() {
        config = default_form_view();
    }
    let defaults = default_form_view();
    for key in ["api", "create", "update"] {
        if !config.get(key).is_some_and(Value::is_object) {
            config[key] = defaults[key].clone();
        }
    }
    for action in ["create", "update"] {
        if !config[action].get("fields").is_some_and(Value::is_array)
            || config[action]["fields"].as_array().is_some_and(Vec::is_empty)
        {
            config[action]["fields"] = Value::Array(
                schema_fields
                    .iter()
                    .filter_map(|field| {
                        Some(serde_json::json!({
                            "name": field.get("name")?.as_str()?,
                            "label": field.get("label")?.as_str()?,
                            "type": field.get("type").and_then(Value::as_str).unwrap_or("text"),
                            "required": field.get("required").and_then(Value::as_bool).unwrap_or(false)
                        }))
                    })
                    .collect(),
            );
        }
    }
    config
}

fn default_collection_columns(schema_fields: &[Value]) -> Vec<Value> {
    let mut columns = schema_fields
        .iter()
        .filter_map(|field| {
            let name = field.get("name")?.as_str()?;
            Some(serde_json::json!({
                "name":name,
                "label":field.get("label").and_then(Value::as_str).unwrap_or(name),
                "mapping":format!("data.{name}"),
                "sortable":true
            }))
        })
        .collect::<Vec<_>>();
    columns.push(serde_json::json!({"name":"id","label":"ID","mapping":"id","sortable":true}));
    columns.push(serde_json::json!({"name":"published","label":"Status","mapping":"published","sortable":true}));
    columns
}

fn expand_collection_columns(columns: Vec<Value>, schema_fields: &[Value]) -> Vec<Value> {
    let mut expanded = Vec::new();
    for column in columns {
        if column.get("name").and_then(Value::as_str) == Some("*") {
            for field in schema_fields {
                let Some(name) = field.get("name").and_then(Value::as_str) else { continue };
                expanded.push(serde_json::json!({
                    "name":name,
                    "label":column.get("label").and_then(Value::as_str).unwrap_or("{label}").replace("{label}", field.get("label").and_then(Value::as_str).unwrap_or(name)),
                    "mapping":column.get("mapping").and_then(Value::as_str).unwrap_or("data.{name}").replace("{name}", name),
                    "sortable":column.get("sortable").and_then(Value::as_bool).unwrap_or(false)
                }));
            }
        } else {
            expanded.push(column);
        }
    }
    expanded
}

#[component]
fn PluginCollectionView(
    plugin_id: String, view_id: String, plugin_name: String, definition: Value,
    workflows: Vec<Workflow>, content_types: Vec<Value>,
) -> Element {
    let initial_uid = content_types
        .first()
        .and_then(|item| item.get("uid"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut types = use_signal(|| content_types.clone());
    let mut selected_uid = use_signal(|| initial_uid.clone());
    let mut refresh = use_signal(|| 0_u64);
    let mut query = use_signal(String::new);
    let mut status_filter = use_signal(|| "all".to_owned());
    let mut page = use_signal(|| 1_usize);
    let mut run_id = use_signal(|| None::<i32>);
    let mut run_input = use_signal(|| "{}".to_owned());
    let mut form_mode = use_signal(|| None::<String>);
    let mut form_record_id = use_signal(|| None::<Value>);
    let mut form_data = use_signal(|| Value::Object(serde_json::Map::new()));
    let mut form_published = use_signal(|| false);
    let mut settings_open = use_signal(|| false);
    let mut list_view = use_signal(|| {
        definition
            .get("config")
            .and_then(|config| config.get("listView"))
            .cloned()
            .unwrap_or_else(default_list_view)
    });
    let mut form_view = use_signal(|| {
        definition
            .get("config")
            .and_then(|config| config.get("formView"))
            .cloned()
            .unwrap_or_else(default_form_view)
    });

    let uid_for_records = selected_uid();
    let root_list_view = list_view();
    let api_path = root_list_view
        .get("byType")
        .and_then(Value::as_object)
        .and_then(|views| views.get(uid_for_records.as_str()))
        .and_then(|view| view.get("api"))
        .or_else(|| root_list_view.get("api"))
        .and_then(|api| api.get("path"))
        .and_then(Value::as_str)
        .unwrap_or("/api/content/{uid}")
        .to_owned();
    let refresh_version = refresh();
    let records = use_server_future(move || {
        let uid = uid_for_records.clone();
        let path = api_path.clone();
        let _refresh_version = refresh_version;
        async move {
            if uid.is_empty() { Ok(Vec::new()) } else { list_content_records(uid, path).await }
        }
    })?;

    let current_type = types()
        .iter()
        .find(|item| item.get("uid").and_then(Value::as_str) == Some(selected_uid().as_str()))
        .cloned();
    let collection_title = current_type
        .as_ref()
        .and_then(|item| item.get("display_name"))
        .and_then(Value::as_str)
        .unwrap_or("Content types")
        .to_owned();
    let type_fields = current_type
        .as_ref()
        .and_then(|item| item.get("schema"))
        .and_then(|schema| schema.get("fields"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let per_type_view = list_view()
        .get("byType")
        .and_then(Value::as_object)
        .and_then(|views| views.get(selected_uid().as_str()))
        .cloned()
        .unwrap_or_else(|| {
            let mut base = list_view();
            base.as_object_mut()
                .map(|object| {
                    object.remove("byType");
                })
                .unwrap_or(());
            base
        });
    let per_type_view = normalize_list_view(per_type_view, &type_fields);
    let per_type_form_view = form_view()
        .get("byType")
        .and_then(Value::as_object)
        .and_then(|views| views.get(selected_uid().as_str()))
        .cloned()
        .unwrap_or_else(|| {
            let mut base = form_view();
            if let Some(object) = base.as_object_mut() {
                object.remove("byType");
            }
            base
        });
    let settings = per_type_view.get("settings").cloned().unwrap_or_else(|| serde_json::json!({}));
    let mut sort_key = use_signal(|| {
        settings.get("defaultSortAttribute").and_then(Value::as_str).unwrap_or("id").to_owned()
    });
    let mut sort_direction = use_signal(|| {
        settings.get("defaultSortOrder").and_then(Value::as_str).unwrap_or("desc").to_owned()
    });
    let enable_search = settings.get("enableSearch").and_then(Value::as_bool).unwrap_or(true);
    let enable_filters = settings.get("enableFilters").and_then(Value::as_bool).unwrap_or(true);
    let enable_bulk = settings.get("enableBulkActions").and_then(Value::as_bool).unwrap_or(false);
    let page_size = settings.get("entriesPerPage").and_then(Value::as_u64).unwrap_or(20) as usize;
    let mut visible_fields = per_type_view
        .get("columns")
        .or_else(|| per_type_view.get("fields"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if visible_fields.is_empty() {
        visible_fields = default_collection_columns(&type_fields);
    } else {
        visible_fields = expand_collection_columns(visible_fields, &type_fields);
    }
    let mut rows = records().and_then(Result::ok).unwrap_or_default();
    if enable_search && !query().trim().is_empty() {
        let needle = query().trim().to_lowercase();
        rows.retain(|record| {
            record.get("data").is_some_and(|data| data.to_string().to_lowercase().contains(&needle))
                || record.get("id").is_some_and(|id| id.to_string().contains(&needle))
        });
    }
    if enable_filters {
        match status_filter().as_str() {
            "published" => {
                rows.retain(|record| record.get("published").and_then(Value::as_bool) == Some(true))
            }
            "draft" => {
                rows.retain(|record| record.get("published").and_then(Value::as_bool) != Some(true))
            }
            _ => {}
        }
    }
    if !sort_key().is_empty() {
        let current_sort = sort_key();
        let current_direction = sort_direction();
        rows.sort_by(|a, b| {
            let column = visible_fields.iter().find(|column| {
                column.get("name").and_then(Value::as_str) == Some(current_sort.as_str())
            });
            let av = column
                .map(|column| collection_field_mapping(a, column))
                .unwrap_or_else(|| collection_field(a, &current_sort).to_string());
            let bv = column
                .map(|column| collection_field_mapping(b, column))
                .unwrap_or_else(|| collection_field(b, &current_sort).to_string());
            let ordering = av.cmp(&bv);
            if current_direction == "desc" { ordering.reverse() } else { ordering }
        });
    }
    let page_count = rows.len().div_ceil(page_size.max(1)).max(1);
    let current_page = page().clamp(1, page_count);
    let visible_rows = rows
        .iter()
        .skip((current_page - 1) * page_size)
        .take(page_size)
        .cloned()
        .collect::<Vec<_>>();
    let workflows_for_dialog = workflows.clone();
    let per_type_view_for_update = per_type_view.clone();

    rsx! {
        Shell { active: "plugins", header: "Plugin view", intro: "Manage plugin-defined collections and configure their list views.", actions: rsx! {},
            div { class: "plugins-layout plugin-collection-layout",
                aside { class: "plugins-sidebar panel",
                    span { class: "data-eyebrow", "{plugin_name} / COLLECTIONS" }
                    for item in types().iter() {
                        { let type_uid=item.get("uid").and_then(Value::as_str).unwrap_or("").to_owned(); let display=item.get("display_name").and_then(Value::as_str).unwrap_or(&type_uid).to_owned(); rsx! {
                                    button { class: if selected_uid()==type_uid { "plugin-option selected" } else { "plugin-option" }, onclick: move |_| { selected_uid.set(type_uid.clone()); query.set(String::new()); status_filter.set("all".into()); page.set(1); let all=list_view();let settings=all.get("byType").and_then(Value::as_object).and_then(|views|views.get(type_uid.as_str())).and_then(|view|view.get("settings")).or_else(||all.get("settings"));sort_key.set(settings.and_then(|s|s.get("defaultSortAttribute")).and_then(Value::as_str).unwrap_or("id").to_owned());sort_direction.set(settings.and_then(|s|s.get("defaultSortOrder")).and_then(Value::as_str).unwrap_or("desc").to_owned()); }, strong { "{display}" } small { "{type_uid}" } }
                        } }
                    }
                    if types().is_empty() { p { class: "plugin-empty-note", "No content types yet. Create one using the plugin workflow." } }
                }
                section { class: "plugins-main panel",
                    div { class: "plugins-heading",
                        div { span { class: "data-eyebrow", "COLLECTION / {selected_uid()}" }
                            h2 { "{collection_title}" }
                        }
                        div { class: "plugins-actions",
                            if !selected_uid().is_empty() {
                                button { class: "button button-ghost", onclick: move |_| settings_open.set(true), "Configure view" }
                                button { class: "button button-primary", onclick: move |_| { form_data.set(Value::Object(serde_json::Map::new())); form_record_id.set(None); form_published.set(false); form_mode.set(Some("create".to_owned())); }, "+ Create entry" }
                            }
                            if let Some(workflow)=workflow_named(&workflows,"content-types-create") {
                                button { class: "button button-quiet", onclick: { let id=workflow.id; move |_| { run_input.set("{\"uid\":\"\",\"display_name\":\"\",\"schema\":{\"fields\":[]}}".into()); run_id.set(Some(id)); } }, "+ Content type" }
                            }
                        }
                    }
                    if !selected_uid().is_empty() {
                        div { class: "shared-table-toolbar",
                            div { class: "shared-table-toolbar__filters",
                                if enable_search { input { r#type:"search", class:"workflow-list-search", placeholder:"Search entries…", value:"{query}", oninput:move |event|{query.set(event.value());page.set(1);} } }
                                if enable_filters { select { class:"execution-filter", value:"{status_filter}", onchange:move |event|{status_filter.set(event.value());page.set(1);}, option {value:"all","All statuses"} option {value:"draft","Draft"} option {value:"published","Published"} } }
                            }
                            button { class:"button button-quiet", onclick:move |_| settings_open.set(true), "View settings" }
                        }
                        div { class:"shared-table-shell shared-table-scroll", table { class:"shared-data-table",
                            thead { tr { if enable_bulk { th { "Select" } } for field in visible_fields.iter() { { let name=field.get("name").and_then(Value::as_str).unwrap_or("").to_owned(); let label=field.get("label").and_then(Value::as_str).unwrap_or("").to_owned(); let sortable=field.get("sortable").and_then(Value::as_bool).unwrap_or(false); let active=sort_key()==name; let arrow=if active { if sort_direction()=="asc" {" ↑"}else{" ↓"} }else{""}; rsx!{th { class: if sortable {"shared-table-heading plugin-sortable-heading"}else{"shared-table-heading"}, onclick: move |_| { if sortable { if sort_key()==name { sort_direction.set(if sort_direction()=="asc" {"desc".to_owned()}else{"asc".to_owned()}); } else { sort_key.set(name.clone()); sort_direction.set("asc".to_owned()); } } }, "{label}{arrow}" }} } } th {"Actions"} } }
                            tbody { for record in visible_rows.iter() { tr { class:"shared-table-row",
                                if enable_bulk { td { input {r#type:"checkbox"} } }
                                for field in visible_fields.iter() { { let value=collection_field_mapping(record, field); rsx!{td{"{value}"}} } }
                                td { class:"plugins-actions",
                                    button { class:"button button-quiet", onclick: { let id=record.get("id").cloned(); let data=record.get("data").cloned().unwrap_or_else(||Value::Object(serde_json::Map::new())); let published=record.get("published").and_then(Value::as_bool).unwrap_or(false); move |_| { form_record_id.set(id.clone()); form_data.set(data.clone()); form_published.set(published); form_mode.set(Some("update".to_owned())); } }, "Edit" }
                                    if let Some(workflow)=workflow_named(&workflows,"content-records-delete") {
                                        button { class:"button button-quiet plugin-delete", onclick: { let id=workflow.id; let uid=selected_uid(); let record_id=record.get("id").cloned().unwrap_or(Value::Null); move |_| { run_input.set(serde_json::json!({"uid":uid,"id":record_id}).to_string()); run_id.set(Some(id)); } }, "Delete" }
                                    }
                                }
                            } } }
                        } }
                        div { class:"shared-table-pagination", label { "Entries per page " select {value:"{page_size}",onchange:move|event|{let uid=selected_uid();let mut all=list_view();if !all.get("byType").is_some_and(Value::is_object){all["byType"]=serde_json::json!({});}if !all["byType"].get(uid.as_str()).is_some_and(Value::is_object){all["byType"][uid.as_str()]=per_type_view_for_update.clone();}all["byType"][uid.as_str()]["settings"]["entriesPerPage"]=Value::from(event.value().parse::<u64>().unwrap_or(20));list_view.set(all);page.set(1);},option{value:"10","10"}option{value:"20","20"}option{value:"50","50"}option{value:"100","100"}}} span{"Page {current_page} of {page_count}"} button{class:"button button-quiet",disabled:current_page<=1,onclick:move|_|page.set(current_page.saturating_sub(1)),"Previous"} button{class:"button button-quiet",disabled:current_page>=page_count,onclick:move|_|page.set((current_page+1).min(page_count)),"Next"} }
                    } else {
                        div { class:"plugins-empty-state",span{class:"data-eyebrow","CONTENT TYPES"}h2{"Create a content type"}p{"Use the plugin's schema workflow to add the first collection."} }
                        if let Some(workflow)=workflow_named(&workflows,"content-types-create") { button { class:"button button-primary", onclick:{let id=workflow.id;move |_|{run_input.set("{\"uid\":\"\",\"display_name\":\"\",\"schema\":{\"fields\":[]}}".into());run_id.set(Some(id));}}, "Create content type" } }
                    }
                }
            }
        }
        if settings_open() && !selected_uid().is_empty() {
            PluginListViewSettings {
                plugin_id: plugin_id.clone(), view_id: view_id.clone(), content_type_uid: selected_uid(),
                fields: type_fields.clone(), current: per_type_view.clone(), current_form: per_type_form_view.clone(),
                on_close: move |_| settings_open.set(false),
                on_saved: move |(list_config, form_config): (Value, Value)| {
                    let uid = selected_uid();
                    sort_key.set(list_config.get("settings").and_then(|settings|settings.get("defaultSortAttribute")).and_then(Value::as_str).unwrap_or("id").to_owned());
                    sort_direction.set(list_config.get("settings").and_then(|settings|settings.get("defaultSortOrder")).and_then(Value::as_str).unwrap_or("desc").to_owned());
                    let mut all = list_view();
                    if !all.get("byType").is_some_and(Value::is_object) { all["byType"] = serde_json::json!({}); }
                    all["byType"][uid.as_str()] = list_config;
                    list_view.set(all);
                    let mut forms = form_view();
                    if !forms.get("byType").is_some_and(Value::is_object) { forms["byType"] = serde_json::json!({}); }
                    forms["byType"][uid.as_str()] = form_config;
                    form_view.set(forms);
                    settings_open.set(false);
                }
            }
        }
        if let Some(mode) = form_mode() {
            PluginRecordForm {
                uid: selected_uid(), title: collection_title.clone(),
                fields: type_fields.clone(), config: per_type_form_view.clone(), mode,
                record_id: form_record_id(), initial_data: form_data(), initial_published: form_published(),
                on_close: move |_| form_mode.set(None),
                on_complete: move |_| { form_mode.set(None); refresh += 1; }
            }
        }
        if let Some(id) = run_id() {
            WorkflowRunDialog {
                workflows: workflows_for_dialog,
                selected_workflow: Some(id),
                initial_input: run_input(),
                on_close: move |_| run_id.set(None),
                on_complete: move |_| {
                    run_id.set(None);
                    refresh += 1;
                    spawn(async move {
                        if let Ok(items) = list_content_types().await {
                            types.set(items);
                            if selected_uid().is_empty() {
                                if let Some(uid) = types().first().and_then(|item| item.get("uid")).and_then(Value::as_str) {
                                    selected_uid.set(uid.to_owned());
                                }
                            }
                        }
                    });
                },
            }
        }
    }
}

#[component]
fn PluginListViewSettings(
    plugin_id: String, view_id: String, content_type_uid: String, fields: Vec<Value>,
    current: Value, current_form: Value, on_close: EventHandler<()>,
    on_saved: EventHandler<(Value, Value)>,
) -> Element {
    let mut config = use_signal(|| normalize_list_view(current.clone(), &fields));
    let mut form_config = use_signal(|| normalize_form_view(current_form.clone(), &fields));
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let initial_fields =
        config().get("columns").and_then(Value::as_array).cloned().unwrap_or_default();
    let configured_fields = expand_collection_columns(initial_fields, &fields);
    let settings = config().get("settings").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "enableSearch": true, "enableFilters": true, "enableBulkActions": false,
            "entriesPerPage": 20, "defaultSortAttribute": "id", "defaultSortOrder": "desc"
        })
    });
    let size = settings.get("entriesPerPage").and_then(Value::as_u64).unwrap_or(20);
    let sort_attr =
        settings.get("defaultSortAttribute").and_then(Value::as_str).unwrap_or("id").to_owned();
    let sort_order =
        settings.get("defaultSortOrder").and_then(Value::as_str).unwrap_or("desc").to_owned();
    let api_path = config()
        .get("api")
        .and_then(|api| api.get("path"))
        .and_then(Value::as_str)
        .unwrap_or("/api/content/{uid}")
        .to_owned();
    let current_forms = form_config();
    let create_title = current_forms
        .get("create")
        .and_then(|v| v.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("Create entry")
        .to_owned();
    let create_submit = current_forms
        .get("create")
        .and_then(|v| v.get("submitLabel"))
        .and_then(Value::as_str)
        .unwrap_or("Create")
        .to_owned();
    let update_title = current_forms
        .get("update")
        .and_then(|v| v.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("Edit entry")
        .to_owned();
    let update_submit = current_forms
        .get("update")
        .and_then(|v| v.get("submitLabel"))
        .and_then(Value::as_str)
        .unwrap_or("Save changes")
        .to_owned();
    let create_api_path = current_forms
        .get("api")
        .and_then(|v| v.get("create"))
        .and_then(|v| v.get("path"))
        .and_then(Value::as_str)
        .unwrap_or("/api/content/{uid}")
        .to_owned();
    let update_api_path = current_forms
        .get("api")
        .and_then(|v| v.get("update"))
        .and_then(|v| v.get("path"))
        .and_then(Value::as_str)
        .unwrap_or("/api/content/{uid}/{id}")
        .to_owned();
    rsx! {
        div { class: "dialog-backdrop", onclick: move |_| on_close.call(()),
            section { class: "plugin-create-dialog panel", role: "dialog", aria_modal: "true", onclick: |event| event.stop_propagation(),
                div { class: "builder-modal__header",
                    div { span { class: "data-eyebrow", "PLUGIN UI SETTINGS" } h2 { "Configure view" } p { "Configure list data and columns, plus the create and update forms for this collection." } }
                    button { class: "icon-button", onclick: move |_| on_close.call(()), "×" }
                }
                div { class: "plugin-view-settings-section", h3 { "Settings" }
                    label { class: "field", span { "List data API path" }
                        input { value: "{api_path}", placeholder: "/api/content/:uid", oninput: move |event| { let mut next=config(); next["api"]["method"]=Value::String("GET".to_owned()); next["api"]["path"]=Value::String(event.value()); config.set(next); } }
                    }
                    for (key, label) in [("enableSearch", "Enable search"), ("enableFilters", "Enable filters"), ("enableBulkActions", "Enable bulk actions")] {
                        label { class: "plugin-view-setting-row", span { "{label}" }
                            input { r#type: "checkbox", checked: settings.get(key).and_then(Value::as_bool).unwrap_or(false), onchange: move |event| { let mut next=config(); next["settings"][key]=Value::Bool(event.checked()); config.set(next); } }
                        }
                    }
                    label { class: "field", span { "Entries per page" }
                        select { value: "{size}", onchange: move |event| { let mut next=config(); next["settings"]["entriesPerPage"]=Value::from(event.value().parse::<u64>().unwrap_or(20)); config.set(next); },
                            option { value: "10", "10" } option { value: "20", "20" } option { value: "50", "50" } option { value: "100", "100" }
                        }
                    }
                    label { class: "field", span { "Default sort attribute" }
                        select { value: "{sort_attr}", onchange: move |event| { let mut next=config(); next["settings"]["defaultSortAttribute"]=Value::String(event.value()); config.set(next); },
                            for field in fields.iter() { if let (Some(name), Some(label)) = (field.get("name").and_then(Value::as_str), field.get("label").and_then(Value::as_str)) { option { value: "{name}", "{label}" } } }
                            option { value: "id", "ID" } option { value: "published", "Status" }
                        }
                    }
                    label { class: "field", span { "Default sort order" }
                        select { value: "{sort_order}", onchange: move |event| { let mut next=config(); next["settings"]["defaultSortOrder"]=Value::String(event.value()); config.set(next); }, option { value: "asc", "Ascending" } option { value: "desc", "Descending" } }
                    }
                }
                    div { class: "plugin-view-settings-section", h3 { "View columns and data mapping" }
                    for (index, field) in configured_fields.iter().enumerate() {
                        { let name=field.get("name").and_then(Value::as_str).unwrap_or("").to_owned(); let label=field.get("label").and_then(Value::as_str).unwrap_or("").to_owned(); let sortable=field.get("sortable").and_then(Value::as_bool).unwrap_or(false); let count=configured_fields.len(); let mapping=field.get("mapping").and_then(Value::as_str).unwrap_or("").to_owned(); rsx! {
                            div { class: "plugin-view-setting-row", span { "{name}" }
                                input { value: "{label}", aria_label: "Column label", oninput: move |event| { let mut next=config(); if let Some(item)=next.get_mut("columns").and_then(Value::as_array_mut).and_then(|items|items.get_mut(index)) { item["label"]=Value::String(event.value()); } config.set(next); } }
                                input { value: "{mapping}", aria_label: "Data mapping", placeholder: "data.title", oninput: move |event| { let mut next=config(); if let Some(item)=next.get_mut("columns").and_then(Value::as_array_mut).and_then(|items|items.get_mut(index)) { item["mapping"]=Value::String(event.value()); } config.set(next); } }
                                label { class: "plugin-view-setting-row__sort", span { "Enable sort" } input { r#type: "checkbox", checked: sortable, onchange: move |event| { let mut next=config(); if let Some(item)=next.get_mut("columns").and_then(Value::as_array_mut).and_then(|items|items.get_mut(index)) { item["sortable"]=Value::Bool(event.checked()); } config.set(next); } } }
                                button { class: "button button-quiet", disabled: index == 0, onclick: move |_| { let mut next=config(); if let Some(items)=next.get_mut("columns").and_then(Value::as_array_mut) { if index>0 { items.swap(index,index-1); } } config.set(next); }, "↑" }
                                button { class: "button button-quiet", disabled: index + 1 >= count, onclick: move |_| { let mut next=config(); if let Some(items)=next.get_mut("columns").and_then(Value::as_array_mut) { if index+1<items.len() { items.swap(index,index+1); } } config.set(next); }, "↓" }
                                button { class: "button button-quiet", onclick: move |_| { let mut next=config(); if let Some(items)=next.get_mut("columns").and_then(Value::as_array_mut) { if index<items.len() { items.remove(index); } } config.set(next); }, "Remove" }
                            }
                        } }
                    }
                    button { class: "button button-quiet", onclick: move |_| { let mut next=config(); if !next.get("columns").is_some_and(Value::is_array) { next["columns"]=Value::Array(Vec::new()); } if let Some(items)=next.get_mut("columns").and_then(Value::as_array_mut) { for field in &fields { let name=field.get("name").and_then(Value::as_str).unwrap_or(""); if !items.iter().any(|item|item.get("name").and_then(Value::as_str)==Some(name)) { items.push(serde_json::json!({"name":name,"label":field.get("label").and_then(Value::as_str).unwrap_or(name),"mapping":format!("data.{name}"),"sortable":true})); break; } } } config.set(next); }, "+ Add column" }
                }
                div { class: "plugin-view-settings-section", h3 { "Create form" }
                    label { class: "field", span { "Form title" } input { value: "{create_title}", oninput: move |event| { let mut next=form_config(); next["create"]["title"]=Value::String(event.value()); form_config.set(next); } } }
                    label { class: "field", span { "Submit button" } input { value: "{create_submit}", oninput: move |event| { let mut next=form_config(); next["create"]["submitLabel"]=Value::String(event.value()); form_config.set(next); } } }
                    FormFieldsEditor { fields: form_config().get("create").and_then(|v|v.get("fields")).and_then(Value::as_array).cloned().unwrap_or_default(), available_fields: fields.clone(), on_change: move |items| { let mut next=form_config(); next["create"]["fields"]=items; form_config.set(next); } }
                }
                div { class: "plugin-view-settings-section", h3 { "Update form" }
                    label { class: "field", span { "Form title" } input { value: "{update_title}", oninput: move |event| { let mut next=form_config(); next["update"]["title"]=Value::String(event.value()); form_config.set(next); } } }
                    label { class: "field", span { "Submit button" } input { value: "{update_submit}", oninput: move |event| { let mut next=form_config(); next["update"]["submitLabel"]=Value::String(event.value()); form_config.set(next); } } }
                    FormFieldsEditor { fields: form_config().get("update").and_then(|v|v.get("fields")).and_then(Value::as_array).cloned().unwrap_or_default(), available_fields: fields.clone(), on_change: move |items| { let mut next=form_config(); next["update"]["fields"]=items; form_config.set(next); } }
                }
                div { class: "plugin-view-settings-section", h3 { "Form APIs" }
                    label { class: "field", span { "Create API path" } input { value: "{create_api_path}", oninput: move |event| { let mut next=form_config(); next["api"]["create"]["method"]=Value::String("POST".into()); next["api"]["create"]["path"]=Value::String(event.value()); form_config.set(next); } } }
                    label { class: "field", span { "Update API path" } input { value: "{update_api_path}", oninput: move |event| { let mut next=form_config(); next["api"]["update"]["method"]=Value::String("PUT".into()); next["api"]["update"]["path"]=Value::String(event.value()); form_config.set(next); } } }
                }
                if let Some(message) = error() { div { class: "error-panel", "{message}" } }
                div { class: "content-type-dialog__actions",
                    button { class: "button button-quiet", onclick: move |_| on_close.call(()), "Cancel" }
                    button { class: "button button-primary", disabled: saving(), onclick: move |_| {
                        saving.set(true); let uid=plugin_id.clone(); let view=view_id.clone(); let type_uid=content_type_uid.clone(); let saved=config(); let saved_form=form_config();
                        spawn(async move {
                            let result=async { let response=reqwest::Client::new().put(api_url(&format!("/api/plugins/{uid}/views/{view}/config"))).json(&serde_json::json!({"contentTypeUid":type_uid,"listView":saved,"formView":saved_form})).send().await.map_err(|e|e.to_string())?; let status=response.status(); if status.is_success() { Ok(()) } else { Err(format!("Save failed ({status}): {}",response.text().await.unwrap_or_default())) } }.await;
                            match result { Ok(())=>{on_saved.call((saved,saved_form));error.set(None)},Err(message)=>error.set(Some(message)) } saving.set(false);
                        });
                    }, if saving() { "Saving…" } else { "Save" } }
                }
            }
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
fn collection_field(record: &Value, name: &str) -> Value {
    if name == "published" {
        return Value::String(
            if record.get("published").and_then(Value::as_bool) == Some(true) {
                "Published"
            } else {
                "Draft"
            }
            .to_owned(),
        );
    }
    if name == "id" {
        return record.get("id").cloned().unwrap_or(Value::Null);
    }
    record.get("data").and_then(|data| data.get(name)).cloned().unwrap_or(Value::Null)
}

fn collection_field_mapping(record: &Value, column: &Value) -> String {
    let name = column.get("name").and_then(Value::as_str).unwrap_or_default();
    let mapping = column
        .get("mapping")
        .and_then(Value::as_str)
        .filter(|mapping| !mapping.is_empty())
        .unwrap_or(name);
    let mapping = mapping.replace("{name}", name);
    if mapping == "published" {
        return collection_field(record, "published").as_str().unwrap_or("Draft").to_owned();
    }
    let path = mapping.trim_start_matches("$").trim_start_matches('.');
    let mut value = record;
    for part in path.split('.').filter(|part| !part.is_empty()) {
        let Some(next) = value.get(part) else { return String::new() };
        value = next;
    }
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

#[component]
fn FormFieldsEditor(
    fields: Vec<Value>, available_fields: Vec<Value>, on_change: EventHandler<Value>,
) -> Element {
    let add_fields = fields.clone();
    let add_available_fields = available_fields.clone();
    rsx! {
        div { class: "plugin-form-field-editor",
            for (index, field) in fields.iter().enumerate() {
                { let name=field.get("name").and_then(Value::as_str).unwrap_or("").to_owned(); let label=field.get("label").and_then(Value::as_str).unwrap_or(&name).to_owned(); let required=field.get("required").and_then(Value::as_bool).unwrap_or(false); let count=fields.len(); let f_label=fields.clone(); let f_required=fields.clone(); let f_up=fields.clone(); let f_down=fields.clone(); let f_remove=fields.clone(); rsx! {
                    div { class: "plugin-view-setting-row",
                        strong { "{name}" }
                        input { value: "{label}", aria_label: "Form field label", oninput: move |event| { let mut next=f_label.clone(); if let Some(item)=next.get_mut(index) { item["label"]=Value::String(event.value()); } on_change.call(Value::Array(next)); } }
                        label { class: "plugin-view-setting-row__sort", span { "Required" } input { r#type: "checkbox", checked: required, onchange: move |event| { let mut next=f_required.clone(); if let Some(item)=next.get_mut(index) { item["required"]=Value::Bool(event.checked()); } on_change.call(Value::Array(next)); } } }
                        button { class: "button button-quiet", disabled: index == 0, onclick: move |_| { let mut next=f_up.clone(); if index>0 { next.swap(index,index-1); } on_change.call(Value::Array(next)); }, "↑" }
                        button { class: "button button-quiet", disabled: index + 1 >= count, onclick: move |_| { let mut next=f_down.clone(); if index+1<next.len() { next.swap(index,index+1); } on_change.call(Value::Array(next)); }, "↓" }
                        button { class: "button button-quiet", onclick: move |_| { let mut next=f_remove.clone(); if index<next.len() { next.remove(index); } on_change.call(Value::Array(next)); }, "Hide" }
                    }
                } }
            }
            button { class: "button button-quiet", onclick: move |_| {
                let mut next=add_fields.clone();
                if let Some(field)=add_available_fields.iter().find(|field| !next.iter().any(|item|item.get("name")==field.get("name"))) {
                    let name=field.get("name").and_then(Value::as_str).unwrap_or("");
                    next.push(serde_json::json!({"name":name,"label":field.get("label").and_then(Value::as_str).unwrap_or(name),"type":field.get("type").and_then(Value::as_str).unwrap_or("text"),"required":field.get("required").and_then(Value::as_bool).unwrap_or(false)}));
                }
                on_change.call(Value::Array(next));
            }, "+ Add form field" }
        }
    }
}

#[component]
fn PluginRecordForm(
    uid: String, title: String, fields: Vec<Value>, config: Value, mode: String,
    record_id: Option<Value>, initial_data: Value, initial_published: bool,
    on_close: EventHandler<()>, on_complete: EventHandler<()>,
) -> Element {
    let mut data = use_signal(|| initial_data.clone());
    let mut published = use_signal(|| initial_published);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let is_update = mode == "update";
    let action = if is_update { "update" } else { "create" };
    let form = config.get(action).cloned().unwrap_or_default();
    let form_fields = form.get("fields").and_then(Value::as_array).cloned().unwrap_or_else(|| {
        normalize_form_view(default_form_view(), &fields)
            .get(action)
            .and_then(|value| value.get("fields"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    });
    let form_title = form
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(if is_update { "Edit entry" } else { "Create entry" })
        .to_owned();
    let submit_label = form
        .get("submitLabel")
        .and_then(Value::as_str)
        .unwrap_or(if is_update { "Save changes" } else { "Create" })
        .to_owned();
    let api = config.get("api").and_then(|api|api.get(action)).cloned().unwrap_or_else(|| {
        serde_json::json!({"method":if is_update{"PUT"}else{"POST"},"path":if is_update{"/api/content/{uid}/{id}"}else{"/api/content/{uid}"}})
    });
    let api_path = api.get("path").and_then(Value::as_str).unwrap_or_default().to_owned();
    let api_method = api
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or(if is_update { "PUT" } else { "POST" })
        .to_owned();
    rsx! {
        div { class: "dialog-backdrop", onclick: move |_| on_close.call(()),
            section { class: "plugin-create-dialog plugin-record-form panel", role: "dialog", aria_modal: "true", onclick: |event|event.stop_propagation(),
                div { class: "builder-modal__header",
                    div { span { class: "data-eyebrow", if is_update { "UPDATE / {uid}" } else { "CREATE / {uid}" } } h2 { "{form_title}" } p { "{title}" } }
                    button { class: "icon-button", onclick: move |_|on_close.call(()), "×" }
                }
                div { class: "plugin-record-form__fields",
                    for field in form_fields.iter() {
                        { let name=field.get("name").and_then(Value::as_str).unwrap_or("").to_owned(); let label=field.get("label").and_then(Value::as_str).unwrap_or(&name).to_owned(); let kind=field.get("type").and_then(Value::as_str).unwrap_or("text").to_owned(); let required=field.get("required").and_then(Value::as_bool).unwrap_or(false); let value=data().get(name.as_str()).cloned().unwrap_or(Value::Null); rsx! {
                            label { class: "plugin-record-form__field",
                                span { "{label}" if required { em { " *" } } }
                                if kind == "boolean" {
                                    input { r#type: "checkbox", checked: value.as_bool().unwrap_or(false), onchange: move |event| { let mut next=data(); next[name.as_str()]=Value::Bool(event.checked()); data.set(next); } }
                                } else if kind == "json" {
                                    textarea { rows: "5", value: if value.is_null() { String::new() } else { value.to_string() }, oninput: move |event| { let mut next=data(); next[name.as_str()]=serde_json::from_str(&event.value()).unwrap_or(Value::String(event.value())); data.set(next); } }
                                } else {
                                    input { r#type: if kind == "number" { "number" } else if kind == "date" { "date" } else { "text" }, required: required, value: if value.is_string() { value.as_str().unwrap_or_default().to_owned() } else if value.is_null() { String::new() } else { value.to_string() }, oninput: move |event| { let raw=event.value(); let parsed=if kind=="number" { raw.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null) } else { Value::String(raw) }; let mut next=data(); next[name.as_str()]=parsed; data.set(next); } }
                                }
                            }
                        } }
                    }
                }
                label { class: "plugin-record-form__published", input { r#type: "checkbox", checked: published(), onchange: move |event|published.set(event.checked()) } span { "Published" } }
                if let Some(message)=error() { div { class: "error-panel", "{message}" } }
                div { class: "content-type-dialog__actions",
                    button { class: "button button-quiet", onclick: move |_|on_close.call(()), "Cancel" }
                    button { class: "button button-primary", disabled: saving(), onclick: move |_| {
                        let form_data=data();
                        for field in &form_fields {
                            if field.get("required").and_then(Value::as_bool)==Some(true) {
                                let key=field.get("name").and_then(Value::as_str).unwrap_or("");
                                if form_data.get(key).is_none_or(|value|value.is_null()||value.as_str().is_some_and(str::is_empty)) { error.set(Some(format!("{} is required",field.get("label").and_then(Value::as_str).unwrap_or(key)))); return; }
                            }
                        }
                        saving.set(true); error.set(None);
                        let mut path=api_path.clone().replace("{uid}",&uid);
                        let mut payload=serde_json::json!({"uid":uid,"data":form_data,"published":published()});
                        if let Some(id)=record_id.as_ref() { path=path.replace("{id}",&id.to_string()); payload["id"]=id.clone(); }
                        let method=api_method.clone();
                        spawn(async move {
                            let request=reqwest::Client::new();
                            let request=match method.as_str() { "POST"=>request.post(api_url(&path)), "PUT"=>request.put(api_url(&path)), _=>request.post(api_url(&path)) };
                            let result=async {
                                let response=request.json(&payload).send().await.map_err(|e|e.to_string())?;
                                let status=response.status();
                                let body=response.text().await.unwrap_or_default();
                                if !status.is_success() { return Err(format!("Save failed ({status}): {body}")); }
                                let value:Value=serde_json::from_str(&body).unwrap_or(Value::Null);
                                if value.get("error").is_some_and(|error|!error.is_null()) { return Err(value["error"].to_string()); }
                                if value.get("logs").and_then(Value::as_array).is_some_and(|logs|logs.iter().any(|log|log.get("status").and_then(Value::as_str)==Some("failed"))) { return Err("The workflow could not save this entry".to_owned()); }
                                Ok(())
                            }.await;
                            match result { Ok(())=>on_complete.call(()),Err(message)=>error.set(Some(message)) }
                            saving.set(false);
                        });
                    }, if saving() { "Saving…" } else { "{submit_label}" } }
                }
            }
        }
    }
}

#[component]
pub(crate) fn ContentTypesPluginView() -> Element {
    rsx! {
        PluginViewPage {
            plugin_id: "ferrisflow.content-types".to_owned(),
            view_id: "content-manager".to_owned(),
        }
    }
}
