use crate::app_state::AppState;
use crate::models::execution::{
    ActiveModel as ExecutionActiveModel, Entity as ExecutionEntity, Model as ExecutionModel,
};
use crate::models::workflow::{
    Column, Entity, Model, WorkflowCreate, WorkflowResponse, WorkflowUpdate,
};
use crate::ows::{normalize_ows_json, parse_ows_document};
use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    response::IntoResponse,
};
use ows_runtime::Runtime;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use tracing::debug;
use validator::Validate;
/// Maximum number of items per page (hard cap).
const MAX_PAGE_SIZE: u64 = 100;

/// Pagination and filtering query parameters for list endpoints.
#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_page")]
    pub page: u64,
    #[serde(default = "default_page_size")]
    pub page_size: u64,
    /// Case-insensitive substring search on the primary name/title field.
    pub search: Option<String>,
    /// Sort field: `id`, `created_at` (default), or `last_updated`.
    pub sort: Option<String>,
    /// Sort direction: `asc` or `desc` (default).
    pub order: Option<String>,
}
fn default_page() -> u64 {
    1
}
fn default_page_size() -> u64 {
    25
}

async fn load_item<C>(db: &C, item_id: i32) -> Result<Model, (StatusCode, Json<serde_json::Value>)>
where
    C: ConnectionTrait,
{
    debug!("Loading workflow with id: {}", item_id);
    Entity::find_by_id(item_id)
        .one(db)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))))
}
pub async fn list(
    State(state): State<AppState>, Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    debug!("Fetching list of workflows");
    let page_size = params.page_size.clamp(1, MAX_PAGE_SIZE);
    let page = params.page.max(1);
    let offset = (page.saturating_sub(1)) * page_size;

    let total = if let Some(ref s) = params.search {
        Entity::find().filter(Column::Name.contains(s.as_str())).count(&state.db).await.map_err(
            |e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
            },
        )?
    } else {
        Entity::find().count(&state.db).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
        })?
    };
    let mut q = Entity::find();
    if let Some(ref s) = params.search {
        q = q.filter(Column::Name.contains(s.as_str()));
    }
    let sort_column = match params.sort.as_deref() {
        Some("name") => Column::Name,
        Some("active") => Column::Active,
        Some("definition_json") => Column::DefinitionJson,
        Some("description") => Column::Description,
        Some("version") => Column::Version,
        Some("created_at") => Column::CreatedAt,
        Some("last_updated") => Column::LastUpdated,
        _ => Column::Id,
    };
    q = if params.order.as_deref() == Some("desc") {
        q.order_by_desc(sort_column)
    } else {
        q.order_by_asc(sort_column)
    };
    q = q.order_by_asc(Column::Id);
    let models = q.limit(page_size).offset(offset).all(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
    })?;

    debug!("Loaded {} workflows", models.len());
    let responses: Vec<WorkflowResponse> = models.into_iter().map(Into::into).collect();

    let count = responses.len() as u64;
    let start: u64 = offset;
    let end: u64 = if count == 0 { start } else { start + count - 1 };
    let mut headers = HeaderMap::new();
    let content_range_value = format!("items {}-{}/{}", start, end, total);
    if let Ok(val) = HeaderValue::from_str(&content_range_value) {
        headers.insert("Content-Range", val);
    }
    headers.insert("Accept-Ranges", HeaderValue::from_static("items"));
    Ok((StatusCode::OK, headers, Json(responses)))
}

pub async fn create(
    State(state): State<AppState>, Json(create): Json<WorkflowCreate>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    create.validate().map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()})))
    })?;
    let mut create = create;
    create.definition_json = normalize_ows_json(&create.definition_json)
        .map_err(|error| ows_error(StatusCode::UNPROCESSABLE_ENTITY, error))?;
    debug!("Creating new OWS workflow - Request: {:?}", create);

    let active_model = create.into_active_model();
    let model = active_model.insert(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
    })?;
    reconcile_database_triggers(&state).await?;
    debug!("Created workflow with id: {:?}", model.id);
    let resp: WorkflowResponse = model.into();
    Ok((StatusCode::CREATED, Json(resp)))
}

