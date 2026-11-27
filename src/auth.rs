//! OAuth 2.0 bearer-token authentication and optional OIDC browser sign-in.
//!
//! The resource server validates access tokens through an OAuth 2.0 token
//! introspection endpoint (RFC 7662). Configuration is read from the process
//! environment so local and desktop builds remain usable without an identity
//! provider.

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD},
};
use hmac::{Hmac, Mac};
use jsonwebtoken::{DecodingKey, Validation, decode};
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};
use sha2::Digest;
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
    Router::new()
        .route("/api/auth/me", get(current_principal))
        .route("/api/auth/config", get(auth_config))
        .route("/auth/oidc/login", get(oidc_login))
        .route("/auth/oidc/callback", get(oidc_callback))
        .route("/auth/oidc/logout", get(oidc_logout))
}

#[derive(Serialize)]
struct AuthUiConfig {
    oidc_enabled: bool,
}

async fn auth_config() -> Json<AuthUiConfig> {
    Json(AuthUiConfig { oidc_enabled: oidc_enabled() })
}

fn oidc_enabled() -> bool {
    std::env::var("OIDC_ENABLED")
        .ok()
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

#[derive(Clone)]
struct OidcSettings {
    issuer: String,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

impl OidcSettings {
    fn from_env() -> Result<Self, Response> {
        let value = |name: &str| std::env::var(name).ok().filter(|value| !value.trim().is_empty());
        let Some(issuer) = value("OIDC_ISSUER") else {
            return Err(oidc_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "OIDC_ISSUER is not configured",
            ));
        };
        let Some(client_id) = value("OIDC_CLIENT_ID") else {
            return Err(oidc_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "OIDC_CLIENT_ID is not configured",
            ));
        };
        let Some(client_secret) = value("OIDC_CLIENT_SECRET") else {
            return Err(oidc_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "OIDC_CLIENT_SECRET is not configured",
            ));
        };
        let redirect_uri = value("OIDC_REDIRECT_URI")
            .unwrap_or_else(|| "http://localhost:8080/auth/oidc/callback".to_owned());
        if !safe_provider_url(&issuer) || !safe_provider_url(&redirect_uri) {
            return Err(oidc_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "OIDC URLs must use HTTPS (HTTP is allowed on loopback)",
            ));
        }
        Ok(Self {
            issuer: issuer.trim_end_matches('/').to_owned(),
            client_id,
            client_secret,
            redirect_uri,
        })
    }
}

#[derive(Deserialize)]
struct OidcMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
}

#[derive(Serialize, Deserialize)]
struct OidcFlow {
    state: String,
    nonce: String,
    verifier: String,
    expires_at: u64,
}

