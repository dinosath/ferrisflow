#![allow(non_snake_case)]

#[cfg(feature = "ui")]
use dioxus::prelude::*;

mod api_http;

#[cfg(feature = "ui")]
mod components;
#[cfg(feature = "ui")]
mod pages;

#[cfg(feature = "ui")]
use pages::{
    AiSettings, ContentTypeViewer, Credentials, DataTables, EditPage, EditWorkflow, Executions,
    NewPage, NewWorkflow, NovelViewer, Overview, Pages, PluginDetailPage, PluginPagePage,
    PluginViewPage, Plugins, SyncSettingsPage, Variables, ViewWorkflowExecution, WorkflowDetail,
    WorkflowList,
};

#[cfg(any(feature = "api-server", all(feature = "server", not(feature = "ui"))))]
#[tokio::main]
async fn main() {
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let state = ferrisflow::app_state::AppState::new().await;
    let api = ferrisflow::build_router(state);
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
    not(target_arch = "wasm32"),
    not(feature = "local-app")
))]
fn main() {
    dioxus::serve(|| async move {
        let state = ferrisflow::app_state::AppState::new().await;
        let api = ferrisflow::build_router(state);
        // The Dioxus router renders app routes; merge in the API routes
        // while keeping the UI renderer as the final fallback.
        Ok(dioxus::server::router(app).merge(api.reset_fallback()))
    });
}

#[cfg(all(
    feature = "ui",
    feature = "local-app",
    not(target_arch = "wasm32"),
    not(feature = "api-server")
))]
fn main() {
    #[cfg(all(feature = "mobile", target_os = "android"))]
    initialize_android_database();
    #[cfg(all(feature = "local-app", not(feature = "mobile")))]
    initialize_desktop_database();
    initialize_local_app();
    dioxus::launch(app);
}

#[cfg(all(
    feature = "ui",
    not(feature = "api-server"),
    not(feature = "local-app"),
    any(not(feature = "server"), target_arch = "wasm32")
))]
fn main() {
    dioxus::launch(app);
}

#[cfg(all(feature = "mobile", target_os = "android"))]
fn initialize_android_database() {
    use std::path::PathBuf;

    use jni::objects::JString;

    let data_dir = robius_android_env::with_activity(|env, activity| {
        let files = env.call_method(activity, "getFilesDir", "()Ljava/io/File;", &[])?.l()?;
        let absolute =
            env.call_method(&files, "getAbsolutePath", "()Ljava/lang/String;", &[])?.l()?;
        let path: String = env.get_string(&JString::from(absolute))?.into();
        Ok::<_, jni::errors::Error>(PathBuf::from(path))
    })
    .expect("read Android application data directory")
    .expect("resolve Android application data directory");
    let database_path = data_dir.join("ferris-workflow.db");
    let database_url = format!("sqlite://{}?mode=rwc", database_path.display());
    unsafe {
        std::env::set_var("DATABASE_URL", database_url);
    }
}

#[cfg(all(feature = "local-app", not(feature = "mobile"), not(target_arch = "wasm32")))]
fn initialize_desktop_database() {
    use std::path::PathBuf;

    if std::env::var_os("DATABASE_URL").is_some() {
        return;
    }
    #[cfg(target_os = "windows")]
    let data_dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .join("Ferrisflow");
    #[cfg(target_os = "macos")]
    let data_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .join("Library/Application Support/Ferrisflow");
    #[cfg(all(unix, not(target_os = "macos")))]
    let data_dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .join("ferrisflow");

    std::fs::create_dir_all(&data_dir).expect("create Ferrisflow data directory");
    let database_path = data_dir.join("ferris-workflow.db");
    let database_url = format!("sqlite://{}?mode=rwc", database_path.display());
    unsafe {
        std::env::set_var("DATABASE_URL", database_url);
    }
}

#[cfg(all(feature = "local-app", not(target_arch = "wasm32")))]
fn initialize_local_app() {
    use std::{sync::mpsc, time::Duration};

    let (started, ready) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("ferrisflow-local-runtime".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("create local app runtime");
            runtime.block_on(async move {
                let state = ferrisflow::app_state::AppState::new().await;
                let http_server_enabled = ferrisflow::http_server_enabled(&state).await;
                let router = ferrisflow::build_router(state);
                crate::api_http::install_router(router.clone())
                    .expect("initialize in-process API router");
                ferrisflow::configure_optional_http_server(router, http_server_enabled);
                started.send(()).expect("notify local app startup");
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            });
        })
        .expect("start local app runtime");
    ready.recv().expect("local app failed to start");
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
    #[route("/content-types/:id")]
    ContentTypeViewer { id: String },
    #[route("/novels/:id")]
    NovelViewer { id: String },
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
    #[route("/settings/sync")]
    SyncSettingsPage {},
}