pub async fn update(
    State(state): State<AppState>, Path(item_id): Path<i32>, Json(update): Json<WorkflowUpdate>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    update.validate().map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()})))
    })?;
    let mut update = update;
    update.definition_json = normalize_ows_json(&update.definition_json)
        .map_err(|error| ows_error(StatusCode::UNPROCESSABLE_ENTITY, error))?;
    debug!("Updating OWS workflow with id: {}", item_id);

    let _ = load_item(&state.db, item_id).await?;
    let mut active_model = update.into_active_model();
    active_model.id = Set(item_id);
    let model = active_model.update(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
    })?;
    reconcile_database_triggers(&state).await?;
    debug!("Successfully updated workflow with id: {}", item_id);
    let resp: WorkflowResponse = model.into();
    Ok(Json(resp))
}

pub async fn remove(
    State(state): State<AppState>, Path(item_id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    debug!("Deleting workflow with id: {}", item_id);

    let model = load_item(&state.db, item_id).await?;
    model.into_active_model().delete(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
    })?;
    reconcile_database_triggers(&state).await?;
    debug!("Successfully deleted workflow with id: {}", item_id);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn read_one(
    State(state): State<AppState>, Path(item_id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    debug!("Fetching workflow with id: {}", item_id);

    let model = load_item(&state.db, item_id).await?;
    debug!("Successfully fetched workflow with id: {}", item_id);
    let resp: WorkflowResponse = model.into();
    Ok(Json(resp))
}

#[derive(Debug, Deserialize)]
pub struct ValidateRequest {
    pub definition_json: String,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteRequest {
    pub definition_json: String,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub workflow_id: Option<i32>,
    #[serde(default)]
    pub workflow_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunSavedWorkflowRequest {
    pub workflow_id: i32,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ExecutionResponse {
    pub id: i32,
    pub execution_id: String,
    pub workflow_id: Option<i32>,
    pub workflow_name: String,
    pub status: String,
    pub started_at: chrono::NaiveDateTime,
    pub finished_at: Option<chrono::NaiveDateTime>,
    pub duration_ms: Option<i64>,
    pub input_json: String,
    pub output_json: Option<String>,
    pub error: Option<String>,
    pub logs: Vec<ExecutionStepResponse>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExecutionStepResponse {
    pub name: String,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub input_json: Option<String>,
    pub output_json: Option<String>,
}

impl From<ExecutionModel> for ExecutionResponse {
    fn from(model: ExecutionModel) -> Self {
        Self {
            id: model.id,
            execution_id: model.id.to_string(),
            workflow_id: model.workflow_id,
            workflow_name: model.workflow_name,
            status: model.status,
            started_at: model.started_at,
            finished_at: model.finished_at,
            duration_ms: model.duration_ms,
            input_json: model.input_json,
            output_json: model.output_json,
            error: model.error,
            logs: serde_json::from_str(&model.logs_json).unwrap_or_default(),
        }
    }
}

pub async fn list_executions(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let models = ExecutionEntity::find()
        .order_by_desc(crate::models::execution::Column::StartedAt)
        .limit(500)
        .all(&state.db)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": error.to_string()})),
            )
        })?;
    let items: Vec<ExecutionResponse> = models.into_iter().map(Into::into).collect();
    Ok(Json(items))
}

pub async fn execute_saved_workflow(
    State(state): State<AppState>, Json(request): Json<RunSavedWorkflowRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let workflow = load_item(&state.db, request.workflow_id).await?;
    run_ows(
        &state,
        ExecuteRequest {
            definition_json: workflow.definition_json,
            input: request.input,
            workflow_id: Some(workflow.id),
            workflow_name: Some(workflow.name),
        },
    )
    .await
}

/// Validate an OWS document without persisting it. This powers the editor's
/// validate action and gives API clients a stable validation contract.
pub async fn validate_ows(
    Json(request): Json<ValidateRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match parse_ows_document(&request.definition_json) {
        Ok(definition) => Ok(Json(serde_json::json!({
            "valid": true,
            "definition": definition,
        }))),
        Err(error) => Ok(Json(serde_json::json!({
            "valid": false,
            "errors": [{ "message": error }],
        }))),
    }
}

async fn reconcile_database_triggers(
    state: &AppState,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    crate::db_triggers::reconcile(state).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("could not configure database triggers: {error}")})),
        )
    })
}

pub async fn database_tables(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let (backend, tables) = crate::db_triggers::list_tables(&state.db).await.map_err(|error| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": error})))
    })?;
    Ok(Json(serde_json::json!({ "backend": backend, "tables": tables })))
}

#[derive(Debug, Deserialize)]
pub struct ListenForDatabaseEventRequest {
    pub table: String,
}

