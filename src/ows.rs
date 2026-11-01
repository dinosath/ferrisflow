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

    let mut definition_value = value
        .get("definition")
        .filter(|_| value.get("document").is_none())
        .cloned()
        .unwrap_or(value);
    migrate_http_exposure(&mut definition_value);
    validate_database_triggers(http_triggers(&definition_value))?;
    validate_rest_api(&definition_value)?;

    let definition_json = serde_json::to_string(&definition_value)
        .map_err(|error| format!("could not read OWS definition: {error}"))?;
    let definition = ows_dsl::from_json(&definition_json)
        .map_err(|error| format!("definition is not valid OWS: {error}"))?;

    let report = ows_dsl::validate(&definition);
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
    let mut original = parse_ows_value(raw)?;
    if original.get("document").is_none() {
        if let Some(source) = original.get_mut("definition") {
            migrate_http_exposure(source);
        }
    } else {
        migrate_http_exposure(&mut original);
    }
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

fn migrate_http_exposure(document: &mut Value) {
    let Some(metadata) = document
        .get_mut("document")
        .and_then(|document| document.get_mut("metadata"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let Some(triggers) = metadata.get("triggers").and_then(Value::as_array).cloned() else {
        return;
    };
    let http_trigger = triggers
        .iter()
        .find(|trigger| trigger.get("type").and_then(Value::as_str) == Some("http"))
        .cloned();
    if metadata.get("rest_api").is_none() {
        if let Some(trigger) = http_trigger {
            metadata.insert(
                "rest_api".into(),
                serde_json::json!({
                    "enabled": true,
                    "method": trigger.get("method").and_then(Value::as_str).unwrap_or("POST"),
                    "path": trigger.get("path").and_then(Value::as_str).unwrap_or("/")
                }),
            );
        }
    }
    let remaining = triggers
        .iter()
        .filter(|trigger| trigger.get("type").and_then(Value::as_str) != Some("http"))
        .cloned()
        .collect::<Vec<_>>();
    if remaining.is_empty() {
        metadata.remove("triggers");
    } else {
        metadata.insert("triggers".into(), Value::Array(remaining));
    }
}

fn validate_database_triggers(triggers: Option<&Value>) -> Result<(), String> {
    let Some(triggers) = triggers else {
        return Ok(());
    };
    let triggers = triggers
        .as_array()
        .ok_or_else(|| "document.metadata.triggers must be an array".to_string())?;
    for (index, trigger) in triggers.iter().enumerate() {
        match trigger.get("type").and_then(Value::as_str) {
            Some("database") => {
                validate_database_trigger(trigger, index)?;
                continue;
            }
            _ => {
                return Err(format!("document.metadata.triggers/{index}/type must be database"));
            }
        }
    }
    Ok(())
}

fn validate_rest_api(document: &Value) -> Result<(), String> {
    let Some(exposure) = document
        .get("document")
        .and_then(|document| document.get("metadata"))
        .and_then(|metadata| metadata.get("rest_api"))
    else {
        return Ok(());
    };
    if !exposure.is_object() {
        return Err("document.metadata.rest_api must be an object".into());
    }
    if !exposure.get("enabled").is_some_and(Value::is_boolean) {
        return Err("document.metadata.rest_api/enabled must be a boolean".into());
    }
    let method = exposure
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| "document.metadata.rest_api/method is required".to_string())?;
    if !["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
        .contains(&method.to_ascii_uppercase().as_str())
    {
        return Err("document.metadata.rest_api/method is not a supported HTTP method".into());
    }
    let path = exposure
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "document.metadata.rest_api/path is required".to_string())?;
    if !path.starts_with("/api/") || path.contains('?') || path.contains('#')
    {
        return Err(
            "document.metadata.rest_api/path must start with /api/ and cannot contain a query or fragment".into(),
        );
    }
    let reserved_paths = [
        "/api/workflows",
        "/api/workflows/validate",
        "/api/workflows/execute",
        "/api/database/tables",
        "/api/database/events/listen",
        "/api/executions",
    ];
    let workflow_id_route = path
        .strip_prefix("/api/workflows/")
        .is_some_and(|suffix| !suffix.is_empty() && !suffix.contains('/'));
    if reserved_paths.contains(&path) || workflow_id_route {
        return Err("document.metadata.rest_api/path conflicts with a built-in API route".into());
    }
    Ok(())
}

fn validate_database_trigger(trigger: &Value, index: usize) -> Result<(), String> {
    let prefix = format!("document.metadata.triggers/{index}");
    if !trigger.get("table").and_then(Value::as_str).is_some_and(|table| !table.trim().is_empty()) {
        return Err(format!("{prefix}/table is required"));
    }
    let events = trigger
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{prefix}/events must be an array"))?;
    if events.is_empty() {
        return Err(format!("{prefix}/events must include at least one database operation"));
    }
    for event in events {
        let operation = event.as_str().unwrap_or_default();
        if !["INSERT", "UPDATE", "DELETE"]
            .iter()
            .any(|supported| operation.eq_ignore_ascii_case(supported))
        {
            return Err(format!("{prefix}/events contains an unsupported database operation"));
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
