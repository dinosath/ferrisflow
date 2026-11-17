//! Optional OAuth 2.0 bearer-token authentication for the HTTP API.
//!
//! The resource server validates access tokens through an OAuth 2.0 token
//! introspection endpoint (RFC 7662). Configuration is read from the process
//! environment so local and desktop builds remain usable without an identity
//! provider.

use axum::{
    Json, Router,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct AuthMiddlewareState {
    pub config: AuthConfig,
    pub db: sea_orm::DatabaseConnection,
}

#[derive(Clone, Debug)]
pub struct AuthConfig {
    required: bool,
    introspection_url: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    read_scope: String,
    write_scope: String,
    admin_scope: String,
    http: reqwest::Client,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let required = std::env::var("AUTH_REQUIRED")
            .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        Self {
            required,
            introspection_url: std::env::var("OAUTH_INTROSPECTION_URL").ok(),
            client_id: std::env::var("OAUTH_CLIENT_ID").ok(),
            client_secret: std::env::var("OAUTH_CLIENT_SECRET").ok(),
            read_scope: std::env::var("OAUTH_READ_SCOPE")
                .unwrap_or_else(|_| "ferrisflow:read".to_owned()),
            write_scope: std::env::var("OAUTH_WRITE_SCOPE")
                .unwrap_or_else(|_| "ferrisflow:write".to_owned()),
            admin_scope: std::env::var("OAUTH_ADMIN_SCOPE")
                .unwrap_or_else(|_| "ferrisflow:admin".to_owned()),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("create OAuth introspection client"),
        }
    }

    fn configured(&self) -> bool {
        let secure_endpoint = self
            .introspection_url
            .as_deref()
            .and_then(|value| url::Url::parse(value).ok())
            .is_some_and(|endpoint| {
                if !endpoint.username().is_empty()
                    || endpoint.password().is_some()
                    || endpoint.fragment().is_some()
                {
                    return false;
                }
                if endpoint.scheme() == "https" {
                    return endpoint.host().is_some();
                }
                endpoint.scheme() == "http"
                    && endpoint.host_str().is_some_and(|host| {
                        host.eq_ignore_ascii_case("localhost")
                            || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
                    })
            });
        secure_endpoint
            && self.client_id.as_ref().is_some_and(|value| !value.is_empty())
            && self.client_secret.as_ref().is_some_and(|value| !value.is_empty())
    }
}

/// Authenticated OAuth principal, attached to requests after token validation.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AuthPrincipal {
    pub subject: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl AuthPrincipal {
    /// Returns whether the principal has the requested OAuth scope.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|granted| granted == scope)
    }
}

#[derive(Debug, Deserialize)]
struct IntrospectionResponse {
    active: bool,
    sub: Option<String>,
    username: Option<String>,
    scope: Option<String>,
}

pub fn routes() -> Router<crate::app_state::AppState> {
    Router::new().route("/api/auth/me", get(current_principal))
}

async fn current_principal(
    principal: Option<axum::Extension<AuthPrincipal>>,
) -> Result<Json<AuthPrincipal>, StatusCode> {
    principal.map(|axum::Extension(principal)| Json(principal)).ok_or(StatusCode::UNAUTHORIZED)
}

/// Require an authenticated principal with at least one of the supplied scopes.
/// This can be applied to individual routes with `route_layer` as the API grows
/// more granular authorization policies.
pub fn principal_has_any_scope(principal: &AuthPrincipal, scopes: &[&str]) -> bool {
    scopes.iter().any(|scope| principal.has_scope(scope))
}

