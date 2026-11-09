use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::Route;
use crate::{
    components::icons::Icon,
    pages::models::Workflow,
    pages::shared::{ErrorPanel, LoadingPanel, Shell},
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

fn component_label(kind: &str) -> &'static str {
    match kind {
        "form" => "Content form",
        "kanban" => "Content board",
        "gallery" => "Content gallery",
        "calendar" => "Content calendar",
        _ => "Content table",
    }
}

#[derive(Clone, Copy)]
struct PageComponentSpec {
    kind: &'static str,
    label: &'static str,
    description: &'static str,
    mark: &'static str,
}

// This is the page renderer catalog: only registered components can be saved
// into a page document, and their stable IDs are stored in the page JSON.
const PAGE_COMPONENTS: &[PageComponentSpec] = &[
    PageComponentSpec { kind: "list", label: "List", description: "Static items or workflow output", mark: "☷" },
    PageComponentSpec { kind: "html", label: "Rich text", description: "Formatted text and simple HTML", mark: "T" },
    PageComponentSpec { kind: "image", label: "Image", description: "Image with alternative text and caption", mark: "▧" },
    PageComponentSpec { kind: "heading", label: "Heading", description: "Title text", mark: "H" },
    PageComponentSpec { kind: "text", label: "Text", description: "Paragraph", mark: "T" },
    PageComponentSpec { kind: "button", label: "Button", description: "Styled action", mark: "●" },
    PageComponentSpec { kind: "divider", label: "Divider", description: "Section separator", mark: "—" },
    PageComponentSpec { kind: "spacer", label: "Spacer", description: "Vertical space", mark: "↕" },
];

fn page_component_label(kind: &str) -> &'static str {
    PAGE_COMPONENTS.iter().find(|spec| spec.kind == kind).map(|spec| spec.label).unwrap_or("Component")
}

fn validate_page_model(doc: &Value) -> Result<(), String> {
    let Some(component_value) = doc.get("components") else { return Ok(()); };
    let Some(components) = component_value.as_array() else {
        return Err("/components: expected an array".into());
    };
    let mut ids = HashSet::new();
    for (index, component) in components.iter().enumerate() {
        let pointer = format!("/components/{index}");
        let id = component.get("id").and_then(Value::as_str)
            .ok_or_else(|| format!("{pointer}/id: component needs a stable id"))?;
        if !ids.insert(id) { return Err(format!("{pointer}/id: duplicate component id `{id}`")); }
        let kind = page_component_kind(component)
            .ok_or_else(|| format!("{pointer}/component: component name is required"))?;
        if !PAGE_COMPONENTS.iter().any(|spec| spec.kind == kind) {
            return Err(format!("{pointer}/component: `{kind}` is not in the page component catalog"));
        }
        let Some(inputs) = page_component_inputs(component) else { continue; };
        if !inputs.is_object() { return Err(format!("{pointer}/inputs: expected an object")); }
        let allowed: &[(&str, &str)] = match kind {
            "list" => &[("title", "string"), ("sourceMode", "string"), ("itemsJson", "string"), ("itemsPath", "string"), ("itemLabel", "string"), ("showSearch", "boolean")],
            "html" => &[("html", "string")],
            "image" => &[("src", "string"), ("alt", "string"), ("caption", "string"), ("fit", "string")],
            "heading" | "text" => &[("text", "string")],
            "button" => &[("label", "string"), ("variant", "string"), ("action", "string"), ("href", "string"), ("eventJson", "string")],
            "spacer" => &[("height", "number")],
            "divider" => &[],
            _ => &[],
        };
        for (key, value) in inputs.as_object().into_iter().flatten() {
            let Some((_, expected)) = allowed.iter().find(|(name, _)| name == key) else {
                return Err(format!("{pointer}/inputs/{key}: input is not supported by `{kind}`"));
            };
            let valid = match *expected {
                "string" => value.is_string(),
                "boolean" => value.is_boolean(),
                "number" => value.as_u64().is_some(),
                _ => false,
            };
            if !valid { return Err(format!("{pointer}/inputs/{key}: expected {expected}")); }
        }
        if kind == "image" && inputs.get("fit").and_then(Value::as_str).is_some_and(|fit| !["cover", "contain"].contains(&fit)) {
            return Err(format!("{pointer}/inputs/fit: expected `cover` or `contain`"));
        }
        if kind == "button" && inputs.get("variant").and_then(Value::as_str).is_some_and(|variant| !["primary", "quiet"].contains(&variant)) {
            return Err(format!("{pointer}/inputs/variant: expected `primary` or `quiet`"));
        }
        if kind == "list" {
            let mode = inputs.get("sourceMode").and_then(Value::as_str).unwrap_or("static");
            if !["static", "workflow"].contains(&mode) { return Err(format!("{pointer}/inputs/sourceMode: expected `static` or `workflow`")); }
            if mode == "static" {
                let items = inputs.get("itemsJson").and_then(Value::as_str).unwrap_or("[]");
                if !serde_json::from_str::<Vec<Value>>(items).is_ok() { return Err(format!("{pointer}/inputs/itemsJson: expected a JSON array")); }
            } else if doc.pointer(&format!("/workflows/component-{id}/workflow")).is_none() {
                return Err(format!("{pointer}/inputs/sourceMode: choose a workflow for this list"));
            }
        }
        if kind == "button" {
            let action = inputs.get("action").and_then(Value::as_str).unwrap_or("none");
            if !["none", "link", "workflow"].contains(&action) { return Err(format!("{pointer}/inputs/action: unsupported button action")); }
            if action == "workflow" && doc.pointer("/workflows/pageAction/workflow").is_none() {
                return Err(format!("{pointer}/inputs/action: choose a page workflow in Page properties"));
            }
            if let Some(input) = inputs.get("eventJson").and_then(Value::as_str) {
                serde_json::from_str::<Value>(input).map_err(|parse| format!("{pointer}/inputs/eventJson: {parse}"))?;
            }
        }
    }
    Ok(())
}

async fn get_json(path: &str) -> Result<Value, String> {
    let response = reqwest::get(api_url(path)).await.map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Request failed ({status}): {}",
            response.text().await.unwrap_or_default()
        ));
    }
    response.json().await.map_err(|e| e.to_string())
}

async fn list_saved_views() -> Result<Vec<Value>, String> {
    let mut views = get_json("/api/pages")
        .await?
        .as_array()
        .cloned()
        .ok_or_else(|| "Page API returned an invalid list".to_owned())?;
    views.retain(|page| {
        !(page.get("generated").and_then(Value::as_bool) == Some(true)
            && page.get("extends").is_some_and(Value::is_null))
    });
    Ok(views)
}

#[derive(Clone, Debug, Deserialize)]
struct TypeList {
    output: Vec<Value>,
}

async fn list_content_types() -> Result<Vec<Value>, String> {
    let response =
        reqwest::get(api_url("/api/content-type-registry")).await.map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Could not load content types ({status}): {}",
            response.text().await.unwrap_or_default()
        ));
    }
    let result: TypeList = response.json().await.map_err(|e| e.to_string())?;
    Ok(result.output)
}

async fn load_view(record_id: i32) -> Result<Value, String> {
    get_json(&format!("/api/pages/{record_id}")).await
}

async fn load_content_preview(
    source: String, view: Value, record_id: Option<i32>,
) -> Result<Vec<Value>, String> {
    if source.is_empty() {
        return Ok(Vec::new());
    }
    if record_id.is_some() && view.pointer("/workflows/dataSource/workflow").is_some() {
        let view_id = view.get("id").and_then(Value::as_str).unwrap_or_default();
        let path =
            format!("/api/content-types/{source}/pages/{view_id}/workflows/dataSource/execute");
        let filters =
            view.pointer("/query/filters").cloned().unwrap_or_else(|| serde_json::json!({}));
        let sort = view.pointer("/query/sort").cloned().unwrap_or_else(|| serde_json::json!([]));
        let page_size = view.pointer("/query/pageSize").cloned().unwrap_or(Value::from(20));
        let response = reqwest::Client::new()
            .post(api_url(&path))
            .json(&serde_json::json!({
                "filters":filters,
                "sort":sort,
                "page":{"size":page_size}
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "Preview workflow failed ({status}): {}",
                response.text().await.unwrap_or_default()
            ));
        }
        return response
            .json::<Value>()
            .await
            .map_err(|e| e.to_string())?
            .pointer("/result/rows")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| "The associated workflow must map its output to a rows array.".into());
    }
    get_json(&format!("/api/content/{source}"))
        .await?
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "Content API returned an invalid preview result".into())
}

