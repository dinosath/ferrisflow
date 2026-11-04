use crate::{app_state::AppState, models::plugin};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
};

type ApiResult<T> = Result<T, (StatusCode, Json<Value>)>;
const BUILTIN_CONTENT_TYPES_PLUGIN: &str = "ferrisflow.content-types";
fn error(status: StatusCode, message: impl ToString) -> (StatusCode, Json<Value>) {
    (status, Json(serde_json::json!({"error": message.to_string()})))
}
fn db_error(e: sea_orm::DbErr) -> (StatusCode, Json<Value>) {
    error(StatusCode::INTERNAL_SERVER_ERROR, e)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/plugins", get(list_plugins).post(create_plugin))
        .route("/api/plugins/import", axum::routing::post(import_plugin_url))
        .route("/api/plugins/menu-items", get(list_menu_items))
        .route(
            "/api/plugins/{uid}/views/{view_id}/config",
            axum::routing::put(update_plugin_view_config),
        )
        .route("/api/plugins/{uid}", get(read_plugin).put(update_plugin).delete(delete_plugin))
        .route("/api/plugins/{uid}/enabled", axum::routing::put(set_enabled))
}

async fn update_plugin_view_config(
    State(state): State<AppState>, Path((uid, view_id)): Path<(String, String)>,
    Json(patch): Json<Value>,
) -> ApiResult<Json<Value>> {
    let list_view = patch.get("listView").cloned();
    let form_view = patch.get("formView").cloned();
    if list_view.is_none() && form_view.is_none() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "View config must include listView or formView settings",
        ));
    }
    if let Some(config) = list_view.as_ref() {
        validate_list_view_config(config)?;
    }
    if let Some(config) = form_view.as_ref() {
        validate_form_view_config(config)?;
    }
    let row = plugin::Entity::find_by_id(uid)
        .one(&state.db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Plugin not found"))?;
    let mut manifest: Value = serde_json::from_str(&row.manifest_json)
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let declared_workflows =
        manifest.get("workflows").and_then(Value::as_array).cloned().unwrap_or_default();
    let views = manifest
        .get_mut("views")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Plugin view not found"))?;
    let view = views
        .iter_mut()
        .find(|view| view.get("id").and_then(Value::as_str) == Some(&view_id))
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Plugin view not found"))?;
    let is_collection_manager =
        view.get("dataSource").and_then(|source| source.get("type")).and_then(Value::as_str)
            == Some("database.contentTypes");
    if is_collection_manager {
        if let Some(config) = list_view.as_ref() {
            let api = config
                .get("api")
                .ok_or_else(|| error(StatusCode::BAD_REQUEST, "listView.api is required"))?;
            validate_api_is_plugin_workflow(api, &declared_workflows)?;
        }
        if let Some(config) = form_view.as_ref() {
            let api = config
                .get("api")
                .ok_or_else(|| error(StatusCode::BAD_REQUEST, "formView.api is required"))?;
            for action in ["create", "update"] {
                let action_api = api.get(action).ok_or_else(|| {
                    error(StatusCode::BAD_REQUEST, format!("formView.api.{action} is required"))
                })?;
                validate_api_is_plugin_workflow(action_api, &declared_workflows)?;
            }
        }
    }
    if !view.get("config").is_some_and(Value::is_object) {
        view["config"] = serde_json::json!({});
    }
    let content_type_uid = patch.get("contentTypeUid").and_then(Value::as_str);
    if let Some(list_view) = list_view {
        if let Some(content_type_uid) = content_type_uid {
            if !view["config"]["listView"].is_object() {
                view["config"]["listView"] = serde_json::json!({});
            }
            view["config"]["listView"]["byType"][content_type_uid] = list_view;
        } else {
            view["config"]["listView"] = list_view;
        }
    }
    if let Some(form_view) = form_view {
        if let Some(content_type_uid) = content_type_uid {
            if !view["config"]["formView"].is_object() {
                view["config"]["formView"] = serde_json::json!({});
            }
            view["config"]["formView"]["byType"][content_type_uid] = form_view;
        } else {
            view["config"]["formView"] = form_view;
        }
    }
    let mut active = row.into_active_model();
    active.manifest_json =
        Set(serde_json::to_string(&manifest).map_err(|e| error(StatusCode::BAD_REQUEST, e))?);
    active.updated_at = Set(chrono::Utc::now().naive_utc());
    let updated = active.update(&state.db).await.map_err(db_error)?;
    let plugin = PluginResponse::try_from(updated)?;
    Ok(Json(
        serde_json::json!({"plugin":plugin.uid,"view":view_id,"config":plugin.manifest["views"].as_array().and_then(|views| views.iter().find(|view| view["id"] == view_id)).and_then(|view| view.get("config")).cloned().unwrap_or(Value::Null)}),
    ))
}

