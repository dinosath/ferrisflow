use dioxus::prelude::*;
#[derive(Clone, Debug, PartialEq)]
pub enum ImportSource {
    Url(String),
    Contents(String),
    FileBundle { contents: String, files: Vec<(String, String)> },
}

pub(crate) fn is_plugin_manifest_json(contents: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(contents) else {
        return false;
    };
    let manifest = if let Some(manifest) = value.get("manifest") {
        manifest.clone()
    } else if let (Some(entry), Some(files)) = (
        value.get("entry").and_then(serde_json::Value::as_str),
        value.get("files").and_then(serde_json::Value::as_object),
    ) {
        let Some(raw) = files.get(entry) else { return false };
        match raw {
            serde_json::Value::String(text) => {
                let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) else {
                    return false;
                };
                parsed
            }
            value => value.clone(),
        }
    } else {
        value
    };
    let meta = manifest.get("plugin").filter(|plugin| plugin.is_object()).unwrap_or(&manifest);
    let id = meta.get("id").or_else(|| meta.get("uid")).and_then(serde_json::Value::as_str).unwrap_or_default();
    !id.is_empty()
        && id.len() <= 96
        && id.split('.').all(|part| {
            !part.is_empty()
                && part.chars().all(|character| {
                    character.is_ascii_alphanumeric() || character == '-' || character == '_'
                })
        })
        && meta.get("name").and_then(serde_json::Value::as_str).is_some_and(|name| !name.trim().is_empty())
}

