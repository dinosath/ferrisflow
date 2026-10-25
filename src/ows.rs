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
    let value = parse_ows_value(raw)?;

    let definition_value = value
        .get("definition")
        .filter(|_| value.get("document").is_none())
        .cloned()
        .unwrap_or(value);
    validate_http_triggers(http_triggers(&definition_value))?;

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
    let original = parse_ows_value(raw)?;
    let definition = parse_ows_document(raw)?;
    let mut normalized = serde_json::to_value(definition)
        .map_err(|error| format!("could not serialize OWS definition: {error}"))?;
    let source = original
        .get("definition")
        .filter(|_| original.get("document").is_none())
        .unwrap_or(&original);
    if let Some(metadata) = source.get("document").and_then(|document| document.get("metadata")) {
        normalized["document"]["metadata"] = metadata.clone();
    }
    // Migrate documents created before the trigger binding was moved into OWS
    // document metadata. The canonical representation is `document.metadata.triggers`.
    serde_json::to_string_pretty(&normalized)
        .map_err(|error| format!("could not serialize OWS definition: {error}"))
}

fn http_triggers(document: &Value) -> Option<&Value> {
    document
        .get("document")
        .and_then(|document| document.get("metadata"))
        .and_then(|metadata| metadata.get("triggers"))
}

fn parse_ows_value(raw: &str) -> Result<Value, String> {
    serde_json::from_str(raw)
        .or_else(|_| serde_yaml::from_str(raw).map_err(|error| error.to_string()))
        .map_err(|error| format!("definition_json is not valid JSON or YAML: {error}"))
}

fn validate_http_triggers(triggers: Option<&Value>) -> Result<(), String> {
    let Some(triggers) = triggers else {
        return Ok(());
    };
    let triggers = triggers
        .as_array()
        .ok_or_else(|| "document.metadata.triggers must be an array".to_string())?;
    let mut routes = std::collections::HashSet::new();
    for (index, trigger) in triggers.iter().enumerate() {
        if trigger.get("type").and_then(Value::as_str) != Some("http") {
            return Err(format!("document.metadata.triggers/{index}/type must be http"));
        }
        if trigger
            .get("authentication")
            .and_then(Value::as_str)
            .is_some_and(|authentication| authentication != "none")
        {
            return Err(format!(
                "document.metadata.triggers/{index}/authentication is not supported yet"
            ));
        }
        let method = trigger
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("document.metadata.triggers/{index}/method is required"))?;
        if !["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
            .contains(&method.to_ascii_uppercase().as_str())
        {
            return Err(format!(
                "document.metadata.triggers/{index}/method is not a supported HTTP method"
            ));
        }
        let path = trigger
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("document.metadata.triggers/{index}/path is required"))?;
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains('?')
            || path.contains('#')
        {
            return Err(format!(
                "document.metadata.triggers/{index}/path must be an absolute path without query or fragment"
            ));
        }
        let route = (method.to_ascii_uppercase(), path.to_owned());
        if !routes.insert(route) {
            return Err(format!(
                "document.metadata.triggers/{index} duplicates an HTTP method and path"
            ));
        }
    }
    Ok(())
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