fn validate_list_view_config(config: &Value) -> ApiResult<()> {
    let settings = config
        .get("settings")
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "listView.settings is required"))?;
    for key in ["enableSearch", "enableFilters", "enableBulkActions"] {
        if settings.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(error(
                StatusCode::BAD_REQUEST,
                format!("listView.settings.{key} must be boolean"),
            ));
        }
    }
    if let Some(size) = settings.get("entriesPerPage") {
        if !matches!(size.as_u64(), Some(10 | 20 | 50 | 100)) {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "entriesPerPage must be 10, 20, 50, or 100",
            ));
        }
    }
    if settings
        .get("defaultSortOrder")
        .is_some_and(|value| !matches!(value.as_str(), Some("asc" | "desc")))
    {
        return Err(error(StatusCode::BAD_REQUEST, "defaultSortOrder must be asc or desc"));
    }
    if let Some(api) = config.get("api") {
        validate_plugin_view_api(api, "listView.api", &["GET"])?;
    }
    let uses_columns = config.get("columns").is_some();
    let fields =
        config
            .get("columns")
            .or_else(|| config.get("fields"))
            .and_then(Value::as_array)
            .ok_or_else(|| error(StatusCode::BAD_REQUEST, "listView.columns must be an array"))?;
    for field in fields {
        if field.get("name").and_then(Value::as_str).is_none_or(str::is_empty)
            || field.get("label").and_then(Value::as_str).is_none_or(str::is_empty)
            || (uses_columns
                && field.get("mapping").and_then(Value::as_str).is_none_or(str::is_empty))
            || field.get("sortable").is_some_and(|value| !value.is_boolean())
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Each list view column needs a name, label, data mapping, and boolean sortable setting",
            ));
        }
    }
    Ok(())
}

fn validate_form_view_config(config: &Value) -> ApiResult<()> {
    let api = config
        .get("api")
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "formView.api is required"))?;
    validate_plugin_view_api(
        api.get("create").unwrap_or(&Value::Null),
        "formView.api.create",
        &["POST"],
    )?;
    validate_plugin_view_api(
        api.get("update").unwrap_or(&Value::Null),
        "formView.api.update",
        &["PUT"],
    )?;
    for action in ["create", "update"] {
        let form = config.get(action).ok_or_else(|| {
            error(StatusCode::BAD_REQUEST, format!("formView.{action} is required"))
        })?;
        let fields = form.get("fields").and_then(Value::as_array).ok_or_else(|| {
            error(StatusCode::BAD_REQUEST, format!("formView.{action}.fields must be an array"))
        })?;
        for field in fields {
            if field.get("name").and_then(Value::as_str).is_none_or(str::is_empty)
                || field.get("label").and_then(Value::as_str).is_none_or(str::is_empty)
                || field.get("required").is_some_and(|value| !value.is_boolean())
            {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "Each form field needs a name, label, and optional required flag",
                ));
            }
        }
    }
    Ok(())
}

fn validate_plugin_view_api(api: &Value, name: &str, methods: &[&str]) -> ApiResult<()> {
    let method = api.get("method").and_then(Value::as_str).unwrap_or_default();
    let path = api.get("path").and_then(Value::as_str).unwrap_or_default();
    if !methods.iter().any(|allowed| method.eq_ignore_ascii_case(allowed))
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains("..")
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            format!("{name} must use an allowed method and an app-relative path"),
        ));
    }
    Ok(())
}

