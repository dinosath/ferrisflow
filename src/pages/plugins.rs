use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

use crate::{
    components::data_table::{DataTable, DataTableCell, DataTableRow},
    components::icons::Icon,
    components::import_source::{ImportSource, ImportSourceDialog},
    components::ui::button::{Button, ButtonSize, ButtonVariant},
    pages::shared::{ErrorPanel, LoadingPanel, Shell},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Plugin {
    uid: String,
    name: String,
    version: String,
    description: Option<String>,
    manifest: Value,
    files: Value,
    source_url: Option<String>,
    enabled: bool,
}

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
async fn decode<T: for<'de> Deserialize<'de>>(response: reqwest::Response) -> Result<T, String> {
    let status = response.status();
    if status.is_success() {
        response.json().await.map_err(|error| error.to_string())
    } else {
        Err(format!("Request failed ({status}): {}", response.text().await.unwrap_or_default()))
    }
}
async fn list_plugins() -> Result<Vec<Plugin>, String> {
    decode(reqwest::get(api_url("/api/plugins")).await.map_err(|error| error.to_string())?).await
}
fn section_count(manifest: &Value, key: &str) -> usize {
    match key {
        "contentTypes" => manifest
            .get("database")
            .and_then(|db| db.get("contentTypes").or_else(|| db.get("content_types")))
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        "menuItems" => manifest
            .get("menuItems")
            .or_else(|| manifest.get("menu_items"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        _ => manifest.get(key).and_then(Value::as_array).map_or(0, Vec::len),
    }
}

fn plugin_resources(manifest: &Value, kind: &str) -> Vec<Value> {
    match kind {
        "databases" => manifest
            .get("database")
            .and_then(|database| {
                database.get("contentTypes").or_else(|| database.get("content_types"))
            })
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        "menuItems" => manifest
            .get("menuItems")
            .or_else(|| manifest.get("menu_items"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => manifest.get(kind).and_then(Value::as_array).cloned().unwrap_or_default(),
    }
}

fn plugin_resource_count(manifest: &Value) -> usize {
    ["databases", "workflows", "pages", "menuItems", "views"]
        .iter()
        .map(|kind| plugin_resources(manifest, kind).len())
        .sum()
}

fn plugin_reference_label(reference: &Value) -> String {
    match reference {
        Value::String(reference) => reference.clone(),
        Value::Object(object) => {
            let id = object
                .get("id")
                .or_else(|| object.get("uid"))
                .or_else(|| object.get("name"))
                .and_then(Value::as_str);
            let version = object.get("version").and_then(Value::as_str);
            match (id, version) {
                (Some(id), Some(version)) => format!("{id} ({version})"),
                (Some(id), None) => id.to_owned(),
                (None, Some(version)) => version.to_owned(),
                (None, None) => reference.to_string(),
            }
        }
        _ => reference.to_string(),
    }
}

fn plugin_import_request(source: ImportSource) -> Result<(&'static str, Value), String> {
    match source {
        ImportSource::Url(url) => Ok(("/api/plugins/import", serde_json::json!({"url": url}))),
        ImportSource::Contents(contents) => Ok(("/api/plugins", parse_plugin_package(&contents)?)),
        ImportSource::FileBundle { contents, files } => {
            let mut package = parse_plugin_package(&contents)?;
            merge_plugin_files(&mut package, files)?;
            Ok(("/api/plugins", package))
        }
    }
}

fn parse_plugin_package(contents: &str) -> Result<Value, String> {
    let package = serde_json::from_str::<Value>(contents)
        .map_err(|message| format!("Plugin package JSON is invalid: {message}"))?;
    if !package.is_object() {
        return Err("Plugin package must be a JSON object.".to_string());
    }
    Ok(package)
}

fn merge_plugin_files(
    package: &mut Value, attachments: Vec<(String, String)>,
) -> Result<(), String> {
    let manifest = package.get("manifest").unwrap_or(package);
    let mut references = HashSet::new();
    collect_plugin_file_references(manifest, &mut references);
    if let (Some(entry), Some(files)) = (
        package.get("entry").and_then(Value::as_str),
        package.get("files").and_then(Value::as_object),
    ) {
        if let Some(raw) = files.get(entry) {
            let entry_manifest = raw
                .as_str()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .unwrap_or_else(|| raw.clone());
            collect_plugin_file_references(&entry_manifest, &mut references);
        }
    }
    // A referenced dependency JSON may itself reference schemas or workflows.
    for (_, contents) in &attachments {
        if let Ok(value) = serde_json::from_str::<Value>(contents) {
            collect_plugin_file_references(&value, &mut references);
        }
    }
    let files = package
        .as_object_mut()
        .expect("package was validated as an object")
        .entry("files")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let files = files
        .as_object_mut()
        .ok_or_else(|| "Plugin package 'files' must be a JSON object.".to_string())?;

    for (name, contents) in attachments {
        let normalized = name.replace('\\', "/");
        let basename = normalized.rsplit('/').next().unwrap_or(&normalized);
        let exact = references.contains(&normalized).then_some(normalized.as_str());
        let matching: Vec<&str> = references
            .iter()
            .map(String::as_str)
            .filter(|reference| reference.rsplit('/').next() == Some(basename))
            .collect();
        let key = if let Some(path) = exact {
            path.to_string()
        } else if matching.len() == 1 {
            matching[0].to_string()
        } else if matching.len() > 1 {
            return Err(format!(
                "The file '{basename}' matches multiple plugin references. Select files with distinct names or embed them in the package."
            ));
        } else {
            normalized
        };
        if files.contains_key(&key) {
            return Err(format!("Plugin package already contains '{key}'."));
        }
        files.insert(key, Value::String(contents));
    }
    Ok(())
}

fn collect_plugin_file_references(value: &Value, references: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            for key in ["$file", "file"] {
                if let Some(path) = object.get(key).and_then(Value::as_str) {
                    references.insert(path.replace('\\', "/"));
                }
            }
            for child in object.values() {
                collect_plugin_file_references(child, references);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_plugin_file_references(child, references);
            }
        }
        _ => {}
    }
}

#[component]
pub(crate) fn Plugins() -> Element {
    let mut plugins = use_server_future(list_plugins)?;
    let mut refresh = use_signal(|| 0_u64);
    let mut creating_json = use_signal(|| false);
    let mut creating = use_signal(|| false);
    let mut import_open = use_signal(|| false);
    let mut package_json = use_signal(|| default_plugin_json());
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let mut query = use_signal(String::new);
    let navigator = use_navigator();
    let _refresh_version = refresh();
    let on_plugin_import = move |source: Result<ImportSource, String>| {
        let request = source.and_then(plugin_import_request);
        let (path, payload) = match request {
            Ok(request) => request,
            Err(message) => {
                error.set(Some(message));
                return;
            }
        };
        error.set(None);
        busy.set(true);
        spawn(async move {
            let result = async {
                let response = reqwest::Client::new()
                    .post(api_url(path))
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|message| message.to_string())?;
                decode::<Plugin>(response).await
            }
            .await;
            match result {
                Ok(plugin) => {
                    import_open.set(false);
                    plugins.restart();
                    refresh += 1;
                    navigator.push(crate::Route::PluginDetailPage { plugin_id: plugin.uid });
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    rsx! {
        Shell {
            active: "plugins",
            header: "Plugins",
            intro: "Package database schemas, workflows, navigation, and pages as reusable JSON bundles.",
            actions: rsx! {
                Button { size: ButtonSize::Sm, variant: ButtonVariant::Secondary, r#type: "button", onclick: move |_| { package_json.set(default_plugin_json()); creating_json.set(true); error.set(None); }, "JSON package" }
                Button { size: ButtonSize::Sm, variant: ButtonVariant::Primary, r#type: "button", onclick: move |_| { creating.set(true); error.set(None); }, "+ New plugin" }
            },
            section { class: "plugins-table-section",
                if let Some(message) = error() { ErrorPanel { message } }
                match plugins() {
                    Some(Ok(items)) => {
                        let needle = query().trim().to_lowercase();
                        let filtered: Vec<Plugin> = items.into_iter().filter(|plugin| {
                            needle.is_empty() || plugin.name.to_lowercase().contains(&needle) || plugin.uid.to_lowercase().contains(&needle)
                        }).collect();
                        let row_ids = filtered.iter().map(|plugin| plugin.uid.clone()).collect();
                        rsx! {
                            DataTable {
                                columns: vec!["Plugin".to_owned(), "Version".to_owned(), "Status".to_owned(), "Resources".to_owned(), "Description".to_owned()],
                                row_ids,
                                toolbar: rsx! {
                                    label { class: "workflow-list-search",
                                        Icon { name: "search" }
                                        input { r#type: "search", aria_label: "Filter plugins by name or ID", placeholder: "Filter plugins...", value: "{query}", oninput: move |event| query.set(event.value()) }
                                    }
                                    button {
                                        class: "button button-quiet workflow-list-import",
                                        r#type: "button",
                                        disabled: busy(),
                                        onclick: move |_| { error.set(None); import_open.set(true); },
                                        Icon { name: "upload" }
                                        "Import"
                                    }
                                },
                                empty_message: if query().is_empty() { "No plugins yet. Create a plugin to get started.".to_owned() } else { "No plugins match your search.".to_owned() },
                                class: "plugins-data-table",
                                for (index, plugin) in filtered.into_iter().enumerate() {
                                    {
                                        let resources = plugin_resource_count(&plugin.manifest);
                                        let description = plugin.description.as_deref().unwrap_or("No description");
                                        let plugin_id = plugin.uid.clone();
                                        rsx! {
                                            DataTableRow { id: plugin.uid.clone(), index,
                                                DataTableCell { index: 0, class: "name-cell plugin-table-name-cell",
                                                    Link { class: "plugin-table-name", to: crate::Route::PluginDetailPage { plugin_id },
                                                        strong { "{plugin.name}" }
                                                        small { "{plugin.uid}" }
                                                    }
                                                }
                                                DataTableCell { index: 1, "v{plugin.version}" }
                                                DataTableCell { index: 2, span { class: if plugin.enabled { "status status-active" } else { "status status-paused" }, if plugin.enabled { "Enabled" } else { "Disabled" } } }
                                                DataTableCell { index: 3, "{resources}" }
                                                DataTableCell { index: 4, class: "description-cell", "{description}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Some(Err(message)) => rsx! { ErrorPanel { message } },
                    None => rsx! { LoadingPanel {} },
                }
                if import_open() {
                    ImportSourceDialog {
                        title: "Import plugin".to_string(),
                        description: "Load a plugin package from its JSON file. The file must contain a valid plugin manifest.".to_string(),
                        file_label: "Choose a plugin JSON package".to_string(),
                        accept: ".json,application/json".to_string(),
                        url_placeholder: "https://example.com/plugin.json".to_string(),
                        text_placeholder: "Paste a plugin package JSON object…".to_string(),
                        action_label: "Import plugin".to_string(),
                        fetch_url: false,
                        error: error(),
                        external_busy: busy(),
                        https_only: true,
                        validate_plugin_json: true,
                        on_import: on_plugin_import,
                        on_close: move |_| import_open.set(false),
                    }
                }
            }
        }
        if creating_json() {
            div { class: "dialog-backdrop", onclick: move |_| creating_json.set(false),
                section { class: "plugin-create-dialog panel", role: "dialog", aria_modal: "true", onclick: |event| event.stop_propagation(),
                    div { class: "builder-modal__header", div { span { class: "data-eyebrow", "PLUGIN PACKAGE" } h2 { "Create plugin from JSON" } p { "Paste a manifest or multi-file bundle." } } button { class: "icon-button", r#type: "button", onclick: move |_| creating_json.set(false), "×" } }
                    textarea { rows: "20", spellcheck: "false", value: "{package_json}", oninput: move |event| package_json.set(event.value()) }
                    if let Some(message) = error() { div { class: "error-panel", "{message}" } }
                    div { class: "content-type-dialog__actions",
                        Button { variant: ButtonVariant::Secondary, r#type: "button", onclick: move |_| creating_json.set(false), "Cancel" }
                        Button { size: ButtonSize::Sm, variant: ButtonVariant::Primary, r#type: "button", disabled: busy(), onclick: move |_| {
                            busy.set(true); let raw = package_json();
                            spawn(async move {
                                let result = match serde_json::from_str::<Value>(&raw) {
                                    Ok(package) => async { let response = reqwest::Client::new().post(api_url("/api/plugins")).json(&package).send().await.map_err(|e|e.to_string())?; decode::<Plugin>(response).await }.await,
                                    Err(message) => Err(format!("Plugin package JSON is invalid: {message}")),
                                };
                                match result { Ok(plugin) => { creating_json.set(false); plugins.restart(); refresh += 1; navigator.push(crate::Route::PluginDetailPage { plugin_id: plugin.uid }); }, Err(message) => error.set(Some(message)) }
                                busy.set(false);
                            });
                        }, if busy() { "Saving…" } else { "Create plugin" } }
                    }
                }
            }
        }
        if creating() {
            PluginResourceDialog {
                plugin: None,
                on_close: move |_| creating.set(false),
                on_saved: move |plugin: Plugin| { creating.set(false); plugins.restart(); refresh += 1; navigator.push(crate::Route::PluginDetailPage { plugin_id: plugin.uid }); }
            }
        }
    }
}

async fn read_plugin(uid: String) -> Result<Plugin, String> {
    decode(
        reqwest::get(api_url(&format!("/api/plugins/{uid}")))
            .await
            .map_err(|error| error.to_string())?,
    )
    .await
}

#[component]
pub(crate) fn PluginDetailPage(plugin_id: String) -> Element {
    let id_for_loader = plugin_id.clone();
    let mut plugin_result = use_server_future(move || read_plugin(id_for_loader.clone()))?;
    let mut versioning = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let navigator = use_navigator();
    let loaded = plugin_result();

    rsx! {
        Shell {
            active: "plugins",
            header: "Plugin",
            intro: "Inspect and manage this plugin package.",
            actions: rsx! {
                Link { class: "button button-quiet", to: crate::Route::Plugins {}, "Back to plugins" }
            },
            match loaded {
                Some(Ok(plugin)) => rsx! {
                    section { class: "plugins-main panel plugin-detail-page",
                        div { class: "plugins-heading",
                            div { span { class: "data-eyebrow", "PLUGIN / {plugin.uid}" } h2 { "{plugin.name}" } p { "Version {plugin.version}" } if let Some(description) = plugin.description.as_ref() { p { "{description}" } } }
                            div { class: "plugins-actions",
                                button { class: "button button-quiet", r#type: "button", disabled: plugin.manifest["builtin"] == true, onclick: move |_| { versioning.set(true); error.set(None); }, "New version" }
                                button { class: "button button-ghost", r#type: "button", disabled: (plugin.manifest["builtin"] == true) || busy(), title: if plugin.manifest["builtin"] == true { "Built-in plugins stay enabled" } else { "" }, onclick: { let uid = plugin.uid.clone(); let enabled = !plugin.enabled; move |_| {
                                    let uid = uid.clone();
                                    busy.set(true);
                                    spawn(async move {
                                        let result = async { let response = reqwest::Client::new().put(api_url(&format!("/api/plugins/{uid}/enabled"))).json(&serde_json::json!({"enabled":enabled})).send().await.map_err(|e|e.to_string())?; decode::<Plugin>(response).await }.await;
                                        match result { Ok(_) => plugin_result.restart(), Err(message) => error.set(Some(message)) }
                                        busy.set(false);
                                    });
                                }}, if plugin.enabled { "Disable" } else { "Enable" } }
                                button { class: "button button-ghost plugin-delete", r#type: "button", disabled: (plugin.manifest["builtin"] == true) || busy(), title: if plugin.manifest["builtin"] == true { "Built-in plugins cannot be deleted" } else { "" }, onclick: { let uid = plugin.uid.clone(); move |_| {
                                    let uid = uid.clone();
                                    busy.set(true);
                                    spawn(async move {
                                        let result = reqwest::Client::new().delete(api_url(&format!("/api/plugins/{uid}"))).send().await;
                                        match result { Ok(response) if response.status().is_success() => { navigator.push(crate::Route::Plugins {}); }, Ok(response) => error.set(Some(format!("Delete failed: {}", response.status()))), Err(message) => error.set(Some(message.to_string())) }
                                        busy.set(false);
                                    });
                                }}, "Delete" }
                            }
                        }
                        if let Some(message) = error() { div { class: "error-panel", "{message}" } }
                        if let Some(warnings) = plugin.manifest.get("importWarnings").and_then(Value::as_array) {
                            for warning in warnings.iter().filter_map(Value::as_str) {
                                p { class: "plugin-source-note", "{warning}" }
                            }
                        }
                        div { class: "plugin-resource-grid",
                            ResourceCount { title: "Database", count: section_count(&plugin.manifest, "contentTypes"), detail: "Content types and schema" }
                            ResourceCount { title: "Workflows", count: section_count(&plugin.manifest, "workflows"), detail: "Workflow definitions" }
                            ResourceCount { title: "Menu items", count: section_count(&plugin.manifest, "menuItems"), detail: "Workspace navigation" }
                            ResourceCount { title: "Views", count: section_count(&plugin.manifest, "views"), detail: "Saved presentations" }
                            ResourceCount { title: "Pages", count: section_count(&plugin.manifest, "pages"), detail: "Workspace pages" }
                        }
                        div { class: "plugin-parts-grid",
                            PluginResourceSection { title: "Database schemas", kind: "databases", items: plugin_resources(&plugin.manifest, "databases") }
                            PluginResourceSection { title: "Workflows", kind: "workflows", items: plugin_resources(&plugin.manifest, "workflows") }
                            PluginResourceSection { title: "Pages", kind: "pages", items: plugin_resources(&plugin.manifest, "pages") }
                            PluginResourceSection { title: "Menu items", kind: "menuItems", items: plugin_resources(&plugin.manifest, "menuItems") }
                            PluginResourceSection { title: "Views", kind: "views", items: plugin_resources(&plugin.manifest, "views") }
                        }
                        if let Some(views) = plugin.manifest.get("views").and_then(Value::as_array) {
                            if !views.is_empty() {
                                div { class: "plugin-ui-section",
                                    div { span { class: "data-eyebrow", "PLUGIN UI" } h3 { "Views" } p { "Open a plugin view to configure its list, create, and update layouts." } }
                                    div { class: "plugins-actions",
                                        for view in views {
                                            if let (Some(name), Some(route)) = (view.get("name").and_then(Value::as_str), view.get("route").and_then(Value::as_str)) {
                                                a { class: "button button-ghost", href: "{route}", "{name}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(url) = plugin.source_url.as_ref() { p { class: "plugin-source-note", "Imported from {url}" } }
                        if let Some(refs) = plugin.manifest.get("plugins").or_else(|| plugin.manifest.get("requires")).and_then(Value::as_array) {
                            div { class: "plugin-dependencies", span { class: "data-eyebrow", "PLUGIN DEPENDENCIES" } for reference in refs { span { class: "plugin-dependency", "{plugin_reference_label(reference)}" } } }
                        }
                        div { class: "plugin-manifest-view", span { class: "data-eyebrow", "MANIFEST" } pre { code { "{serde_json::to_string_pretty(&plugin.manifest).unwrap_or_else(|_| plugin.manifest.to_string())}" } } }
                        if plugin.files.as_object().is_some_and(|files| !files.is_empty()) {
                            div { class: "plugin-files-view", span { class: "data-eyebrow", "BUNDLE FILES" } for (path, _) in plugin.files.as_object().into_iter().flatten() { span { class: "plugin-file-path", "{path}" } } }
                        }
                        if versioning() {
                            PluginResourceDialog {
                                plugin: Some(plugin.clone()),
                                on_close: move |_| versioning.set(false),
                                on_saved: move |_| { versioning.set(false); plugin_result.restart(); }
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct PluginResources {
    workflows: Vec<Value>,
    databases: Vec<Value>,
    pages: Vec<Value>,
}

async fn load_plugin_resources() -> Result<PluginResources, String> {
    let workflows: Vec<crate::pages::models::Workflow> = decode(
        reqwest::get(api_url("/api/workflows?page=1&page_size=100&sort=name&order=asc"))
            .await
            .map_err(|error| error.to_string())?,
    )
    .await?;
    let workflows = workflows
        .into_iter()
        .map(|workflow| {
            serde_json::from_str::<Value>(&workflow.definition_json)
                .map_err(|error| format!("Workflow {} has invalid JSON: {error}", workflow.name))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let type_response = reqwest::get(api_url("/api/content-type-registry"))
        .await
        .map_err(|error| error.to_string())?;
    let type_data: Value = decode(type_response).await?;
    let databases = type_data
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "Content type API returned an invalid list".to_owned())?;

    let page_response =
        reqwest::get(api_url("/api/pages")).await.map_err(|error| error.to_string())?;
    let page_data: Value = decode(page_response).await?;
    let pages = page_data
        .as_array()
        .cloned()
        .ok_or_else(|| "Page API returned an invalid list".to_owned())?
        .into_iter()
        .filter(|page| {
            !(page.get("generated").and_then(Value::as_bool) == Some(true)
                && page.get("extends").is_some_and(Value::is_null))
        })
        .map(|mut page| {
            if let Some(object) = page.as_object_mut() {
                object.remove("recordId");
                object.remove("updatedAt");
                object.remove("isDefault");
            }
            page
        })
        .collect();

    Ok(PluginResources { workflows, databases, pages })
}

fn resource_key(kind: &str, value: &Value) -> String {
    match kind {
        "workflows" => {
            let document = value.get("document").unwrap_or(value);
            format!(
                "workflows:{}:{}:{}",
                document.get("namespace").and_then(Value::as_str).unwrap_or(""),
                document.get("name").and_then(Value::as_str).unwrap_or(""),
                document.get("version").and_then(Value::as_str).unwrap_or(""),
            )
        }
        "databases" => format!(
            "databases:{}",
            value.get("uid").or_else(|| value.get("id")).and_then(Value::as_str).unwrap_or("")
        ),
        _ => format!("pages:{}", value.get("id").and_then(Value::as_str).unwrap_or("")),
    }
}

fn plugin_version_next(version: &str) -> String {
    let mut parts = version.split('.').collect::<Vec<_>>();
    if parts.len() == 3 {
        if let Ok(patch) = parts[2].parse::<u64>() {
            parts[2] = "";
            return format!("{}.{}.{}", parts[0], parts[1], patch.saturating_add(1));
        }
    }
    format!("{version}.1")
}

#[component]
fn PluginResourceDialog(
    plugin: Option<Plugin>, on_close: EventHandler<()>, on_saved: EventHandler<Plugin>,
) -> Element {
    let resources = use_server_future(load_plugin_resources)?;
    let mut selected = use_signal(HashSet::<String>::new);
    let mut initialized = use_signal(|| false);
    let mut plugin_id = use_signal(|| {
        plugin
            .as_ref()
            .map(|plugin| plugin.uid.clone())
            .unwrap_or_else(|| "example.inventory".to_owned())
    });
    let mut plugin_name = use_signal(|| {
        plugin.as_ref().map(|plugin| plugin.name.clone()).unwrap_or_else(|| "Inventory".to_owned())
    });
    let mut version = use_signal(|| {
        plugin
            .as_ref()
            .map(|plugin| plugin_version_next(&plugin.version))
            .unwrap_or_else(|| "1.0.0".to_owned())
    });
    let is_new_plugin = plugin.is_none();
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    if let Some(Ok(data)) = resources() {
        if !initialized() {
            let mut initial = HashSet::new();
            for (kind, values) in [
                ("workflows", data.workflows.as_slice()),
                ("databases", data.databases.as_slice()),
                ("pages", data.pages.as_slice()),
            ] {
                let existing = plugin.as_ref().and_then(|plugin| match kind {
                    "workflows" => plugin.manifest.get("workflows").and_then(Value::as_array),
                    "databases" => plugin
                        .manifest
                        .get("database")
                        .and_then(|db| db.get("contentTypes").or_else(|| db.get("content_types")))
                        .and_then(Value::as_array),
                    _ => plugin.manifest.get("pages").and_then(Value::as_array),
                });
                for candidate in values {
                    if existing.is_some_and(|items| {
                        items
                            .iter()
                            .any(|item| resource_key(kind, item) == resource_key(kind, candidate))
                    }) {
                        initial.insert(resource_key(kind, candidate));
                    }
                }
            }
            selected.set(initial);
            initialized.set(true);
        }
    }

    let resources_for_view = resources();
    rsx! {
        div { class: "dialog-backdrop", onclick: move |_| on_close.call(()),
            section { class: "plugin-create-dialog plugin-version-dialog panel", role: "dialog", aria_modal: "true", onclick: |event| event.stop_propagation(),
                div { class: "builder-modal__header", div { span { class: "data-eyebrow", "PLUGIN PACKAGE" } h2 { if is_new_plugin { "Create plugin" } else { "Create plugin version" } } p { if is_new_plugin { "Start with a name and choose the existing resources to include." } else { "Choose resources to include in the next version of {plugin.as_ref().map(|item| item.name.as_str()).unwrap_or(\"plugin\")}." } } } button { class: "icon-button", r#type: "button", onclick: move |_| on_close.call(()), "×" } }
                if is_new_plugin {
                    label { class: "plugin-version-input", span { "Plugin name" } input { value: "{plugin_name}", placeholder: "Inventory", oninput: move |event| plugin_name.set(event.value()) } }
                    label { class: "plugin-version-input", span { "Plugin ID" } input { value: "{plugin_id}", placeholder: "example.inventory", oninput: move |event| plugin_id.set(event.value()) } }
                }
                label { class: "plugin-version-input", span { if is_new_plugin { "Version" } else { "New version" } } input { value: "{version}", oninput: move |event| version.set(event.value()) } }
                match resources_for_view {
                    Some(Ok(ref data)) => rsx! {
                        PluginResourceGroup { title: "Workflows", description: "Valid workflow documents available in this workspace.", kind: "workflows", items: data.workflows.clone(), selected: selected }
                        PluginResourceGroup { title: "Database schemas", description: "Content types and their database schema.", kind: "databases", items: data.databases.clone(), selected: selected }
                        PluginResourceGroup { title: "Pages", description: "Workspace pages available to bundle with this plugin.", kind: "pages", items: data.pages.clone(), selected: selected }
                    },
                    Some(Err(ref message)) => rsx! { div { class: "error-panel", "Could not load workspace resources: {message}" } },
                    None => rsx! { LoadingPanel {} },
                }
                if let Some(message) = error() { div { class: "error-panel", "{message}" } }
                div { class: "content-type-dialog__actions",
                    Button { variant: ButtonVariant::Secondary, r#type: "button", onclick: move |_| on_close.call(()), "Cancel" }
                    Button { size: ButtonSize::Sm, variant: ButtonVariant::Primary, r#type: "button", disabled: busy() || resources_for_view.as_ref().is_none_or(|result| result.is_err()) || version().trim().is_empty() || (is_new_plugin && (plugin_id().trim().is_empty() || plugin_name().trim().is_empty())), onclick: move |_| {
                        let Some(Ok(data)) = resources() else { return; };
                        let selected_keys = selected();
                        let mut manifest = plugin.as_ref().map(|plugin| plugin.manifest.clone()).unwrap_or_else(|| serde_json::json!({
                            "schemaVersion": 1,
                            "id": plugin_id().trim(),
                            "name": plugin_name().trim(),
                            "version": version().trim(),
                            "description": "",
                            "plugins": [],
                            "database": {},
                            "workflows": [],
                            "menuItems": [],
                            "views": [],
                            "pages": []
                        }));
                        let selected_values = |kind: &str, values: &[Value]| values.iter().filter(|value| selected_keys.contains(&resource_key(kind, value))).cloned().collect::<Vec<_>>();
                        manifest["workflows"] = Value::Array(selected_values("workflows", &data.workflows));
                        if !manifest.get("database").is_some_and(Value::is_object) { manifest["database"] = serde_json::json!({}); }
                        manifest["database"]["contentTypes"] = Value::Array(selected_values("databases", &data.databases));
                        manifest["pages"] = Value::Array(selected_values("pages", &data.pages));
                        if let Some(meta) = manifest.get_mut("plugin").filter(|meta| meta.is_object()) { meta["version"] = Value::String(version().trim().to_owned()); }
                        else { manifest["version"] = Value::String(version().trim().to_owned()); }
                        let files = plugin.as_ref().map(|plugin| plugin.files.clone()).unwrap_or_else(|| serde_json::json!({}));
                        let endpoint = plugin.as_ref().map(|plugin| format!("/api/plugins/{}", plugin.uid)).unwrap_or_else(|| "/api/plugins".to_owned());
                        let manifest = manifest.clone();
                        busy.set(true);
                        spawn(async move {
                            let result = async {
                                let client = reqwest::Client::new();
                                let request = if is_new_plugin { client.post(api_url(&endpoint)) } else { client.put(api_url(&endpoint)) };
                                let response = request.json(&serde_json::json!({"manifest":manifest,"files":files})).send().await.map_err(|e|e.to_string())?;
                                decode::<Plugin>(response).await
                            }.await;
                            match result { Ok(saved) => on_saved.call(saved), Err(message) => error.set(Some(message)) }
                            busy.set(false);
                        });
                    }, if busy() { "Saving…" } else if is_new_plugin { "Create plugin" } else { "Create version" } }
                }
            }
        }
    }
}

#[component]
fn PluginResourceGroup(
    title: &'static str, description: &'static str, kind: &'static str, items: Vec<Value>,
    selected: Signal<HashSet<String>>,
) -> Element {
    rsx! { section { class: "plugin-resource-picker",
        div { class: "plugin-resource-picker__heading", div { h3 { "{title}" } p { "{description}" } } span { class: "data-eyebrow", "{items.len()} AVAILABLE" } }
        if items.is_empty() { p { class: "plugin-empty-note", "No {title.to_lowercase()} found." } }
        else { div { class: "plugin-resource-picker__list", for item in items {
            { let key = resource_key(kind, &item); let active = selected().contains(&key); let label = resource_label(kind, &item); rsx! {
                button { class: if active { "plugin-resource-choice selected" } else { "plugin-resource-choice" }, r#type: "button", aria_pressed: "{active}", onclick: move |_| { let mut next = selected(); if !next.remove(&key) { next.insert(key.clone()); } selected.set(next); },
                    span { class: if active { "plugin-resource-choice__check checked" } else { "plugin-resource-choice__check" }, if active { "✓" } else { "" } }
                    span { class: "plugin-resource-choice__copy", strong { "{label}" } small { "{resource_detail(kind, &item)}" } }
                }
            } }
        } } }
    } }
}

fn resource_label(kind: &str, value: &Value) -> String {
    if kind == "workflows" {
        let document = value.get("document").unwrap_or(value);
        format!(
            "{} / {}",
            document.get("namespace").and_then(Value::as_str).unwrap_or("default"),
            document.get("name").and_then(Value::as_str).unwrap_or("Untitled workflow")
        )
    } else {
        value
            .get("display_name")
            .or_else(|| value.get("label"))
            .or_else(|| value.get("name"))
            .or_else(|| value.get("title"))
            .or_else(|| value.get("uid"))
            .or_else(|| value.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("Untitled resource")
            .to_owned()
    }
}

fn resource_detail(kind: &str, value: &Value) -> String {
    match kind {
        "workflows" => {
            let document = value.get("document").unwrap_or(value);
            format!(
                "Version {} · Serverless Workflow",
                document.get("version").and_then(Value::as_str).unwrap_or("unknown")
            )
        }
        "databases" => {
            value.get("uid").and_then(Value::as_str).unwrap_or("Database content type").to_owned()
        }
        "menuItems" => value
            .get("path")
            .or_else(|| value.get("href"))
            .and_then(Value::as_str)
            .unwrap_or("Workspace navigation item")
            .to_owned(),
        _ => format!(
            "{} · {}",
            value.get("source").and_then(Value::as_str).unwrap_or("Page"),
            value.get("type").and_then(Value::as_str).unwrap_or("custom")
        ),
    }
}

#[component]
fn PluginResourceSection(title: &'static str, kind: &'static str, items: Vec<Value>) -> Element {
    rsx! {
        section { class: "plugin-parts-section",
            header { span { class: "data-eyebrow", "{title}" } span { class: "plugin-parts-count", "{items.len()}" } }
            if items.is_empty() {
                p { class: "plugin-parts-empty", "No {title.to_lowercase()} included." }
            } else {
                ul { class: "plugin-parts-list",
                    for item in items {
                        {
                            let label = resource_label(kind, &item);
                            let detail = resource_detail(kind, &item);
                            rsx! { li { span { class: "plugin-parts-item__copy", strong { "{label}" } small { "{detail}" } } } }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ResourceCount(title: &'static str, count: usize, detail: &'static str) -> Element {
    rsx! { article { class: "plugin-resource-card", span { class: "data-eyebrow", "{title}" } strong { "{count}" } small { "{detail}" } } }
}

fn default_plugin_json() -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "schemaVersion": 1,
        "id": "example.inventory",
        "name": "Inventory",
        "version": "1.0.0",
        "description": "Inventory resources and admin views",
        "plugins": [],
        "database": {"contentTypes": []},
        "workflows": [],
        "menuItems": [],
        "views": [],
        "pages": []
    }))
    .unwrap_or_default()
}
