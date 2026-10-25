use crate::components::icons::Icon;
use crate::components::ui::card::Card;
use dioxus::prelude::*;

use crate::{
    Route,
    pages::{
        api::list_workflows,
        models::Workflow,
        shared::{ErrorPanel, LoadingPanel, Shell, StatusPill, format_date},
    },
};

#[component]
pub(crate) fn Overview() -> Element {
    let workflows = use_resource(list_workflows);

    rsx! {
        Shell { active: "overview", header: "Overview", intro: "Monitor your workspace at a glance.", actions: rsx! { Link { class: "button button-primary", to: Route::NewWorkflow {}, "+ New workflow" } },
            match workflows() {
                Some(Ok(rows)) => rsx! { OverviewContent { rows } },
                Some(Err(error)) => rsx! { ErrorPanel { message: error } },
                None => rsx! { LoadingPanel {} },
            }
        }
    }
}

#[component]
fn OverviewContent(rows: Vec<Workflow>) -> Element {
    let total = rows.len();
    let active = rows.iter().filter(|row| row.active).count();
    let paused = total.saturating_sub(active);
    let active_percent = if total == 0 { 0.0 } else { active as f32 / total as f32 * 100.0 };
    let mut recent = rows;
    recent.sort_by(|left, right| right.last_updated.cmp(&left.last_updated));
    recent.truncate(5);

    rsx! {
        section { class: "dashboard-metrics",
            MetricCard { label: "Total workflows", value: "{total}", detail: "Saved in this workspace", icon: "workflow", tone: "violet" }
            MetricCard { label: "Active", value: "{active}", detail: "Ready to run", icon: "check", tone: "green" }
            MetricCard { label: "Paused", value: "{paused}", detail: "Currently disabled", icon: "clock", tone: "amber" }
        }
        div { class: "dashboard-grid",
            section { class: "dashboard-card dashboard-recent",
                div { class: "dashboard-card__heading",
                    div { h2 { "Recent workflows" } p { "The latest changes in your workspace." } }
                    Link { class: "dashboard-card__link", to: Route::WorkflowList {}, "View all →" }
                }
                if recent.is_empty() {
                    div { class: "dashboard-empty", span { Icon { name: "workflow" } } strong { "No workflows yet" } p { "Create your first workflow to start building your workspace." } Link { class: "button button-primary", to: Route::NewWorkflow {}, "Create workflow" } }
                } else {
                    div { class: "dashboard-table-wrap",
                        table { class: "dashboard-table",
                            thead { tr { th { "WORKFLOW" } th { "STATUS" } th { "VERSION" } th { "LAST UPDATED" } th { "" } } }
                            tbody {
                                for row in recent {
                                    tr { key: "{row.id}",
                                        td { class: "dashboard-workflow-name", Link { to: Route::WorkflowDetail { id: row.id }, "{row.name}" } small { "#{row.id} · OWS workflow" } }
                                        td { StatusPill { active: row.active } }
                                        td { "v{row.version}" }
                                        td { class: "date-cell", "{format_date(&row.last_updated)}" }
                                        td { Link { class: "dashboard-row-action", to: Route::WorkflowDetail { id: row.id }, Icon { name: "arrow-up-right" } } }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            aside { class: "dashboard-side-stack",
                section { class: "dashboard-card dashboard-health",
                    div { class: "dashboard-card__heading",
                        div { h2 { "Workflow status" } p { "Current availability" } }
                        span { class: "dashboard-health__icon", Icon { name: "activity" } }
                    }
                    div { class: "status-bar",
                        div { class: "status-bar__active", style: "width: {active_percent}%" }
                        div { class: "status-bar__paused", style: "width: {100.0 - active_percent}%" }
                    }
                    div { class: "status-breakdown",
                        div { span { class: "dashboard-legend-dot active-dot" } span { "Active" } strong { "{active}" } }
                        div { span { class: "dashboard-legend-dot paused-dot" } span { "Paused" } strong { "{paused}" } }
                    }
                    Link { class: "dashboard-secondary-link", to: Route::WorkflowList {}, "Manage workflows →" }
                }
                section { class: "dashboard-card dashboard-shortcuts",
                    div { class: "dashboard-card__heading", div { h2 { "Quick access" } p { "Jump into your workspace." } } }
                    Link { class: "dashboard-shortcut", to: Route::DataTables {}, span { class: "shortcut-icon violet", Icon { name: "table" } } span { strong { "Data tables" } small { "Browse database records" } } span { Icon { name: "arrow-up-right" } } }
                    Link { class: "dashboard-shortcut", to: Route::WorkflowList {}, span { class: "shortcut-icon blue", Icon { name: "workflow" } } span { strong { "Workflows" } small { "Manage OWS definitions" } } span { Icon { name: "arrow-up-right" } } }
                }
            }
        }
    }
}

#[component]
fn MetricCard(
    label: &'static str, value: String, detail: &'static str, icon: &'static str,
    tone: &'static str,
) -> Element {
    rsx! {
        Card { class: "metric-card",
            div { class: "metric-card__top", span { "{label}" } span { class: "metric-icon {tone}", Icon { name: icon } } }
            strong { class: "metric-card__value", "{value}" }
            small { class: "metric-card__detail", "{detail}" }
        }
    }
}