fn validate_api_is_plugin_workflow(api: &Value, workflows: &[Value]) -> ApiResult<()> {
    let method = api.get("method").and_then(Value::as_str).unwrap_or_default();
    let path = api.get("path").and_then(Value::as_str).unwrap_or_default();
    let is_declared = workflows.iter().any(|workflow| {
        let rest = workflow
            .get("document")
            .and_then(|document| document.get("metadata"))
            .and_then(|metadata| metadata.get("rest_api"));
        rest.and_then(|rest| rest.get("enabled")).and_then(Value::as_bool) == Some(true)
            && rest
                .and_then(|rest| rest.get("method"))
                .and_then(Value::as_str)
                .is_some_and(|declared| declared.eq_ignore_ascii_case(method))
            && rest.and_then(|rest| rest.get("path")).and_then(Value::as_str) == Some(path)
    });
    if !is_declared {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Configured view API must match an enabled REST route declared by a plugin workflow",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
struct PluginResponse {
    uid: String,
    name: String,
    version: String,
    description: Option<String>,
    manifest: Value,
    files: Value,
    source_url: Option<String>,
    enabled: bool,
    created_at: chrono::NaiveDateTime,
    updated_at: chrono::NaiveDateTime,
}
impl TryFrom<plugin::Model> for PluginResponse {
    type Error = (StatusCode, Json<Value>);
    fn try_from(row: plugin::Model) -> Result<Self, Self::Error> {
        Ok(Self {
            uid: row.uid,
            name: row.name,
            version: row.version,
            description: row.description,
            manifest: serde_json::from_str(&row.manifest_json)
                .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?,
            files: serde_json::from_str(&row.files_json)
                .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?,
            source_url: row.source_url,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Clone)]
struct NormalizedPackage {
    uid: String,
    name: String,
    version: String,
    description: Option<String>,
    manifest: Value,
    files: BTreeMap<String, Value>,
}

fn package_from_value(input: Value) -> ApiResult<NormalizedPackage> {
    let outer = input
        .as_object()
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Plugin package must be a JSON object"))?;
    let files: BTreeMap<String, Value> = outer
        .get("files")
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    if files.len() > 100 {
        return Err(error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "A plugin bundle may contain at most 100 files",
        ));
    }
    let bytes = serde_json::to_vec(&input).map_err(|e| error(StatusCode::BAD_REQUEST, e))?.len();
    if bytes > 2 * 1024 * 1024 {
        return Err(error(StatusCode::PAYLOAD_TOO_LARGE, "Plugin package exceeds 2 MiB"));
    }
    for path in files.keys() {
        validate_file_path(path)?;
    }

    let manifest = if let Some(manifest) = outer.get("manifest") {
        manifest.clone()
    } else if let Some(entry) = outer.get("entry").and_then(Value::as_str) {
        let entry = safe_file_path(entry)?;
        let raw = files.get(&entry).ok_or_else(|| {
            error(StatusCode::BAD_REQUEST, format!("Entry file '{entry}' is missing"))
        })?;
        parse_file_value(raw)?
    } else {
        let mut manifest = outer.clone();
        manifest.remove("files");
        manifest.remove("source_url");
        manifest.remove("sourceUrl");
        Value::Object(manifest)
    };
    validate_manifest(&manifest, &files)?;
    let meta = manifest.get("plugin").filter(|v| v.is_object()).unwrap_or(&manifest);
    let uid = meta
        .get("id")
        .or_else(|| meta.get("uid"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let name = meta.get("name").and_then(Value::as_str).unwrap_or_default().trim().to_owned();
    let version = meta.get("version").and_then(Value::as_str).unwrap_or("0.1.0").to_owned();
    let description = meta.get("description").and_then(Value::as_str).map(str::to_owned);
    Ok(NormalizedPackage { uid, name, version, description, manifest, files })
}

fn validate_manifest(manifest: &Value, files: &BTreeMap<String, Value>) -> ApiResult<()> {
    if !manifest.is_object() {
        return Err(error(StatusCode::BAD_REQUEST, "Plugin manifest must be a JSON object"));
    }
    let meta = manifest.get("plugin").filter(|v| v.is_object()).unwrap_or(manifest);
    let id = meta.get("id").or_else(|| meta.get("uid")).and_then(Value::as_str).unwrap_or_default();
    if id.is_empty()
        || id.len() > 96
        || !id.split('.').all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Plugin id must use letters, numbers, dots, dashes, or underscores",
        ));
    }
    if meta.get("name").and_then(Value::as_str).is_none_or(|s| s.trim().is_empty()) {
        return Err(error(StatusCode::BAD_REQUEST, "Plugin name is required"));
    }
    for key in ["workflows", "menuItems", "menu_items", "views"] {
        if manifest.get(key).is_some_and(|v| !v.is_array()) {
            return Err(error(StatusCode::BAD_REQUEST, format!("'{key}' must be an array")));
        }
    }
    let menu_items = manifest.get("menuItems").or_else(|| manifest.get("menu_items"));
    if let Some(items) = menu_items.and_then(Value::as_array) {
        for item in items {
            if item.get("$file").is_some() {
                continue;
            }
            if item.get("label").and_then(Value::as_str).is_none_or(|label| label.trim().is_empty())
            {
                return Err(error(StatusCode::BAD_REQUEST, "Menu items require a label"));
            }
            let path = item
                .get("path")
                .or_else(|| item.get("href"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !(path.starts_with('/') || path.starts_with("https://")) {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "Menu item paths must be app-relative or HTTPS URLs",
                ));
            }
        }
    }
    if let Some(database) = manifest.get("database") {
        if !database.is_object() {
            return Err(error(StatusCode::BAD_REQUEST, "'database' must be an object"));
        }
        for key in ["contentTypes", "content_types", "tables"] {
            if database.get(key).is_some_and(|v| !v.is_array()) {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    format!("database.{key} must be an array"),
                ));
            }
        }
    }
    let refs = manifest.get("plugins").or_else(|| manifest.get("requires"));
    if let Some(refs) = refs {
        let refs = refs.as_array().ok_or_else(|| {
            error(StatusCode::BAD_REQUEST, "'plugins' must be an array of plugin references")
        })?;
        for reference in refs {
            let object = reference.as_object().ok_or_else(|| {
                error(
                    StatusCode::BAD_REQUEST,
                    "Plugin references must be objects with a 'url' or 'file'",
                )
            })?;
            let url = object.get("url").and_then(Value::as_str);
            let file = object.get("file").and_then(Value::as_str);
            if url.is_some() == file.is_some() {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "Each plugin reference must contain exactly one of 'url' or 'file'",
                ));
            }
            if let Some(url) = url {
                validate_remote_url(url)?;
            }
            if let Some(file) = file {
                let path = safe_file_path(file)?;
                if !files.contains_key(&path) {
                    return Err(error(
                        StatusCode::BAD_REQUEST,
                        format!("Plugin reference file '{path}' is missing from this bundle"),
                    ));
                }
            }
        }
    }
    for (key, values) in [
        ("workflows", manifest.get("workflows")),
        ("menuItems", manifest.get("menuItems").or_else(|| manifest.get("menu_items"))),
        ("views", manifest.get("views")),
        (
            "database.contentTypes",
            manifest
                .get("database")
                .and_then(|db| db.get("contentTypes").or_else(|| db.get("content_types"))),
        ),
    ] {
        if let Some(items) = values.and_then(Value::as_array) {
            for item in items {
                if !item.is_object() && !item.get("$file").is_some() {
                    return Err(error(
                        StatusCode::BAD_REQUEST,
                        format!("Every {key} entry must be an object or file reference"),
                    ));
                }
                if let Some(path) = item.get("$file").and_then(Value::as_str) {
                    let path = safe_file_path(path)?;
                    if !files.contains_key(&path) {
                        return Err(error(
                            StatusCode::BAD_REQUEST,
                            format!("Referenced file '{path}' is missing"),
                        ));
                    }
                }
                if key == "workflows" && item.get("$file").is_none() {
                    let raw = serde_json::to_string(item)
                        .map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
                    crate::ows::normalize_ows_json(&raw).map_err(|reason| {
                        error(
                            StatusCode::UNPROCESSABLE_ENTITY,
                            format!("Plugin workflow is not a valid OWS workflow: {reason}"),
                        )
                    })?;
                }
            }
        }
    }
    Ok(())
}