fn preview_value(value: &Value) -> String {
    match value {
        Value::Null => "—".into(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => {
            if *value {
                "Yes".into()
            } else {
                "No".into()
            }
        }
        Value::Number(value) => value.to_string(),
        _ => value.to_string(),
    }
}

#[component]
pub(crate) fn Pages() -> Element {
    let views = use_server_future(list_saved_views)?;
    rsx! {
        Shell { active: "pages", header: "Pages", intro: "Create and manage workspace pages.", actions: rsx! {
            Link { class: "button button-primary", to: Route::NewPage {}, Icon { name: "plus" } " Create page" }
        },
            match views() {
                Some(Ok(rows)) if rows.is_empty() => rsx! {
                    section { class: "resource-empty panel",
                        span { class: "resource-empty__icon", Icon { name: "table" } }
                        h2 { "No pages yet" }
                        p { "Start with a built-in content page and customize only what you need." }
                        Link { class: "button button-primary", to: Route::NewPage {}, "Create page" }
                    }
                },
                Some(Ok(rows)) => rsx! {
                    section { class: "panel content-views-panel",
                        div { class: "content-views-panel__header",
                            div { span { class: "data-eyebrow", "WORKSPACE PAGES" } h2 { "Pages" } p { "Each page inherits its component defaults and stores only your changes." } }
                            span { class: "content-views-count", "{rows.len()} pages" }
                        }
                        div { class: "shared-table-shell shared-table-scroll",
                            table { class: "shared-data-table",
                                thead { tr { th { "Page" } th { "Content type" } th { "Component" } th { "" } } }
                                tbody {
                                    for view in rows.iter() {
                                        { let id=view.get("recordId").and_then(Value::as_i64).unwrap_or_default() as i32;
                                          let name=view.get("name").and_then(Value::as_str).unwrap_or("Untitled page").to_owned();
                                          let source=view.get("source").and_then(Value::as_str).unwrap_or("—").to_owned();
                                          let kind=if source=="core.page" { "Static page" } else { component_label(view.get("type").and_then(Value::as_str).unwrap_or("grid")) };
                                          let is_default=view.get("isDefault").and_then(Value::as_bool).unwrap_or(false);
                                          let generated=view.get("generated").and_then(Value::as_bool).unwrap_or(false);
                                          rsx! {
                                            tr { class: "shared-table-row",
                                                td { strong { "{name}" } if is_default { span { class: "content-view-badge", "Default" } } if generated { span { class: "content-view-badge content-view-badge--muted", "Generated" } } }
                                                td { if source=="core.page" { "Standalone" } else { code { "{source}" } } }
                                                td { span { class: "content-view-type", "{kind}" } }
                                                td { Link { class: "button button-quiet", to: Route::EditPage { record_id: id }, "Edit page" } }
                                            }
                                          }
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
    }
}

#[component]
pub(crate) fn NewPage() -> Element {
    let types = use_server_future(list_content_types)?;
    match types() {
        Some(Ok(types)) => {
            let initial = blank_page_document();
            rsx! { ViewEditor { record_id: None, initial, types } }
        }
        Some(Err(message)) => {
            rsx! { Shell { active: "pages", header: "Create page", intro: "Could not load content types.", actions: rsx! {}, ErrorPanel { message } } }
        }
        None => {
            rsx! { Shell { active: "pages", header: "Create page", intro: "Loading content types…", actions: rsx! {}, LoadingPanel {} } }
        }
    }
}

fn blank_page_document() -> Value {
    serde_json::json!({
        "modelVersion":1,"id":"core.page.pages.new-page","source":"core.page","type":"grid","extends":null,
        "name":"New page","workflows":{},"options":{"showContent":false},"components":[]
    })
}

#[component]
pub(crate) fn EditPage(record_id: i32) -> Element {
    let types = use_server_future(list_content_types)?;
    let view = use_server_future(move || load_view(record_id))?;
    match (view(), types()) {
        (Some(Ok(initial)), Some(Ok(types))) => {
            rsx! { ViewEditor { record_id: Some(record_id), initial, types } }
        }
        (Some(Err(message)), _) | (_, Some(Err(message))) => {
            rsx! { Shell { active: "pages", header: "Edit page", intro: "Could not load this page.", actions: rsx! {}, ErrorPanel { message } } }
        }
        _ => {
            rsx! { Shell { active: "pages", header: "Edit page", intro: "Loading page…", actions: rsx! {}, LoadingPanel {} } }
        }
    }
}

fn default_view_document(content_type: &Value) -> Value {
    let source = content_type.get("uid").and_then(Value::as_str).unwrap_or_default();
    let title = content_type.get("display_name").and_then(Value::as_str).unwrap_or(source);
    serde_json::json!({
        "modelVersion":1,"id":format!("{source}.pages.{}", page_slug(title)),"source":source,"type":"grid","extends":format!("{source}#base"),
        "name":format!("{title} page"),"workflows":{},"components":[]
    })
}

fn page_slug(title: &str) -> String {
    let slug =
        title
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '-' }
            })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
    if slug.is_empty() { "page".into() } else { slug }
}

fn page_component_default(component_type: &str, property: &str) -> Value {
    match (component_type, property) {
        ("heading", "text") => Value::String("Page heading".into()),
        ("text", "text") => Value::String("Add your text here".into()),
        ("button", "label") => Value::String("Button".into()),
        ("button", "variant") => Value::String("primary".into()),
        ("button", "href") => Value::String(String::new()),
        ("spacer", "height") => Value::from(24),
        ("list", "title") => Value::String("List".into()),
        ("list", "sourceMode") => Value::String("static".into()),
        ("list", "itemsJson") => Value::String("[\"First item\", \"Second item\", \"Third item\"]".into()),
        ("list", "itemsPath") => Value::String("items".into()),
        ("list", "itemLabel") => Value::String("name".into()),
        ("list", "showSearch") => Value::Bool(false),
        ("html", "html") => {
            Value::String("<h2>Welcome</h2>\n<p>Write your page content here.</p>".into())
        }
        ("image", "src") => Value::String(String::new()),
        ("image", "alt") => Value::String(String::new()),
        ("image", "caption") => Value::String(String::new()),
        ("image", "fit") => Value::String("cover".into()),
        ("button", "action") => Value::String("none".into()),
        ("button", "eventJson") => Value::String("{}".into()),
        _ => Value::Null,
    }
}

// Page components follow the same contract as Webiny editor components: a stable
// registry name identifies the renderer, while `inputs` stores editor overrides.
// Read the earlier local `type`/`props` shape so existing pages remain editable.
fn page_component_kind(component: &Value) -> Option<&str> {
    component.get("component").and_then(Value::as_str)
        .or_else(|| component.get("type").and_then(Value::as_str))
        .map(|name| name.strip_prefix("ferrisflow/").unwrap_or(name))
        .map(|name| match name { "workflow-list" => "list", "workflow-button" => "button", other => other })
}

fn page_component_inputs(component: &Value) -> Option<&Value> {
    component.get("inputs").or_else(|| component.get("props"))
}

fn normalize_page_document(mut doc: Value) -> Value {
    if !doc.is_object() { return doc; }
    doc["modelVersion"] = Value::from(1);
    if let Some(components) = doc.get_mut("components").and_then(Value::as_array_mut) {
        for component in components {
            if let Some(kind) = component.get("type").and_then(Value::as_str).map(str::to_owned) {
                if component.get("component").is_none() {
                    let kind = match kind.as_str() { "workflow-list" => "list", "workflow-button" => "button", _ => kind.as_str() };
                    component["component"] = Value::String(format!("ferrisflow/{kind}"));
                }
                if let Some(object) = component.as_object_mut() { object.remove("type"); }
            }
            if let Some(name) = component.get("component").and_then(Value::as_str).map(str::to_owned) {
                if name == "ferrisflow/workflow-list" { component["component"] = Value::String("ferrisflow/list".into()); }
                if name == "ferrisflow/workflow-button" { component["component"] = Value::String("ferrisflow/button".into()); }
            }
            if component.get("inputs").is_none() {
                if let Some(inputs) = component.get("props").cloned() {
                    component["inputs"] = inputs;
                    if let Some(object) = component.as_object_mut() { object.remove("props"); }
                }
            }
            if page_component_kind(component) == Some("list") {
                if component.pointer("/inputs/sourceMode").is_none() { component["inputs"]["sourceMode"] = Value::String("static".into()); }
                if component.pointer("/inputs/showSearch").is_none() { component["inputs"]["showSearch"] = Value::Bool(false); }
                if component.get("id").and_then(Value::as_str).is_some_and(|id| id.starts_with("workflow-list-"))
                    && component.pointer("/inputs/itemsJson").is_none() {
                    component["inputs"]["itemsJson"] = Value::String("[]".into());
                }
            }
            if page_component_kind(component) == Some("button") {
                if component.pointer("/inputs/action").is_none() {
                    let legacy_workflow_button = component.get("id").and_then(Value::as_str).is_some_and(|id| id.starts_with("workflow-button-"));
                    let old_link = component.pointer("/inputs/href").and_then(Value::as_str).is_some_and(|href| !href.is_empty());
                    component["inputs"]["action"] = Value::String(if legacy_workflow_button { "workflow" } else if old_link { "link" } else { "none" }.into());
                }
                if let Some(input) = component.pointer("/inputs/inputJson").cloned() {
                    component["inputs"]["eventJson"] = input;
                    if let Some(inputs) = component.get_mut("inputs").and_then(Value::as_object_mut) { inputs.remove("inputJson"); }
                }
            }
        }
    }
    doc
}

fn page_component_type(doc: &Value, id: &str) -> Option<String> {
    let component = doc.get("components")?
        .as_array()?
        .iter()
        .find(|component| component.get("id").and_then(Value::as_str) == Some(id))?;
    page_component_kind(component).map(str::to_owned)
}

fn page_component_property(doc: &Value, id: &str, property: &str) -> Value {
    let Some(component) = doc.get("components").and_then(Value::as_array).and_then(|items| {
        items.iter().find(|item| item.get("id").and_then(Value::as_str) == Some(id))
    }) else {
        return Value::Null;
    };
    page_component_inputs(component)
        .and_then(|inputs| inputs.get(property))
        .cloned().unwrap_or_else(|| {
        page_component_default(page_component_kind(component).unwrap_or_default(), property)
    })
}

fn page_canvas_order(doc: &Value) -> Vec<String> {
    let mut order = if doc.pointer("/options/showContent").and_then(Value::as_bool).unwrap_or(true)
    {
        vec!["content".to_owned()]
    } else {
        Vec::new()
    };
    let components = doc.get("components").and_then(Value::as_array).cloned().unwrap_or_default();
    order.extend(
        components
            .iter()
            .filter_map(|component| component.get("id").and_then(Value::as_str).map(str::to_owned)),
    );
    if let Some(saved) = doc.pointer("/layout/order").and_then(Value::as_array) {
        let valid = order.iter().cloned().collect::<std::collections::HashSet<_>>();
        let mut resolved = saved
            .iter()
            .filter_map(Value::as_str)
            .filter(|id| valid.contains(*id))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for id in order {
            if !resolved.contains(&id) {
                resolved.push(id);
            }
        }
        return resolved;
    }
    order
}

fn set_page_canvas_order(mut doc: Value, order: &[String]) -> Value {
    let mut default_order =
        if doc.pointer("/options/showContent").and_then(Value::as_bool).unwrap_or(true) {
            vec!["content".to_owned()]
        } else {
            Vec::new()
        };
    default_order.extend(
        doc.get("components")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|component| component.get("id").and_then(Value::as_str).map(str::to_owned)),
    );
    if order == default_order {
        let remove_layout = if let Some(layout) = doc.get_mut("layout") {
            if let Some(layout) = layout.as_object_mut() {
                layout.remove("order");
            }
            layout.as_object().is_some_and(|object| object.is_empty())
        } else {
            false
        };
        if remove_layout {
            doc.as_object_mut().map(|object| {
                object.remove("layout");
            });
        }
    } else {
        if !doc.get("layout").is_some_and(Value::is_object) {
            doc["layout"] = serde_json::json!({});
        }
        doc["layout"]["order"] = serde_json::json!(order);
    }
    doc
}

fn add_page_component(doc: Value, component_type: &str, index: usize) -> (Value, String) {
    let mut next = doc;
    let components = next["components"].as_array_mut();
    if components.is_none() {
        next["components"] = serde_json::json!([]);
    }
    let components = next["components"].as_array_mut().expect("components array initialized");
    let mut suffix = components.len() + 1;
    let id = loop {
        let id = format!("{component_type}-{suffix}");
        if components
            .iter()
            .all(|component| component.get("id").and_then(Value::as_str) != Some(id.as_str()))
        {
            break id;
        }
        suffix += 1;
    };
    components.push(serde_json::json!({"id":id,"component":format!("ferrisflow/{component_type}")}));
    let mut order = page_canvas_order(&next);
    let current = order.iter().position(|item| item == &id).unwrap_or(order.len() - 1);
    let id = order.remove(current);
    order.insert(index.min(order.len()), id.clone());
    (set_page_canvas_order(next, &order), id)
}

fn move_page_component(doc: Value, id: &str, target: usize) -> Value {
    let mut order = page_canvas_order(&doc);
    let Some(current) = order.iter().position(|item| item == id) else {
        return doc;
    };
    let item = order.remove(current);
    order.insert(target.min(order.len()), item);
    set_page_canvas_order(doc, &order)
}

fn remove_page_component(mut doc: Value, id: &str) -> Value {
    if let Some(components) = doc.get_mut("components").and_then(Value::as_array_mut) {
        components.retain(|component| component.get("id").and_then(Value::as_str) != Some(id));
        if components.is_empty() {
            doc.as_object_mut().map(|object| {
                object.remove("components");
            });
        }
    }
    let order = page_canvas_order(&doc).into_iter().filter(|item| item != id).collect::<Vec<_>>();
    set_page_canvas_order(doc, &order)
}

fn set_page_component_property(mut doc: Value, id: &str, property: &str, value: Value) -> Value {
    let Some(components) = doc.get_mut("components").and_then(Value::as_array_mut) else {
        return doc;
    };
    let Some(component) = components
        .iter_mut()
        .find(|component| component.get("id").and_then(Value::as_str) == Some(id))
    else {
        return doc;
    };
    let component_type = page_component_kind(component).unwrap_or_default().to_owned();
    if value == page_component_default(&component_type, property) {
        for key in ["inputs", "props"] {
            if let Some(props) = component.get_mut(key).and_then(Value::as_object_mut) {
                props.remove(property);
            }
            if component.get(key).and_then(Value::as_object).is_some_and(|props| props.is_empty()) {
                component.as_object_mut().map(|object| { object.remove(key); });
            }
        }
    } else {
        component["inputs"][property] = value;
        if let Some(object) = component.as_object_mut() { object.remove("props"); }
    }
    doc
}

fn remove_page_json_pointer(doc: &mut Value, pointer: &str) {
    let segments = pointer.split('/').skip(1).collect::<Vec<_>>();
    if segments.is_empty() {
        return;
    }
    for depth in (1..=segments.len()).rev() {
        let path = format!("/{}", segments[..depth].join("/"));
        let empty =
            doc.pointer(&path).and_then(Value::as_object).is_some_and(|object| object.is_empty());
        if depth == segments.len() || empty {
            let parent_path = if depth == 1 {
                "".to_owned()
            } else {
                format!("/{}", segments[..depth - 1].join("/"))
            };
            let key = segments[depth - 1];
            let parent = if parent_path.is_empty() {
                Some(&mut *doc)
            } else {
                doc.pointer_mut(&parent_path)
            };
            if let Some(parent) = parent {
                if let Some(parent) = parent.as_object_mut() {
                    parent.remove(key);
                }
            }
        }
    }
}

fn set_sparse_page_setting(mut doc: Value, pointer: &str, value: Value, default: Value) -> Value {
    if value == default {
        remove_page_json_pointer(&mut doc, pointer);
        return doc;
    }
    let segments = pointer.split('/').skip(1).collect::<Vec<_>>();
    if segments.is_empty() {
        return doc;
    }
    let mut target = &mut doc;
    for segment in &segments[..segments.len() - 1] {
        if !target.get(*segment).is_some_and(Value::is_object) {
            target[*segment] = serde_json::json!({});
        }
        target = &mut target[*segment];
    }
    target[segments[segments.len() - 1]] = value;
    doc
}

fn document_text(doc: &Value) -> String {
    let mut doc = doc.clone();
    if let Some(object) = doc.as_object_mut() {
        object.remove("recordId");
        object.remove("updatedAt");
    }
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

fn update_document(doc: &mut Signal<Value>, text: &mut Signal<String>, next: Value) {
    text.set(document_text(&next));
    doc.set(next);
}

#[derive(Clone, PartialEq)]
struct WorkflowChoice {
    key: String,
    label: String,
    reference: Value,
}

fn view_workflow_choices(workflows: Vec<Workflow>) -> Vec<WorkflowChoice> {
    let mut choices = workflows
        .into_iter()
        .filter(|workflow| workflow.active)
        .filter_map(|workflow| {
            let definition: Value = serde_json::from_str(&workflow.definition_json).ok()?;
            if definition.pointer("/document/metadata/ferrisflow/contentType").and_then(Value::as_str)
                == Some("core.page")
            {
                return None;
            }
            let exposed = definition
                .pointer("/document/metadata/exposedTo")
                .and_then(Value::as_array)
                .is_some_and(|targets| targets.iter().any(|target| target.as_str() == Some("view")));
            if !exposed { return None; }
            let document = definition.get("document")?;
            let namespace = document.get("namespace")?.as_str()?;
            let name = document.get("name")?.as_str()?;
            let version = document.get("version")?.as_str()?;
            let mut version_parts = version.split('.');
            let major = version_parts.next()?;
            let minor = version_parts.next().unwrap_or("0");
            Some(WorkflowChoice {
                key: format!("{namespace}/{name}"),
                label: format!("{} · {namespace}/{name} · v{version}", workflow.name),
                reference: serde_json::json!({"namespace":namespace,"name":name,"version":format!("^{major}.{minor}")}),
            })
        })
        .collect::<Vec<_>>();
    choices.sort_by(|left, right| left.label.cmp(&right.label));
    choices
}

fn set_associated_workflow(
    mut doc: Value, slot: &str, selected: &str, choices: &[WorkflowChoice],
) -> Value {
    if selected.is_empty() {
        if let Some(config) = doc.pointer_mut(&format!("/workflows/{slot}")) {
            if let Some(config) = config.as_object_mut() {
                config.remove("workflow");
            }
        }
        return doc;
    }
    let Some(choice) = choices.iter().find(|choice| choice.key == selected) else {
        return doc;
    };
    let config = &mut doc["workflows"][slot];
    if !config.is_object() {
        *config = if slot == "dataSource" {
            serde_json::json!({
                "input":{"from":{"uid":"${ $view.source }","filters":"${ $filters }","sort":"${ $sort }","page":"${ $page }"}},
                "output":{"as":{"rows":"${ .rows }","total":"${ .total }"}}
            })
        } else if slot.starts_with("component-") {
            serde_json::json!({
                "input":{"from":"${ $event }"},
                "output":{"as":"${ . }"}
            })
        } else {
            serde_json::json!({
                "input":{"from":{"uid":"${ $view.source }","data":"${ $form }"}},
                "output":{"as":"${ . }"}
            })
        };
    }
    config["workflow"] = choice.reference.clone();
    doc
}

fn set_component_workflow(
    doc: Value, component_id: &str, selected: &str, choices: &[WorkflowChoice],
) -> Value {
    let slot = format!("component-{component_id}");
    let mut next = set_associated_workflow(doc, &slot, selected, choices);
    if selected.is_empty() {
        if let Some(workflows) = next.get_mut("workflows").and_then(Value::as_object_mut) { workflows.remove(&slot); }
    }
    next = set_page_component_property(
        next,
        component_id,
        "sourceMode",
        Value::String(if selected.is_empty() { "static" } else { "workflow" }.into()),
    );
    next
}

fn set_component_workflow_input(mut doc: Value, component_id: &str, mapping: Value) -> Value {
    let slot = format!("component-{component_id}");
    doc["workflows"][slot]["input"]["from"] = mapping;
    doc
}

fn set_page_action_workflow(mut doc: Value, selected: &str, choices: &[WorkflowChoice]) -> Value {
    if selected.is_empty() {
        if let Some(workflows) = doc.get_mut("workflows").and_then(Value::as_object_mut) {
            workflows.remove("pageAction");
        }
        return doc;
    }
    let Some(choice) = choices.iter().find(|choice| choice.key == selected) else {
        return doc;
    };
    doc["workflows"]["pageAction"] = serde_json::json!({
        "workflow":choice.reference,
        "input":{"from":"${ $event }"},
        "output":{"as":"${ . }"}
    });
    doc
}

#[component]
fn ViewEditor(record_id: Option<i32>, initial: Value, types: Vec<Value>) -> Element {
    let initial = normalize_page_document(initial);
    let workflow_list = use_server_future(super::api::list_workflows)?;
    let mut doc = use_signal(|| initial.clone());
    let mut editor_text = use_signal(|| document_text(&initial));
    let mut active_tab = use_signal(|| "build".to_owned());
    let mut add_modal = use_signal(|| false);
    let selected_component = use_signal(|| "page".to_owned());
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let current = doc();
    let id = current.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let source = current.get("source").and_then(Value::as_str).unwrap_or_default().to_owned();
    let kind = current.get("type").and_then(Value::as_str).unwrap_or("grid").to_owned();
    let workflow_choices =
        workflow_list().and_then(Result::ok).map(view_workflow_choices).unwrap_or_default();
    let associated_slot = if kind == "form" { "onSubmit" } else { "dataSource" };
    let associated_ref = current.pointer(&format!("/workflows/{associated_slot}/workflow"));
    let associated_key = associated_ref
        .and_then(|reference| {
            Some(format!(
                "{}/{}",
                reference.get("namespace")?.as_str()?,
                reference.get("name")?.as_str()?
            ))
        })
        .unwrap_or_default();
    let has_associated_workflow = !associated_key.is_empty()
        || (kind == "form" && current.pointer("/workflows/onSave/workflow").is_some());
    let requires_data_workflow =
        current.pointer("/options/showContent").and_then(Value::as_bool).unwrap_or(true);
    let workflow_ready = !requires_data_workflow || has_associated_workflow;
    let available_fields = types
        .iter()
        .find(|item| item.get("uid").and_then(Value::as_str) == Some(source.as_str()))
        .and_then(|item| item.pointer("/schema/fields"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    rsx! {
        Shell { active: "pages", header: "", intro: "", actions: rsx! {},
            div { class: "view-editor-layout",
                section { class: "view-editor-main",
                    div { class: "view-editor-modebar",
                        div { class: "view-editor-tabs", role: "tablist", aria_label: "View editor mode",
                            button { class: if active_tab()=="build" { "view-editor-tab active" } else { "view-editor-tab" }, role: "tab", aria_selected: "{active_tab()==\"build\"}", onclick: move |_| active_tab.set("build".into()), "Build" }
                            button { class: if active_tab()=="preview" { "view-editor-tab active" } else { "view-editor-tab" }, role: "tab", aria_selected: "{active_tab()==\"preview\"}", onclick: move |_| active_tab.set("preview".into()), "Preview" }
                            button { class: if active_tab()=="text" { "view-editor-tab active" } else { "view-editor-tab" }, role: "tab", aria_selected: "{active_tab()==\"text\"}", onclick: move |_| active_tab.set("text".into()), "JSON" }
                        }
                        if active_tab()=="build" { button { class: "button button-primary page-editor-add", r#type: "button", onclick: move |_| add_modal.set(true), "+ Add" } }
                    }
                    if active_tab()=="build" || active_tab()=="preview" {
                        PageBuilder { doc: current.clone(), fields: available_fields.clone(), record_id, types: types.clone(), workflow_choices: workflow_choices.clone(), workflow_loading: workflow_list().is_none(), is_editing: record_id.is_some(), editable: active_tab()=="build", selected_component, add_modal, on_change: move |next| update_document(&mut doc,&mut editor_text,next) }
                    } else if active_tab()=="text" {
                        div { class: "view-editor-config",
                            label { class: "field", span { "Page JSON" } }
                            textarea { class: "view-json-editor", value: "{editor_text}", spellcheck: "false", oninput: move |event| {
                                let text=event.value(); editor_text.set(text.clone());
                                match serde_json::from_str::<Value>(&text) {
                                    Ok(mut next) if next.is_object()=>{if let Some(id)=doc().get("recordId").cloned(){next["recordId"]=id;} doc.set(next);error.set(None)},
                                    Ok(_)=>error.set(Some("View document must be a JSON object.".into())),
                                    Err(parse_error)=>error.set(Some(parse_error.to_string()))
                                }
                            } }
                            p { class: "page-builder-hint", "Default component settings are inherited. This document contains only page metadata and overrides." }
                        }
                    }
                    if let Some(message)=error() { div { class: "error-panel", "{message}" } }
                    div { class: "view-editor-actions",
                        Link { class: "button button-quiet", to: Route::Pages {}, "Cancel" }
                        button { class: "button button-primary", disabled: saving() || error().is_some() || source.is_empty() || id.is_empty() || !workflow_ready, onclick: move |_| {
                            let mut saved=normalize_page_document(doc());
                            if let Err(message)=validate_page_model(&saved) { error.set(Some(message)); return; }
                            saved.as_object_mut().map(|object|{object.remove("recordId");object.remove("updatedAt");}); saving.set(true); let record_id=record_id; let selected_source=source.clone();
                            spawn(async move {
                                let client=reqwest::Client::new();
                                let result=async {
                                    let url=if let Some(id)=record_id { api_url(&format!("/api/pages/{id}")) } else { api_url(&format!("/api/content-types/{selected_source}/pages")) };
                                    let method=if record_id.is_some(){client.put(url)}else{client.post(url)};
                                    let response=method.json(&saved).send().await.map_err(|e|e.to_string())?; let status=response.status();
                                    if status.is_success(){Ok(())}else{Err(format!("Save failed ({status}): {}",response.text().await.unwrap_or_default()))}
                                }.await;
                                match result {Ok(())=>{navigator().push(Route::Pages {});},Err(message)=>error.set(Some(message))} saving.set(false);
                            });
                        }, if saving() { "Saving…" } else if record_id.is_some() { "Save page" } else { "Create page" } }
                    }
                }
            }
        }
    }
}

#[component]
fn PageBuilder(
    doc: Value, fields: Vec<Value>, record_id: Option<i32>, types: Vec<Value>,
    workflow_choices: Vec<WorkflowChoice>, workflow_loading: bool, is_editing: bool,
    editable: bool, selected_component: Signal<String>, add_modal: Signal<bool>,
    on_change: EventHandler<Value>,
) -> Element {
    let order = page_canvas_order(&doc);
    let order_len = order.len();
    let selected = selected_component();
    rsx! {
        div { class: "page-builder-workspace",
            main { class: "page-builder-live-area",
                section { class: if editable { "page-builder-live-preview editing" } else { "page-builder-live-preview" }, onclick: { let mut selected_component=selected_component; move |event| { if editable { event.stop_propagation(); selected_component.set("page".into()); } } },
                    for (index, component_id) in order.iter().enumerate() {
                        { let id=component_id.clone(); let is_selected=selected==id; let selected_component=selected_component; let on_change=on_change.clone(); let id_for_drag=id.clone();
                            let node_label=if id=="content" { component_label(doc.get("type").and_then(Value::as_str).unwrap_or("grid")).to_owned() } else { page_component_type(&doc,&id).map(|kind|page_component_label(&kind).to_owned()).unwrap_or_else(||"Component".into()) };
                            rsx! {
                                div { key: "{id}", class: if editable && is_selected { "page-live-element selected" } else { "page-live-element" }, draggable: editable,
                                    onclick: { let id_for_click=id.clone(); let mut selected_component=selected_component; move |event| { if editable { event.stop_propagation(); selected_component.set(id_for_click.clone()); } } },
                                    ondragstart: move |event| { if editable { let _=event.data_transfer().set_data("text/plain",&id_for_drag); } },
                                    ondragover: move |event| if editable { event.prevent_default() },
                                    ondrop: { let doc=doc.clone(); let on_change=on_change.clone(); let id=id.clone(); let mut selected_component=selected_component; move |event| { if editable { event.prevent_default(); event.stop_propagation(); let payload=event.data_transfer().get_data("text/plain").unwrap_or_default(); let order=page_canvas_order(&doc); let target=order.iter().position(|item|item==&id).unwrap_or(order.len()); if order.contains(&payload) { let next=move_page_component(doc.clone(),&payload,target); selected_component.set(payload); on_change.call(next); } } } },
                                    if editable && is_selected { div { class: "page-live-element-tag", span { "{node_label}" } span { "{index+1}/{order_len}" } } }
                                    if id == "content" { ViewPreview { doc: doc.clone(), fields: fields.clone(), record_id } }
                                    else { PageComponentPreview { doc: doc.clone(), id: id.clone(), editable, record_id } }
                                }
                            }
                        }
                    }
                }
                if add_modal() { div { class: "page-add-modal-backdrop", role: "presentation", onclick: move |_| add_modal.set(false),
                    section { class: "page-add-modal", role: "dialog", aria_modal: "true", aria_label: "Add a component", onclick: move |event| event.stop_propagation(),
                        header { h2 { "Add component" } button { class: "page-add-modal-close", r#type: "button", aria_label: "Close", onclick: move |_| add_modal.set(false), "×" } }
                        p { "Choose a component to add to your page." }
                        div { class: "page-add-options",
                            for spec in PAGE_COMPONENTS.iter() {
                                { let component_type=spec.kind; let label=spec.label; let description=spec.description; let mark=spec.mark;
                                rsx! { button { class: "page-add-option", r#type: "button", onclick: { let doc=doc.clone(); let on_change=on_change.clone(); let mut selected_component=selected_component; let mut add_modal=add_modal; move |_| { let order=page_canvas_order(&doc); let (next,id)=add_page_component(doc.clone(),component_type,order.len()); on_change.call(next); selected_component.set(id); add_modal.set(false); } },
                                    span { class: "page-component-option__mark", "{mark}" }
                                    span { class: "page-component-option__copy", strong { "{label}" } small { "{description}" } }
                                    span { class: "page-component-option__plus", "+" }
                                } }
                                }
                            }
                        }
                    }
                } }
            }
            if editable { aside { class: "page-component-inspector",
                span { class: "data-eyebrow", "PROPERTIES" }
                PageInspector { doc: doc.clone(), fields: fields.clone(), selected_id: selected, selected_component, types: types.clone(), workflow_choices: workflow_choices.clone(), workflow_loading, is_editing, on_change }
            } }
        }
    }
}

#[component]
fn PageInspector(
    doc: Value, fields: Vec<Value>, selected_id: String, selected_component: Signal<String>,
    types: Vec<Value>, workflow_choices: Vec<WorkflowChoice>, workflow_loading: bool,
    is_editing: bool, on_change: EventHandler<Value>,
) -> Element {
    let workflow_input_draft = use_signal(|| (String::new(), None::<String>));
    if selected_id == "page" {
        return rsx! { PageSettings { doc, types, workflow_choices, workflow_loading, is_editing, on_change } };
    }
    if selected_id == "content" {
        let kind = doc.get("type").and_then(Value::as_str).unwrap_or("grid").to_owned();
        let page_size = doc
            .pointer("/query/pageSize")
            .and_then(Value::as_u64)
            .map(|size| size.to_string())
            .unwrap_or_default();
        let overrides_count =
            doc.get("fields").and_then(Value::as_object).map_or(0, serde_json::Map::len);
        return rsx! {
            div { class: "page-inspector-content",
                h3 { "{component_label(&kind)}" }
                p { "Built-in component and workspace controls." }
                details { class: "page-overrides",
                    summary { "Fields" span { "{overrides_count} overrides" } }
                    if fields.is_empty() { p { class: "page-builder-hint", "No fields in this content type." } }
                    else { div { class: "page-field-overrides", for field in fields.iter() { PageFieldOverride { doc: doc.clone(), field: field.clone(), on_change } } }
                    }
                }
                if kind == "grid" { label { class: "page-size-override", span { "Rows per page" }
                    input { r#type: "number", min: "1", max: "200", placeholder: "Default · 20", value: "{page_size}", oninput: { let doc=doc.clone(); let on_change=on_change.clone(); move |event| { let mut next=doc.clone(); if let Ok(size)=event.value().parse::<u64>() { next["query"]["pageSize"]=Value::from(size.clamp(1,200)); } else { let remove_query=if let Some(query)=next.get_mut("query").and_then(Value::as_object_mut) { query.remove("pageSize"); query.is_empty() } else { false }; if remove_query { next.as_object_mut().map(|object|{object.remove("query");}); } } on_change.call(next); } } }
                } }
                p { class: "page-builder-hint", "Only changed values are saved to the page JSON." }
            }
        };
    }
    let component_type = page_component_type(&doc, &selected_id).unwrap_or_default();
    if component_type == "list" {
        let title = page_component_property(&doc, &selected_id, "title").as_str().unwrap_or("List").to_owned();
        let source_mode = page_component_property(&doc, &selected_id, "sourceMode").as_str().unwrap_or("static").to_owned();
        let items_json = page_component_property(&doc, &selected_id, "itemsJson").as_str().unwrap_or("[]").to_owned();
        let items_path = page_component_property(&doc, &selected_id, "itemsPath").as_str().unwrap_or("items").to_owned();
        let item_label = page_component_property(&doc, &selected_id, "itemLabel").as_str().unwrap_or("name").to_owned();
        let show_search = page_component_property(&doc, &selected_id, "showSearch").as_bool().unwrap_or(false);
        let slot = format!("component-{selected_id}");
        let selected_workflow = doc.pointer(&format!("/workflows/{slot}/workflow")).and_then(|reference| {
            Some(format!("{}/{}", reference.get("namespace")?.as_str()?, reference.get("name")?.as_str()?))
        }).unwrap_or_default();
        let workflow_input = doc.pointer(&format!("/workflows/{slot}/input/from")).cloned().unwrap_or_else(||serde_json::json!("${ $event }"));
        let workflow_input_default=serde_json::to_string_pretty(&workflow_input).unwrap_or_else(|_|"\"${ $event }\"".into());
        let (draft_id,draft_text)=workflow_input_draft();
        let workflow_input_text=if draft_id==selected_id { draft_text.unwrap_or(workflow_input_default) } else { workflow_input_default };
        return rsx! { div { class: "page-inspector-component",
            h3 { "List" }
            label { class: "field", span { "Title" }
                input { value: "{title}", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"title",Value::String(event.value()))) }
            } }
            label { class: "field", span { "Source" }
                select { value: "{source_mode}", onchange: { let doc=doc.clone(); let id=selected_id.clone(); let choices=workflow_choices.clone(); let on_change=on_change.clone(); move |event| {
                    if event.value()=="workflow" {
                        let current=doc.pointer(&format!("/workflows/component-{id}/workflow")).and_then(|reference|Some(format!("{}/{}",reference.get("namespace")?.as_str()?,reference.get("name")?.as_str()?))).unwrap_or_default();
                        let mut next=set_page_component_property(doc.clone(),&id,"sourceMode",Value::String("workflow".into()));
                        if !current.is_empty() { next=set_component_workflow(next,&id,&current,&choices); }
                        on_change.call(next);
                    } else {
                        let mut next=set_page_component_property(doc.clone(),&id,"sourceMode",Value::String("static".into()));
                        if let Some(workflows)=next.get_mut("workflows").and_then(Value::as_object_mut) { workflows.remove(&format!("component-{id}")); }
                        on_change.call(next);
                    }
                } }, option { value: "static", "Static items" } option { value: "workflow", "Workflow output" } }
            }
            if source_mode=="workflow" {
                label { class: "field", span { "Workflow" }
                    select { value: "{selected_workflow}", disabled: workflow_loading, onchange: { let doc=doc.clone(); let id=selected_id.clone(); let choices=workflow_choices.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_component_workflow(doc.clone(),&id,&event.value(),&choices)) },
                        option { value: "", "Select workflow" }
                        for choice in workflow_choices.iter() { option { value: "{choice.key}", "{choice.label}" } }
                    }
                }
                label { class: "field", span { "Items in output" }
                    input { value: "{items_path}", placeholder: "items or rows", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"itemsPath",Value::String(event.value()))) }
                } }
                label { class: "field", span { "Workflow input mapping (JSON)" }
                    textarea { value: "{workflow_input_text}", spellcheck: "false", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); let mut draft=workflow_input_draft; move |event| { let text=event.value(); draft.set((id.clone(),Some(text.clone()))); if let Ok(mapping)=serde_json::from_str::<Value>(&text) { on_change.call(set_component_workflow_input(doc.clone(),&id,mapping)); } } }
                } }
                if serde_json::from_str::<Value>(&workflow_input_text).is_err() { small { class: "page-setting-warning", "Enter valid JSON. String values may contain jq expressions for the event context." } }
            } else {
                label { class: "field", span { "Items (JSON array)" }
                    textarea { value: "{items_json}", spellcheck: "false", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"itemsJson",Value::String(event.value()))) }
                } }
            }
            label { class: "field", span { "Item label field" }
                input { value: "{item_label}", placeholder: "name", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"itemLabel",Value::String(event.value()))) }
            } }
            label { class: "page-setting-row page-setting-toggle", span { "Show search" }
                input { r#type: "checkbox", checked: show_search, onchange: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"showSearch",Value::Bool(event.checked()))) }
            } }
            p { class: "page-builder-hint", if source_mode=="workflow" { "The list reads items from the selected workflow's output." } else { "Edit the static list as a JSON array of strings or objects." } }
            button { class: "button button-quiet page-component-remove", r#type: "button", onclick: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); let mut selected_component=selected_component; move |_| { on_change.call(remove_page_component(doc.clone(),&id)); selected_component.set("page".into()); } }, "Remove component" }
        } };
    }
    if component_type == "html" {
        let html = page_component_property(&doc, &selected_id, "html")
            .as_str()
            .unwrap_or_default()
            .to_owned();
        return rsx! { div { class: "page-inspector-component",
            h3 { "HTML content" }
            label { class: "field", span { "HTML" }
                textarea { class: "page-html-editor", value: "{html}", spellcheck: "false", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"html",Value::String(event.value()))) }
            } }
            p { class: "page-builder-hint", "Basic HTML is supported. Scripts, styles, embeds and unsafe attributes are removed before preview and rendering." }
            button { class: "button button-quiet page-component-remove", r#type: "button", onclick: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); let mut selected_component=selected_component; move |_| { on_change.call(remove_page_component(doc.clone(),&id)); selected_component.set("page".into()); } }, "Remove component" }
        } };
    }
    if component_type == "image" {
        let src = page_component_property(&doc, &selected_id, "src").as_str().unwrap_or_default().to_owned();
        let alt = page_component_property(&doc, &selected_id, "alt").as_str().unwrap_or_default().to_owned();
        let caption = page_component_property(&doc, &selected_id, "caption").as_str().unwrap_or_default().to_owned();
        let fit = page_component_property(&doc, &selected_id, "fit").as_str().unwrap_or("cover").to_owned();
        return rsx! { div { class: "page-inspector-component",
            h3 { "Image" }
            label { class: "field", span { "Image URL" }
                input { value: "{src}", placeholder: "https://… or /uploads/…", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"src",Value::String(event.value()))) }
            } }
            label { class: "field", span { "Alternative text" }
                input { value: "{alt}", placeholder: "Describe the image", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"alt",Value::String(event.value()))) }
            } }
            label { class: "field", span { "Caption" }
                input { value: "{caption}", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"caption",Value::String(event.value()))) }
            } }
            label { class: "field", span { "Fit" }
                select { value: "{fit}", onchange: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"fit",Value::String(event.value()))) }, option { value: "cover", "Cover" } option { value: "contain", "Contain" } }
            }
            p { class: "page-builder-hint", "Images are rendered by the page component and use the page's responsive width." }
            button { class: "button button-quiet page-component-remove", r#type: "button", onclick: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); let mut selected_component=selected_component; move |_| { on_change.call(remove_page_component(doc.clone(),&id)); selected_component.set("page".into()); } }, "Remove component" }
        } };
    }
    let label = page_component_label(&component_type);
    let text_value =
        page_component_property(&doc, &selected_id, "text").as_str().unwrap_or_default().to_owned();
    let button_label = page_component_property(&doc, &selected_id, "label")
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let button_variant = page_component_property(&doc, &selected_id, "variant")
        .as_str()
        .unwrap_or("primary")
        .to_owned();
    let button_href =
        page_component_property(&doc, &selected_id, "href").as_str().unwrap_or_default().to_owned();
    let button_action = page_component_property(&doc, &selected_id, "action").as_str().unwrap_or("none").to_owned();
    let button_event = page_component_property(&doc, &selected_id, "eventJson").as_str().unwrap_or("{}").to_owned();
    let spacer_height =
        page_component_property(&doc, &selected_id, "height").as_u64().unwrap_or(24);
    rsx! {
        div { class: "page-inspector-component",
            h3 { "{label}" }
            if component_type == "heading" || component_type == "text" { label { class: "field", span { "Text" } textarea { value: "{text_value}", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"text",Value::String(event.value()))) } } } }
            if component_type == "button" {
                label { class: "field", span { "Label" } input { value: "{button_label}", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"label",Value::String(event.value()))) } } }
                label { class: "field", span { "Style" } select { value: "{button_variant}", onchange: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"variant",Value::String(event.value()))) }, option { value: "primary", "Primary" } option { value: "quiet", "Quiet" } } }
                label { class: "field", span { "Action" } select { value: "{button_action}", onchange: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"action",Value::String(event.value()))) }, option { value: "none", "No action" } option { value: "link", "Open URL" } option { value: "workflow", "Run page workflow" } } }
                if button_action=="link" { label { class: "field", span { "Link URL" } input { value: "{button_href}", placeholder: "https://… or /path", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"href",Value::String(event.value()))) } } } }
                if button_action=="workflow" {
                    label { class: "field", span { "Workflow input (JSON)" } textarea { value: "{button_event}", spellcheck: "false", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_component_property(doc.clone(),&id,"eventJson",Value::String(event.value()))) } } }
                    p { class: "page-builder-hint", "Choose the workflow under Page properties. This button calls it with the JSON input above." }
                }
            }
            if component_type == "spacer" { label { class: "field", span { "Height" } input { r#type: "number", min: "8", max: "160", value: "{spacer_height}", oninput: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); move |event| if let Ok(height)=event.value().parse::<u64>() { on_change.call(set_page_component_property(doc.clone(),&id,"height",Value::from(height.clamp(8,160)))) } } } } }
            if component_type == "divider" { p { class: "page-builder-hint", "Uses the default divider style." } }
            button { class: "button button-quiet page-component-remove", r#type: "button", onclick: { let doc=doc.clone(); let id=selected_id.clone(); let on_change=on_change.clone(); let mut selected_component=selected_component; move |_| { on_change.call(remove_page_component(doc.clone(),&id)); selected_component.set("page".into()); } }, "Remove component" }
        }
    }
}

#[component]
fn PageSettings(
    doc: Value, types: Vec<Value>, workflow_choices: Vec<WorkflowChoice>, workflow_loading: bool,
    is_editing: bool, on_change: EventHandler<Value>,
) -> Element {
    let mut filter_editor = use_signal(|| false);
    let source = doc.get("source").and_then(Value::as_str).unwrap_or_default().to_owned();
    let title = doc.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
    let kind = doc.get("type").and_then(Value::as_str).unwrap_or("grid").to_owned();
    let slot = if kind == "form" { "onSubmit" } else { "dataSource" };
    let workflow_key = doc
        .pointer(&format!("/workflows/{slot}/workflow"))
        .and_then(|reference| {
            Some(format!(
                "{}/{}",
                reference.get("namespace")?.as_str()?,
                reference.get("name")?.as_str()?
            ))
        })
        .unwrap_or_default();
    let page_workflow_key = doc
        .pointer("/workflows/pageAction/workflow")
        .and_then(|reference| {
            Some(format!(
                "{}/{}",
                reference.get("namespace")?.as_str()?,
                reference.get("name")?.as_str()?
            ))
        })
        .unwrap_or_default();
    let filters = doc.pointer("/query/filters").cloned().unwrap_or_else(|| serde_json::json!({}));
    let filter_text = serde_json::to_string_pretty(&filters).unwrap_or_else(|_| "{}".into());
    let gallery_theme =
        doc.pointer("/options/gallery/theme").and_then(Value::as_str).unwrap_or("card").to_owned();
    let gallery_cover =
        doc.pointer("/options/gallery/coverField").and_then(Value::as_str).unwrap_or("").to_owned();
    let gallery_columns = doc
        .pointer("/options/gallery/cardsPerRow")
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_owned();
    let create_label = doc
        .pointer("/options/buttons/createLabel")
        .and_then(Value::as_str)
        .unwrap_or("Create")
        .to_owned();
    let visible_fields = types
        .iter()
        .find(|item| item.get("uid").and_then(Value::as_str) == Some(source.as_str()))
        .and_then(|item| item.pointer("/schema/fields"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_workflow = !workflow_key.is_empty()
        || (kind == "form" && doc.pointer("/workflows/onSave/workflow").is_some());
    let show_content = doc.pointer("/options/showContent").and_then(Value::as_bool).unwrap_or(true);
    rsx! {
        div { class: "page-settings-panel",
            div { class: "page-settings-title", h3 { "Page" } button { class: "page-settings-menu", aria_label: "Page settings menu", "⋮" } }
            label { class: "page-setting-row page-setting-row--stacked", span { "Title" }
                input { value: "{title}", oninput: { let doc=doc.clone(); let on_change=on_change.clone(); move |event| { let mut next=doc.clone(); let title=event.value(); next["name"]=Value::String(title.clone()); if !is_editing { let uid=next.get("source").and_then(Value::as_str).unwrap_or_default(); next["id"]=Value::String(format!("{uid}.pages.{}",page_slug(&title))); } on_change.call(next); } } }
            }
            div { class: "page-setting-group",
                h4 { "Data" }
                label { class: "page-setting-row", span { "Source" }
                    select { value: "{source}", disabled: is_editing, onchange: { let types=types.clone(); let doc=doc.clone(); let on_change=on_change.clone(); move |event| { let uid=event.value(); let mut next=doc.clone(); if uid=="core.page" { next=blank_page_document(); } else if let Some(content_type)=types.iter().find(|item|item.get("uid").and_then(Value::as_str)==Some(uid.as_str())) { next=default_view_document(content_type); } on_change.call(next); } },
                        option { value: "core.page", "Blank page" }
                        for item in types.iter() { { let uid=item.get("uid").and_then(Value::as_str).unwrap_or(""); let label=item.get("display_name").and_then(Value::as_str).unwrap_or(uid); rsx! { option { value: "{uid}", "{label}" } } } }
                    }
                }
                if show_content {
                    div { class: "page-setting-row", span { "Filter by" }
                        button { class: "page-setting-value", onclick: move |_| filter_editor.set(!filter_editor()), if filters.as_object().is_some_and(|filter|filter.is_empty()) { "None" } else { "Configured" } }
                        button { class: "page-setting-gear", aria_label: "Configure filters", onclick: move |_| filter_editor.set(!filter_editor()), "⚙" }
                    }
                    if filter_editor() {
                        label { class: "page-setting-json", span { "Filter conditions (JSON)" }
                            textarea { value: "{filter_text}", spellcheck: "false", oninput: { let doc=doc.clone(); let on_change=on_change.clone(); move |event| { if let Ok(filters)=serde_json::from_str::<Value>(&event.value()) { on_change.call(set_sparse_page_setting(doc.clone(),"/query/filters",filters.clone(),serde_json::json!({}))); } } } }
                        }
                    }
                    label { class: "page-setting-row page-setting-row--stacked", span { "Workflow" }
                        select { value: "{workflow_key}", disabled: workflow_loading, onchange: { let choices=workflow_choices.clone(); let slot=slot.to_owned(); let doc=doc.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_associated_workflow(doc.clone(),&slot,&event.value(),&choices)) },
                            option { value: "", "Select workflow" }
                            for choice in workflow_choices.iter() { option { value: "{choice.key}", "{choice.label}" } }
                        }
                        small { class: "page-setting-help", if kind=="form" { "Runs when the form is submitted." } else { "Loads records for this page." } }
                        if !has_workflow { small { class: "page-setting-warning", "Select an exposed workflow to save this page." } }
                    }
                } else {
                    small { class: "page-setting-help", if source=="core.page" { "This page is independent of a content type." } else { "The workflow list component supplies this page’s data." } }
                }
            }
            div { class: "page-setting-group",
                h4 { "Workflow" }
                label { class: "page-setting-row page-setting-row--stacked", span { "Page action" }
                    select { value: "{page_workflow_key}", disabled: workflow_loading, onchange: { let choices=workflow_choices.clone(); let doc=doc.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_page_action_workflow(doc.clone(),&event.value(),&choices)) },
                        option { value: "", "No workflow" }
                        for choice in workflow_choices.iter() { option { value: "{choice.key}", "{choice.label}" } }
                    }
                    small { class: "page-setting-help", "Components can run this workflow with their configured input." }
                }
            }
            div { class: "page-setting-group",
                h4 { "Appearance" }
                PageSettingToggle { doc: doc.clone(), label: "Show content data view", pointer: "/options/showContent".into(), default: true, disabled: source=="core.page", on_change }
                PageSettingToggle { doc: doc.clone(), label: "Show description", pointer: "/options/showDescription".into(), default: false, on_change }
                label { class: "page-setting-row", span { "Visualization" }
                    select { value: "{kind}", onchange: { let doc=doc.clone(); let on_change=on_change.clone(); move |event| { let mut next=doc.clone(); next["type"]=Value::String(event.value()); on_change.call(next); } },
                        option { value: "grid", "Grid" }
                        option { value: "gallery", "Gallery" }
                        option { value: "kanban", "Kanban" }
                        option { value: "calendar", "Calendar" }
                        option { value: "form", "Form" }
                    }
                }
                if kind=="gallery" {
                    label { class: "page-setting-row", span { "Theme" }
                        select { value: "{gallery_theme}", onchange: { let doc=doc.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_sparse_page_setting(doc.clone(),"/options/gallery/theme",Value::String(event.value()),Value::String("card".into()))) }, option { value: "card", "Card" } option { value: "poster", "Poster" } option { value: "minimal", "Minimal" } option { value: "simple", "Simple" } }
                    }
                    label { class: "page-setting-row", span { "Cover field" }
                        select { value: "{gallery_cover}", onchange: { let doc=doc.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_sparse_page_setting(doc.clone(),"/options/gallery/coverField",Value::String(event.value()),Value::String(String::new()))) }, option { value: "", "None" }
                            for field in visible_fields.iter() { { let field_name=field.get("name").and_then(Value::as_str).unwrap_or(""); let label=field.get("label").and_then(Value::as_str).unwrap_or(field_name); rsx! { option { value: "{field_name}", "{label}" } } } }
                        }
                    }
                    label { class: "page-setting-row", span { "Cards per row" }
                        select { value: "{gallery_columns}", onchange: { let doc=doc.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_sparse_page_setting(doc.clone(),"/options/gallery/cardsPerRow",Value::String(event.value()),Value::String("auto".into()))) }, option { value: "auto", "Auto" } for count in 1..=6 { option { value: "{count}", "{count}" } } }
                    }
                }
            }
            div { class: "page-setting-group",
                h4 { "User filters" }
                if doc.get("userFilters").and_then(Value::as_array).is_some_and(|filters|!filters.is_empty()) {
                    for (index, filter) in doc.get("userFilters").and_then(Value::as_array).cloned().unwrap_or_default().iter().enumerate() {
                        { let field=filter.get("field").and_then(Value::as_str).unwrap_or("").to_owned();
                            rsx! { div { class: "page-user-filter-row", select { value: "{field}", onchange: { let doc=doc.clone(); let on_change=on_change.clone(); move |event| { let mut next=doc.clone(); if let Some(filter)=next.get_mut("userFilters").and_then(Value::as_array_mut).and_then(|items|items.get_mut(index)){filter["field"]=Value::String(event.value());} on_change.call(next); } }, option { value: "", "Choose field" } for item in visible_fields.iter() { { let field_name=item.get("name").and_then(Value::as_str).unwrap_or(""); let label=item.get("label").and_then(Value::as_str).unwrap_or(field_name); rsx! { option { value: "{field_name}", "{label}" } } } } } button { class: "page-setting-gear", aria_label: "Remove user filter", onclick: { let doc=doc.clone(); let on_change=on_change.clone(); move |_| { let mut next=doc.clone(); if let Some(filters)=next.get_mut("userFilters").and_then(Value::as_array_mut){filters.remove(index);if filters.is_empty(){next.as_object_mut().map(|object|{object.remove("userFilters");});}} on_change.call(next); } }, "×" } } }
                        }
                    }
                }
                button { class: "page-setting-add", onclick: { let doc=doc.clone(); let on_change=on_change.clone(); let field=visible_fields.first().and_then(|item|item.get("name")).cloned().unwrap_or(Value::Null); move |_| { let mut next=doc.clone(); let filters=next["userFilters"].as_array_mut(); if filters.is_none(){next["userFilters"]=serde_json::json!([]);} next["userFilters"].as_array_mut().map(|filters|filters.push(serde_json::json!({"type":"dropdown","field":field}))); on_change.call(next); } }, "+ Add filter" }
            }
            div { class: "page-setting-group",
                h4 { "User actions" }
                PageSettingToggle { doc: doc.clone(), label: "Sort", pointer: "/options/userActions/sort".into(), default: true, on_change }
                PageSettingToggle { doc: doc.clone(), label: "Search", pointer: "/options/userActions/search".into(), default: true, on_change }
                PageSettingToggle { doc: doc.clone(), label: "Filter", pointer: "/options/userActions/filter".into(), default: true, on_change }
                PageSettingToggle { doc: doc.clone(), label: "Add record through form", pointer: "/options/addThroughForm".into(), default: false, on_change }
                label { class: "page-setting-row", span { "Create button" }
                    input { value: "{create_label}", oninput: { let doc=doc.clone(); let on_change=on_change.clone(); move |event| on_change.call(set_sparse_page_setting(doc.clone(),"/options/buttons/createLabel",Value::String(event.value()),Value::String("Create".into()))) } }
                }
            }
            div { class: "page-setting-group",
                h4 { "Advanced" }
                PageSettingToggle { doc: doc.clone(), label: "Allow printing", pointer: "/options/allowPrinting".into(), default: false, on_change }
                PageSettingToggle { doc: doc.clone(), label: "Allow export to CSV", pointer: "/options/allowExport".into(), default: false, on_change }
                PageSettingToggle { doc: doc.clone(), label: "Allow import from CSV", pointer: "/options/allowImport".into(), default: false, on_change }
                PageSettingToggle { doc: doc.clone(), label: "Default page", pointer: "/isDefault".into(), default: false, on_change }
            }
        }
    }
}

