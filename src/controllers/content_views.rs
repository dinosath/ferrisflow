use crate::{
    app_state::AppState,
    models::{content_entry, content_type, content_view, workflow},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use ows_core::ExpressionEngine;
use ows_expressions::JqEngine;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, Set,
};
use semver::{Version, VersionReq};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

type ApiResult<T> = Result<T, (StatusCode, Json<Value>)>;

fn error(status: StatusCode, message: impl ToString) -> (StatusCode, Json<Value>) {
    (status, Json(serde_json::json!({"error":message.to_string()})))
}

fn db_error(error: sea_orm::DbErr) -> (StatusCode, Json<Value>) {
    self::error(StatusCode::INTERNAL_SERVER_ERROR, error)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/pages", get(list_all_views))
        .route(
            "/api/pages/{record_id}",
            get(read_view_record).put(update_view_record).delete(delete_view_record),
        )
        .route("/api/content-types/{uid}/pages", get(list_views).post(create_view))
        .route(
            "/api/content-types/{uid}/pages/{view_id}",
            get(read_view).put(update_view).delete(delete_view),
        )
        .route(
            "/api/content-types/{uid}/pages/{view_id}/workflows/{slot}/execute",
            axum::routing::post(execute_view_workflow),
        )
        .route("/api/content-views", get(list_all_views))
        .route(
            "/api/content-views/{record_id}",
            get(read_view_record).put(update_view_record).delete(delete_view_record),
        )
        .route("/api/content-types/{uid}/views", get(list_views).post(create_view))
        .route(
            "/api/content-types/{uid}/views/{view_id}",
            get(read_view).put(update_view).delete(delete_view),
        )
        .route(
            "/api/content-types/{uid}/views/{view_id}/workflows/{slot}/execute",
            axum::routing::post(execute_view_workflow),
        )
}