fn validate_file_path(path: &str) -> ApiResult<()> {
    safe_file_path(path).map(|_| ())
}
fn safe_file_path(path: &str) -> ApiResult<String> {
    let normalized = path.trim().replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.split('/').any(|part| part == ".." || part.is_empty())
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Plugin file paths must be relative and cannot traverse parent folders",
        ));
    }
    Ok(normalized)
}
fn parse_file_value(value: &Value) -> ApiResult<Value> {
    if let Some(text) = value.as_str() {
        serde_json::from_str(text).map_err(|e| {
            error(StatusCode::BAD_REQUEST, format!("Plugin file is not valid JSON: {e}"))
        })
    } else {
        Ok(value.clone())
    }
}
fn validate_remote_url(raw: &str) -> ApiResult<url::Url> {
    let parsed = url::Url::parse(raw)
        .map_err(|_| error(StatusCode::BAD_REQUEST, "Plugin URL is invalid"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Plugin URLs must use HTTPS and cannot contain credentials",
        ));
    }
    if let Some(host) = parsed.host_str() {
        if host.eq_ignore_ascii_case("localhost")
            || host.ends_with(".localhost")
            || host.ends_with(".local")
        {
            return Err(error(StatusCode::BAD_REQUEST, "Plugin URLs cannot target local hosts"));
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            if !is_public_ip(ip) {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "Plugin URLs cannot target private or reserved IP addresses",
                ));
            }
        }
    }
    Ok(parsed)
}
fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || ip.is_multicast()
                || a == 0
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 198 && (b == 18 || b == 19 || b == 51))
                || (a == 203 && b == 0 && ip.octets()[2] == 113)
                || (a == 192 && (b == 0 || b == 88)))
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(mapped));
            }
            let segments = ip.segments();
            let globally_routable_prefix = segments[0] & 0xe000 == 0x2000;
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || !globally_routable_prefix
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
                || (segments[0] == 0x2001 && (segments[1] == 0 || segments[1] == 0x0db8))
                || segments[0] == 0x2002
                || segments[0] == 0x3fff)
        }
    }
}

