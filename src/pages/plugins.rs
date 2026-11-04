use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
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

#[component]
pub(crate) fn Plugins() -> Element {
    let mut plugins = use_server_future(list_plugins)?;
    let mut selected = use_signal(|| None::<String>);
    let mut refresh = use_signal(|| 0_u64);
    let mut creating = use_signal(|| false);
    let mut package_json = use_signal(|| default_plugin_json());
    let mut source_url = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let list = plugins().and_then(Result::ok).unwrap_or_default();
    let current = selected()
        .and_then(|uid| list.iter().find(|plugin| plugin.uid == uid).cloned())
        .or_else(|| list.first().cloned());
    let _refresh_version = refresh();

    rsx! {
        Shell {
            active: "plugins",
            header: "Plugins",
            intro: "Package database schemas, workflows, navigation, and views as reusable JSON bundles.",
            actions: rsx! {
                label { class: "plugin-import-url", input { placeholder: "https://…/plugin.json", value: "{source_url}", oninput: move |event| source_url.set(event.value()) }
                    Button { size: ButtonSize::Sm, variant: ButtonVariant::Secondary, r#type: "button", disabled: busy() || source_url().trim().is_empty(), onclick: move |_| {
                        busy.set(true); let url = source_url().trim().to_string();
                        spawn(async move {
                            let result = async { let response = reqwest::Client::new().post(api_url("/api/plugins/import")).json(&serde_json::json!({"url":url})).send().await.map_err(|e|e.to_string())?; decode::<Plugin>(response).await }.await;
                            match result { Ok(plugin) => { selected.set(Some(plugin.uid)); source_url.set(String::new()); plugins.restart(); refresh += 1; error.set(None); }, Err(message) => error.set(Some(message)) }
                            busy.set(false);
                        });
                    }, "Import URL" }
                }
                Button { size: ButtonSize::Sm, variant: ButtonVariant::Primary, r#type: "button", onclick: move |_| { package_json.set(default_plugin_json()); creating.set(true); error.set(None); }, "+ New plugin" }
            },
            div { class: "plugins-layout",
                aside { class: "plugins-sidebar panel",
                    span { class: "data-eyebrow", "INSTALLED PLUGINS" }
                    match plugins() {
                        Some(Ok(items)) if items.is_empty() => rsx! { p { class: "plugin-empty-note", "No plugins yet." } },
                        Some(Ok(items)) => rsx! { for item in items {
                            button { class: if current.as_ref().is_some_and(|active| active.uid == item.uid) { "plugin-option selected" } else { "plugin-option" }, r#type: "button", onclick: move |_| selected.set(Some(item.uid.clone())),
                                span { class: if item.enabled { "plugin-status-dot enabled" } else { "plugin-status-dot" } }
                                span { class: "plugin-option__copy", strong { "{item.name}" } small { "{item.uid} · v{item.version}" } }
                            }
                        } },
                        Some(Err(message)) => rsx! { ErrorPanel { message } },
                        None => rsx! { LoadingPanel {} },
                    }
                }
                section { class: "plugins-main panel",
                    if let Some(plugin) = current {
                        div { class: "plugins-heading",
                            div { span { class: "data-eyebrow", "PLUGIN / {plugin.uid}" } h2 { "{plugin.name}" } p { "Version {plugin.version}" } if let Some(description) = plugin.description.as_ref() { p { "{description}" } } }
                            div { class: "plugins-actions",
                                button { class: "button button-ghost", r#type: "button", disabled: plugin.uid == "ferrisflow.content-types", title: if plugin.uid == "ferrisflow.content-types" { "Built-in plugins stay enabled" } else { "" }, onclick: { let plugin_uid = plugin.uid.clone(); let next_enabled = !plugin.enabled; move |_| {
                                    let uid = plugin_uid.clone(); let enabled = next_enabled;
                                    spawn(async move {
                                        let result = async { let response = reqwest::Client::new().put(api_url(&format!("/api/plugins/{uid}/enabled"))).json(&serde_json::json!({"enabled":enabled})).send().await.map_err(|e|e.to_string())?; decode::<Plugin>(response).await }.await;
                                        match result { Ok(_) => { plugins.restart(); refresh += 1; }, Err(message) => error.set(Some(message)) }
                                    });
                                }}, if plugin.enabled { "Disable" } else { "Enable" } }
                                button { class: "button button-ghost plugin-delete", r#type: "button", disabled: plugin.uid == "ferrisflow.content-types", title: if plugin.uid == "ferrisflow.content-types" { "Built-in plugins cannot be deleted" } else { "" }, onclick: { let plugin_uid = plugin.uid.clone(); move |_| {
                                    let uid = plugin_uid.clone();
                                    spawn(async move { let result = reqwest::Client::new().delete(api_url(&format!("/api/plugins/{uid}"))).send().await; match result { Ok(response) if response.status().is_success() => { selected.set(None); plugins.restart(); refresh += 1; }, Ok(response) => error.set(Some(format!("Delete failed: {}", response.status()))), Err(message) => error.set(Some(message.to_string())) } });
                                }}, "Delete" }
                            }
                        }
                        if let Some(message) = error() { div { class: "error-panel", "{message}" } }
                        div { class: "plugin-resource-grid",
                            ResourceCount { title: "Database", count: section_count(&plugin.manifest, "contentTypes"), detail: "Content types and schema" }
                            ResourceCount { title: "Workflows", count: section_count(&plugin.manifest, "workflows"), detail: "Workflow definitions" }
                            ResourceCount { title: "Menu items", count: section_count(&plugin.manifest, "menuItems"), detail: "Workspace navigation" }
                            ResourceCount { title: "Views", count: section_count(&plugin.manifest, "views"), detail: "Saved presentations" }
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
                    } else {
                        div { class: "plugins-empty-state", span { class: "data-eyebrow", "PLUGIN REGISTRY" } h2 { "Add your first plugin" } p { "A plugin can be a single JSON manifest or a multi-file bundle with other plugins referenced by URL or bundled file." } }
                    }
                }
            }
        }
        if creating() {
            div { class: "dialog-backdrop", onclick: move |_| creating.set(false),
                section { class: "plugin-create-dialog panel", onclick: |event| event.stop_propagation(),
                    div { class: "builder-modal__header", div { span { class: "data-eyebrow", "PLUGIN PACKAGE" } h2 { "Create plugin" } p { "Paste one manifest or a file bundle JSON object." } } button { class: "icon-button", r#type: "button", onclick: move |_| creating.set(false), "×" } }
                    textarea { rows: "20", spellcheck: "false", value: "{package_json}", oninput: move |event| package_json.set(event.value()) }
                    if let Some(message) = error() { div { class: "error-panel", "{message}" } }
                    div { class: "content-type-dialog__actions",
                        Button { variant: ButtonVariant::Secondary, r#type: "button", onclick: move |_| creating.set(false), "Cancel" }
                        Button { size: ButtonSize::Sm, variant: ButtonVariant::Primary, r#type: "button", disabled: busy(), onclick: move |_| {
                            busy.set(true); let raw = package_json();
                            spawn(async move {
                                let result = match serde_json::from_str::<Value>(&raw) {
                                    Ok(package) => async { let response = reqwest::Client::new().post(api_url("/api/plugins")).json(&package).send().await.map_err(|e|e.to_string())?; decode::<Plugin>(response).await }.await,
                                    Err(message) => Err(format!("Plugin package JSON is invalid: {message}")),
                                };
                                match result { Ok(plugin) => { selected.set(Some(plugin.uid)); creating.set(false); plugins.restart(); refresh += 1; error.set(None); }, Err(message) => error.set(Some(message)) }
                                busy.set(false);
                            });
                        }, if busy() { "Saving…" } else { "Save plugin" } }
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
        "views": []
    }))
    .unwrap_or_default()
}

fn plugin_reference_label(reference: &Value) -> &str {
    reference
        .get("url")
        .or_else(|| reference.get("file"))
        .and_then(Value::as_str)
        .unwrap_or("Invalid reference")
}
