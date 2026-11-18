#[cfg(not(target_arch = "wasm32"))]
pub mod connect;
#[cfg(not(target_arch = "wasm32"))]
pub mod controllers;
#[cfg(not(target_arch = "wasm32"))]
pub mod db_triggers;
#[cfg(not(target_arch = "wasm32"))]
pub mod models {
    automod::dir!(pub "src/models");
}
#[cfg(not(target_arch = "wasm32"))]
pub mod app_state;
#[cfg(not(target_arch = "wasm32"))]
pub mod ows;
#[cfg(not(target_arch = "wasm32"))]
mod workflow_schedules;

/// Build the axum [`Router`] with all routes and middleware.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_router(state: app_state::AppState) -> axum::Router {
    use axum::http::HeaderValue;
    use axum::http::header;
    use tower_http::set_header::SetResponseHeaderLayer;

    let mut router = axum::Router::new();
    router = router.merge(controllers::routes());
    let connect_service = connect::service(controllers::routes().with_state(state.clone()));
    router = router.route_service(
        "/entities.v1.EntityService/Execute",
        connect_service.into_service::<axum::body::Body>(),
    );
    // ─── Security headers (always applied) ──────────────────────────────
    router = router
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("x-xss-protection"),
            HeaderValue::from_static("1; mode=block"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("x-download-options"),
            HeaderValue::from_static("noopen"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ));

    router.with_state(state)
}

#[cfg(not(target_arch = "wasm32"))]
static HTTP_SERVER_CONTROL: std::sync::OnceLock<tokio::sync::watch::Sender<bool>> =
    std::sync::OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
pub async fn http_server_enabled(state: &app_state::AppState) -> bool {
    controllers::settings::http_server_enabled(state).await
}

/// Starts the optional network listener and returns immediately. The router remains usable
/// in-process regardless of whether the listener is enabled.
#[cfg(not(target_arch = "wasm32"))]
pub fn configure_optional_http_server(router: axum::Router, enabled: bool) {
    use tokio::sync::watch;

    let (sender, receiver) = watch::channel(enabled);
    if HTTP_SERVER_CONTROL.set(sender).is_err() {
        tracing::warn!("optional HTTP server control has already been configured");
        return;
    }
    tokio::spawn(run_optional_http_server(router, receiver));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn set_http_server_enabled(enabled: bool) {
    if let Some(sender) = HTTP_SERVER_CONTROL.get() {
        sender.send_replace(enabled);
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn run_optional_http_server(
    router: axum::Router, mut enabled: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        if !*enabled.borrow_and_update() {
            if enabled.changed().await.is_err() {
                return;
            }
            continue;
        }

        let ip = std::env::var("IP").unwrap_or_else(|_| "0.0.0.0".to_owned());
        let port =
            std::env::var("PORT").ok().and_then(|value| value.parse::<u16>().ok()).unwrap_or(8080);
        let listener = match tokio::net::TcpListener::bind((ip.as_str(), port)).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!(%error, %ip, port, "could not start optional HTTP server");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
        };
        tracing::info!(address = %listener.local_addr().unwrap_or_else(|_| "0.0.0.0:8080".parse().unwrap()), "optional HTTP server started");
        let shutdown = wait_for_http_server_disable(enabled.clone());
        if let Err(error) =
            axum::serve(listener, router.clone()).with_graceful_shutdown(shutdown).await
        {
            tracing::error!(%error, "optional HTTP server stopped with an error");
        }
        tracing::info!("optional HTTP server stopped");
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn wait_for_http_server_disable(mut enabled: tokio::sync::watch::Receiver<bool>) {
    while *enabled.borrow() {
        if enabled.changed().await.is_err() {
            break;
        }
    }
}