async fn fetch_remote_package(raw: &str) -> ApiResult<Value> {
    let url = validate_remote_url(raw)?;
    let host = url.host_str().expect("validated host");
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| error(StatusCode::BAD_GATEWAY, format!("Could not resolve plugin host: {e}")))?
        .collect();
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Plugin URL resolves to a private or reserved address",
        ));
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|e| error(StatusCode::BAD_GATEWAY, e))?;
    let response = client
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| error(StatusCode::BAD_GATEWAY, e))?;
    if !response.status().is_success() {
        return Err(error(
            StatusCode::BAD_GATEWAY,
            format!("Plugin URL returned {}", response.status()),
        ));
    }
    if response.content_length().is_some_and(|length| length > 2 * 1024 * 1024) {
        return Err(error(StatusCode::PAYLOAD_TOO_LARGE, "Remote plugin package exceeds 2 MiB"));
    }
    let bytes = response.bytes().await.map_err(|e| error(StatusCode::BAD_GATEWAY, e))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(error(StatusCode::PAYLOAD_TOO_LARGE, "Remote plugin package exceeds 2 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        error(StatusCode::BAD_GATEWAY, format!("Remote plugin is not valid JSON: {e}"))
    })
}

fn expand_file_references(
    values: Vec<Value>, files: &BTreeMap<String, Value>,
) -> ApiResult<Vec<Value>> {
    let mut expanded = Vec::new();
    for value in values {
        if let Some(path) = value.get("$file").and_then(Value::as_str) {
            let path = safe_file_path(path)?;
            let file = files.get(&path).ok_or_else(|| {
                error(
                    StatusCode::BAD_REQUEST,
                    format!("Referenced plugin file '{path}' is missing"),
                )
            })?;
            match parse_file_value(file)? {
                Value::Array(items) => expanded.extend(items),
                item => expanded.push(item),
            }
        } else {
            expanded.push(value);
        }
    }
    Ok(expanded)
}

