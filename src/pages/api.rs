use serde::Deserialize;

use super::models::{DatabaseTables, ExecutionRecord, ExecutionResult, Workflow, WorkflowPayload};
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct ApiError {
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ValidationResponse {
    valid: bool,
    errors: Option<Vec<ValidationError>>,
}

#[derive(Debug, Deserialize)]
struct ValidationError {
    message: Option<String>,
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
            "{}{}",
            std::env::var("FERRIS_WORKFLOW_API_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
            path
        )
    }
}

pub(super) async fn list_workflows() -> Result<Vec<Workflow>, String> {
    let response = reqwest::get(api_url("/api/workflows?page=1&page_size=100&sort=name&order=asc"))
        .await
        .map_err(|error| error.to_string())?;
    decode_response(response).await
}

pub(super) async fn read_workflow(id: i32) -> Result<Workflow, String> {
    let response = reqwest::get(api_url(&format!("/api/workflows/{id}")))
        .await
        .map_err(|error| error.to_string())?;
    decode_response(response).await
}

pub(super) async fn save_workflow(
    payload: WorkflowPayload, id: Option<i32>,
) -> Result<Workflow, String> {
    let client = reqwest::Client::new();
    let request = match id {
        Some(id) => client.put(api_url(&format!("/api/workflows/{id}"))),
        None => client.post(api_url("/api/workflows")),
    };
    let response = request.json(&payload).send().await.map_err(|error| error.to_string())?;
    decode_response(response).await
}

pub(super) async fn delete_workflow(id: i32) -> Result<(), String> {
    let response = reqwest::Client::new()
        .delete(api_url(&format!("/api/workflows/{id}")))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status().is_success() { Ok(()) } else { Err(response_error(response).await) }
}

pub(super) async fn validate_workflow(definition_json: String) -> Result<Vec<String>, String> {
    let response = reqwest::Client::new()
        .post(api_url("/api/workflows/validate"))
        .json(&serde_json::json!({ "definition_json": definition_json }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let validation: ValidationResponse = decode_response(response).await?;
    if validation.valid {
        Ok(Vec::new())
    } else {
        Ok(validation
            .errors
            .unwrap_or_default()
            .into_iter()
            .map(|error| error.message.unwrap_or_else(|| "Invalid OWS definition".into()))
            .collect())
    }
}

pub(super) async fn execute_workflow(
    definition_json: String, workflow_id: Option<i32>, workflow_name: Option<String>,
) -> Result<ExecutionResult, String> {
    let response = reqwest::Client::new()
        .post(api_url("/api/workflows/execute"))
        .json(&serde_json::json!({ "definition_json": definition_json, "workflow_id": workflow_id, "workflow_name": workflow_name, "input": {} }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    decode_response(response).await
}

pub(super) async fn list_executions() -> Result<Vec<ExecutionRecord>, String> {
    let response =
        reqwest::get(api_url("/api/executions")).await.map_err(|error| error.to_string())?;
    decode_response(response).await
}

pub(super) async fn list_database_tables() -> Result<DatabaseTables, String> {
    let response =
        reqwest::get(api_url("/api/database/tables")).await.map_err(|error| error.to_string())?;
    decode_response(response).await
}

#[derive(Debug, Deserialize)]
struct DatabaseListenResponse {
    event: Option<Value>,
    timeout: bool,
}

pub(super) async fn listen_database_event(table: String) -> Result<Option<Value>, String> {
    let response = reqwest::Client::new()
        .post(api_url("/api/database/events/listen"))
        .json(&serde_json::json!({ "table": table }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let response: DatabaseListenResponse = decode_response(response).await?;
    if response.timeout { Ok(None) } else { Ok(response.event) }
}

pub(crate) async fn execute_saved_workflow(
    workflow_id: i32, input_json: String,
) -> Result<ExecutionResult, String> {
    let input: serde_json::Value = serde_json::from_str(&input_json)
        .map_err(|error| format!("Input must be valid JSON: {error}"))?;
    let response = reqwest::Client::new()
        .post(api_url("/api/executions"))
        .json(&serde_json::json!({ "workflow_id": workflow_id, "input": input }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    decode_response(response).await
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, String> {
    if response.status().is_success() {
        response.json().await.map_err(|error| error.to_string())
    } else {
        Err(response_error(response).await)
    }
}

async fn response_error(response: reqwest::Response) -> String {
    let status = response.status();
    match response.json::<ApiError>().await {
        Ok(body) => body.error.unwrap_or_else(|| format!("Request failed ({status})")),
        Err(_) => format!("Request failed ({status})"),
    }
}