pub(crate) async fn generate_for_type<C: ConnectionTrait>(db: &C, uid: &str) -> Result<(), String> {
    let content = content_type::Entity::find_by_id(uid.to_owned())
        .one(db)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "content type not found".to_owned())?;
    let schema: Value = serde_json::from_str(&content.schema_json).map_err(|e| e.to_string())?;
    let fields = schema.get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
    let base_id = format!("{uid}#base");
    let base_fields=fields.iter().filter_map(|field| {
        let name=field.get("name")?.as_str()?;
        Some((name.to_owned(),serde_json::json!({
            "label":field.get("label").and_then(Value::as_str).unwrap_or(name),
            "widget":widget_for(field.get("type").and_then(Value::as_str).unwrap_or("text")),
            "visible":true,"sortable":true,"searchable":true,"editable":true,
            "required":field.get("required").and_then(Value::as_bool).unwrap_or(false),"order":fields.iter().position(|item|item==field).unwrap_or(0)
        })))
    }).collect::<Map<String,Value>>();
    let base = serde_json::json!({
        "id":base_id,"source":uid,"type":"grid","extends":null,"generated":true,
        "scope":{"kind":"public","id":null},"fields":base_fields,"layout":null,
        "query":{"filters":{},"sort":["updated_at:desc"],"pageSize":20},"options":{}
    });
    save_generated_view(db, uid, &base, "Base", false, 0).await?;
    let grid_id = format!("{uid}.views.grid");
    let grid = serde_json::json!({"id":grid_id,"source":uid,"type":"grid","extends":base_id,"generated":true,"scope":null,"fields":{},"layout":null,"query":{},"options":{}});
    save_generated_view(db, uid, &grid, "All entries", true, 1).await?;
    let form_id = format!("{uid}.views.form");
    let form = serde_json::json!({"id":form_id,"source":uid,"type":"form","extends":base_id,"generated":true,"scope":null,"fields":{},"layout":null,"query":{},"options":{}});
    save_generated_view(db, uid, &form, "Entry form", false, 2).await?;

    for (name, operation) in [
        ("find", "list"),
        ("create", "create"),
        ("read", "read"),
        ("update", "update"),
        ("delete", "delete"),
    ] {
        let definition = generated_workflow(uid, name, operation, &fields);
        let definition_json = crate::ows::normalize_ows_json(&definition.to_string())
            .map_err(|e| format!("generated {name} workflow is invalid OWS: {e}"))?;
        let workflow_name = format!("view-{uid}-{name}");
        if let Some(existing) = workflow::Entity::find()
            .filter(workflow::Column::Name.eq(workflow_name.clone()))
            .one(db)
            .await
            .map_err(|e| e.to_string())?
        {
            let mut row = existing.into_active_model();
            row.definition_json = Set(definition_json);
            row.description = Set(Some(format!("Generated {operation} workflow for {uid}")));
            row.active = Set(true);
            row.last_updated = Set(chrono::Utc::now().naive_utc());
            row.update(db).await.map_err(|e| e.to_string())?;
        } else {
            workflow::ActiveModel {
                active: Set(true),
                definition_json: Set(definition_json),
                description: Set(Some(format!("Generated {operation} workflow for {uid}"))),
                name: Set(workflow_name),
                version: Set(1),
                ..Default::default()
            }
            .insert(db)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    let mut workflow_names = BTreeMap::new();
    for name in ["find", "create", "read", "update", "delete"] {
        let workflow_name = format!("view-{uid}-{name}");
        if let Some(row) = workflow::Entity::find()
            .filter(workflow::Column::Name.eq(workflow_name))
            .one(db)
            .await
            .map_err(|e| e.to_string())?
        {
            let definition: Value =
                serde_json::from_str(&row.definition_json).map_err(|e| e.to_string())?;
            let doc = definition.get("document").ok_or("generated workflow document missing")?;
            workflow_names.insert(name.to_owned(),serde_json::json!({"namespace":doc["namespace"],"name":doc["name"],"version":"^1.0"}));
        }
    }
    let mut grid = grid;
    grid["workflows"] = serde_json::json!({
        "dataSource":{"workflow":workflow_names["find"],"input":{"from":{"uid":"${ $view.source }","filters":"${ $filters }","sort":"${ $sort }","page":"${ $page }"}},"output":{"as":{"rows":"${ .rows }","total":"${ .total }"}}},
        "rowActions":[{"label":"Delete","workflow":workflow_names["delete"],"input":{"from":{"uid":"${ $view.source }","id":"${ $record.id }"}},"output":{"as":"${ . }"},"ui":{"onSuccess":[{"do":"toast","level":"success","text":"Entry deleted"},{"do":"refresh","scope":"view"}]}}]
    });
    let mut form = form;
    form["workflows"] = serde_json::json!({
        "onSubmit":{"workflow":workflow_names["create"],"input":{"from":{"uid":"${ $view.source }","data":"${ $form }","published":"${ $form.published // false }"}},"output":{"as":"${ . }"},"ui":{"onSuccess":[{"do":"toast","level":"success","text":"Entry created"},{"do":"close"},{"do":"refresh","scope":"view"}]}},
        "onSave":{"workflow":workflow_names["update"],"input":{"from":{"uid":"${ $view.source }","id":"${ $record.id }","data":"${ $changes // $form }","published":"${ $form.published // false }"}},"output":{"as":"${ . }"},"ui":{"onSuccess":[{"do":"toast","level":"success","text":"Entry updated"},{"do":"close"},{"do":"refresh","scope":"view"}]}}
    });
    save_generated_view(db, uid, &grid, "All entries", true, 1).await?;
    save_generated_view(db, uid, &form, "Entry form", false, 2).await?;
    Ok(())
}

fn widget_for(field_type: &str) -> &'static str {
    match field_type {
        "number" => "number",
        "boolean" => "checkbox",
        "date" => "date",
        "json" => "json",
        "relation" => "relation",
        _ => "text",
    }
}