fn expand_package_resources(
    manifest: &mut Value, files: &BTreeMap<String, Value>,
) -> ApiResult<()> {
    let root = manifest
        .as_object_mut()
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Plugin manifest must be an object"))?;
    for key in ["workflows", "menuItems", "menu_items", "views"] {
        if let Some(items) = root.get(key).and_then(Value::as_array).cloned() {
            root.insert(key.to_owned(), Value::Array(expand_file_references(items, files)?));
        }
    }
    if let Some(database) = root.get_mut("database").and_then(Value::as_object_mut) {
        for key in ["contentTypes", "content_types", "tables"] {
            if let Some(items) = database.get(key).and_then(Value::as_array).cloned() {
                database
                    .insert(key.to_owned(), Value::Array(expand_file_references(items, files)?));
            }
        }
    }
    Ok(())
}

fn dependency_refs(manifest: &Value) -> Vec<Value> {
    manifest
        .get("plugins")
        .or_else(|| manifest.get("requires"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn append_items(target: &mut Value, key: &str, source: Option<&Value>) {
    let Some(items) = source.and_then(Value::as_array) else { return };
    if !target.get(key).is_some_and(Value::is_array) {
        target[key] = Value::Array(Vec::new());
    }
    if let Some(current) = target.get_mut(key).and_then(Value::as_array_mut) {
        current.extend(items.iter().cloned());
    }
}

fn merge_plugin_resources(target: &mut Value, source: &Value) {
    append_items(target, "workflows", source.get("workflows"));
    append_items(target, "views", source.get("views"));
    append_items(target, "menuItems", source.get("menuItems").or_else(|| source.get("menu_items")));
    if let Some(source_db) = source.get("database") {
        if !target.get("database").is_some_and(Value::is_object) {
            target["database"] = serde_json::json!({});
        }
        let target_db = target
            .get_mut("database")
            .and_then(Value::as_object_mut)
            .expect("database was initialized");
        let source_types = source_db.get("contentTypes").or_else(|| source_db.get("content_types"));
        for (key, source_value) in
            [("contentTypes", source_types), ("tables", source_db.get("tables"))]
        {
            let Some(items) = source_value.and_then(Value::as_array) else { continue };
            let list = target_db.entry(key).or_insert_with(|| Value::Array(Vec::new()));
            if let Some(list) = list.as_array_mut() {
                list.extend(items.iter().cloned());
            }
        }
    }
}

async fn resolve_plugin_dependencies(mut root: NormalizedPackage) -> ApiResult<NormalizedPackage> {
    expand_package_resources(&mut root.manifest, &root.files)?;
    let mut visited = std::collections::HashSet::from([root.uid.clone()]);
    let mut queue = std::collections::VecDeque::new();
    for reference in dependency_refs(&root.manifest) {
        queue.push_back((reference, root.files.clone()));
    }
    let mut loaded = 0usize;
    while let Some((reference, owner_files)) = queue.pop_front() {
        loaded += 1;
        if loaded > 32 {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Plugin dependency graph exceeds 32 packages",
            ));
        }
        let file_reference = reference.get("file").and_then(Value::as_str);
        let value = if let Some(url) = reference.get("url").and_then(Value::as_str) {
            fetch_remote_package(url).await?
        } else if let Some(path) = file_reference {
            let path = safe_file_path(path)?;
            parse_file_value(owner_files.get(&path).ok_or_else(|| {
                error(
                    StatusCode::BAD_REQUEST,
                    format!("Plugin dependency file '{path}' is missing"),
                )
            })?)?
        } else {
            return Err(error(StatusCode::BAD_REQUEST, "Plugin reference needs a URL or file"));
        };
        let mut value = value;
        // A manifest loaded from a bundle file can refer to sibling files in
        // that same bundle. Make those files visible during package parsing,
        // since parsing validates resource references immediately.
        if file_reference.is_some() && value.get("files").is_none() {
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "files".to_owned(),
                    serde_json::to_value(&owner_files)
                        .map_err(|e| error(StatusCode::BAD_REQUEST, e))?,
                );
            }
        }
        let mut child = package_from_value(value)?;
        if child.files.is_empty() && file_reference.is_some() {
            child.files = owner_files.clone();
        }
        if !visited.insert(child.uid.clone()) {
            return Err(error(
                StatusCode::BAD_REQUEST,
                format!("Plugin dependency cycle or duplicate id: {}", child.uid),
            ));
        }
        expand_package_resources(&mut child.manifest, &child.files)?;
        validate_manifest(&child.manifest, &child.files)?;
        for nested in dependency_refs(&child.manifest) {
            queue.push_back((nested, child.files.clone()));
        }
        for (path, file) in &child.files {
            root.files
                .entry(format!("plugins/{}/{}", child.uid, path))
                .or_insert_with(|| file.clone());
        }
        merge_plugin_resources(&mut root.manifest, &child.manifest);
    }
    validate_manifest(&root.manifest, &root.files)?;
    Ok(root)
}

