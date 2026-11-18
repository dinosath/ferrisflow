use dioxus::prelude::*;

use super::{
    api::{
        HttpServerSettings, SyncSettings, load_http_server_settings, load_sync_settings,
        save_http_server_settings, save_sync_settings,
    },
    shared::{ErrorPanel, LoadingPanel, Shell},
};

#[component]
pub(crate) fn SyncSettingsPage() -> Element {
    let mut saved = use_server_future(load_sync_settings)?;
    let mut server_saved = use_server_future(load_http_server_settings)?;
    let mut initialized = use_signal(|| false);
    let mut enabled = use_signal(|| false);
    let mut endpoint = use_signal(String::new);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut notice = use_signal(|| None::<String>);
    let mut http_server_enabled = use_signal(|| false);
    let mut server_error = use_signal(|| None::<String>);
    let mut server_notice = use_signal(|| None::<String>);
    let mut saving_server = use_signal(|| false);

    use_effect(move || {
        if let Some(Ok(settings)) = saved() {
            if !initialized() {
                enabled.set(settings.enabled);
                endpoint.set(settings.endpoint.clone());
                initialized.set(true);
            }
        }
    });

    use_effect(move || {
        if let Some(Ok(settings)) = server_saved() {
            http_server_enabled.set(settings.enabled);
        }
    });

    let save = move |_| {
        let settings = SyncSettings { enabled: enabled(), endpoint: endpoint().trim().to_owned() };
        saving.set(true);
        error.set(None);
        notice.set(None);
        spawn(async move {
            match save_sync_settings(settings).await {
                Ok(settings) => {
                    enabled.set(settings.enabled);
                    endpoint.set(settings.endpoint);
                    notice.set(Some("Sync settings saved.".into()));
                    saved.restart();
                }
                Err(message) => error.set(Some(message)),
            }
            saving.set(false);
        });
    };

    let save_http_server = move |_| {
        let settings = HttpServerSettings { enabled: http_server_enabled() };
        saving_server.set(true);
        server_error.set(None);
        server_notice.set(None);
        spawn(async move {
            match save_http_server_settings(settings).await {
                Ok(settings) => {
                    http_server_enabled.set(settings.enabled);
                    server_notice.set(Some(
                        if settings.enabled {
                            "HTTP server enabled on this device."
                        } else {
                            "HTTP server disabled. Local workflows and data remain available."
                        }
                        .into(),
                    ));
                    server_saved.restart();
                }
                Err(message) => server_error.set(Some(message)),
            }
            saving_server.set(false);
        });
    };

    rsx! {
        Shell {
            active: "settings_sync",
            header: "Sync",
            intro: "Keep this device’s local workspace connected to a Ferrisflow server when you choose to sync.",
            actions: rsx! {},
            section { class: "ai-settings-content sync-settings-content",
                if let Some(message) = error() { ErrorPanel { message } }
                if let Some(message) = notice() { div { class: "workflow-notice", role: "status", "{message}" } }
                if let Some(message) = server_error() { ErrorPanel { message } }
                if let Some(message) = server_notice() { div { class: "workflow-notice", role: "status", "{message}" } }
                match saved() {
                    Some(Err(message)) => rsx! { ErrorPanel { message } },
                    None => rsx! { LoadingPanel {} },
                    Some(Ok(_)) => rsx! {
                        div { class: "sync-settings-card",
                            label { class: "sync-settings-toggle",
                                input { r#type: "checkbox", checked: enabled(), onchange: move |event| enabled.set(event.checked()) }
                                span { strong { "Enable sync" } small { "This device continues to work offline when sync is disabled or unavailable." } }
                            }
                            label { class: "field",
                                span { "Ferrisflow server endpoint" }
                                input {
                                    r#type: "url",
                                    value: "{endpoint}",
                                    placeholder: "https://ferrisflow.example.com",
                                    disabled: !enabled(),
                                    oninput: move |event| endpoint.set(event.value()),
                                }
                            }
                            div { class: "sync-settings-scope",
                                strong { "Data to sync" }
                                p { "All app data: workflows, run history, plugins, providers, content, pages, credentials, variables, and plugin-managed data." }
                            }
                            p { class: "workflow-settings-dialog__hint", "Use an HTTPS endpoint for remote servers. Local changes stay on this device until synchronization is requested." }
                            button { class: "button button-primary", r#type: "button", disabled: saving(), onclick: save,
                                if saving() { "Saving…" } else { "Save sync settings" }
                            }
                        }
                    },
                }
                if cfg!(feature = "local-app") {
                    section { class: "sync-settings-card http-server-settings-card",
                        h2 { "HTTP server" }
                        p { "The local app handles workflows and database operations directly. Enable the HTTP server only when other devices or services need to connect to this device." }
                        label { class: "sync-settings-toggle",
                            input { r#type: "checkbox", checked: http_server_enabled(), onchange: move |event| http_server_enabled.set(event.checked()) }
                            span { strong { "Enable HTTP server" } small { "When enabled, Ferrisflow listens on the configured IP and port (IP defaults to 0.0.0.0 and PORT to 8080)." } }
                        }
                        button { class: "button button-primary", r#type: "button", disabled: saving_server(), onclick: save_http_server,
                            if saving_server() { "Saving…" } else { "Save HTTP server setting" }
                        }
                    }
                }
            }
        }
    }
}
