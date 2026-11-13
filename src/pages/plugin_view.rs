use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::Value;

use crate::components::data_table::{DataTable, DataTableCell, DataTableRow};

use crate::{
    Route,
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

async fn load_content_type(uid: String) -> Result<Value, String> {
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
    let result: Value = response.json().await.map_err(|error| error.to_string())?;
    result
        .get("output")
        .or_else(|| result.get("data"))
        .and_then(Value::as_array)
        .and_then(|types| types.iter().find(|item| item["uid"].as_str() == Some(&uid)))
        .cloned()
        .ok_or_else(|| format!("Content type '{uid}' was not found"))
}

async fn api_json(path: &str) -> Result<Value, String> {
    let response = reqwest::get(api_url(path)).await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Could not load novel data ({status}): {}", response.text().await.unwrap_or_default()));
    }
    response.json().await.map_err(|error| error.to_string())
}

async fn load_novel_view(id: String) -> Result<(Value, Vec<Value>), String> {
    let novel_response = api_json(&format!("/api/content/local_novels_novel/{id}")).await?;
    let novel = novel_response.get("output").cloned().unwrap_or(novel_response);
    if !novel.is_object() || novel["source"].as_str().is_none() {
        return Err(format!("Novel '{id}' was not found"));
    }
    let source = novel["source"].as_str().unwrap_or_default();
    let chapter_response = api_json("/api/collections/local_novels_chapter").await?;
    let chapters = chapter_response
        .get("output")
        .or_else(|| chapter_response.get("data"))
        .and_then(Value::as_array)
        .map(|items| items.iter().filter(|item| item["novel_source"].as_str() == Some(source)).cloned().collect::<Vec<_>>())
        .ok_or_else(|| "Chapter API returned an invalid list".to_owned())?;
    Ok((novel, chapters))
}