fn active_from_package(
    package: NormalizedPackage, source_url: Option<String>,
) -> ApiResult<plugin::ActiveModel> {
    let manifest_json =
        serde_json::to_string(&package.manifest).map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    let files_json =
        serde_json::to_string(&package.files).map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    let description = package.description.clone();
    Ok(plugin::ActiveModel {
        uid: Set(package.uid),
        name: Set(package.name),
        version: Set(package.version),
        description: Set(description),
        manifest_json: Set(manifest_json),
        files_json: Set(files_json),
        source_url: Set(source_url),
        enabled: Set(true),
        ..Default::default()
    })
}

async fn list_plugins(State(state): State<AppState>) -> ApiResult<Json<Vec<PluginResponse>>> {
    let rows = plugin::Entity::find()
        .order_by_asc(plugin::Column::Name)
        .all(&state.db)
        .await
        .map_err(db_error)?;
    Ok(Json(rows.into_iter().map(PluginResponse::try_from).collect::<ApiResult<Vec<_>>>()?))
}
async fn create_plugin(
    State(state): State<AppState>, Json(input): Json<Value>,
) -> ApiResult<impl IntoResponse> {
    let package = resolve_plugin_dependencies(package_from_value(input)?).await?;
    let active = active_from_package(package, None)?;
    let row = active.insert(&state.db).await.map_err(|e| error(StatusCode::CONFLICT, e))?;
    Ok((StatusCode::CREATED, Json(PluginResponse::try_from(row)?)))
}
async fn import_plugin_url(
    State(state): State<AppState>, Json(input): Json<ImportUrl>,
) -> ApiResult<impl IntoResponse> {
    let raw_url = input.url;
    let mut value = fetch_remote_package(&raw_url).await?;
    if let Some(map) = value.as_object_mut() {
        map.entry("source_url").or_insert(Value::String(raw_url.clone()));
    }
    let package = resolve_plugin_dependencies(package_from_value(value)?).await?;
    let active = active_from_package(package, Some(raw_url))?;
    let row = active.insert(&state.db).await.map_err(|e| error(StatusCode::CONFLICT, e))?;
    Ok((StatusCode::CREATED, Json(PluginResponse::try_from(row)?)))
}
#[derive(Deserialize)]
struct ImportUrl {
    url: String,
}
async fn read_plugin(
    State(state): State<AppState>, Path(uid): Path<String>,
) -> ApiResult<Json<PluginResponse>> {
    let row = plugin::Entity::find_by_id(uid)
        .one(&state.db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Plugin not found"))?;
    Ok(Json(PluginResponse::try_from(row)?))
}
async fn update_plugin(
    State(state): State<AppState>, Path(uid): Path<String>, Json(input): Json<Value>,
) -> ApiResult<Json<PluginResponse>> {
    if uid == BUILTIN_CONTENT_TYPES_PLUGIN {
        return Err(error(StatusCode::FORBIDDEN, "Built-in plugins cannot be edited"));
    }
    let package = resolve_plugin_dependencies(package_from_value(input)?).await?;
    if package.uid != uid {
        return Err(error(StatusCode::BAD_REQUEST, "Plugin id cannot be changed"));
    }
    let mut row = plugin::Entity::find_by_id(uid)
        .one(&state.db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Plugin not found"))?
        .into_active_model();
    row.name = Set(package.name);
    row.version = Set(package.version);
    row.description = Set(package.description);
    row.manifest_json =
        Set(serde_json::to_string(&package.manifest)
            .map_err(|e| error(StatusCode::BAD_REQUEST, e))?);
    row.files_json =
        Set(serde_json::to_string(&package.files).map_err(|e| error(StatusCode::BAD_REQUEST, e))?);
    row.updated_at = Set(chrono::Utc::now().naive_utc());
    Ok(Json(PluginResponse::try_from(row.update(&state.db).await.map_err(db_error)?)?))
}
async fn delete_plugin(
    State(state): State<AppState>, Path(uid): Path<String>,
) -> ApiResult<StatusCode> {
    if uid == BUILTIN_CONTENT_TYPES_PLUGIN {
        return Err(error(StatusCode::FORBIDDEN, "Built-in plugins cannot be deleted"));
    }
    if plugin::Entity::delete_by_id(uid).exec(&state.db).await.map_err(db_error)?.rows_affected == 0
    {
        return Err(error(StatusCode::NOT_FOUND, "Plugin not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
struct EnabledInput {
    enabled: bool,
}
async fn set_enabled(
    State(state): State<AppState>, Path(uid): Path<String>, Json(input): Json<EnabledInput>,
) -> ApiResult<Json<PluginResponse>> {
    if uid == BUILTIN_CONTENT_TYPES_PLUGIN && !input.enabled {
        return Err(error(StatusCode::FORBIDDEN, "Built-in plugins cannot be disabled"));
    }
    let mut row = plugin::Entity::find_by_id(uid)
        .one(&state.db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Plugin not found"))?
        .into_active_model();
    row.enabled = Set(input.enabled);
    row.updated_at = Set(chrono::Utc::now().naive_utc());
    Ok(Json(PluginResponse::try_from(row.update(&state.db).await.map_err(db_error)?)?))
}

#[derive(Serialize)]
struct MenuItemResponse {
    plugin_uid: String,
    plugin_name: String,
    label: String,
    path: String,
    icon: Option<String>,
    position: usize,
}
async fn list_menu_items(State(state): State<AppState>) -> ApiResult<Json<Vec<MenuItemResponse>>> {
    let rows = plugin::Entity::find()
        .filter(plugin::Column::Enabled.eq(true))
        .order_by_asc(plugin::Column::Name)
        .all(&state.db)
        .await
        .map_err(db_error)?;
    let mut output = Vec::new();
    for row in rows {
        let manifest: Value = serde_json::from_str(&row.manifest_json)
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        let Some(items) = manifest
            .get("menuItems")
            .or_else(|| manifest.get("menu_items"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for (position, item) in items.iter().enumerate() {
            let label = item.get("label").and_then(Value::as_str).unwrap_or_default();
            let path = item
                .get("path")
                .or_else(|| item.get("href"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !label.is_empty() && (path.starts_with('/') || path.starts_with("https://")) {
                output.push(MenuItemResponse {
                    plugin_uid: row.uid.clone(),
                    plugin_name: row.name.clone(),
                    label: label.to_owned(),
                    path: path.to_owned(),
                    icon: item.get("icon").and_then(Value::as_str).map(str::to_owned),
                    position,
                });
            }
        }
    }
    Ok(Json(output))
}
