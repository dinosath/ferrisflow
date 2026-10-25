use crate::components::icons::Icon;
use crate::components::ui::badge::{Badge, BadgeVariant};
use dioxus::prelude::*;

use crate::Route;

#[component]
pub(crate) fn Shell(
    active: &'static str, header: &'static str, intro: &'static str, actions: Element,
    #[props(default)] breadcrumb_title: Option<String>,
    #[props(default)] on_document_action: Option<EventHandler<String>>,
    #[props(default)] breadcrumb_back: Option<Element>,
    #[props(default)] breadcrumb_save: Option<Element>,
    #[props(default)] on_breadcrumb_rename: Option<EventHandler<String>>, children: Element,
) -> Element {
    let mut tenant = use_context::<Signal<String>>();
    let mut tenant_menu = use_signal(|| false);
    let mut document_menu = use_signal(|| false);
    let mut breadcrumb_editing = use_signal(|| false);
    let mut breadcrumb_draft = use_signal(String::new);
    rsx! {
        div { class: "app-shell",
            aside { class: "app-sidebar",
                div { class: "tenant-switcher-wrap",
                    button { class: "tenant-switcher", r#type: "button", aria_expanded: "{tenant_menu()}", onclick: move |_| tenant_menu.toggle(),
                        span { class: "tenant-switcher__logo", Icon { name: "command" } }
                        span { class: "tenant-switcher__copy", strong { "{tenant()}" } small { "Ferrisflow workspace" } }
                        span { class: "tenant-switcher__chevron", Icon { name: "chevron-down" } }
                    }
                    if tenant_menu() {
                        div { class: "tenant-menu",
                            p { class: "tenant-menu__label", "TENANTS" }
                            for (name, subtitle, mark) in [("Ferrisflow", "Personal workspace", "command"), ("Platform", "Engineering team", "workflow"), ("Operations", "Operations team", "layout")] {
                                button { class: if tenant() == name { "tenant-option selected" } else { "tenant-option" }, r#type: "button", onclick: move |_| { tenant.set(name.to_string()); tenant_menu.set(false); },
                                    span { class: "tenant-option__mark", Icon { name: mark } }
                                    span { class: "tenant-option__copy", strong { "{name}" } small { "{subtitle}" } }
                                    if tenant() == name { span { class: "tenant-option__check", Icon { name: "check" } } }
                                }
                            }
                            div { class: "tenant-menu__footnote", "Tenant selection changes the workspace label. Database isolation is not configured yet." }
                        }
                    }
                }
                div { class: "sidebar-section-label", "General" }
                nav { class: "sidebar-nav sidebar-nav--dashboard",
                    Link { class: if active == "overview" { "sidebar-link selected" } else { "sidebar-link" }, to: Route::Overview {}, span { class: "sidebar-icon", Icon { name: "layout" } } "Overview" }
                }
                div { class: "sidebar-section-label", "Workspace" }
                nav { class: "sidebar-nav",
                    Link { class: if active == "datatables" { "sidebar-link selected" } else { "sidebar-link" }, to: Route::DataTables {}, span { class: "sidebar-icon", Icon { name: "table" } } "Data tables" }
                    Link { class: if active == "workflows" { "sidebar-link selected" } else { "sidebar-link" }, to: Route::WorkflowList {}, span { class: "sidebar-icon", Icon { name: "workflow" } } "Workflows" }
                    Link { class: if active == "executions" { "sidebar-link selected" } else { "sidebar-link" }, to: Route::Executions {}, span { class: "sidebar-icon", Icon { name: "clock" } } "Executions" }
                }
                div { class: "sidebar-section-label", "Resources" }
                nav { class: "sidebar-nav",
                    Link { class: if active == "credentials" { "sidebar-link selected" } else { "sidebar-link" }, to: Route::Credentials {}, span { class: "sidebar-icon", Icon { name: "key" } } "Credentials" }
                    Link { class: if active == "variables" { "sidebar-link selected" } else { "sidebar-link" }, to: Route::Variables {}, span { class: "sidebar-icon", Icon { name: "variable" } } "Variables" }
                }
                div { class: "sidebar-spacer" }
                nav { class: "sidebar-nav sidebar-nav--bottom",
                    button { class: "sidebar-link", r#type: "button", span { class: "sidebar-icon", Icon { name: "settings" } } "Settings" }
                    button { class: "sidebar-link", r#type: "button", span { class: "sidebar-icon", Icon { name: "help" } } "Help center" }
                }
                div { class: "sidebar-user", span { class: "sidebar-user__avatar", "FF" } span { strong { "Ferris Flow" } small { "Local user" } } span { class: "sidebar-user__menu", Icon { name: "chevron-down" } } }
            }
            div { class: "workspace",
                header { class: "topbar",
                    div { class: "breadcrumbs",
                        if let Some(back) = breadcrumb_back { {back} }
                        span { class: "crumb-workspace", "Personal" }
                        span { "/" }
                        if let Some(title) = breadcrumb_title.as_ref() {
                            if breadcrumb_editing() {
                                input {
                                    class: "breadcrumbs__rename-input",
                                    value: "{breadcrumb_draft}",
                                    autofocus: true,
                                    aria_label: "Workflow name",
                                    oninput: {
                                        move |event| {
                                            let value = event.value();
                                            breadcrumb_draft.set(value.clone());
                                        }
                                    },
                                    onblur: {
                                        let handler = on_breadcrumb_rename.clone();
                                        move |_| {
                                            if let Some(handler) = handler.as_ref() { handler.call(breadcrumb_draft()); }
                                            breadcrumb_editing.set(false);
                                        }
                                    },
                                    onkeydown: {
                                        let handler = on_breadcrumb_rename.clone();
                                        let original_title = title.clone();
                                        move |event| {
                                            if event.key() == Key::Enter {
                                                if let Some(handler) = handler.as_ref() { handler.call(breadcrumb_draft()); }
                                                breadcrumb_editing.set(false);
                                            } else if event.key() == Key::Escape {
                                                breadcrumb_draft.set(original_title.clone());
                                                breadcrumb_editing.set(false);
                                            }
                                        }
                                    },
                                }
                            } else {
                                span {
                                    class: "breadcrumbs__document",
                                    title: "Double-click to rename",
                                    ondoubleclick: {
                                        let title = title.clone();
                                        move |_| {
                                            breadcrumb_draft.set(title.clone());
                                            breadcrumb_editing.set(true);
                                        }
                                    },
                                    "{title}"
                                }
                            }
                            div { class: "document-menu-wrap",
                                button {
                                    class: "document-menu-trigger",
                                    r#type: "button",
                                    title: "More workflow actions",
                                    aria_label: "More workflow actions",
                                    aria_expanded: "{document_menu()}",
                                    onclick: move |_| document_menu.toggle(),
                                    Icon { name: "more" }
                                }
                                if document_menu() {
                                    div { class: "document-menu",
                                        for (label, icon, action) in [
                                            ("Rename", "pencil", "rename"),
                                            ("Version history", "history", "version-history"),
                                            ("Settings", "settings", "settings"),
                                            ("Archive", "archive", "archive"),
                                        ] {
                                            button {
                                                class: if action == "archive" { "document-menu__item document-menu__item--danger" } else { "document-menu__item" },
                                                r#type: "button",
                                                onclick: {
                                                    let handler = on_document_action.clone();
                                                    let title = breadcrumb_title.clone();
                                                    move |_| {
                                                        if action == "rename" {
                                                            if let Some(title) = title.as_ref() { breadcrumb_draft.set(title.clone()); }
                                                            breadcrumb_editing.set(true);
                                                        }
                                                        if let Some(handler) = handler.as_ref() { handler.call(action.to_string()); }
                                                        document_menu.set(false);
                                                    }
                                                },
                                                Icon { name: icon }
                                                span { "{label}" }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            "{header}"
                        }
                    }
                    div { class: "topbar-right",
                        span { class: "service-state", span { class: "state-dot" } "Connected" }
                        if let Some(save) = breadcrumb_save { {save} }
                    }
                }
                main { class: "main-content",
                    if breadcrumb_title.is_none() {
                        div { class: "page-heading",
                            div { h1 { "{header}" } p { "{intro}" } }
                            div { class: "heading-actions", {actions} }
                        }
                    }
                    {children}
                }
            }
        }
    }
}

#[component]
pub(crate) fn StatusPill(active: bool) -> Element {
    rsx! { Badge { variant: if active { BadgeVariant::Success } else { BadgeVariant::Secondary }, class: if active { "status status-active" } else { "status status-paused" }, span { class: "status-dot" }, if active { "Active" } else { "Paused" } } }
}

#[component]
pub(crate) fn TableStatus(active: bool) -> Element {
    let icon = if active { "circle-check" } else { "circle-minus" };
    let label = if active { "Active" } else { "Paused" };
    rsx! {
        span { class: "shared-table-status",
            Icon { name: icon }
            "{label}"
        }
    }
}

#[component]
pub(crate) fn EmptyState(searched: bool) -> Element {
    rsx! {
        div { class: "empty-state",
            div { class: "empty-glyph", "≋" }
            h3 { if searched { "No workflows match that search" } else { "Your registry is ready" } }
            p { if searched { "Try another name or clear the search." } else { "Create your first workflow to begin managing OWS documents." } }
            if !searched { Link { class: "button button-primary", to: Route::NewWorkflow {}, "Create workflow" } }
        }
    }
}

#[component]
pub(crate) fn ErrorPanel(message: String) -> Element {
    rsx! { div { class: "error-panel", strong { "Something went wrong" } p { "{message}" } } }
}

#[component]
pub(crate) fn LoadingPanel() -> Element {
    rsx! { div { class: "loading-panel", span { class: "loader" }, "Loading workflow data…" } }
}

pub(super) fn format_date(value: &str) -> String {
    value.split('T').next().unwrap_or(value).to_string()
}

pub(super) fn format_datetime(value: &str) -> String {
    value.replace('T', " ").trim_end_matches(".000").to_string()
}

pub(super) fn pretty_json(value: &str) -> String {
    serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| value.to_string())
}