#[derive(Deserialize)]
struct OidcCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct OidcTokens {
    access_token: String,
    id_token: String,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct IdClaims {
    iss: String,
    sub: String,
    nonce: String,
    exp: usize,
}

async fn oidc_login() -> Response {
    if !oidc_enabled() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let settings = match OidcSettings::from_env() {
        Ok(settings) => settings,
        Err(response) => return response,
    };
    let metadata = match oidc_metadata(&settings).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let state = random_url_token();
    let nonce = random_url_token();
    let verifier = random_url_token();
    let challenge = URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    let flow = OidcFlow { state, nonce, verifier, expires_at: unix_time() + 300 };
    let cookie = match sign_flow(&settings.client_secret, &flow) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let mut url = match url::Url::parse(&metadata.authorization_endpoint) {
        Ok(url) => url,
        Err(_) => {
            return oidc_error(StatusCode::BAD_GATEWAY, "Invalid OIDC authorization endpoint");
        }
    };
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &settings.client_id)
        .append_pair("redirect_uri", &settings.redirect_uri)
        .append_pair("scope", "openid profile email")
        .append_pair("state", &flow.state)
        .append_pair("nonce", &flow.nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    let mut response = Redirect::to(url.as_str()).into_response();
    set_cookie(
        &mut response,
        &format!(
            "ferrisflow_oidc_flow={cookie}; Path=/; HttpOnly; SameSite=Lax; Max-Age=300{}",
            secure_cookie(&settings.redirect_uri)
        ),
    );
    response
}

async fn oidc_callback(headers: HeaderMap, Query(query): Query<OidcCallback>) -> Response {
    if !oidc_enabled() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if query.error.is_some() {
        return oidc_error(StatusCode::UNAUTHORIZED, "Identity provider declined sign-in");
    }
    let settings = match OidcSettings::from_env() {
        Ok(settings) => settings,
        Err(response) => return response,
    };
    let flow_cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|header| header.to_str().ok())
        .and_then(|value| {
            value.split(';').find_map(|part| part.trim().strip_prefix("ferrisflow_oidc_flow="))
        });
    let Some(flow_cookie) = flow_cookie else {
        return oidc_error(StatusCode::BAD_REQUEST, "Missing OIDC login state");
    };
    let flow = match verify_flow(&settings.client_secret, flow_cookie) {
        Ok(flow) => flow,
        Err(response) => return response,
    };
    if flow.expires_at < unix_time() || query.state.as_deref() != Some(&flow.state) {
        return oidc_error(StatusCode::BAD_REQUEST, "OIDC login state did not match");
    }
    let Some(code) = query.code else {
        return oidc_error(StatusCode::BAD_REQUEST, "Missing OIDC authorization code");
    };
    let metadata = match oidc_metadata(&settings).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build();
    let Ok(http) = http else { return oauth_unavailable() };
    let token_result = match http
        .post(&metadata.token_endpoint)
        .basic_auth(&settings.client_id, Some(&settings.client_secret))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", settings.redirect_uri.as_str()),
            ("code_verifier", flow.verifier.as_str()),
        ])
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response.json::<OidcTokens>().await,
        _ => return oidc_error(StatusCode::BAD_GATEWAY, "OIDC token exchange failed"),
    };
    let tokens = match token_result {
        Ok(tokens) => tokens,
        Err(_) => return oidc_error(StatusCode::BAD_GATEWAY, "OIDC token response was invalid"),
    };
    if let Err(response) =
        validate_id_token(&http, &metadata, &settings, &tokens.id_token, &flow.nonce).await
    {
        return response;
    }
    let mut response = Redirect::to("/").into_response();
    set_cookie(
        &mut response,
        &format!(
            "ferrisflow_oidc={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
            tokens.access_token,
            tokens.expires_in.unwrap_or(3600),
            secure_cookie(&settings.redirect_uri)
        ),
    );
    set_cookie(
        &mut response,
        &format!(
            "ferrisflow_oidc_flow=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
            secure_cookie(&settings.redirect_uri)
        ),
    );
    response
}

async fn oidc_logout() -> Response {
    let secure =
        std::env::var("OIDC_REDIRECT_URI").ok().is_some_and(|uri| uri.starts_with("https://"));
    let mut response = Redirect::to("/").into_response();
    set_cookie(
        &mut response,
        &format!(
            "ferrisflow_oidc=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
            if secure { "; Secure" } else { "" }
        ),
    );
    response
}

async fn oidc_metadata(settings: &OidcSettings) -> Result<OidcMetadata, Response> {
    let discovery = format!("{}/.well-known/openid-configuration", settings.issuer);
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| oauth_unavailable())?
        .get(discovery)
        .send()
        .await
        .map_err(|_| oauth_unavailable())?;
    if !response.status().is_success() {
        return Err(oauth_unavailable());
    }
    let metadata = response.json::<OidcMetadata>().await.map_err(|_| oauth_unavailable())?;
    if metadata.issuer != settings.issuer
        || !safe_provider_url(&metadata.authorization_endpoint)
        || !safe_provider_url(&metadata.token_endpoint)
        || !safe_provider_url(&metadata.jwks_uri)
    {
        return Err(oidc_error(
            StatusCode::BAD_GATEWAY,
            "OIDC provider metadata did not match the configured issuer",
        ));
    }
    Ok(metadata)
}