/// Shared URL, file, and pasted-text source picker used by import flows.
#[component]
pub fn ImportSourceDialog(
    title: String, description: String, file_label: String, accept: String,
    url_placeholder: String, text_placeholder: String, action_label: String,
    #[props(default = true)] fetch_url: bool, error: Option<String>,
    #[props(default)] external_busy: bool, #[props(default)] https_only: bool,
    #[props(default)] allow_attached_files: bool,
    #[props(default)] validate_plugin_json: bool,
    on_import: EventHandler<Result<ImportSource, String>>, on_close: EventHandler<()>,
) -> Element {
    let mut source = use_signal(|| "url".to_string());
    let mut url = use_signal(String::new);
    let mut contents = use_signal(String::new);
    let mut attached_files = use_signal(Vec::<(String, String)>::new);
    let mut busy = use_signal(|| false);
    let source_options = vec![("url", "URL"), ("file", "File"), ("text", "Text")];

    rsx! {
        div { class: "workflow-import-scrim", role: "presentation", onclick: move |_| on_close.call(()),
            section {
                class: "workflow-import-dialog",
                role: "dialog",
                aria_modal: "true",
                aria_labelledby: "import-source-title",
                onclick: move |event| event.stop_propagation(),
                header { class: "workflow-import-dialog__header",
                    div { h2 { id: "import-source-title", "{title}" } p { "{description}" } }
                    button { class: "workflow-editor__icon-action", r#type: "button", title: "Close", aria_label: "Close import dialog", onclick: move |_| on_close.call(()), "×" }
                }
                div { class: "workflow-import-tabs", role: "tablist", aria_label: "Import source",
                    for (key, label) in source_options {
                        button {
                            class: if source() == key { "workflow-import-tab is-active" } else { "workflow-import-tab" },
                            role: "tab", aria_selected: source() == key, r#type: "button",
                            onclick: move |_| source.set(key.to_string()),
                            "{label}"
                        }
                    }
                }
                if source() == "url" {
                    label { class: "workflow-import-field",
                        span { "Package URL" }
                        input { r#type: "url", placeholder: "{url_placeholder}", value: "{url}", oninput: move |event| url.set(event.value()) }
                        small { if fetch_url { "The URL must allow browser access (CORS)." } else if https_only { "Use an HTTPS URL. The server will fetch and validate this package." } else { "The server will fetch and validate this package URL." } }
                    }
                } else if source() == "file" {
                    label { class: "workflow-import-field",
                        span { "{file_label}" }
                        input { r#type: "file", accept: "{accept}", onchange: move |event| {
                            let files = event.files();
                            if let Some(file) = files.into_iter().next() {
                                spawn(async move {
                                    match file.read_string().await {
                                        Ok(text) if validate_plugin_json && !is_plugin_manifest_json(&text) => {
                                            contents.set(String::new());
                                            on_import.call(Err("This JSON file is not a valid plugin manifest. It must define a valid plugin id and a non-empty name.".to_string()));
                                        }
                                        Ok(text) => contents.set(text),
                                        Err(message) => on_import.call(Err(format!("Could not read file: {message}"))),
                                    }
                                });
                            }
                        } }
                    }
                    if allow_attached_files {
                        label { class: "workflow-import-field",
                            span { "Referenced JSON files (optional)" }
                            input { r#type: "file", accept: ".json,application/json", multiple: true, onchange: move |event| {
                                let files = event.files();
                                spawn(async move {
                                    let mut selected = Vec::new();
                                    for file in files {
                                        let name = file.name();
                                        if !name.to_lowercase().ends_with(".json") { continue; }
                                        match file.read_string().await {
                                            Ok(text) => selected.push((name, text)),
                                            Err(message) => {
                                                on_import.call(Err(format!("Could not read referenced JSON file: {message}")));
                                                return;
                                            }
                                        }
                                    }
                                    attached_files.set(selected);
                                });
                            } }
                            small { "Select companion JSON files referenced by the plugin manifest. Files may also be embedded in the package’s files object." }
                        }
                        if !attached_files().is_empty() {
                            p { class: "workflow-import-field", "{attached_files().len()} companion JSON files selected" }
                        }
                    }
                    textarea { class: "workflow-import-text", rows: "10", placeholder: "Selected file contents appear here…", value: "{contents}", oninput: move |event| contents.set(event.value()) }
                } else {
                    label { class: "workflow-import-field",
                        span { "Package contents" }
                        textarea { class: "workflow-import-text", rows: "12", placeholder: "{text_placeholder}", value: "{contents}", oninput: move |event| contents.set(event.value()) }
                    }
                }
                if let Some(message) = error.as_ref() { p { class: "workflow-import-error", role: "alert", "{message}" } }
                footer { class: "workflow-import-dialog__footer",
                    button { class: "button button-quiet", r#type: "button", onclick: move |_| on_close.call(()), "Cancel" }
                    button {
                        class: "button button-primary", r#type: "button",
                        disabled: busy() || external_busy || (source() == "url" && url().trim().is_empty()) || (source() == "file" || source() == "text") && contents().trim().is_empty(),
                        onclick: move |_| async move {
                            busy.set(true);
                            let selected_source = source();
                            let result = if selected_source == "url" {
                                let address = url().trim().to_string();
                                if !(address.starts_with("https://") || (!https_only && address.starts_with("http://"))) {
                                    Err(if https_only { "Enter a valid HTTPS URL.".to_string() } else { "Enter a valid http or https URL.".to_string() })
                                } else if fetch_url {
                                    match reqwest::get(&address).await {
                                        Ok(response) if response.status().is_success() => response.text().await.map(ImportSource::Contents).map_err(|e| e.to_string()),
                                        Ok(response) => Err(format!("The URL returned {}.", response.status())),
                                        Err(message) => Err(format!("Could not load URL: {message}")),
                                    }
                                } else {
                                    Ok(ImportSource::Url(address))
                                }
                            } else if selected_source == "file" && allow_attached_files {
                                Ok(ImportSource::FileBundle { contents: contents(), files: attached_files() })
                            } else {
                                Ok(ImportSource::Contents(contents()))
                            };
                            on_import.call(result);
                            busy.set(false);
                        },
                        if busy() || external_busy { "Importing…" } else { "{action_label}" }
                    }
                }
            }
        }
    }
}
