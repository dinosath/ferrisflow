use dioxus::prelude::*;

use super::shared::Shell;
use crate::components::data_table::DataTable;
use crate::components::icons::Icon;

#[component]
pub(crate) fn Executions() -> Element {
    rsx! {
        Shell { active: "executions", header: "Executions", intro: "Review workflow runs and their results.", actions: rsx! {},
            DataTable {
                columns: vec!["Execution".to_string(), "Workflow".to_string(), "Status".to_string(), "Started".to_string(), "Duration".to_string()],
                row_ids: vec![],
                toolbar: rsx! {},
                empty_message: "No executions yet. Run a workflow to see its history here.".to_string(),
            }
        }
    }
}

#[component]
pub(crate) fn Credentials() -> Element {
    rsx! {
        Shell { active: "credentials", header: "Credentials", intro: "Manage the connections your workflows can use.", actions: rsx! {},
            DataTable {
                columns: vec!["Name".to_string(), "Type".to_string(), "Used by".to_string(), "Created".to_string(), "Updated".to_string()],
                row_ids: vec![],
                toolbar: rsx! {},
                empty_message: "No credentials configured yet.".to_string(),
            }
        }
    }
}

#[component]
pub(crate) fn Variables() -> Element {
    rsx! { WorkspaceSection { active: "variables", title: "Variables", description: "Manage values shared across your workspace.", icon: "variable", empty_title: "No variables defined", empty_copy: "Create workspace variables to reuse values across workflows." } }
}

#[component]
fn WorkspaceSection(
    active: &'static str, title: &'static str, description: &'static str, icon: &'static str,
    empty_title: &'static str, empty_copy: &'static str,
) -> Element {
    rsx! {
        Shell { active, header: title, intro: description, actions: rsx! {},
            section { class: "resource-empty panel",
                span { class: "resource-empty__icon", Icon { name: icon } }
                h2 { "{empty_title}" }
                p { "{empty_copy}" }
            }
        }
    }
}
