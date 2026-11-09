use dioxus::prelude::*;
use serde_json::Value;

use super::import_source::{ImportSource, ImportSourceDialog};

#[derive(Clone, Debug, PartialEq)]
pub struct ImportedWorkflow {
    pub name: String,
    pub description: Option<String>,
    pub version: i32,
    pub definition_json: String,
}

#[component]
pub fn WorkflowImportDialog(
    on_import: EventHandler<Result<ImportedWorkflow, String>>, on_close: EventHandler<()>,
) -> Element {
    rsx! {
        ImportSourceDialog {
            title: "Import workflow".to_string(),
            description: "Load an OWS definition in JSON or YAML from a URL, file, or pasted text.".to_string(),
            file_label: "Choose a JSON or YAML file".to_string(),
            accept: ".json,.yaml,.yml,application/json,application/yaml,application/x-yaml,text/yaml".to_string(),
            url_placeholder: "https://example.com/workflow.yaml".to_string(),
            text_placeholder: "Paste an OWS workflow definition…".to_string(),
            action_label: "Import workflow".to_string(),
            fetch_url: true,
            error: None,
            on_import: move |source: Result<ImportSource, String>| {
                let result = source.and_then(|source| match source {
                    ImportSource::Contents(raw) => parse_imported_workflow(raw),
                    ImportSource::Url(_) => Err("Workflow URL was not loaded.".to_string()),
                    ImportSource::FileBundle { .. } => Err("Companion files are supported for plugin packages only.".to_string()),
                });
                on_import.call(result);
            },
            on_close,
        }
    }
}

pub fn export_data_url(json: &str) -> String {
    encode_data_url("application/json", json)
}

pub fn export_yaml_data_url(json: &str) -> String {
    let yaml = serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|value| serde_yaml::to_string(&value).ok())
        .unwrap_or_else(|| "{}\n".to_string());
    encode_data_url("application/yaml", &yaml)
}

fn encode_data_url(content_type: &str, contents: &str) -> String {
    let encoded = contents
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect::<String>();
    format!("data:{content_type};charset=utf-8,{encoded}")
}

fn parse_imported_workflow(raw: String) -> Result<ImportedWorkflow, String> {
    let value: Value = serde_json::from_str(&raw)
        .or_else(|_| serde_yaml::from_str(&raw))
        .map_err(|error| format!("Invalid OWS JSON or YAML: {error}"))?;
    if !value["document"].is_object() || !value["do"].is_array() {
        return Err(
            "This file is not an OWS workflow. It must contain a document and do list.".into()
        );
    }
    let name = value["document"]["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "The OWS document needs a name.".to_string())?
        .to_string();
    let version = value["document"]["version"]
        .as_str()
        .and_then(|version| version.split('.').next())
        .and_then(|major| major.parse().ok())
        .filter(|version| *version > 0)
        .unwrap_or(1);
    let description = value["document"]["summary"].as_str().map(ToOwned::to_owned);
    let definition_json =
        serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    Ok(ImportedWorkflow { name, description, version, definition_json })
}