async fn save_generated_view<C: ConnectionTrait>(
    db: &C, uid: &str, doc: &Value, name: &str, is_default: bool, position: i32,
) -> Result<(), String> {
    let id = doc.get("id").and_then(Value::as_str).ok_or("view id required")?;
    if let Some(row) = content_view::Entity::find()
        .filter(content_view::Column::ContentTypeUid.eq(uid))
        .filter(content_view::Column::ConfigJson.contains(id))
        .one(db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut model = row.into_active_model();
        model.name = Set(name.to_owned());
        model.view_type = Set(doc["type"].as_str().unwrap_or("grid").to_owned());
        model.config_json = Set(doc.to_string());
        model.is_default = Set(is_default);
        model.position = Set(position);
        model.updated_at = Set(chrono::Utc::now().naive_utc());
        model.update(db).await.map_err(|e| e.to_string())?;
    } else {
        content_view::ActiveModel {
            content_type_uid: Set(uid.to_owned()),
            name: Set(name.to_owned()),
            view_type: Set(doc["type"].as_str().unwrap_or("grid").to_owned()),
            config_json: Set(doc.to_string()),
            is_default: Set(is_default),
            position: Set(position),
            ..Default::default()
        }
        .insert(db)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn generated_workflow(uid: &str, name: &str, operation: &str, fields: &[Value]) -> Value {
    let workflow_uid = uid.replace('.', "-");
    let mut properties = Map::new();
    properties.insert("uid".into(), serde_json::json!({"type":"string"}));
    if operation == "list" {
        properties.insert("filters".into(), serde_json::json!({"type":"object"}));
        properties
            .insert("sort".into(), serde_json::json!({"type":"array","items":{"type":"string"}}));
        properties.insert("page".into(), serde_json::json!({"type":"object"}));
    }
    if matches!(operation, "create" | "update") {
        let mut data = Map::new();
        for field in fields {
            if let (Some(name), Some(kind)) = (
                field.get("name").and_then(Value::as_str),
                field.get("type").and_then(Value::as_str),
            ) {
                data.insert(name.into(),serde_json::json!({"type":match kind{"number"=>"number","boolean"=>"boolean",_=>"string"}}));
            }
        }
        properties.insert("data".into(), serde_json::json!({"type":"object","properties":data}));
        properties.insert("published".into(), serde_json::json!({"type":"boolean"}));
    }
    if matches!(operation, "read" | "update" | "delete") {
        properties.insert("id".into(), serde_json::json!({"type":"integer"}));
    }
    let required = match operation {
        "create" => vec!["uid", "data"],
        "update" => vec!["uid", "id", "data"],
        "read" | "delete" => vec!["uid", "id"],
        _ => vec!["uid"],
    };
    let output_schema = if operation == "list" {
        serde_json::json!({"type":"object","required":["rows","total"],"properties":{"rows":{"type":"array","items":{"type":"object"}},"total":{"type":"integer"}}})
    } else {
        serde_json::json!({"type":"object"})
    };
    let output_mapping = if operation == "list" {
        serde_json::json!("${ { rows: ., total: length(.) } }")
    } else {
        serde_json::json!("${ . }")
    };
    serde_json::json!({
        "document":{"dsl":"1.0.3","namespace":"content","name":format!("{name}-{workflow_uid}"),"version":"1.0.0","metadata":{"exposedTo":["view"],"ferrisflow":{"generated":true,"contentType":uid,"operation":operation}}},
        "input":{"schema":{"format":"json","document":{"type":"object","required":required,"properties":properties}}},
        "do":[{"databaseAction":{"call":"ferrisflow.plugin.database","with":{"resource":"content-records","operation":operation,"input":"${ . }"}}}],
        "output":{"schema":{"format":"json","document":output_schema},"as":output_mapping}
    })
}

async fn list_views(
    State(state): State<AppState>, Path(uid): Path<String>,
) -> ApiResult<Json<Value>> {
    let rows = content_view::Entity::find()
        .filter(content_view::Column::ContentTypeUid.eq(uid))
        .order_by_asc(content_view::Column::Position)
        .all(&state.db)
        .await
        .map_err(db_error)?;
    let mut views = Vec::new();
    for row in rows {
        let mut doc: Value = serde_json::from_str(&row.config_json)
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        doc["name"] = Value::String(row.name);
        doc["isDefault"] = Value::Bool(row.is_default);
        views.push(doc);
    }
    Ok(Json(Value::Array(views)))
}

async fn list_all_views(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let rows = content_view::Entity::find()
        .order_by_asc(content_view::Column::ContentTypeUid)
        .order_by_asc(content_view::Column::Position)
        .all(&state.db)
        .await
        .map_err(db_error)?;
    let docs = rows
        .into_iter()
        .map(|row| {
            let mut doc: Value = serde_json::from_str(&row.config_json)
                .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
            doc["recordId"] = Value::from(row.id);
            doc["name"] = Value::String(row.name);
            doc["source"] = Value::String(row.content_type_uid);
            doc["isDefault"] = Value::Bool(row.is_default);
            doc["updatedAt"] = serde_json::json!(row.updated_at);
            Ok(doc)
        })
        .collect::<ApiResult<Vec<_>>>()?;
    Ok(Json(Value::Array(docs)))
}

async fn read_view_record(
    State(state): State<AppState>, Path(record_id): Path<i32>,
) -> ApiResult<Json<Value>> {
    let row = content_view::Entity::find_by_id(record_id)
        .one(&state.db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "view not found"))?;
    let mut doc: Value = serde_json::from_str(&row.config_json)
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    doc["recordId"] = Value::from(row.id);
    doc["name"] = Value::String(row.name);
    doc["source"] = Value::String(row.content_type_uid);
    doc["isDefault"] = Value::Bool(row.is_default);
    doc["updatedAt"] = serde_json::json!(row.updated_at);
    Ok(Json(doc))
}

async fn update_view_record(
    State(state): State<AppState>, Path(record_id): Path<i32>, Json(doc): Json<Value>,
) -> ApiResult<Json<Value>> {
    let row = content_view::Entity::find_by_id(record_id)
        .one(&state.db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "view not found"))?;
    let expected_id = row_view_id(&row.config_json);
    validate_view(&state, &row.content_type_uid, &doc, expected_id.as_deref()).await?;
    let mut model = row.into_active_model();
    model.name = Set(doc.get("name").and_then(Value::as_str).unwrap_or("View").to_owned());
    model.view_type = Set(doc.get("type").and_then(Value::as_str).unwrap_or("grid").to_owned());
    model.config_json = Set(doc.to_string());
    model.is_default = Set(doc.get("isDefault").and_then(Value::as_bool).unwrap_or(false));
    model.position = Set(doc.get("position").and_then(Value::as_i64).unwrap_or(0) as i32);
    model.updated_at = Set(chrono::Utc::now().naive_utc());
    model.update(&state.db).await.map_err(db_error)?;
    Ok(Json(serde_json::json!({"recordId":record_id,"saved":true})))
}

