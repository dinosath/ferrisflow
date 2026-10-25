use dioxus::prelude::*;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct ImportedWorkflow {
    pub name: String,
    pub description: Option<String>,
    pub version: i32,
    pub definition_json: String,
}

#[component]
pub fn WorkflowImportDialog(
    on_import: EventHandler<ImportedWorkflow>, on_close: EventHandler<()>,
) -> Element {
    let mut source = use_signal(|| "text".to_string());
    let mut url = use_signal(String::new);
    let mut definition_text = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    rsx! {
        div { class: "workflow-import-scrim", role: "presentation", onclick: move |_| on_close.call(()),
            section {
                class: "workflow-import-dialog",
                role: "dialog",
                aria_modal: "true",
                aria_labelledby: "workflow-import-title",
                onclick: move |event| event.stop_propagation(),
                header { class: "workflow-import-dialog__header",
                    div {
                        h2 { id: "workflow-import-title", "Import workflow" }
                        p { "Load an OWS definition in JSON or YAML from a URL, file, or pasted text." }
                    }
                    button { class: "workflow-editor__icon-action", r#type: "button", title: "Close", aria_label: "Close import dialog", onclick: move |_| on_close.call(()), "×" }
                }
                div { class: "workflow-import-tabs", role: "tablist", aria_label: "Import source",
                    for (key, label) in [("url", "URL"), ("file", "File"), ("text", "Text")] {
                        button {
                            class: if source() == key { "workflow-import-tab is-active" } else { "workflow-import-tab" },
                            role: "tab",
                            aria_selected: source() == key,
                            r#type: "button",
                            onclick: move |_| { source.set(key.to_string()); error.set(None); },
                            "{label}"
                        }
                    }
                }
                if source() == "url" {
                    label { class: "workflow-import-field",
                        span { "Workflow URL" }
                        input {
                            r#type: "url",
                            placeholder: "https://example.com/workflow.yaml",
                            value: "{url}",
                            oninput: move |event| url.set(event.value()),
                        }
                        small { "The URL must allow browser access (CORS)." }
                    }
                } else if source() == "file" {
                    label { class: "workflow-import-field",
                        span { "Choose a JSON or YAML file" }
                        input {
                            r#type: "file",
                            accept: ".json,.yaml,.yml,application/json,application/yaml,application/x-yaml,text/yaml",
                            onchange: move |event| {
                                let files = event.files();
                                if let Some(file) = files.into_iter().next() {
                                    spawn(async move {
                                        match file.read_string().await {
                                            Ok(contents) => { definition_text.set(contents); error.set(None); }
                                            Err(read_error) => error.set(Some(format!("Could not read file: {read_error}"))),
                                        }
                                    });
                                }
                            },
                        }
                    }
                    textarea {
                        class: "workflow-import-text",
                        rows: "10",
                        placeholder: "Selected file contents appear here…",
                        value: "{definition_text}",
                        oninput: move |event| definition_text.set(event.value()),
                    }
                } else {
                    label { class: "workflow-import-field",
                        span { "OWS JSON or YAML" }
                        textarea {
                            class: "workflow-import-text",
                            rows: "12",
                            placeholder: "Paste an OWS workflow definition…",
                            value: "{definition_text}",
                            oninput: move |event| definition_text.set(event.value()),
                        }
                    }
                }
                if let Some(message) = error() {
                    p { class: "workflow-import-error", role: "alert", "{message}" }
                }
                footer { class: "workflow-import-dialog__footer",
                    button { class: "button button-quiet", r#type: "button", onclick: move |_| on_close.call(()), "Cancel" }
                    button {
                        class: "button button-primary",
                        r#type: "button",
                        disabled: busy() || (source() == "url" && url().trim().is_empty()) || (source() != "url" && definition_text().trim().is_empty()),
                        onclick: move |_| async move {
                            busy.set(true);
                            error.set(None);
                            let raw = if source() == "url" {
                                let address = url();
                                if !(address.starts_with("https://") || address.starts_with("http://")) {
                                    Err("Enter a valid http or https URL.".to_string())
                                } else {
                                    match reqwest::get(&address).await {
                                        Ok(response) if response.status().is_success() => response.text().await.map_err(|e| e.to_string()),
                                        Ok(response) => Err(format!("The URL returned {}.", response.status())),
                                        Err(fetch_error) => Err(format!("Could not load URL: {fetch_error}")),
                                    }
                                }
                            } else {
                                Ok(definition_text())
                            };
                            match raw.and_then(parse_imported_workflow) {
                                Ok(imported) => on_import.call(imported),
                                Err(message) => error.set(Some(message)),
                            }
                            busy.set(false);
                        },
                        if busy() { "Loading…" } else { "Import workflow" }
                    }
                }
            }
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
