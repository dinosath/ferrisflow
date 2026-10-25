use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(super) struct Workflow {
    pub(super) id: i32,
    pub(super) active: bool,
    pub(super) definition_json: String,
    pub(super) description: Option<String>,
    pub(super) name: String,
    pub(super) version: i32,
    pub(super) created_at: String,
    pub(super) last_updated: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct WorkflowPayload {
    pub(super) active: bool,
    pub(super) definition_json: String,
    pub(super) description: Option<String>,
    pub(super) name: String,
    pub(super) version: i32,
}

pub(super) const DEFAULT_OWS: &str = r#"{
  "document": {
    "dsl": "1.0.3",
    "namespace": "default",
    "name": "hello",
    "version": "1.0.0"
  },
  "do": [
    { "greet": { "set": { "message": "hello" } } }
  ]
}"#;