#[component]
fn PageSettingToggle(
    doc: Value, label: &'static str, pointer: &'static str, default: bool,
    #[props(default = false)] disabled: bool, on_change: EventHandler<Value>,
) -> Element {
    let checked = doc.pointer(pointer).and_then(Value::as_bool).unwrap_or(default);
    rsx! { label { class: "page-setting-row page-setting-toggle", span { "{label}" }
        input { r#type: "checkbox", checked, disabled, onchange: { let doc=doc.clone(); let pointer=pointer.to_owned(); let on_change=on_change.clone(); move |event| on_change.call(set_sparse_page_setting(doc.clone(),&pointer,Value::Bool(event.checked()),Value::Bool(default))) }
    } } }
}

#[component]
fn PageFieldOverride(doc: Value, field: Value, on_change: EventHandler<Value>) -> Element {
    let field_name = field.get("name").and_then(Value::as_str).unwrap_or("").to_owned();
    let field_label = field.get("label").and_then(Value::as_str).unwrap_or(&field_name).to_owned();
    let hidden = doc.pointer(&format!("/fields/{field_name}/visible")).and_then(Value::as_bool)
        == Some(false);
    rsx! { div { class: if hidden { "page-field-override hidden" } else { "page-field-override" },
        span { "{field_label}" }
        button { class: "button button-quiet", r#type: "button", onclick: { let doc=doc.clone(); let key=field_name.clone(); let on_change=on_change.clone(); move |_| { let mut next=doc.clone(); if hidden { if let Some(config)=next.pointer_mut(&format!("/fields/{key}")) { if let Some(config)=config.as_object_mut(){config.remove("visible");} } } else { next["fields"][key.as_str()]["visible"]=Value::Bool(false); } if next.pointer(&format!("/fields/{key}")).and_then(Value::as_object).is_some_and(|config|config.is_empty()) { if let Some(fields)=next.get_mut("fields").and_then(Value::as_object_mut){fields.remove(key.as_str());} } if next.get("fields").and_then(Value::as_object).is_some_and(|fields|fields.is_empty()){next.as_object_mut().map(|object|{object.remove("fields");});} on_change.call(next); } }, if hidden { "Show" } else { "Hide" } }
    } }
}

#[component]
fn PageComponentPreview(doc: Value, id: String, editable: bool, record_id: Option<i32>) -> Element {
    let component_type = page_component_type(&doc, &id).unwrap_or_default();
    if component_type == "list" {
        let title = page_component_property(&doc, &id, "title").as_str().unwrap_or("List").to_owned();
        let source_mode = page_component_property(&doc, &id, "sourceMode").as_str().unwrap_or("static").to_owned();
        let items_json = page_component_property(&doc, &id, "itemsJson").as_str().unwrap_or("[]").to_owned();
        let items_path = page_component_property(&doc, &id, "itemsPath").as_str().unwrap_or("items").to_owned();
        let item_label = page_component_property(&doc, &id, "itemLabel").as_str().unwrap_or("name").to_owned();
        let show_search = page_component_property(&doc, &id, "showSearch").as_bool().unwrap_or(false);
        let source = doc.get("source").and_then(Value::as_str).unwrap_or_default().to_owned();
        let view_id = doc.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
        let workflow_key=doc.pointer(&format!("/workflows/component-{id}/workflow")).map(Value::to_string).unwrap_or_default();
        return rsx! { div { key: "{id}:{source_mode}:{workflow_key}", PageListComponent { source, view_id, component_id: id, title, source_mode, items_json, items_path, item_label, show_search, record_id } } };
    }
    if component_type == "html" {
        let html =
            page_component_property(&doc, &id, "html").as_str().unwrap_or_default().to_owned();
        let tags = [
            "a",
            "b",
            "blockquote",
            "br",
            "code",
            "dd",
            "div",
            "dl",
            "dt",
            "em",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "hr",
            "i",
            "img",
            "li",
            "ol",
            "p",
            "pre",
            "s",
            "span",
            "strong",
            "sub",
            "sup",
            "table",
            "tbody",
            "td",
            "th",
            "thead",
            "tr",
            "u",
            "ul",
        ]
        .into_iter()
        .collect::<HashSet<_>>();
        let attributes = HashMap::from([
            ("a", ["href", "title"].into_iter().collect::<HashSet<_>>()),
            ("img", ["src", "alt", "title", "width", "height"].into_iter().collect()),
            ("td", ["colspan", "rowspan"].into_iter().collect()),
            ("th", ["colspan", "rowspan"].into_iter().collect()),
        ]);
        let safe_html = ammonia::Builder::default()
            .tags(tags)
            .tag_attributes(attributes)
            .generic_attributes(HashSet::new())
            .clean(&html)
            .to_string();
        return rsx! { div { class: "page-component-render page-component-html", dangerous_inner_html: "{safe_html}" } };
    }
    if component_type == "image" {
        let src = page_component_property(&doc, &id, "src").as_str().unwrap_or_default().to_owned();
        let alt = page_component_property(&doc, &id, "alt").as_str().unwrap_or_default().to_owned();
        let caption = page_component_property(&doc, &id, "caption").as_str().unwrap_or_default().to_owned();
        let fit = match page_component_property(&doc, &id, "fit").as_str().unwrap_or("cover") { "contain" => "contain", _ => "cover" };
        let safe_src = src.trim();
        let safe_src = if safe_src.starts_with('/') || safe_src.starts_with("https://") || safe_src.starts_with("http://") { safe_src } else { "" };
        return rsx! { figure { class: "page-component-render page-component-image",
            if !safe_src.is_empty() { img { src: "{safe_src}", alt: "{alt}", class: if fit=="contain" { "page-component-image__contain" } else { "page-component-image__cover" } } }
            else { div { class: "page-component-image__empty", "Choose an image URL in Properties" } }
            if !caption.is_empty() { figcaption { "{caption}" } }
        } };
    }
    let text = page_component_property(&doc, &id, "text").as_str().unwrap_or_default().to_owned();
    let label = page_component_property(&doc, &id, "label").as_str().unwrap_or_default().to_owned();
    let variant =
        page_component_property(&doc, &id, "variant").as_str().unwrap_or("primary").to_owned();
    let href = page_component_property(&doc, &id, "href").as_str().unwrap_or_default().to_owned();
    let height = page_component_property(&doc, &id, "height").as_u64().unwrap_or(24);
    let action = page_component_property(&doc, &id, "action").as_str().unwrap_or("none").to_owned();
    let event_json = page_component_property(&doc, &id, "eventJson").as_str().unwrap_or("{}").to_owned();
    let source = doc.get("source").and_then(Value::as_str).unwrap_or_default().to_owned();
    let view_id = doc.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let workflow_configured = doc.pointer("/workflows/pageAction/workflow").is_some();
    rsx! { div { class: "page-component-render",
        if component_type=="heading" { h2 { "{text}" } }
        else if component_type=="text" { p { "{text}" } }
        else if component_type=="button" {
            if action=="workflow" { PageWorkflowActionButton { source, view_id, label, event_json, enabled: !editable && record_id.is_some() && workflow_configured } }
            else if action=="link" && !href.is_empty() { a { class: if variant=="quiet" { "button button-quiet" } else { "button button-primary" }, href: "{href}", "{label}" } }
            else { button { class: if variant=="quiet" { "button button-quiet" } else { "button button-primary" }, disabled: true, "{label}" } }
        }
        else if component_type=="divider" { hr {} }
        else if component_type=="spacer" { div { style: "height: {height}px" } }
    } }
}

async fn load_page_list(
    source: String, view_id: String, component_id: String, source_mode: String,
    items_json: String, items_path: String, record_id: Option<i32>,
) -> Result<Vec<Value>, String> {
    if source_mode != "workflow" {
        return serde_json::from_str::<Vec<Value>>(&items_json)
            .map_err(|parse| format!("Static list must be a JSON array: {parse}"));
    }
    let Some(_) = record_id else { return Ok(Vec::new()); };
    let slot = format!("component-{component_id}");
    let response = reqwest::Client::new()
        .post(api_url(&format!("/api/content-types/{source}/pages/{view_id}/workflows/{slot}/execute")))
        .json(&serde_json::json!({"event":{"uid":source,"filters":{},"sort":[],"page":{"size":50}}}))
        .send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("List workflow failed ({status}): {}", response.text().await.unwrap_or_default()));
    }
    let payload = response.json::<Value>().await.map_err(|error| error.to_string())?;
    if payload.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(payload.pointer("/execution/error/detail").and_then(Value::as_str).unwrap_or("List workflow did not complete successfully.").to_owned());
    }
    let output = payload.get("result").cloned().unwrap_or(Value::Null);
    let items = if let Some(items) = output.as_array() {
        Some(items)
    } else {
        let pointer = if items_path.starts_with('/') { items_path.clone() } else { format!("/{}", items_path.split('.').collect::<Vec<_>>().join("/")) };
        output.pointer(&pointer).and_then(Value::as_array)
            .or_else(|| output.get("rows").and_then(Value::as_array))
            .or_else(|| output.get("items").and_then(Value::as_array))
    };
    items.cloned().ok_or_else(|| format!("Workflow output must be an array or contain an array at `{items_path}`."))
}

