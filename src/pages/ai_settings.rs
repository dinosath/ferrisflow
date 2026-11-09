use dioxus::prelude::*;

use crate::{
    components::{
        data_table::{DataTable, DataTableCell, DataTableRow},
        icons::Icon,
    },
    pages::{
        api::{AiProviderInput, delete_ai_provider, list_ai_providers, save_ai_provider},
        shared::{ErrorPanel, LoadingPanel, Shell},
    },
};

#[component]
pub(crate) fn AiSettings() -> Element {
    let mut providers = use_server_future(list_ai_providers)?;
    let mut form_open = use_signal(|| false);
    let mut editing_id = use_signal(|| None::<i32>);
    let mut name = use_signal(String::new);
    let mut provider = use_signal(|| "openai".to_string());
    let mut base_url = use_signal(|| default_base_url("openai"));
    let mut model = use_signal(|| "gpt-4o-mini".to_string());
    let mut api_key = use_signal(String::new);
    let mut active = use_signal(|| true);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let open_new_provider = move |_| {
        editing_id.set(None);
        name.set(String::new());
        provider.set("openai".into());
        base_url.set(default_base_url("openai"));
        model.set("gpt-4o-mini".into());
        api_key.set(String::new());
        active.set(providers().and_then(Result::ok).is_none_or(|items| items.is_empty()));
        error.set(None);
        form_open.set(true);
    };

    let save = move |_| {
        let input = AiProviderInput {
            name: name().trim().to_string(),
            provider: provider(),
            base_url: base_url().trim().to_string(),
            model: model().trim().to_string(),
            api_key: if api_key().trim().is_empty() { None } else { Some(api_key()) },
            active: active(),
        };
        let id = editing_id();
        saving.set(true);
        error.set(None);
        spawn(async move {
            match save_ai_provider(input, id).await {
                Ok(_) => {
                    form_open.set(false);
                    editing_id.set(None);
                    api_key.set(String::new());
                    providers.restart();
                }
                Err(message) => error.set(Some(message)),
            }
            saving.set(false);
        });
    };

    rsx! {
        Shell {
            active: "settings_ai",
            header: "AI integration",
            intro: "Configure AI providers for the workflow assistant.",
            actions: rsx! {
                button { class: "button button-primary", r#type: "button", onclick: open_new_provider, Icon { name: "plus" } "Add provider" }
            },
            section { class: "ai-settings-content",
                if let Some(message) = error() { ErrorPanel { message } }
                match providers() {
                    Some(Ok(items)) => {
                        let row_ids = items.iter().map(|item| item.id.to_string()).collect();
                        rsx! {
                            DataTable {
                                class: "ai-provider-table",
                                columns: vec!["Provider".into(), "Model".into(), "Base URL".into(), "API key".into(), "Status".into(), "Actions".into()],
                                row_ids,
                                toolbar: rsx! {},
                                empty_message: "No AI providers yet. Add a provider to enable the workflow assistant.".to_string(),
                                for (index, item) in items.into_iter().enumerate() {
                                    DataTableRow { id: item.id.to_string(), index,
                                        DataTableCell { index: 0, class: "ai-provider-table__provider",
                                            span { class: "ai-provider-card__icon", Icon { name: provider_icon(&item.provider) } }
                                            span { class: "ai-provider-table__name", strong { "{item.name}" } small { "{provider_label(&item.provider)}" } }
                                        }
                                        DataTableCell { index: 1, "{item.model}" }
                                        DataTableCell { index: 2, class: "ai-provider-table__url", "{item.base_url}" }
                                        DataTableCell { index: 3,
                                            span { class: "ai-provider-table__key", Icon { name: "key" } if item.api_key_set { "Saved" } else { "Not set" } }
                                        }
                                        DataTableCell { index: 4,
                                            span { class: if item.active { "ai-provider-status is-active" } else { "ai-provider-status" }, if item.active { "Active" } else { "Inactive" } }
                                        }
                                        DataTableCell { index: 5, class: "ai-provider-table__actions",
                                            if !item.active {
                                                button {
                                                    class: "button button-quiet",
                                                    r#type: "button",
                                                    onclick: { let item = item.clone(); move |_| {
                                                        let input = AiProviderInput { name: item.name.clone(), provider: item.provider.clone(), base_url: item.base_url.clone(), model: item.model.clone(), api_key: None, active: true };
                                                        let id = item.id;
                                                        spawn(async move {
                                                            if let Err(message) = save_ai_provider(input, Some(id)).await { error.set(Some(message)); }
                                                            providers.restart();
                                                        });
                                                    } },
                                                    "Set active"
                                                }
                                            }
                                            button {
                                                class: "workflow-editor__icon-action",
                                                r#type: "button",
                                                title: "Edit provider",
                                                aria_label: "Edit provider",
                                                onclick: { let item = item.clone(); move |_| {
                                                    editing_id.set(Some(item.id));
                                                    name.set(item.name.clone());
                                                    provider.set(item.provider.clone());
                                                    base_url.set(item.base_url.clone());
                                                    model.set(item.model.clone());
                                                    api_key.set(String::new());
                                                    active.set(item.active);
                                                    error.set(None);
                                                    form_open.set(true);
                                                } },
                                                Icon { name: "settings" }
                                            }
                                            button {
                                                class: "workflow-editor__icon-action workflow-editor__icon-action--danger",
                                                r#type: "button",
                                                title: "Delete provider",
                                                aria_label: "Delete provider",
                                                onclick: move |_| {
                                                    let id = item.id;
                                                    spawn(async move {
                                                        if let Err(message) = delete_ai_provider(id).await { error.set(Some(message)); }
                                                        providers.restart();
                                                    });
                                                },
                                                Icon { name: "trash" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Some(Err(message)) => rsx! { ErrorPanel { message } },
                    None => rsx! { LoadingPanel {} },
                }
            }
            if form_open() {
                div { class: "workflow-settings-overlay", role: "presentation", onclick: move |_| { form_open.set(false); error.set(None); },
                    section { class: "workflow-settings-dialog ai-provider-modal", role: "dialog", aria_modal: "true", aria_label: "AI provider settings", onclick: move |event| event.stop_propagation(),
                        header { class: "workflow-settings-dialog__header",
                            div { p { class: "workflow-editor__eyebrow", "AI INTEGRATION" } h2 { if editing_id().is_some() { "Edit provider" } else { "Add provider" } } }
                            button { class: "workflow-editor__icon-action", r#type: "button", title: "Close", aria_label: "Close provider settings", onclick: move |_| { form_open.set(false); error.set(None); }, Icon { name: "close" } }
                        }
                        p { class: "workflow-settings-dialog__description", "Provider credentials are kept on the server and are not shown again after saving." }
                        label { class: "field", span { "Provider name" } input { value: "{name}", placeholder: "Work OpenAI", oninput: move |event| name.set(event.value()) } }
                        label { class: "field", span { "Provider" }
                            select { value: "{provider}", onchange: move |event| {
                                let selected = event.value();
                                provider.set(selected.clone());
                                base_url.set(default_base_url(&selected));
                                if selected == "ollama" { model.set("llama3.2".into()); }
                                else if model() == "llama3.2" { model.set("gpt-4o-mini".into()); }
                            },
                                option { value: "openai", "OpenAI" }
                                option { value: "openrouter", "OpenRouter" }
                                option { value: "ollama", "Ollama" }
                                option { value: "custom", "Custom OpenAI-compatible" }
                            }
                        }
                        label { class: "field", span { "Base URL" } input { value: "{base_url}", placeholder: "https://api.openai.com/v1", oninput: move |event| base_url.set(event.value()) } }
                        label { class: "field", span { "Model" } input { value: "{model}", placeholder: "gpt-4o-mini", oninput: move |event| model.set(event.value()) } }
                        label { class: "field", span { if editing_id().is_some() { "Replace API key (optional)" } else { "API key" } }
                            input { r#type: "password", autocomplete: "new-password", value: "{api_key}", placeholder: if editing_id().is_some() { "Leave blank to keep saved key" } else if provider() == "ollama" { "Optional for local Ollama" } else { "Enter provider API key" }, oninput: move |event| api_key.set(event.value()) }
                        }
                        label { class: "toggle-field ai-provider-form__active", input { r#type: "checkbox", checked: active(), onchange: move |event| active.set(event.checked()) } span { "Use as active provider" } }
                        p { class: "workflow-settings-dialog__hint", "Only one provider can be active. The workflow assistant uses its model and API key." }
                        if let Some(message) = error() { div { class: "error-panel", "{message}" } }
                        footer { class: "workflow-settings-dialog__actions",
                            button { class: "button button-quiet", r#type: "button", onclick: move |_| { form_open.set(false); error.set(None); }, "Cancel" }
                            button { class: "button button-primary", r#type: "button", disabled: saving(), onclick: save, if saving() { "Saving…" } else { "Save provider" } }
                        }
                    }
                }
            }
        }
    }
}

fn default_base_url(provider: &str) -> String {
    match provider {
        "openrouter" => "https://openrouter.ai/api/v1".into(),
        "ollama" => "http://localhost:11434/v1".into(),
        "custom" => String::new(),
        _ => "https://api.openai.com/v1".into(),
    }
}

fn provider_label(provider: &str) -> &'static str {
    match provider {
        "openai" => "OpenAI",
        "openrouter" => "OpenRouter",
        "ollama" => "Ollama",
        "custom" => "Custom compatible provider",
        _ => "Provider",
    }
}

fn provider_icon(provider: &str) -> &'static str {
    match provider {
        "ollama" => "activity",
        "openrouter" => "workflow",
        _ => "sparkles",
    }
}
