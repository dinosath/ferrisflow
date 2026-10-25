#![allow(non_snake_case)]

use dioxus::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use mimalloc::MiMalloc;

mod components;
mod pages;

use pages::{
    Credentials, DataTables, EditWorkflow, Executions, NewWorkflow, Overview, Variables,
    WorkflowDetail, WorkflowList,
};

#[global_allocator]
#[cfg(not(target_arch = "wasm32"))]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        let state = ferris_workflow::app_state::AppState::new().await;
        let api = ferris_workflow::build_router(state);
        Ok(dioxus::server::router(app).merge(api))
    });

    #[cfg(not(feature = "server"))]
    dioxus::launch(app);
}

fn app() -> Element {
    let tenant = use_signal(|| "Ferrisflow".to_string());
    use_context_provider(|| tenant);
    rsx! {
        document::Link { rel: "stylesheet", href: asset!("/assets/tailwind.css") }
        document::Link { rel: "stylesheet", href: asset!("/assets/main.css") }
        document::Link { rel: "stylesheet", href: asset!("/assets/workspace.css") }
        document::Link { rel: "stylesheet", href: asset!("/assets/workflow-editor.css") }
        document::Link { rel: "stylesheet", href: asset!("/assets/workflow-editor-canvas.css") }
        document::Link { rel: "stylesheet", href: asset!("/assets/type-scale.css") }
        document::Link { rel: "stylesheet", href: asset!("/assets/workflow-document-menu.css") }
        Router::<Route> {}
    }
}

#[derive(Clone, Routable, Debug, PartialEq)]
enum Route {
    #[route("/")]
    Overview {},
    #[route("/data-tables")]
    DataTables {},
    #[route("/workflows")]
    WorkflowList {},
    #[route("/new")]
    NewWorkflow {},
    #[route("/workflows/:id/edit")]
    EditWorkflow { id: i32 },
    #[route("/workflows/:id")]
    WorkflowDetail { id: i32 },
    #[route("/executions")]
    Executions {},
    #[route("/credentials")]
    Credentials {},
    #[route("/variables")]
    Variables {},
}