pub async fn listen_for_database_event(
    State(state): State<AppState>, Json(request): Json<ListenForDatabaseEventRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let (_, tables) = crate::db_triggers::list_tables(&state.db).await.map_err(|error| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": error})))
    })?;
    if !tables.iter().any(|table| table == &request.table) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "select a table from the connected database"})),
        ));
    }
    let event =
        crate::db_triggers::listen_for_event(&state.db, &request.table).await.map_err(|error| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": error})))
        })?;
    let timeout = event.is_none();
    Ok(Json(serde_json::json!({"event": event, "timeout": timeout})))
}

/// Executes an OWS workflow definition with the Open Workflow runtime.
pub async fn execute_ows(
    State(state): State<AppState>, Json(request): Json<ExecuteRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    run_ows(&state, request).await
}

/// Dispatch an unmatched HTTP request to an active workflow whose
/// `document.metadata.triggers` binding declares the same method and path.
async fn dispatch_http_trigger(
    State(state): State<AppState>, request: Request<Body>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let method = request.method().as_str().to_ascii_uppercase();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    let headers = request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.to_string(), serde_json::Value::String(value.to_string())))
        })
        .collect::<serde_json::Map<_, _>>();
    let body = to_bytes(request.into_body(), 2 * 1024 * 1024).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("could not read request body: {error}")})),
        )
    })?;
    let body = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or_else(|_| {
            serde_json::Value::String(String::from_utf8_lossy(&body).into_owned())
        })
    };

    let workflows =
        Entity::find().filter(Column::Active.eq(true)).all(&state.db).await.map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": error.to_string()})),
            )
        })?;
    let matches = workflows
        .into_iter()
        .filter_map(|workflow| {
            let definition =
                serde_json::from_str::<serde_json::Value>(&workflow.definition_json).ok()?;
            let trigger_matches = definition
                .get("document")
                .and_then(|document| document.get("metadata"))
                .and_then(|metadata| metadata.get("triggers"))
                .and_then(serde_json::Value::as_array)
                .is_some_and(|triggers| {
                    triggers.iter().any(|trigger| {
                        trigger.get("type").and_then(serde_json::Value::as_str) == Some("http")
                            && trigger
                                .get("method")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|value| value.eq_ignore_ascii_case(&method))
                            && trigger.get("path").and_then(serde_json::Value::as_str)
                                == Some(path.as_str())
                    })
                });
            trigger_matches.then_some(workflow)
        })
        .collect::<Vec<_>>();

    let workflow = match matches.as_slice() {
        [] => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "no active workflow matches this HTTP trigger"})),
            ));
        }
        [workflow] => workflow,
        _ => {
            return Err((
                StatusCode::CONFLICT,
                Json(
                    serde_json::json!({"error": "multiple active workflows match this HTTP trigger"}),
                ),
            ));
        }
    };

    run_ows(
        &state,
        ExecuteRequest {
            definition_json: workflow.definition_json.clone(),
            input: Some(serde_json::json!({
                "request": {
                    "method": method,
                    "path": path,
                    "query": query,
                    "headers": headers,
                    "body": body
                }
            })),
            workflow_id: Some(workflow.id),
            workflow_name: Some(workflow.name.clone()),
        },
    )
    .await
}