#[component]
pub(crate) fn NovelViewer(id: String) -> Element {
    let novel_id = id.clone();
    let novel_data = use_server_future(move || load_novel_view(novel_id.clone()))?;
    let mut search = use_signal(String::new);
    let mut newest_first = use_signal(|| true);
    let mut bookmarked = use_signal(|| false);
    let mut reading = use_signal(|| None::<Value>);

    match novel_data() {
        Some(Ok((novel, mut chapters))) => {
            chapters.sort_by_key(|chapter| chapter["number"].as_i64().or_else(|| chapter["number"].as_f64().map(|n| n as i64)).unwrap_or(0));
            if newest_first() { chapters.reverse(); }
            let needle = search().trim().to_lowercase();
            let visible_chapters = chapters.iter().filter(|chapter| {
                needle.is_empty() || chapter["number"].to_string().contains(&needle) || chapter["title"].as_str().is_some_and(|title| title.to_lowercase().contains(&needle))
            }).cloned().collect::<Vec<_>>();
            let title = novel["title"].as_str().unwrap_or("Untitled novel").to_owned();
            let author = novel["author"].as_str().unwrap_or_default().to_owned();
            let synopsis = novel["synopsis"].as_str().unwrap_or_default().to_owned();
            let cover = novel["cover"].as_str().filter(|value| !value.trim().is_empty()).map(str::to_owned);
            let source_site = novel["source_site"].as_str().unwrap_or("Web Novel").to_owned();
            let latest = chapters.iter().max_by_key(|chapter| chapter["number"].as_i64().or_else(|| chapter["number"].as_f64().map(|n| n as i64)).unwrap_or(0)).cloned();
            let first = chapters.iter().min_by_key(|chapter| chapter["number"].as_i64().or_else(|| chapter["number"].as_f64().map(|n| n as i64)).unwrap_or(0)).cloned();
            let latest_date = latest.as_ref().and_then(|chapter| chapter["downloaded_at"].as_str()).unwrap_or("").to_owned();
            let sort_label = if newest_first() { "Latest" } else { "Oldest" };
            rsx! {
                main { class: "novel-reader-page",
                    section { class: "novel-reader-hero",
                        div { class: "novel-reader-hero__inner",
                            if let Some(cover) = cover.as_ref() {
                                img { class: "novel-reader-cover", src: "{cover}", alt: "Cover for {title}" }
                            } else {
                                div { class: "novel-reader-cover novel-reader-cover--empty", "{title.chars().take(1).collect::<String>()}" }
                            }
                            div { class: "novel-reader-summary",
                                h1 { "{title}" }
                                if !author.is_empty() { p { class: "novel-reader-original-title", "{author}" } }
                                div { class: "novel-reader-meta", span { class: "novel-reader-status", "●" " Saved" } span { "Web Novel" } span { "{source_site}" } }
                                div { class: "novel-reader-stats",
                                    strong { "{chapters.len()}" } " chapters"
                                    if !latest_date.is_empty() { span { "Updated " span { class: "novel-reader-emphasis", "{relative_date(&latest_date)}" } } }
                                    if !author.is_empty() { span { "Translated by " span { class: "novel-reader-emphasis", "{author}" } } }
                                }
                                if !synopsis.trim().is_empty() { p { class: "novel-reader-synopsis", "{synopsis}" } }
                                div { class: "novel-reader-actions",
                                    if let Some(first_chapter) = first {
                                        button { class: "novel-reader-button novel-reader-button--primary", r#type: "button", onclick: move |_| reading.set(Some(first_chapter.clone())), "Start Reading" }
                                    }
                                    if let Some(latest_chapter) = latest {
                                        button { class: "novel-reader-button", r#type: "button", onclick: move |_| reading.set(Some(latest_chapter.clone())), "Latest Chapter" }
                                    }
                                    button { class: "novel-reader-button", r#type: "button", onclick: move |_| bookmarked.set(!bookmarked()), if bookmarked() { "✓ Bookmarked" } else { "♧  Bookmark" } }
                                }
                            }
                        }
                    }
                    section { class: "novel-reader-chapters",
                        div { class: "novel-reader-chapters__inner",
                            h2 { "Chapters" }
                            div { class: "novel-reader-tools",
                                label { class: "novel-reader-search", span { "⌕" } input { placeholder: "Search chapter number...", value: "{search}", oninput: move |event| search.set(event.value()) } }
                                button { class: "novel-reader-sort", r#type: "button", onclick: move |_| newest_first.set(!newest_first()), "↕  {sort_label}" }
                            }
                            p { class: "novel-reader-count", "Showing {visible_chapters.len()} of {chapters.len()} chapters" }
                            div { class: "novel-reader-grid",
                                for chapter in visible_chapters.iter() {
                                    {
                                        let selected = chapter.clone();
                                        let number = chapter["number"].as_i64().or_else(|| chapter["number"].as_f64().map(|n| n as i64)).unwrap_or(0);
                                        let date = chapter["downloaded_at"].as_str().map(relative_date).unwrap_or_default();
                                        rsx! { button { class: "novel-reader-chapter", r#type: "button", onclick: move |_| reading.set(Some(selected.clone())),
                                            strong { "Ch. {number}" }
                                            if !date.is_empty() { small { "◷  {date}" } }
                                        } }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(chapter) = reading() {
                        {
                            let chapter_title = chapter["title"].as_str().unwrap_or("Chapter").to_owned();
                            let chapter_body = ammonia::clean(chapter["body"].as_str().unwrap_or_default());
                            rsx! {
                                div { class: "novel-reader-overlay", onclick: move |_| reading.set(None),
                                    article { class: "novel-reader-dialog", onclick: move |event| event.stop_propagation(),
                                        header { h2 { "{chapter_title}" } button { class: "novel-reader-close", r#type: "button", onclick: move |_| reading.set(None), "×" } }
                                        div { class: "novel-reader-body", dangerous_inner_html: "{chapter_body}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Some(Err(message)) => rsx! { Shell { active: "plugins", header: "Novel", intro: "Could not load this novel.", actions: rsx! {}, ErrorPanel { message } } },
        None => rsx! { Shell { active: "plugins", header: "Novel", intro: "Loading novel…", actions: rsx! {}, LoadingPanel {} } },
    }
}

fn relative_date(value: &str) -> String {
    if value.is_empty() { return String::new(); }
    value.split('T').next().unwrap_or(value).to_owned()
}

#[component]
pub(crate) fn ContentTypeViewer(id: String) -> Element {
    let uid = id.clone();
    let content_type = use_server_future(move || load_content_type(uid.clone()))?;
    match content_type() {
        Some(Ok(content_type)) => {
            let title = content_type["display_name"].as_str().unwrap_or(&id).to_owned();
            let fields = content_type["schema"]["fields"].as_array().cloned().unwrap_or_default();
            rsx! {
                Shell { active: "datatables", header: "Content type", intro: "Content type definition", actions: rsx! {},
                    section { class: "panel content-type-viewer",
                        div { class: "content-type-viewer__header",
                            div {
                                span { class: "data-eyebrow", "CONTENT TYPE / {id}" }
                                h2 { "{title}" }
                            }
                            Link { class: "button button-quiet", to: Route::DataTables {}, "Back to data tables" }
                        }
                        div { class: "content-type-viewer__fields",
                            for field in fields.iter() {
                                {
                                    let label = field["label"].as_str().or_else(|| field["name"].as_str()).unwrap_or("Field");
                                    let name = field["name"].as_str().unwrap_or("");
                                    let kind = field["type"].as_str().unwrap_or("text");
                                    let required = field["required"].as_bool().unwrap_or(false);
                                    let relation = field["relation"].as_str().or_else(|| field["target"].as_str());
                                    rsx! {
                                        div { class: "content-type-viewer__field",
                                            span { class: "content-type-viewer__drag", "⠿" }
                                            span { class: "content-type-viewer__icon", "{field_icon(kind)}" }
                                            strong { "{label}" if required { span { class: "content-type-viewer__required", "*" } } }
                                            span { class: "content-type-viewer__name", "{name}" }
                                            span { class: "content-type-viewer__kind", "{field_label(kind, relation)}" }
                                        }
                                    }
                                }
                            }
                            if fields.is_empty() {
                                div { class: "content-type-viewer__empty", "This content type has no fields yet." }
                            }
                            div { class: "content-type-viewer__add", span { "+" } "Add another field to this collection type" }
                        }
                    }
                }
            }
        }
        Some(Err(message)) => rsx! { Shell { active: "datatables", header: "Content type", intro: "Could not load the content type.", actions: rsx! {}, ErrorPanel { message } } },
        None => rsx! { Shell { active: "datatables", header: "Content type", intro: "Loading content type…", actions: rsx! {}, LoadingPanel {} } },
    }
}

fn field_icon(kind: &str) -> &'static str {
    match kind {
        "date" | "datetime" | "time" => "▦",
        "relation" => "↗",
        "richtext" => "▤",
        "media" => "▧",
        "boolean" => "☑",
        "number" | "integer" => "#",
        _ => "Aa",
    }
}

fn field_label(kind: &str, relation: Option<&str>) -> String {
    match kind {
        "relation" => format!("Relation{}", relation.map(|target| format!(" to {target}")).unwrap_or_default()),
        "richtext" => "Rich text (Blocks)".to_owned(),
        "media" => "Media".to_owned(),
        other => {
            let mut chars = other.chars();
            chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        }
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
    let config = &definition["config"];
    let row_navigation = config["rowNavigation"]["pathTemplate"].as_str().map(str::to_owned);
    let mut actions = config["actions"].as_array().cloned().unwrap_or_default();
    if let Some(action) = config.get("createAction") {
        let mut action = action.clone();
        action["label"] = Value::String("+ Create".into());
        actions.push(action);
    }
    let mut row_actions = config["rowActions"].as_array().cloned().unwrap_or_default();
    if let Some(action) = config.get("rowAction") {
        let mut action = action.clone();
        action["label"] = Value::String("Edit".into());
        row_actions.push(action);
    }
    let mut selected = use_signal(|| None::<(i32, String)>);
    let mut initial_input = use_signal(|| "{}".to_owned());
    let view_name = definition.get("name").and_then(Value::as_str).unwrap_or("Data table");
    let content = rsx! {
            section { class: "panel plugin-view-workflows",
                div { class: "plugins-heading",
                    div { span { class: "data-eyebrow", "{plugin_name} / DATA TABLE" } h2 { "{view_name}" } }
                    for action in actions.iter() {
                        if let Some(workflow) = action.get("workflow").and_then(Value::as_str).and_then(|name| workflow_named(&workflows, name)) {
                            button { class: "button button-primary", onclick: { let input = action_input(action, &serde_json::json!({})); move |_| { initial_input.set(input.to_string()); selected.set(Some((workflow.id, "Run".to_owned()))); } }, {action["label"].as_str().unwrap_or("Run")} }
                        }
                    }
                }
                match rows() {
                    Some(Ok(rows)) => rsx! {
                        DataTable {
                            toolbar: rsx! {},
                            columns: { let mut labels=columns.iter().map(|column|column["label"].as_str().unwrap_or_default().to_owned()).collect::<Vec<_>>(); if !row_actions.is_empty(){labels.push("Actions".into());} labels },
                            row_ids: rows.iter().enumerate().map(|(index,row)|row.get("id").or_else(||row.get("uid")).map(ToString::to_string).unwrap_or(index.to_string())).collect::<Vec<_>>(),
                            empty_message: "No entries yet.".to_owned(),
                            for (index,row) in rows.iter().enumerate() {
                                {
                                    let navigation_target = row_navigation.as_deref().and_then(|template| row_navigation_path(template, row));
                                    let content_type_id = navigation_target.as_deref().and_then(|path| path.strip_prefix("/content-types/").map(str::to_owned));
                                    let novel_id = navigation_target.as_deref().and_then(|path| path.strip_prefix("/novels/").map(str::to_owned));
                                    rsx! {
                                        DataTableRow { id: row.get("id").or_else(||row.get("uid")).map(ToString::to_string).unwrap_or(index.to_string()), index,
                                            clickable: content_type_id.is_some() || novel_id.is_some(),
                                            for (column_index,column) in columns.iter().enumerate() {
                                                DataTableCell { index: column_index,
                                                    if let Some(id) = content_type_id.as_ref() {
                                                        Link { class: "plugin-table-row-link", to: Route::ContentTypeViewer { id: id.clone() }, {mapped_value(row, column.get("mapping").and_then(Value::as_str).unwrap_or_default())} }
                                                    } else if let Some(id) = novel_id.as_ref() {
                                                        Link { class: "plugin-table-row-link", to: Route::NovelViewer { id: id.clone() }, {mapped_value(row, column.get("mapping").and_then(Value::as_str).unwrap_or_default())} }
                                                    } else {
                                                        {mapped_value(row, column.get("mapping").and_then(Value::as_str).unwrap_or_default())}
                                                    }
                                                }
                                            }
                                            if !row_actions.is_empty() {
                                                DataTableCell { index: columns.len(), for action in row_actions.iter() {
                                                    if let Some(workflow) = action.get("workflow").and_then(Value::as_str).and_then(|name| workflow_named(&workflows, name)) {
                                                        button { class: "button button-quiet", onclick: { let input = action_input(action, row); move |event| { event.stop_propagation(); initial_input.set(input.to_string()); selected.set(Some((workflow.id, "Run".to_owned()))); } }, {action["label"].as_str().unwrap_or("Run")} }
                                                    }
                                                } }
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
            }
    };
    rsx! {
        if embedded { {content} } else {
            Shell { active: "plugins", header: "Plugin data table", intro: "Plugin-provided data table", actions: rsx! {}, {content} }
        }
        if let Some((workflow_id, _label)) = selected() {
            WorkflowRunDialog { workflows: workflows.clone(), selected_workflow: Some(workflow_id), initial_input: initial_input(), on_close: move |_| selected.set(None), on_complete: move |_| { rows.restart(); } }
        }
    }
}

fn row_navigation_path(template: &str, row: &Value) -> Option<String> {
    let mut path = template.to_owned();
    for key in ["uid", "id"] {
        let placeholder = format!("{{{key}}}");
        if path.contains(&placeholder) {
            let value = row.get(key)?.as_str().map(str::to_owned).or_else(|| {
                row.get(key).and_then(Value::as_i64).map(|value| value.to_string())
            })?;
            path = path.replace(&placeholder, &value);
        }
    }
    (!path.contains('{') && !path.contains('}')).then_some(path)
}

fn action_input(action: &Value, row: &Value) -> Value {
    if let Some(mapping) = action["inputMapping"].as_object() {
        Value::Object(
            mapping
                .iter()
                .map(|(key, path)| {
                    let value = path
                        .as_str()
                        .unwrap_or_default()
                        .split('.')
                        .try_fold(row, |value, part| value.get(part))
                        .cloned()
                        .unwrap_or(Value::Null);
                    (key.clone(), value)
                })
                .collect(),
        )
    } else {
        action.get("input").cloned().unwrap_or_else(|| row.clone())
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