fn page_list_item_value<'a>(item: &'a Value, field: &str) -> Option<&'a Value> {
    let pointer = format!("/{}", field.split('.').map(|part|part.replace('~',"~0").replace('/',"~1")).collect::<Vec<_>>().join("/"));
    item.pointer(&pointer).or_else(||item.get(field))
}

fn page_list_item_text(item: &Value, field: &str) -> String {
    page_list_item_value(item,field).map(preview_value).or_else(||item.as_str().map(str::to_owned)).unwrap_or_else(||item.to_string())
}

#[component]
fn PageListComponent(
    source: String, view_id: String, component_id: String, title: String, source_mode: String,
    items_json: String, items_path: String, item_label: String, show_search: bool,
    record_id: Option<i32>,
) -> Element {
    let mut search = use_signal(String::new);
    let result = use_server_future({
        let source=source.clone(); let view_id=view_id.clone(); let component_id=component_id.clone();
        let source_mode=source_mode.clone(); let items_json=items_json.clone(); let items_path=items_path.clone();
        move || load_page_list(source.clone(),view_id.clone(),component_id.clone(),source_mode.clone(),items_json.clone(),items_path.clone(),record_id)
    })?;
    let loaded = result();
    let error=loaded.as_ref().and_then(|result|result.as_ref().err()).cloned();
    let rows=if source_mode=="static" {
        serde_json::from_str::<Vec<Value>>(&items_json).unwrap_or_default()
    } else if record_id.is_none() {
        Vec::new()
    } else {
        loaded.and_then(Result::ok).unwrap_or_default()
    };
    let needle=search().to_lowercase();
    let rows=rows.into_iter().filter(|item| needle.is_empty() || page_list_item_text(item,&item_label).to_lowercase().contains(&needle)).collect::<Vec<_>>();
    rsx! { section { class: "page-list-component",
        header { class: "page-list-component__heading", h2 { "{title}" } if show_search { input { r#type: "search", placeholder: "Search list…", aria_label: "Search list", value: "{search}", oninput: move |event| search.set(event.value()) } } }
        if source_mode=="workflow" && record_id.is_none() { p { class: "page-builder-hint", "Save this page to preview the workflow-backed list." } }
        if let Some(message)=error { p { class: "page-list-component__error", "{message}" } }
        if rows.is_empty() && !(source_mode=="workflow" && record_id.is_none()) { p { class: "page-list-component__empty", if source_mode=="static" { "No list items. Add items in Properties." } else { "The workflow returned no items." } } }
        else { ul { class: "page-list-component__items", for (index,item) in rows.iter().enumerate() {
            { let label=page_list_item_text(item,&item_label);
                rsx! { li { key: "{index}", "{label}" } }
            }
        } } }
    } }
}

#[component]
fn PageWorkflowActionButton(
    source: String, view_id: String, label: String, event_json: String, enabled: bool,
) -> Element {
    let running = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let result = use_signal(|| None::<Value>);
    rsx! { div { class: "page-workflow-action",
        button { class: "button button-primary", r#type: "button", disabled: !enabled || running(), onclick: move |_| {
            let event = match serde_json::from_str::<Value>(&event_json) {
                Ok(value) => value,
                Err(parse_error) => { error.set(Some(format!("Workflow input must be valid JSON: {parse_error}"))); return; }
            };
            let source=source.clone(); let view_id=view_id.clone(); let mut running=running; let mut error=error; let mut result=result;
            running.set(true); error.set(None); result.set(None);
            spawn(async move {
                let response = reqwest::Client::new()
                    .post(api_url(&format!("/api/content-types/{source}/pages/{view_id}/workflows/pageAction/execute")))
                    .json(&serde_json::json!({"event":event})).send().await;
                match response {
                    Ok(response) if response.status().is_success() => match response.json::<Value>().await {
                        Ok(payload) if payload.get("success").and_then(Value::as_bool)==Some(true) => result.set(Some(payload.get("result").cloned().unwrap_or(Value::Null))),
                        Ok(payload) => error.set(Some(payload.pointer("/execution/error/detail").and_then(Value::as_str).unwrap_or("Workflow did not complete successfully.").to_owned())),
                        Err(parse_error) => error.set(Some(format!("Could not read workflow result: {parse_error}"))),
                    },
                    Ok(response) => error.set(Some(format!("Workflow request failed ({}): {}",response.status(),response.text().await.unwrap_or_default()))),
                    Err(request_error) => error.set(Some(format!("Workflow request failed: {request_error}"))),
                }
                running.set(false);
            });
        }, if running() { "Running…" } else { "{label}" } }
        if !enabled { small { class: "page-builder-hint", "Choose a page workflow and save this page to enable the action." } }
        if let Some(message)=error() { p { class: "page-workflow-action__error", "{message}" } }
        if let Some(value)=result() { pre { class: "page-workflow-action__result", "{serde_json::to_string_pretty(&value).unwrap_or_else(|_|value.to_string())}" } }
    } }
}

fn preview_sample(field_name: &str, row: usize) -> String {
    let key = field_name.to_lowercase();
    if key.contains("price") || key.contains("amount") {
        format!("€{}.00", 19 + row * 5)
    } else if key.contains("email") {
        format!("editor{}@example.com", row + 1)
    } else if key.contains("status") {
        if row % 2 == 0 { "Published".into() } else { "Draft".into() }
    } else {
        format!("Sample {} {}", field_name, row + 1)
    }
}

#[component]
fn ViewPreview(doc: Value, fields: Vec<Value>, record_id: Option<i32>) -> Element {
    let kind = doc.get("type").and_then(Value::as_str).unwrap_or("grid").to_owned();
    let title = doc.get("name").and_then(Value::as_str).unwrap_or("Untitled view").to_owned();
    let source =
        doc.get("source").and_then(Value::as_str).unwrap_or("Choose a content type").to_owned();
    let show_description =
        doc.pointer("/options/showDescription").and_then(Value::as_bool).unwrap_or(false);
    let actions = doc.pointer("/options/userActions");
    let show_search =
        actions.and_then(|value| value.get("search")).and_then(Value::as_bool).unwrap_or(true);
    let show_filter =
        actions.and_then(|value| value.get("filter")).and_then(Value::as_bool).unwrap_or(true);
    let show_sort =
        actions.and_then(|value| value.get("sort")).and_then(Value::as_bool).unwrap_or(true);
    let create_label = doc
        .pointer("/options/buttons/createLabel")
        .and_then(Value::as_str)
        .unwrap_or("Create")
        .to_owned();
    let gallery_theme =
        doc.pointer("/options/gallery/theme").and_then(Value::as_str).unwrap_or("card").to_owned();
    let gallery_columns = doc
        .pointer("/options/gallery/cardsPerRow")
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_owned();
    let gallery_style = if gallery_columns == "auto" {
        "".to_owned()
    } else {
        format!("grid-template-columns:repeat({},minmax(0,1fr))", gallery_columns)
    };
    let preview_records = use_server_future({
        let source = source.clone();
        let doc = doc.clone();
        move || load_content_preview(source.clone(), doc.clone(), record_id)
    })?;
    let preview_result = preview_records();
    let records = preview_result
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .cloned()
        .unwrap_or_default();
    let has_live_result = preview_result.as_ref().is_some_and(Result::is_ok);
    let preview_error = preview_result.and_then(Result::err);
    let mut visible = fields
        .iter()
        .filter(|field| {
            let name = field.get("name").and_then(Value::as_str).unwrap_or("");
            doc.pointer(&format!("/fields/{name}/visible")).and_then(Value::as_bool) != Some(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    visible.sort_by_key(|field| {
        let name = field.get("name").and_then(Value::as_str).unwrap_or("");
        doc.pointer(&format!("/fields/{name}/order")).and_then(Value::as_u64).unwrap_or(u64::MAX)
    });
    let column_labels = visible
        .iter()
        .map(|field| {
            let name = field.get("name").and_then(Value::as_str).unwrap_or("field");
            doc.pointer(&format!("/fields/{name}/label"))
                .and_then(Value::as_str)
                .or_else(|| field.get("label").and_then(Value::as_str))
                .unwrap_or(name)
                .to_owned()
        })
        .collect::<Vec<_>>();
    let sample_rows = if records.is_empty() && !has_live_result {
        (0..3)
            .map(|row| {
                (
                    (100 + row).to_string(),
                    visible
                        .iter()
                        .map(|field| {
                            preview_sample(
                                field.get("name").and_then(Value::as_str).unwrap_or("field"),
                                row,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    } else {
        records
            .iter()
            .map(|record| {
                let id = record.get("id").map(preview_value).unwrap_or_else(|| "—".into());
                let values = visible
                    .iter()
                    .map(|field| {
                        let name = field.get("name").and_then(Value::as_str).unwrap_or("");
                        if name == "published" {
                            preview_value(record.get("published").unwrap_or(&Value::Null))
                        } else if name == "id" {
                            id.clone()
                        } else {
                            preview_value(
                                record.pointer(&format!("/data/{name}")).unwrap_or(&Value::Null),
                            )
                        }
                    })
                    .collect::<Vec<_>>();
                (id, values)
            })
            .collect::<Vec<_>>()
    };
    let form_fields = visible
        .iter()
        .map(|field| {
            let name = field.get("name").and_then(Value::as_str).unwrap_or("field");
            let label = doc
                .pointer(&format!("/fields/{name}/label"))
                .and_then(Value::as_str)
                .or_else(|| field.get("label").and_then(Value::as_str))
                .unwrap_or(name)
                .to_owned();
            let required = doc
                .pointer(&format!("/fields/{name}/required"))
                .and_then(Value::as_bool)
                .unwrap_or_else(|| field.get("required").and_then(Value::as_bool).unwrap_or(false));
            (label, required)
        })
        .collect::<Vec<_>>();
    let days = (1..=35)
        .map(|day| if day <= 30 { day.to_string() } else { String::new() })
        .collect::<Vec<_>>();
    rsx! {
        div { class: "view-preview",
            div { class: "view-preview-header", div { span { class: "data-eyebrow", "LIVE PREVIEW" } h3 { "{title}" } if show_description { p { "Records from {source}" } } } span { class: "content-view-type", "{component_label(&kind)}" } }
            if kind == "grid" {
                div { class: "view-preview-toolbar", if show_search { input { r#type: "search", placeholder: "Search records…", disabled: true } } if show_filter { button { class: "button button-quiet", disabled: true, "Filter" } } if show_sort { button { class: "button button-quiet", disabled: true, "Sort" } } button { class: "button button-primary", disabled: true, "+ {create_label}" } }
                if let Some(message) = preview_error.as_ref() { div { class: "view-preview-empty", "Could not load preview data: {message}" } }
                else if has_live_result && records.is_empty() { div { class: "view-preview-empty", "No entries match this view." } }
                else if visible.is_empty() { div { class: "view-preview-empty", "Visible fields will appear here." } } else {
                    div { class: "view-preview-table-wrap", table { class: "view-preview-table",
                        thead { tr { th { "ID" } for label in column_labels.iter() { th { "{label}" } } th { "Actions" } } }
                        tbody { for (record_id, row) in sample_rows.iter() { tr { td { "{record_id}" } for value in row.iter() { td { "{value}" } } td { button { class: "button button-quiet", disabled: true, "Edit" } } } } }
                    } }
                }
                div { class: "view-preview-pagination", if has_live_result { "Showing {records.len()} content entries" } else if records.is_empty() { "Sample rows · preview data is loading or unavailable" } else { "Showing {records.len()} content entries" } }
            } else if kind == "form" {
                div { class: "view-preview-form", h4 { "Create entry" } p { "Enter the content fields below." }
                    for (label, required) in form_fields.iter() { label { span { "{label}" if *required { span { class: "view-preview-required", "*" } } } input { placeholder: "Enter {label}…", disabled: true } } }
                    div { class: "view-preview-form-actions", button { class: "button button-quiet", disabled: true, "Cancel" } button { class: "button button-primary", disabled: true, "Create entry" } }
                }
            } else if kind == "kanban" {
                div { class: "view-preview-board", for status in ["Draft", "In review", "Published"] { section { class: "view-preview-lane", header { strong { "{status}" } span { "2" } } for index in 0..2 { article { class: "view-preview-card", strong { "Sample record {index+1}" } small { "Content entry · #{index+1}" } } } } } }
            } else if kind == "gallery" {
                if show_search || show_filter || show_sort { div { class: "view-preview-toolbar", if show_search { input { r#type: "search", placeholder: "Search records…", disabled: true } } if show_filter { button { class: "button button-quiet", disabled: true, "Filter" } } if show_sort { button { class: "button button-quiet", disabled: true, "Sort" } } button { class: "button button-primary", disabled: true, "+ {create_label}" } } }
                div { class: "view-preview-gallery view-preview-gallery--{gallery_theme}", style: "{gallery_style}", for index in 0..4 { article { class: "view-preview-gallery-card", div { class: "view-preview-image-placeholder", Icon { name: "image" } } div { class: "view-preview-gallery-copy", strong { "Sample entry {index+1}" } for label in column_labels.iter().take(2) { small { "{label}: Example value" } } } } } }
            } else {
                div { class: "view-preview-calendar", div { class: "view-preview-calendar-toolbar", button { disabled: true, "‹" } strong { "November 2026" } button { disabled: true, "›" } } div { class: "view-preview-calendar-grid", for (index, day) in days.iter().enumerate() { div { class: "view-preview-calendar-day", span { "{day}" } if index==4 || index==13 || index==21 { small { "Sample event" } } } } } }
            }
        }
    }
}