pub(crate) async fn run_ows(
    state: &AppState, request: ExecuteRequest,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let input = request.input.unwrap_or_else(|| serde_json::json!({}));
    let workflow_name = request
        .workflow_name
        .clone()
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&request.definition_json)
                .ok()
                .and_then(|value| value.get("document")?.get("name")?.as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| "Workflow".to_string());
    let started_at = chrono::Utc::now().naive_utc();
    let input_json = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
    let execution = ExecutionActiveModel {
        id: sea_orm::NotSet,
        workflow_id: Set(request.workflow_id),
        workflow_name: Set(workflow_name.clone()),
        status: Set("running".to_string()),
        started_at: Set(started_at),
        finished_at: Set(None),
        duration_ms: Set(None),
        input_json: Set(input_json.clone()),
        output_json: Set(None),
        error: Set(None),
        definition_json: Set(request.definition_json.clone()),
        logs_json: Set("[]".to_string()),
    };
    let execution = execution.insert(&state.db).await.map_err(|error| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": error.to_string()})))
    })?;
    let clock = std::time::Instant::now();

    let result = async {
        let definition = parse_ows_document(&request.definition_json)?;
        let allowed_hosts = workflow_endpoint_hosts(&request.definition_json);
        let policy = ows_runtime_core::RuntimePolicy {
            allow_network: !allowed_hosts.is_empty(),
            allowed_hosts,
            allowed_schemes: vec!["http".to_string(), "https".to_string()],
            ..Default::default()
        };
        let runtime = Runtime::builder()
            .with_policy(policy)
            .build()
            .map_err(|error| format!("could not start OWS runtime: {error}"))?;
        let workflow = runtime
            .register_definition(&definition)
            .map_err(|error| format!("could not compile OWS workflow: {error}"))?;
        let handle = runtime
            .execute(workflow, input.clone())
            .await
            .map_err(|error| format!("could not start OWS execution: {error}"))?;
        let runtime_execution_id = handle.execution_id.clone();
        let outcome = match handle.wait().await {
            Ok(output) => (true, Some(output), None),
            Err(error) => (false, None, Some(error.to_string())),
        };
        let task_order = runtime.take_task_order();
        Ok::<_, String>((runtime_execution_id, outcome, task_order))
    }
    .await;

    let (runtime_execution_id, success, output, error, task_order) = match result {
        Ok((runtime_id, (success, output, error), task_order)) => {
            (Some(runtime_id), success, output, error, task_order)
        }
        Err(error) => (None, false, None, Some(error), Vec::new()),
    };
    let finished_at = chrono::Utc::now().naive_utc();
    let duration_ms = i64::try_from(clock.elapsed().as_millis()).unwrap_or(i64::MAX);
    let output_json = output.as_ref().and_then(|value| serde_json::to_string(value).ok());
    let logs = task_order
        .iter()
        .enumerate()
        .map(|(index, name)| ExecutionStepResponse {
            name: name.clone(),
            status: if success || index + 1 < task_order.len() { "succeeded" } else { "failed" }
                .to_string(),
            duration_ms: None,
            input_json: (index == 0).then(|| input_json.clone()),
            output_json: (index + 1 == task_order.len()).then(|| output_json.clone()).flatten(),
        })
        .collect::<Vec<_>>();
    let logs_json = serde_json::to_string(&logs).unwrap_or_else(|_| "[]".to_string());

    let mut active = execution.clone().into_active_model();
    active.status = Set(if success { "succeeded" } else { "failed" }.to_string());
    active.finished_at = Set(Some(finished_at));
    active.duration_ms = Set(Some(duration_ms));
    active.output_json = Set(output_json.clone());
    active.error = Set(error.clone());
    active.logs_json = Set(logs_json.clone());
    active.update(&state.db).await.map_err(|update_error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": update_error.to_string()})),
        )
    })?;

    let execution_id = runtime_execution_id.unwrap_or_else(|| execution.id.to_string());
    Ok(Json(serde_json::json!({
        "success": success,
        "id": execution.id,
        "execution_id": execution_id,
        "workflow_id": request.workflow_id,
        "workflow_name": workflow_name,
        "status": if success { "succeeded" } else { "failed" },
        "started_at": started_at,
        "duration_ms": duration_ms,
        "input": input,
        "output": output,
        "error": error,
        "logs": logs,
    })))
}

fn workflow_endpoint_hosts(definition_json: &str) -> Vec<String> {
    fn collect(value: &serde_json::Value, hosts: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    if key == "endpoint" {
                        if let Some(endpoint) = value.as_str() {
                            if let Ok(url) = url::Url::parse(endpoint)
                                && matches!(url.scheme(), "http" | "https")
                                && let Some(host) = url.host_str()
                                && !host.is_empty()
                                && !hosts.iter().any(|existing| existing == host)
                            {
                                hosts.push(host.to_string());
                            }
                        }
                    }
                    collect(value, hosts);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, hosts);
                }
            }
            _ => {}
        }
    }
    let mut hosts = Vec::new();
    if let Ok(value) = serde_json::from_str(definition_json) {
        collect(&value, &mut hosts);
    }
    hosts
}

fn ows_error(status: StatusCode, message: String) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": message,
            "code": "invalid_ows_definition",
        })),
    )
}

pub fn routes() -> axum::Router<AppState> {
    use axum::routing::get;
    let router = axum::Router::new()
        .route("/api/workflows", get(list).post(create))
        .route("/api/workflows/validate", axum::routing::post(validate_ows))
        .route("/api/database/tables", get(database_tables))
        .route("/api/database/events/listen", axum::routing::post(listen_for_database_event))
        .route("/api/workflows/execute", axum::routing::post(execute_ows))
        .route("/api/executions", get(list_executions).post(execute_saved_workflow))
        .route("/api/workflows/{id}", get(read_one).put(update).delete(remove))
        .fallback(dispatch_http_trigger);

    router
}
