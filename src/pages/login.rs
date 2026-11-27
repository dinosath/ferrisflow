use dioxus::prelude::*;

use super::api::{self, AuthPrincipal};

#[component]
pub(crate) fn LoginPage() -> Element {
    let mut current_user = use_context::<Signal<Option<AuthPrincipal>>>();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut signing_in = use_signal(|| false);
    let mut oidc_enabled = use_signal(|| false);

    use_effect(move || {
        spawn(async move {
            oidc_enabled.set(api::oidc_enabled().await);
        });
    });

    let submit = move |event: FormEvent| {
        event.prevent_default();
        let username_value = username().trim().to_owned();
        let password_value = password();
        signing_in.set(true);
        error.set(None);
        spawn(async move {
            match api::authenticate(username_value, password_value).await {
                Ok(principal) => current_user.set(Some(principal)),
                Err(_) => error.set(Some("That username or password is incorrect.".to_owned())),
            }
            signing_in.set(false);
        });
    };

    rsx! {
        main { class: "login-screen",
            section { class: "login-card",
                div { class: "login-brand", span { class: "login-brand__mark", "F" } span { "Ferrisflow" } }
                h1 { "Sign in to your workspace" }
                p { class: "login-intro", "Enter your Ferrisflow username and password to continue." }
                if oidc_enabled() {
                    a { class: "button button-primary login-submit", href: "/auth/oidc/login", "Continue with single sign-on" }
                    p { class: "login-footnote", "Sign in with your organization's identity provider." }
                }
                if let Some(message) = error() {
                    div { class: "login-error", role: "alert", "{message}" }
                }
                form { onsubmit: submit,
                    label { class: "field",
                        span { "Username" }
                        input {
                            r#type: "text",
                            name: "username",
                            autocomplete: "username",
                            required: true,
                            autofocus: true,
                            value: "{username}",
                            oninput: move |event| username.set(event.value()),
                        }
                    }
                    label { class: "field",
                        span { "Password" }
                        input {
                            r#type: "password",
                            name: "password",
                            autocomplete: "current-password",
                            required: true,
                            value: "{password}",
                            oninput: move |event| password.set(event.value()),
                        }
                    }
                    button { class: "button button-primary login-submit", r#type: "submit", disabled: signing_in(),
                        if signing_in() { "Signing in…" } else { "Sign in" }
                    }
                }
                p { class: "login-footnote", "Account passwords are provided by your Ferrisflow administrator." }
            }
        }
    }
}
