#![allow(non_snake_case)]

#[cfg(feature = "ui")]
use dioxus::prelude::*;

#[cfg(feature = "ui")]
mod components;
#[cfg(feature = "ui")]
mod pages;

#[cfg(feature = "ui")]
use pages::{
    AiSettings, Credentials, DataTables, EditPage, EditWorkflow, Executions, NewPage, NewWorkflow,
    Overview, Pages, PluginDetailPage, PluginPagePage, PluginViewPage, Plugins, Variables,
    ViewWorkflowExecution, WorkflowDetail, WorkflowList,
};

#[cfg(any(feature = "api-server", all(feature = "server", not(feature = "ui"))))]
#[tokio::main]
async fn main() {
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let state = ferris_workflow::app_state::AppState::new().await;
    let api = ferris_workflow::build_router(state);
    let ip = std::env::var("IP").unwrap_or_else(|_| "0.0.0.0".to_owned());
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_owned())
        .parse::<u16>()
        .expect("PORT must be a valid port number");
    let listener = tokio::net::TcpListener::bind((ip.as_str(), port))
        .await
        .expect("failed to bind server listener");

    axum::serve(listener, api).await.expect("server failed");
}

#[cfg(all(
    feature = "server",
    feature = "ui",
    not(feature = "api-server"),
    not(target_arch = "wasm32")
))]
fn main() {
    dioxus::serve(|| async move {
        let state = ferris_workflow::app_state::AppState::new().await;
        let api = ferris_workflow::build_router(state);
        // The Dioxus router renders app routes; merge in the API routes
        // while keeping the UI renderer as the final fallback.
        Ok(dioxus::server::router(app).merge(api.reset_fallback()))
    });
}

#[cfg(all(
    feature = "ui",
    not(feature = "api-server"),
    any(not(feature = "server"), target_arch = "wasm32")
))]
fn main() {
    dioxus::launch(app);
}

#[cfg(all(not(feature = "ui"), not(feature = "server"), not(feature = "api-server")))]
compile_error!("Select a launch mode: web, server, or fullstack");

#[cfg(feature = "ui")]
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
#[cfg(feature = "ui")]
enum Route {
    #[route("/")]
    Overview {},
    #[route("/data-tables")]
    DataTables {},
    #[route("/pages")]
    Pages {},
    #[route("/pages/new")]
    NewPage {},
    #[route("/pages/edit/:record_id")]
    EditPage { record_id: i32 },
    #[route("/plugins/:plugin_id/pages/:page_id")]
    PluginPagePage { plugin_id: String, page_id: String },
    #[route("/plugins/:plugin_id/views/:view_id")]
    PluginViewPage { plugin_id: String, view_id: String },
    #[route("/plugins")]
    Plugins {},
    #[route("/plugins/:plugin_id")]
    PluginDetailPage { plugin_id: String },
    #[route("/workflows")]
    WorkflowList {},
    #[route("/new")]
    NewWorkflow {},
    #[route("/workflows/:id/edit")]
    EditWorkflow { id: i32 },
    #[route("/workflows/:id/executions/:execution_id")]
    ViewWorkflowExecution { id: i32, execution_id: String },
    #[route("/workflows/:id")]
    WorkflowDetail { id: i32 },
    #[route("/executions")]
    Executions {},
    #[route("/credentials")]
    Credentials {},
    #[route("/variables")]
    Variables {},
    #[route("/settings/ai")]
    AiSettings {},
}