pub async fn authorize_request(
    State(state): State<AuthMiddlewareState>, mut request: Request<axum::body::Body>, next: Next,
) -> Response {
    let config = &state.config;
    let path = request.uri().path();
    if path == "/api/health" || !(path.starts_with("/api/") || path.starts_with("/entities.v1.")) {
        return next.run(request).await;
    }
    let admin_endpoint = path == "/api/users" || path.starts_with("/api/users/");
    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if let Some(credentials) = auth_header.and_then(|value| value.strip_prefix("Basic ")) {
        let decoded = BASE64
            .decode(credentials)
            .ok()
            .and_then(|decoded| String::from_utf8(decoded).ok())
            .and_then(|decoded| {
                let (username, password) = decoded.split_once(':')?;
                Some((username.to_owned(), password.to_owned()))
            });
        if let Some((username, password)) = decoded
            && admin_password_matches(&state.db, &username, &password).await
        {
            let principal = AuthPrincipal {
                subject: "admin".to_owned(),
                username: Some("admin".to_owned()),
                scopes: vec![
                    config.admin_scope.clone(),
                    config.read_scope.clone(),
                    config.write_scope.clone(),
                ],
            };
            request.extensions_mut().insert(principal);
            return next.run(request).await;
        }
        if admin_endpoint {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error":"invalid_admin_credentials"})),
            )
                .into_response();
        }
    }

    let token = auth_header.and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = token.filter(|token| !token.is_empty()) else {
        if !admin_endpoint && !config.required {
            return next.run(request).await;
        }
        if !config.configured() && config.required && !admin_endpoint {
            tracing::error!("AUTH_REQUIRED is enabled but OAuth introspection is not configured");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error":"oauth_introspection_not_configured"})),
            )
                .into_response();
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error":"missing_or_invalid_credentials"})),
        )
            .into_response();
    };
    if !config.configured() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error":"oauth_not_configured"})),
        )
            .into_response();
    }

    let response = config
        .http
        .post(config.introspection_url.as_deref().unwrap_or_default())
        .basic_auth(
            config.client_id.as_deref().unwrap_or_default(),
            config.client_secret.as_deref(),
        )
        .form(&[("token", token)])
        .send()
        .await;
    let introspection = match response {
        Ok(response) if response.status().is_success() => {
            response.json::<IntrospectionResponse>().await
        }
        Ok(_) => return oauth_unavailable(),
        Err(error) => {
            tracing::warn!(%error, "OAuth token introspection failed");
            return oauth_unavailable();
        }
    };
    let introspection = match introspection {
        Ok(result) => result,
        Err(error) => {
            tracing::warn!(%error, "OAuth introspection response was invalid");
            return oauth_unavailable();
        }
    };
    let Some(subject) = introspection.sub.filter(|_| introspection.active) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error":"invalid_bearer_token"})),
        )
            .into_response();
    };
    let scopes =
        introspection.scope.unwrap_or_default().split_whitespace().map(str::to_owned).collect();
    let principal = AuthPrincipal { subject, username: introspection.username, scopes };
    if admin_endpoint || config.required {
        let method = request.method();
        let required_scope = if admin_endpoint {
            &config.admin_scope
        } else if method == axum::http::Method::POST
            || method == axum::http::Method::PUT
            || method == axum::http::Method::PATCH
            || method == axum::http::Method::DELETE
        {
            &config.write_scope
        } else {
            &config.read_scope
        };
        if !principal.has_scope(required_scope) {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error":"insufficient_scope",
                    "required_scope":required_scope
                })),
            )
                .into_response();
        }
    }
    request.extensions_mut().insert(principal);
    next.run(request).await
}

async fn admin_password_matches(
    db: &sea_orm::DatabaseConnection, username: &str, supplied_password: &str,
) -> bool {
    if username != "admin" {
        return false;
    }
    crate::models::app_setting::Entity::find_by_id("auth.admin_password")
        .one(db)
        .await
        .ok()
        .flatten()
        .is_some_and(|setting| {
            let expected = crate::app_state::hash_admin_password(supplied_password);
            constant_time_equal(setting.value.as_bytes(), expected.as_bytes())
        })
}

fn constant_time_equal(expected: &[u8], supplied: &[u8]) -> bool {
    if expected.len() != supplied.len() {
        return false;
    }
    expected.iter().zip(supplied).fold(0u8, |diff, (left, right)| diff | (left ^ right)) == 0
}

fn oauth_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"error":"oauth_provider_unavailable"})),
    )
        .into_response()
}
