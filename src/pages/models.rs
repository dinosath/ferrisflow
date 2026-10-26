use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Workflow {
    pub(crate) id: i32,
    pub(crate) active: bool,
    pub(crate) definition_json: String,
    pub(crate) description: Option<String>,
    pub(crate) name: String,
    pub(crate) version: i32,
    pub(crate) created_at: String,
    pub(crate) last_updated: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub(crate) struct DatabaseTables {
    pub(crate) backend: String,
    pub(crate) tables: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct WorkflowPayload {
    pub(super) active: bool,
    pub(super) definition_json: String,
    pub(super) description: Option<String>,
    pub(super) name: String,
    pub(super) version: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ExecutionResult {
    pub(crate) success: bool,
    pub(crate) execution_id: String,
    pub(crate) started_at: String,
    pub(crate) duration_ms: i64,
    pub(crate) output: Option<Value>,
    pub(crate) error: Option<String>,
    pub(crate) logs: Vec<ExecutionStep>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct ExecutionStep {
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) input_json: Option<String>,
    pub(crate) output_json: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(super) struct ExecutionRecord {
    pub(super) id: i32,
    pub(super) execution_id: String,
    pub(super) workflow_id: Option<i32>,
    pub(super) workflow_name: String,
    pub(super) status: String,
    pub(super) started_at: String,
    pub(super) finished_at: Option<String>,
    pub(super) duration_ms: Option<i64>,
    pub(super) input_json: String,
    pub(super) output_json: Option<String>,
    pub(super) error: Option<String>,
    pub(super) logs: Vec<ExecutionStep>,
}

pub(super) const DEFAULT_OWS: &str = r#"{
  "document": {
    "dsl": "1.0.3",
    "namespace": "default",
    "name": "new-workflow",
    "version": "1.0.0"
  },
  "do": []
}"#;