async fn validate_id_token(
    http: &reqwest::Client, metadata: &OidcMetadata, settings: &OidcSettings, token: &str,
    nonce: &str,
) -> Result<(), Response> {
    let response = http.get(&metadata.jwks_uri).send().await.map_err(|_| oauth_unavailable())?;
    if !response.status().is_success() {
        return Err(oauth_unavailable());
    }
    let jwks: jsonwebtoken::jwk::JwkSet = response
        .json()
        .await
        .map_err(|_| oidc_error(StatusCode::BAD_GATEWAY, "OIDC key set was invalid"))?;
    let header = jsonwebtoken::decode_header(token)
        .map_err(|_| oidc_error(StatusCode::UNAUTHORIZED, "OIDC ID token was invalid"))?;
    let key = jwks.find(header.kid.as_deref().unwrap_or_default()).ok_or_else(|| {
        oidc_error(StatusCode::UNAUTHORIZED, "OIDC ID token signing key was not found")
    })?;
    let algorithm = header.alg;
    let mut validation = Validation::new(algorithm);
    validation.set_issuer(&[&metadata.issuer]);
    validation.set_audience(&[&settings.client_id]);
    let claims = decode::<IdClaims>(
        token,
        &DecodingKey::from_jwk(key)
            .map_err(|_| oidc_error(StatusCode::UNAUTHORIZED, "OIDC signing key was invalid"))?,
        &validation,
    )
    .map_err(|_| oidc_error(StatusCode::UNAUTHORIZED, "OIDC ID token validation failed"))?
    .claims;
    if claims.iss != metadata.issuer
        || claims.nonce != nonce
        || claims.sub.is_empty()
        || claims.exp as u64 <= unix_time()
    {
        return Err(oidc_error(StatusCode::UNAUTHORIZED, "OIDC ID token claims were invalid"));
    }
    Ok(())
}

fn safe_provider_url(value: &str) -> bool {
    url::Url::parse(value).ok().is_some_and(|url| {
        url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && (url.scheme() == "https" && url.host().is_some()
                || url.scheme() == "http"
                    && url.host_str().is_some_and(|host| {
                        host.eq_ignore_ascii_case("localhost")
                            || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
                    }))
    })
}

fn random_url_token() -> String {
    let bytes: [u8; 32] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes)
}
fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}
fn secure_cookie(uri: &str) -> &'static str {
    if uri.starts_with("https://") { "; Secure" } else { "" }
}

fn sign_flow(secret: &str, flow: &OidcFlow) -> Result<String, Response> {
    let payload = serde_json::to_vec(flow).map_err(|_| oauth_unavailable())?;
    let encoded = URL_SAFE_NO_PAD.encode(payload);
    let mut mac =
        Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| oauth_unavailable())?;
    mac.update(encoded.as_bytes());
    Ok(format!("{encoded}.{}", URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())))
}

fn verify_flow(secret: &str, value: &str) -> Result<OidcFlow, Response> {
    let (payload, signature) = value
        .split_once('.')
        .ok_or_else(|| oidc_error(StatusCode::BAD_REQUEST, "OIDC login state was invalid"))?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| oidc_error(StatusCode::BAD_REQUEST, "OIDC login state was invalid"))?;
    let mut mac =
        Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| oauth_unavailable())?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| oidc_error(StatusCode::BAD_REQUEST, "OIDC login state was invalid"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| oidc_error(StatusCode::BAD_REQUEST, "OIDC login state was invalid"))?;
    serde_json::from_slice(&payload)
        .map_err(|_| oidc_error(StatusCode::BAD_REQUEST, "OIDC login state was invalid"))
}

fn set_cookie(response: &mut Response, cookie: &str) {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(axum::http::header::SET_COOKIE, value);
    }
}
fn oidc_error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({"error": message}))).into_response()
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
    if path == "/api/health"
        || path == "/api/auth/config"
        || !(path.starts_with("/api/") || path.starts_with("/entities.v1."))
    {
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

    let cookie_token = request
        .headers()
        .get(axum::http::header::COOKIE)
        .and_then(|header| header.to_str().ok())
        .and_then(|value| {
            value.split(';').find_map(|part| part.trim().strip_prefix("ferrisflow_oidc="))
        })
        .filter(|token| !token.is_empty());
    let token = auth_header
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
        .or(cookie_token);
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

    #[test]
    fn oidc_flow_cookie_is_signed_and_provider_urls_are_restricted() {
        let flow = OidcFlow {
            state: "state".to_owned(),
            nonce: "nonce".to_owned(),
            verifier: "verifier".to_owned(),
            expires_at: unix_time() + 300,
        };
        let cookie = sign_flow("client-secret", &flow).expect("sign state cookie");
        assert_eq!(verify_flow("client-secret", &cookie).unwrap().state, flow.state);
        assert!(verify_flow("wrong-secret", &cookie).is_err());
        assert!(safe_provider_url("https://id.example.com/realms/team"));
        assert!(safe_provider_url("http://localhost:3333/realms/team"));
        assert!(!safe_provider_url("http://id.example.com/realms/team"));
        assert!(!safe_provider_url("https://user:pass@id.example.com/realms/team"));
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