async fn delete_view_record(
    State(state): State<AppState>, Path(record_id): Path<i32>,
) -> ApiResult<StatusCode> {
    if content_view::Entity::delete_by_id(record_id)
        .exec(&state.db)
        .await
        .map_err(db_error)?
        .rows_affected
        == 0
    {
        return Err(error(StatusCode::NOT_FOUND, "view not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn row_view_id(config_json: &str) -> Option<String> {
    serde_json::from_str::<Value>(config_json).ok()?.get("id")?.as_str().map(str::to_owned)
}

async fn read_view(
    State(state): State<AppState>, Path((uid, view_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let rows = content_view::Entity::find()
        .filter(content_view::Column::ContentTypeUid.eq(uid))
        .all(&state.db)
        .await
        .map_err(db_error)?;
    let row = rows
        .into_iter()
        .find(|row| {
            serde_json::from_str::<Value>(&row.config_json)
                .ok()
                .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
                .as_deref()
                == Some(view_id.as_str())
        })
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "view not found"))?;
    let mut doc: Value = serde_json::from_str(&row.config_json)
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    doc["isDefault"] = Value::Bool(row.is_default);
    Ok(Json(doc))
}

async fn create_view(
    State(state): State<AppState>, Path(uid): Path<String>, Json(input): Json<Value>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    validate_view(&state, &uid, &input, None).await?;
    let id = input["id"].as_str().unwrap_or_default().to_owned();
    let name = input.get("name").and_then(Value::as_str).unwrap_or(&id).to_owned();
    let is_default = input.get("isDefault").and_then(Value::as_bool).unwrap_or(false);
    let position = input.get("position").and_then(Value::as_i64).unwrap_or(0) as i32;
    let row = content_view::ActiveModel {
        content_type_uid: Set(uid),
        name: Set(name),
        view_type: Set(input["type"].as_str().unwrap_or("grid").to_owned()),
        config_json: Set(input.to_string()),
        is_default: Set(is_default),
        position: Set(position),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .map_err(db_error)?;
    Ok((
        StatusCode::CREATED,
        Json(
            serde_json::json!({"id":id,"recordId":row.id,"source":row.content_type_uid,"stored":true}),
        ),
    ))
}

async fn update_view(
    State(state): State<AppState>, Path((uid, view_id)): Path<(String, String)>,
    Json(doc): Json<Value>,
) -> ApiResult<Json<Value>> {
    validate_view(&state, &uid, &doc, Some(&view_id)).await?;
    let rows = content_view::Entity::find()
        .filter(content_view::Column::ContentTypeUid.eq(uid.clone()))
        .all(&state.db)
        .await
        .map_err(db_error)?;
    let row = rows
        .into_iter()
        .find(|row| {
            serde_json::from_str::<Value>(&row.config_json)
                .ok()
                .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
                .as_deref()
                == Some(view_id.as_str())
        })
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "view not found"))?;
    let mut model = row.into_active_model();
    model.name = Set(doc.get("name").and_then(Value::as_str).unwrap_or(&view_id).to_owned());
    model.view_type = Set(doc["type"].as_str().unwrap_or("grid").to_owned());
    model.config_json = Set(doc.to_string());
    model.is_default = Set(doc.get("isDefault").and_then(Value::as_bool).unwrap_or(false));
    model.position = Set(doc.get("position").and_then(Value::as_i64).unwrap_or(0) as i32);
    model.updated_at = Set(chrono::Utc::now().naive_utc());
    model.update(&state.db).await.map_err(db_error)?;
    Ok(Json(serde_json::json!({"id":view_id,"saved":true})))
}

async fn delete_view(
    State(state): State<AppState>, Path((uid, view_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let rows = content_view::Entity::find()
        .filter(content_view::Column::ContentTypeUid.eq(uid))
        .all(&state.db)
        .await
        .map_err(db_error)?;
    let Some(row) = rows.into_iter().find(|row| {
        serde_json::from_str::<Value>(&row.config_json)
            .ok()
            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
            .as_deref()
            == Some(view_id.as_str())
    }) else {
        return Err(error(StatusCode::NOT_FOUND, "view not found"));
    };
    content_view::Entity::delete_by_id(row.id).exec(&state.db).await.map_err(db_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn validate_view(
    state: &AppState, uid: &str, doc: &Value, expected_id: Option<&str>,
) -> ApiResult<()> {
    if !doc.is_object()
        || doc.get("id").and_then(Value::as_str).is_none_or(str::is_empty)
        || doc.get("source").and_then(Value::as_str) != Some(uid)
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "view requires id and matching content type source",
        ));
    }
    if expected_id.is_some_and(|id| doc["id"].as_str() != Some(id)) {
        return Err(error(StatusCode::BAD_REQUEST, "view id cannot be changed"));
    }
    let kinds = ["grid", "kanban", "gallery", "calendar", "form"];
    if !kinds.contains(&doc.get("type").and_then(Value::as_str).unwrap_or_default()) {
        return Err(error(StatusCode::BAD_REQUEST, "unsupported view type"));
    }
    let standalone = uid == "core.page";
    let show_content = doc.pointer("/options/showContent").and_then(Value::as_bool).unwrap_or(true);
    let names = if standalone {
        if show_content {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "standalone pages cannot show a content data view",
            ));
        }
        Vec::new()
    } else {
        let content = content_type::Entity::find_by_id(uid.to_owned())
            .one(&state.db)
            .await
            .map_err(db_error)?
            .ok_or_else(|| error(StatusCode::NOT_FOUND, "content type not found"))?;
        let schema: Value = serde_json::from_str(&content.schema_json)
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        schema
            .pointer("/fields")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|f| f.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect::<Vec<_>>()
    };
    if let Some(fields) = doc.get("fields").and_then(Value::as_object) {
        for name in fields.keys() {
            if !names.iter().any(|field| field == name) && name != "id" && name != "published" {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    format!("unknown content type field '{name}'"),
                ));
            }
        }
    }
    if let Some(parent) = doc.get("extends").and_then(Value::as_str) {
        if parent == doc["id"].as_str().unwrap_or("") {
            return Err(error(StatusCode::BAD_REQUEST, "view cannot extend itself"));
        }
    }
    let generated_base = doc.get("generated").and_then(Value::as_bool) == Some(true)
        && doc.get("extends").is_some_and(Value::is_null);
    if !generated_base && show_content {
        let workflows = doc.get("workflows");
        let associated = if doc["type"].as_str() == Some("form") {
            workflows
                .and_then(|value| value.get("onSubmit"))
                .and_then(|value| value.get("workflow"))
                .or_else(|| {
                    workflows
                        .and_then(|value| value.get("onSave"))
                        .and_then(|value| value.get("workflow"))
                })
        } else {
            workflows
                .and_then(|value| value.get("dataSource"))
                .and_then(|value| value.get("workflow"))
        };
        if associated.is_none() {
            return Err(error(
                StatusCode::UNPROCESSABLE_ENTITY,
                if doc["type"].as_str() == Some("form") {
                    "form views require an onSubmit or onSave workflow"
                } else {
                    "views require a dataSource workflow"
                },
            ));
        }
    }
    if let Some(workflows) = doc.get("workflows").and_then(Value::as_object) {
        for (slot, value) in workflows {
            let configs =
                value.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(value));
            for config in configs {
                let reference = config.get("workflow").ok_or_else(|| {
                    error(StatusCode::BAD_REQUEST, format!("workflows.{slot}.workflow is required"))
                })?;
                let resolved = resolve_workflow(&state.db, reference).await?;
                if !exposed_to_view(&resolved) {
                    return Err(error(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!("workflows.{slot} must reference a workflow exposedTo view"),
                    ));
                }
                for path in ["/input/from", "/output/as", "/ui"] {
                    if let Some(mapping) = config.pointer(path) {
                        validate_mapping_syntax(mapping).map_err(|(status, Json(mut issue))| {
                            issue["path"] = Value::String(format!("/workflows/{slot}{path}"));
                            (status, Json(issue))
                        })?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_mapping_syntax(value: &Value) -> ApiResult<()> {
    match value {
        Value::String(text) if text.trim_start().starts_with("${") => JqEngine
            .compile(text)
            .map(|_| ())
            .map_err(|e| error(StatusCode::BAD_REQUEST, format!("invalid jq expression: {e}"))),
        Value::Object(items) => {
            for value in items.values() {
                validate_mapping_syntax(value)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for value in items {
                validate_mapping_syntax(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

struct ResolvedWorkflow {
    id: i32,
    definition: Value,
    version: Version,
}

async fn execute_view_workflow(
    State(state): State<AppState>, Path((uid, view_id, slot)): Path<(String, String, String)>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    let views = content_view::Entity::find()
        .filter(content_view::Column::ContentTypeUid.eq(uid.clone()))
        .all(&state.db)
        .await
        .map_err(db_error)?;
    let view = views
        .into_iter()
        .find_map(|row| {
            serde_json::from_str::<Value>(&row.config_json)
                .ok()
                .filter(|view| view.get("id").and_then(Value::as_str) == Some(&view_id))
        })
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "view not found"))?;
    let mut workflow_config = view
        .pointer(&format!("/workflows/{}", slot.replace('~', "~0").replace('/', "~1")))
        .cloned()
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "view workflow slot not found"))?;
    if let Some(actions) = workflow_config.as_array() {
        let index = request.get("actionIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
        workflow_config = actions
            .get(index)
            .cloned()
            .ok_or_else(|| error(StatusCode::NOT_FOUND, "view workflow action not found"))?;
    }
    let workflow_ref = workflow_config
        .get("workflow")
        .ok_or_else(|| error(StatusCode::CONFLICT, "view workflow reference is missing"))?;
    let resolved = resolve_workflow(&state.db, workflow_ref).await?;
    if !resolved
        .definition
        .pointer("/document/metadata/exposedTo")
        .and_then(Value::as_array)
        .is_some_and(|targets| targets.iter().any(|target| target.as_str() == Some("view")))
    {
        return Err(error(StatusCode::FORBIDDEN, "workflow is not exposed to views"));
    }
    if resolved.definition.pointer("/document/metadata/permission").is_some() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "this workflow requires permissions, but no authenticated permission context is configured",
        ));
    }

    let record = if let Some(id) = request.get("recordId").and_then(Value::as_i64) {
        let id =
            i32::try_from(id).map_err(|_| error(StatusCode::BAD_REQUEST, "invalid recordId"))?;
        let row = content_entry::Entity::find_by_id(id)
            .filter(content_entry::Column::ContentTypeUid.eq(uid.clone()))
            .one(&state.db)
            .await
            .map_err(db_error)?
            .ok_or_else(|| error(StatusCode::NOT_FOUND, "record not found"))?;
        let data: Value = serde_json::from_str(&row.data_json)
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        serde_json::json!({"id":row.id,"data":data,"published":row.published,"created_at":row.created_at,"updated_at":row.updated_at})
    } else {
        Value::Null
    };
    let context = serde_json::json!({
        "view":{"id":view_id,"source":uid,"type":view["type"],"options":view["options"]},
        "record":record,
        "selection":request.get("selection").cloned().unwrap_or_else(||serde_json::json!({"ids":[],"count":0})),
        "form":request.get("form").cloned().unwrap_or(Value::Null),
        "changes":request.get("changes").cloned().unwrap_or(Value::Null),
        "event":request.get("event").cloned().unwrap_or(Value::Null),
        "filters":request.get("filters").cloned().unwrap_or_else(||serde_json::json!({})),
        "sort":request.get("sort").cloned().unwrap_or_else(||serde_json::json!([])),
        "page":request.get("page").cloned().unwrap_or_else(||serde_json::json!({})),
        "params":request.get("params").cloned().unwrap_or_else(||serde_json::json!({}))
    });
    let mut variables = context.as_object().cloned().unwrap_or_default();
    let input = if let Some(mapping) = workflow_config.pointer("/input/from") {
        evaluate_mapping(mapping, &context, &variables)
            .map_err(|e| error(StatusCode::UNPROCESSABLE_ENTITY, e))?
    } else {
        context.clone()
    };
    let definition_json = serde_json::to_string(&resolved.definition)
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let run = crate::controllers::workflow::run_ows(
        &state,
        crate::controllers::workflow::ExecuteRequest {
            definition_json,
            input: Some(input),
            workflow_id: Some(resolved.id),
            workflow_name: resolved.definition["document"]["name"].as_str().map(str::to_owned),
        },
    )
    .await?;
    let execution = run.0;
    let success = execution.get("success").and_then(Value::as_bool).unwrap_or(false);
    let workflow_output = execution.get("output").cloned().unwrap_or(Value::Null);
    let result = if let Some(mapping) = workflow_config.pointer("/output/as") {
        variables.insert("result".into(), workflow_output.clone());
        evaluate_mapping(mapping, &workflow_output, &variables)
            .map_err(|e| error(StatusCode::UNPROCESSABLE_ENTITY, e))?
    } else {
        workflow_output
    };
    variables.insert("result".into(), result.clone());
    let ui = if let Some(ui) = workflow_config.get("ui") {
        evaluate_mapping(ui, &result, &variables)
            .map_err(|e| error(StatusCode::UNPROCESSABLE_ENTITY, e))?
    } else {
        Value::Null
    };
    Ok(Json(serde_json::json!({"success":success,"execution":execution,"result":result,"ui":ui})))
}

fn evaluate_mapping(
    value: &Value, root: &Value, variables: &Map<String, Value>,
) -> Result<Value, String> {
    match value {
        Value::String(text) if text.trim_start().starts_with("${") => JqEngine
            .compile(text)
            .and_then(|compiled| JqEngine.evaluate(&compiled, root, variables))
            .map_err(|e| e.to_string()),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| Ok((key.clone(), evaluate_mapping(value, root, variables)?)))
            .collect::<Result<Map<_, _>, String>>()
            .map(Value::Object),
        Value::Array(values) => values
            .iter()
            .map(|value| evaluate_mapping(value, root, variables))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        _ => Ok(value.clone()),
    }
}
async fn resolve_workflow<C: ConnectionTrait>(
    db: &C, reference: &Value,
) -> ApiResult<ResolvedWorkflow> {
    let namespace = reference.get("namespace").and_then(Value::as_str).unwrap_or_default();
    let name = reference.get("name").and_then(Value::as_str).unwrap_or_default();
    let range = reference.get("version").and_then(Value::as_str).unwrap_or("*");
    if namespace.is_empty() || name.is_empty() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "workflow reference needs namespace, name, and version",
        ));
    }
    let requirement = VersionReq::parse(range).map_err(|e| {
        error(StatusCode::BAD_REQUEST, format!("invalid workflow version range: {e}"))
    })?;
    let rows = workflow::Entity::find()
        .filter(workflow::Column::Active.eq(true))
        .all(db)
        .await
        .map_err(db_error)?;
    let mut matches = Vec::new();
    for row in rows {
        let Ok(definition) = serde_json::from_str::<Value>(&row.definition_json) else { continue };
        let document = &definition["document"];
        if document["namespace"].as_str() != Some(namespace)
            || document["name"].as_str() != Some(name)
        {
            continue;
        }
        let Some(version) = document["version"].as_str().and_then(|v| Version::parse(v).ok())
        else {
            continue;
        };
        if requirement.matches(&version) {
            matches.push(ResolvedWorkflow { id: row.id, definition, version });
        }
    }
    matches.sort_by(|a, b| b.version.cmp(&a.version));
    matches.into_iter().next().ok_or_else(|| {
        error(StatusCode::NOT_FOUND, "no active workflow version matches this view workflow")
    })
}

fn exposed_to_view(workflow: &ResolvedWorkflow) -> bool {
    workflow
        .definition
        .pointer("/document/metadata/exposedTo")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some("view")))
}
