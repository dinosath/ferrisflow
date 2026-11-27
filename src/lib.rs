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
pub mod auth;
#[cfg(not(target_arch = "wasm32"))]
pub mod ows;
#[cfg(not(target_arch = "wasm32"))]
mod workflow_schedules;

/// Build the axum [`Router`] with all routes and middleware.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_router(state: app_state::AppState) -> axum::Router {
    let mut router = axum::Router::new();
    router = router.merge(controllers::routes());
    router = router.merge(auth::routes());
    let (oidc_codec, oidc_issuer) = match auth::oidc_router()
        .expect("OIDC configuration must be valid when OIDC_ENABLED is true")
    {
        Some(oidc) => {
            router = router.merge(oidc.router.with_state::<app_state::AppState>(()));
            (Some(oidc.codec), Some(oidc.issuer))
        }
        None => (None, None),
    };
    let connect_service = connect::service(controllers::routes().with_state(state.clone()));
    router = router.route_service(
        "/entities.v1.EntityService/Execute",
        connect_service.into_service::<axum::body::Body>(),
    );
    let router = router
        .layer(axum::middleware::from_fn_with_state(
            auth::AuthMiddlewareState {
                config: auth::AuthConfig::from_env(),
                db: state.db.clone(),
                oidc_codec,
                oidc_issuer,
            },
            auth::authorize_request,
        ))
        .with_state(state);
    with_security_headers(router)
}

/// Add baseline browser security headers to a router, including the fullstack UI.
#[cfg(not(target_arch = "wasm32"))]
pub fn with_security_headers<S>(router: axum::Router<S>) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    use axum::http::{HeaderValue, header};
    use tower_http::set_header::SetResponseHeaderLayer;

    router
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
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
}
