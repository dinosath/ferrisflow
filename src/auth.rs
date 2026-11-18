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
        let required = authentication_required(std::env::var("AUTH_REQUIRED").ok().as_deref());
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

fn authentication_required(value: Option<&str>) -> bool {
    value.is_none_or(|value| !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no"))
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
        if let Some((username, password)) = decoded {
            if username == "admin" && admin_password_matches(&state.db, &username, &password).await
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

            if let Some(user) = authenticate_user(&state.db, &username, &password).await {
                let principal = AuthPrincipal {
                    subject: user.id.to_string(),
                    username: Some(user.username),
                    scopes: vec![config.read_scope.clone(), config.write_scope.clone()],
                };
                if admin_endpoint {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(serde_json::json!({
                            "error":"insufficient_scope",
                            "required_scope":config.admin_scope
                        })),
                    )
                        .into_response();
                }
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
    use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};

    if username != "admin" {
        return false;
    }
    let Some(setting) = crate::models::app_setting::Entity::find_by_id("auth.admin_password")
        .one(db)
        .await
        .ok()
        .flatten()
    else {
        return false;
    };
    let matched = if setting.value.starts_with("$argon2") {
        use argon2::password_hash::{PasswordHash, PasswordVerifier};
        PasswordHash::new(&setting.value).ok().is_some_and(|hash| {
            argon2::Argon2::default().verify_password(supplied_password.as_bytes(), &hash).is_ok()
        })
    } else if let Some(legacy_hash) = setting.value.strip_prefix("sha256:") {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(supplied_password.as_bytes());
        let supplied = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        constant_time_equal(legacy_hash.as_bytes(), supplied.as_bytes())
    } else {
        constant_time_equal(setting.value.as_bytes(), supplied_password.as_bytes())
    };
    if matched && !setting.value.starts_with("$argon2") {
        let mut active = setting.into_active_model();
        active.value = Set(crate::app_state::hash_admin_password(supplied_password));
        if let Err(error) = active.update(db).await {
            tracing::warn!(%error, "could not upgrade legacy admin password hash");
        }
    }
    matched
}

async fn authenticate_user(
    db: &sea_orm::DatabaseConnection, username: &str, password: &str,
) -> Option<crate::models::user::Model> {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let user = crate::models::user::Entity::find()
        .filter(crate::models::user::Column::Username.eq(username))
        .one(db)
        .await
        .ok()??;
    let password_hash = user.password_hash.as_deref()?;
    let parsed_hash = PasswordHash::new(password_hash).ok()?;
    argon2::Argon2::default().verify_password(password.as_bytes(), &parsed_hash).ok()?;
    Some(user)
}

pub(crate) fn hash_user_password(password: &str) -> String {
    use argon2::password_hash::{PasswordHasher, SaltString};

    let salt_bytes: [u8; 16] = rand::random();
    let salt = SaltString::encode_b64(&salt_bytes).expect("encode random password salt");
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("hash user password")
        .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::get};
    use sea_orm::{ActiveModelTrait, Database, Set};
    use tower::ServiceExt;

    const USERNAME: &str = "regular-user";
    const USER_PASSWORD: &str = "regular-user-password";
    const ADMIN_PASSWORD: &str = "test-admin-password";

    fn config(required: bool) -> AuthConfig {
        AuthConfig {
            required,
            introspection_url: None,
            client_id: None,
            client_secret: None,
            read_scope: "ferrisflow:read".to_owned(),
            write_scope: "ferrisflow:write".to_owned(),
            admin_scope: "ferrisflow:admin".to_owned(),
            http: reqwest::Client::new(),
        }
    }

    async fn test_db(admin_hash: String) -> sea_orm::DatabaseConnection {
        use migration::MigratorTrait;

        let db = Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        crate::models::app_setting::ActiveModel {
            key: Set("auth.admin_password".to_owned()),
            value: Set(admin_hash),
        }
        .insert(&db)
        .await
        .unwrap();
        let now = chrono::Utc::now().naive_utc();
        crate::models::user::ActiveModel {
            username: Set(USERNAME.to_owned()),
            email: Set(None),
            display_name: Set("Regular User".to_owned()),
            password_hash: Set(Some(hash_user_password(USER_PASSWORD))),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        db
    }

    fn app(db: sea_orm::DatabaseConnection, required: bool) -> Router {
        let protected_routes = Router::new()
            .route("/", get(|| async { "login page" }))
            .route("/api/health", get(|| async { StatusCode::OK }))
            .route("/api/workflows", get(|| async { StatusCode::OK }))
            .route("/api/auth/me", get(current_principal))
            .route("/api/users", get(|| async { StatusCode::OK }))
            .layer(axum::middleware::from_fn_with_state(
                AuthMiddlewareState { config: config(required), db },
                authorize_request,
            ));
        crate::with_security_headers(protected_routes)
    }

    async fn request(app: Router, path: &str, credentials: Option<(&str, &str)>) -> Response {
        let mut request = Request::builder().uri(path);
        if let Some((username, password)) = credentials {
            let encoded = BASE64.encode(format!("{username}:{password}"));
            request = request.header(axum::http::header::AUTHORIZATION, format!("Basic {encoded}"));
        }
        app.oneshot(request.body(Body::empty()).unwrap()).await.unwrap()
    }

    #[test]
    fn authentication_is_required_unless_explicitly_disabled() {
        assert!(authentication_required(None));
        assert!(authentication_required(Some("true")));
        assert!(authentication_required(Some("typo")));
        assert!(!authentication_required(Some("false")));
        assert!(!authentication_required(Some("0")));
        assert!(!authentication_required(Some("no")));
    }

    #[tokio::test]
    async fn unauthenticated_api_is_denied_and_browser_headers_are_always_set() {
        let db = test_db(crate::app_state::hash_admin_password(ADMIN_PASSWORD)).await;
        let app = app(db, true);

        let ui = request(app.clone(), "/", None).await;
        assert_eq!(ui.status(), StatusCode::OK);
        assert_security_headers(&ui);

        let health = request(app.clone(), "/api/health", None).await;
        assert_eq!(health.status(), StatusCode::OK);
        assert_security_headers(&health);

        let protected = request(app, "/api/workflows", None).await;
        assert_eq!(protected.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_security_headers(&protected);
    }

    #[tokio::test]
    async fn regular_user_can_read_principal_but_cannot_administer_users() {
        let db = test_db(crate::app_state::hash_admin_password(ADMIN_PASSWORD)).await;
        let app = app(db, true);

        let principal = request(app.clone(), "/api/auth/me", Some((USERNAME, USER_PASSWORD))).await;
        assert_eq!(principal.status(), StatusCode::OK);

        let admin_route = request(app, "/api/users", Some((USERNAME, USER_PASSWORD))).await;
        assert_eq!(admin_route.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn only_valid_admin_credentials_can_access_user_administration() {
        let db = test_db(crate::app_state::hash_admin_password(ADMIN_PASSWORD)).await;
        let app = app(db, true);

        let valid = request(app.clone(), "/api/users", Some(("admin", ADMIN_PASSWORD))).await;
        assert_eq!(valid.status(), StatusCode::OK);

        let invalid = request(app, "/api/users", Some(("admin", "wrong-password"))).await;
        assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn successful_legacy_admin_login_upgrades_sha256_hash() {
        use sea_orm::EntityTrait;
        use sha2::{Digest, Sha256};

        let digest = Sha256::digest(ADMIN_PASSWORD.as_bytes());
        let legacy_hash = format!(
            "sha256:{}",
            digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        );
        let db = test_db(legacy_hash).await;
        let app = app(db.clone(), true);

        let response = request(app, "/api/users", Some(("admin", ADMIN_PASSWORD))).await;
        assert_eq!(response.status(), StatusCode::OK);

        let setting = crate::models::app_setting::Entity::find_by_id("auth.admin_password")
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(setting.value.starts_with("$argon2"));
        assert!(!setting.value.starts_with("sha256:"));
    }

    fn assert_security_headers(response: &Response) {
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert_eq!(
            response.headers()["permissions-policy"],
            "camera=(), microphone=(), geolocation=()"
        );
    }
}
