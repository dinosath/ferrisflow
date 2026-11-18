use dioxus::prelude::*;

use super::shared::Shell;

#[component]
pub(crate) fn AccountPage() -> Element {
    let principal = use_context::<Signal<Option<super::api::AuthPrincipal>>>()();
    let Some(principal) = principal else {
        return rsx! { div { class: "login-loading", "Sign in to view your account." } };
    };
    let is_admin = principal.username.as_deref() == Some("admin")
        || principal.scopes.iter().any(|scope| scope == "ferrisflow:admin");
    let username = principal.username.unwrap_or_else(|| principal.subject.clone());
    let initial = username.chars().next().unwrap_or('U').to_ascii_uppercase().to_string();
    let scope_list = principal.scopes.join(", ");
    let role = if is_admin {
        "Administrator"
    } else {
        "Workspace user"
    };

    rsx! {
        Shell {
            active: "account",
            header: "Account",
            intro: "Review the identity and permissions for your current session.",
            actions: rsx! {},
            section { class: "account-card",
                div { class: "account-avatar", "{initial}" }
                div { class: "account-details",
                    div { class: "account-detail",
                        span { "Username" }
                        strong { "{username}" }
                    }
                    div { class: "account-detail",
                        span { "Account type" }
                        strong { "{role}" }
                    }
                    div { class: "account-detail",
                        span { "Permissions" }
                        strong { "{scope_list}" }
                    }
                }
            }
        }
    }
}
