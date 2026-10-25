//! Canonical Open Workflow Specification (OWS) parsing and validation.
//!
//! OWS is represented by the official `serverless_workflow_core` model. The
//! API stores the canonical JSON document as text so it remains lossless for
//! clients while still rejecting malformed or non-OWS definitions at the
//! boundary.

use serde_json::Value;
use serverless_workflow_core::models::workflow::WorkflowDefinition;

/// Parse a canonical OWS JSON document, or the persisted wrapper shape used by
/// compatible services (`{ "definition": { ... } }`). YAML is accepted for
/// imports and is normalized to JSON before persistence.
pub fn parse_ows_document(raw: &str) -> Result<WorkflowDefinition, String> {
    let value: Value = serde_json::from_str(raw)
        .or_else(|_| serde_yaml::from_str(raw).map_err(|error| error.to_string()))
        .map_err(|error| format!("definition_json is not valid JSON or YAML: {error}"))?;

    let definition_value = value
        .get("definition")
        .filter(|_| value.get("document").is_none())
        .cloned()
        .unwrap_or(value);

    let definition_json = serde_json::to_string(&definition_value)
        .map_err(|error| format!("could not read OWS definition: {error}"))?;
    let definition = ows_runtime_dsl::from_json(&definition_json)
        .map_err(|error| format!("definition is not valid OWS: {error}"))?;

    let report = ows_runtime_dsl::validate(&definition);
    if !report.is_valid() {
        return Err(report
            .issues
            .into_iter()
            .map(|issue| match issue.instance {
                Some(path) => format!("{path}: {}", issue.message),
                None => issue.message,
            })
            .collect::<Vec<_>>()
            .join("; "));
    }

    Ok(definition)
}

/// Validate a definition and return its normalized canonical JSON string.
pub fn normalize_ows_json(raw: &str) -> Result<String, String> {
    let definition = parse_ows_document(raw)?;
    serde_json::to_string_pretty(&definition)
        .map_err(|error| format!("could not serialize OWS definition: {error}"))
}

/// A small, valid OWS definition used by the create screen and API examples.
pub fn starter_ows_json(name: &str, description: Option<&str>) -> String {
    let summary = description
        .map(|value| {
            format!(",\n    \"summary\": {}", serde_json::to_string(value).unwrap_or_default())
        })
        .unwrap_or_default();
    format!(
        r#"{{
  "document": {{
    "dsl": "1.0.3",
    "namespace": "default",
    "name": {},
    "version": "1.0.0"{}
  }},
  "do": [
    {{
      "start": {{
        "set": {{
          "message": "hello"
        }}
      }}
    }}
  ]
}}"#,
        serde_json::to_string(name).unwrap_or_else(|_| "\"workflow\"".to_string()),
        summary
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_ows() {
        let raw = starter_ows_json("demo", Some("A demo workflow"));
        let definition = parse_ows_document(&raw).expect("starter document is valid");
        assert_eq!(definition.document.name, "demo");
        assert_eq!(definition.do_.entries.len(), 1);
    }

    #[test]
    fn rejects_non_ows_json() {
        let error = parse_ows_document(r#"{"nodes": []}"#).expect_err("must reject");
        assert!(error.contains("OWS") || error.contains("required"));
    }
}
