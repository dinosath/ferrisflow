//! OAuth 2.0 bearer-token authentication and optional OIDC browser sign-in.
//!
//! The resource server validates access tokens through an OAuth 2.0 token
//! introspection endpoint (RFC 7662). Configuration is read from the process
//! environment so local and desktop builds remain usable without an identity
//! provider.

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderValue, Request, StatusCode, header},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

type OidcAccount = webgates::accounts::Account<webgates::roles::Role, webgates::groups::Group>;
type OidcClaims = webgates::codecs::jwt::JwtClaims<OidcAccount>;
type OidcCodec = webgates::codecs::jwt::JsonWebToken<OidcClaims>;

const OIDC_SESSION_COOKIE: &str = "ferrisflow_oidc_session";

#[derive(Clone)]
pub struct AuthMiddlewareState {
    pub config: AuthConfig,
    pub db: sea_orm::DatabaseConnection,
    pub oidc_codec: Option<Arc<OidcCodec>>,
    pub oidc_issuer: Option<String>,
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
        .route("/auth/oidc/logout", get(oidc_logout))
}

async fn current_principal(
    principal: Option<axum::Extension<AuthPrincipal>>,
) -> Result<Json<AuthPrincipal>, StatusCode> {
    principal.map(|axum::Extension(principal)| Json(principal)).ok_or(StatusCode::UNAUTHORIZED)
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

pub(crate) struct OidcRouter {
    pub router: Router<()>,
    pub codec: Arc<OidcCodec>,
    pub issuer: String,
}

#[derive(Default)]
struct OidcFlowContext {
    nonce_by_verifier: Mutex<HashMap<String, String>>,
    id_token_by_access_token: Mutex<HashMap<String, (String, String)>>,
}

#[derive(Clone)]
struct OidcCookieMiddlewareState {
    flow: Arc<OidcFlowContext>,
    secure: bool,
}

struct OidcTokenExchanger {
    http: reqwest::Client,
    flow: Arc<OidcFlowContext>,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    iss: String,
    sub: String,
    aud: serde_json::Value,
    nonce: String,
    azp: Option<String>,
    #[allow(dead_code)] // The JWT validator checks expiration from the signed payload.
    exp: usize,
    iat: usize,
}

impl webgates::gate::oauth2::TokenExchanger for OidcTokenExchanger {
    fn exchange_code(
        &self, request: webgates::gate::oauth2::TokenRequest,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        oauth2::StandardTokenResponse<
                            oauth2::EmptyExtraTokenFields,
                            oauth2::basic::BasicTokenType,
                        >,
                        webgates::gate::oauth2::errors::OAuth2Error,
                    >,
                > + Send,
        >,
    > {
        let http = self.http.clone();
        let flow = Arc::clone(&self.flow);
        Box::pin(async move {
            use oauth2::{
                AuthUrl, AuthorizationCode, ClientId, ClientSecret, PkceCodeVerifier, RedirectUrl,
                TokenUrl, basic::BasicClient,
            };
            let nonce = flow
                .nonce_by_verifier
                .lock()
                .ok()
                .and_then(|mut nonces| nonces.remove(&request.pkce_verifier))
                .ok_or_else(|| {
                    webgates::gate::oauth2::errors::OAuth2Error::token_exchange(
                        "OIDC login nonce was missing",
                    )
                })?;
            let token_url = TokenUrl::new(request.token_url.clone()).map_err(|error| {
                webgates::gate::oauth2::errors::OAuth2Error::invalid_url(
                    "token_url",
                    error.to_string(),
                )
            })?;
            let auth_url = AuthUrl::new(request.token_url.clone()).map_err(|error| {
                webgates::gate::oauth2::errors::OAuth2Error::invalid_url(
                    "auth_url",
                    error.to_string(),
                )
            })?;
            let mut client = BasicClient::new(ClientId::new(request.client_id.clone()))
                .set_auth_uri(auth_url)
                .set_token_uri(token_url);
            if let Some(secret) = request.client_secret.as_ref() {
                client = client.set_client_secret(ClientSecret::new(secret.clone()));
            }
            let redirect_url = RedirectUrl::new(request.redirect_url.clone()).map_err(|error| {
                webgates::gate::oauth2::errors::OAuth2Error::invalid_url(
                    "redirect_url",
                    error.to_string(),
                )
            })?;
            client = client.set_redirect_uri(redirect_url);
            let code = AuthorizationCode::new(request.code);
            let verifier = PkceCodeVerifier::new(request.pkce_verifier);
            client
                .exchange_code(code)
                .set_pkce_verifier(verifier)
                .request_async(&|request: oauth2::HttpRequest| {
                    let http = http.clone();
                    let flow = Arc::clone(&flow);
                    let nonce = nonce.clone();
                    async move {
                        let response = http
                            .request(request.method().clone(), request.uri().to_string())
                            .headers(request.headers().clone())
                            .body(request.body().clone())
                            .send()
                            .await?;
                        let status = response.status();
                        let headers = response.headers().clone();
                        let body = response.bytes().await?.to_vec();
                        if status.is_success()
                            && let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body)
                            && let (Some(access_token), Some(id_token)) = (
                                payload.get("access_token").and_then(serde_json::Value::as_str),
                                payload.get("id_token").and_then(serde_json::Value::as_str),
                            )
                            && let Ok(mut tokens) = flow.id_token_by_access_token.lock()
                        {
                            tokens.insert(access_token.to_owned(), (id_token.to_owned(), nonce));
                        }
                        let mut token_response = oauth2::HttpResponse::new(body);
                        *token_response.status_mut() = status;
                        *token_response.headers_mut() = headers;
                        Ok::<oauth2::HttpResponse, reqwest::Error>(token_response)
                    }
                })
                .await
                .map_err(|error| {
                    webgates::gate::oauth2::errors::OAuth2Error::token_exchange(error.to_string())
                })
        })
    }
}

async fn oidc_cookie_middleware(
    State(state): State<OidcCookieMiddlewareState>, request: Request<axum::body::Body>, next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    if path.ends_with("/login") {
        let nonce = random_url_token();
        let mut response = next.run(request).await;
        if let Some(location) =
            response.headers().get(header::LOCATION).and_then(|value| value.to_str().ok())
        {
            if let Ok(mut authorization_url) = url::Url::parse(location) {
                authorization_url.query_pairs_mut().append_pair("nonce", &nonce);
                if let Ok(location) = HeaderValue::from_str(authorization_url.as_str()) {
                    response.headers_mut().insert(header::LOCATION, location);
                    append_cookie(
                        &mut response,
                        &format!(
                            "ferrisflow_oidc_nonce={nonce}; Path=/; HttpOnly; SameSite=Lax; Max-Age=300{}",
                            if state.secure { "; Secure" } else { "" }
                        ),
                    );
                }
            }
        }
        return response;
    }
    if path.ends_with("/callback") {
        let verifier = request_cookie(&request, "ferrisflow_oidc_pkce");
        let nonce = request_cookie(&request, "ferrisflow_oidc_nonce");
        if let (Some(verifier), Some(nonce)) = (&verifier, &nonce)
            && let Ok(mut nonces) = state.flow.nonce_by_verifier.lock()
        {
            nonces.insert(verifier.clone(), nonce.clone());
        }
        let mut response = next.run(request).await;
        append_cookie(
            &mut response,
            &format!(
                "ferrisflow_oidc_nonce=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
                if state.secure { "; Secure" } else { "" }
            ),
        );
        if let Some(verifier) = verifier
            && let Ok(mut nonces) = state.flow.nonce_by_verifier.lock()
        {
            nonces.remove(&verifier);
        }
        return response;
    }
    next.run(request).await
}

async fn validate_oidc_id_token(
    http: &reqwest::Client, jwks_url: &str, issuer: &str, client_id: &str, expected_nonce: &str,
    id_token: &str,
) -> Result<IdTokenClaims, String> {
    let response = http
        .get(jwks_url)
        .send()
        .await
        .map_err(|error| format!("could not fetch OIDC signing keys: {error}"))?;
    if !response.status().is_success() {
        return Err("OIDC signing-key endpoint returned an error".to_owned());
    }
    let jwks: jsonwebtoken::jwk::JwkSet = response
        .json()
        .await
        .map_err(|error| format!("OIDC signing-key response was invalid: {error}"))?;
    let header = jsonwebtoken::decode_header(id_token)
        .map_err(|error| format!("OIDC ID-token header was invalid: {error}"))?;
    let kid = header.kid.as_deref().ok_or("OIDC ID token has no key identifier")?;
    let key = jwks.find(kid).ok_or("OIDC ID-token signing key was not found")?;
    if key
        .common
        .public_key_use
        .as_ref()
        .is_some_and(|key_use| key_use != &jsonwebtoken::jwk::PublicKeyUse::Signature)
        || key.common.key_operations.as_ref().is_some_and(|operations| {
            !operations.contains(&jsonwebtoken::jwk::KeyOperations::Verify)
        })
        || key.common.key_algorithm.is_some_and(|algorithm| algorithm != header.alg.into())
    {
        return Err("OIDC ID-token JWKS key is not valid for this signature".to_owned());
    }
    let allowed_algorithms = match header.alg {
        jsonwebtoken::Algorithm::RS256
        | jsonwebtoken::Algorithm::RS384
        | jsonwebtoken::Algorithm::RS512 => &[
            jsonwebtoken::Algorithm::RS256,
            jsonwebtoken::Algorithm::RS384,
            jsonwebtoken::Algorithm::RS512,
        ][..],
        jsonwebtoken::Algorithm::ES256 | jsonwebtoken::Algorithm::ES384 => {
            &[jsonwebtoken::Algorithm::ES256, jsonwebtoken::Algorithm::ES384][..]
        }
        _ => return Err("OIDC ID-token signing algorithm is not supported".to_owned()),
    };
    let mut validation = Validation::new(header.alg);
    validation.algorithms = allowed_algorithms.to_vec();
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "iat"]);
    let claims = decode::<IdTokenClaims>(
        id_token,
        &DecodingKey::from_jwk(key)
            .map_err(|error| format!("OIDC signing key was invalid: {error}"))?,
        &validation,
    )
    .map_err(|error| format!("OIDC ID-token validation failed: {error}"))?
    .claims;
    if claims.iss != issuer || claims.nonce != expected_nonce || claims.sub.is_empty() {
        return Err("OIDC ID-token issuer, nonce, or subject did not match".to_owned());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?
        .as_secs();
    if claims.iat as u64 > now.saturating_add(300) {
        return Err("OIDC ID-token issue time is in the future".to_owned());
    }
    let audiences = match &claims.aud {
        serde_json::Value::String(audience) => vec![audience.as_str()],
        serde_json::Value::Array(audiences) => audiences
            .iter()
            .map(serde_json::Value::as_str)
            .collect::<Option<Vec<_>>>()
            .ok_or("OIDC ID-token audience was invalid")?,
        _ => return Err("OIDC ID-token audience was invalid".to_owned()),
    };
    if !audiences.contains(&client_id)
        || audiences.iter().any(|audience| *audience != client_id)
            && claims.azp.as_deref() != Some(client_id)
    {
        return Err("OIDC ID-token audience was not valid for this client".to_owned());
    }
    if claims.azp.as_deref().is_some_and(|authorized_party| authorized_party != client_id) {
        return Err("OIDC ID-token authorized party did not match this client".to_owned());
    }
    Ok(claims)
}

fn random_url_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

fn request_cookie(request: &Request<axum::body::Body>, name: &str) -> Option<String> {
    request
        .headers()
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| part.trim().strip_prefix(name)?.strip_prefix('=').map(str::to_owned))
}

fn append_cookie(response: &mut Response, cookie: &str) {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

pub(crate) fn oidc_router() -> Result<Option<OidcRouter>, String> {
    if !oidc_enabled() {
        return Ok(None);
    }
    let value = |name: &str| std::env::var(name).ok().filter(|value| !value.trim().is_empty());
    let issuer = value("OIDC_ISSUER").ok_or("OIDC_ENABLED requires OIDC_ISSUER")?;
    let client_id = value("OIDC_CLIENT_ID").ok_or("OIDC_ENABLED requires OIDC_CLIENT_ID")?;
    let client_secret =
        value("OIDC_CLIENT_SECRET").ok_or("OIDC_ENABLED requires OIDC_CLIENT_SECRET")?;
    let redirect_url = value("OIDC_REDIRECT_URI")
        .unwrap_or_else(|| "http://localhost:8080/auth/oidc/callback".to_owned());
    let auth_url = value("OIDC_AUTH_URL").unwrap_or_else(|| {
        format!("{}/protocol/openid-connect/auth", issuer.trim_end_matches('/'))
    });
    let token_url = value("OIDC_TOKEN_URL").unwrap_or_else(|| {
        format!("{}/protocol/openid-connect/token", issuer.trim_end_matches('/'))
    });
    let jwks_url = value("OIDC_JWKS_URL").unwrap_or_else(|| {
        format!("{}/protocol/openid-connect/jwks.json", issuer.trim_end_matches('/'))
    });
    let introspection_url =
        value("OAUTH_INTROSPECTION_URL").ok_or("OIDC_ENABLED requires OAUTH_INTROSPECTION_URL")?;
    let introspection_id = value("OAUTH_CLIENT_ID").unwrap_or_else(|| client_id.clone());
    let introspection_secret =
        value("OAUTH_CLIENT_SECRET").unwrap_or_else(|| client_secret.clone());
    for (name, url) in [
        ("OIDC_ISSUER", issuer.as_str()),
        ("OIDC_AUTH_URL", auth_url.as_str()),
        ("OIDC_TOKEN_URL", token_url.as_str()),
        ("OIDC_JWKS_URL", jwks_url.as_str()),
        ("OIDC_REDIRECT_URI", redirect_url.as_str()),
        ("OAUTH_INTROSPECTION_URL", introspection_url.as_str()),
    ] {
        if !safe_provider_url(url) {
            return Err(format!("{name} must use HTTPS (HTTP is allowed on loopback)"));
        }
    }
    let read_scope = value("OAUTH_READ_SCOPE").unwrap_or_else(|| "ferrisflow:read".to_owned());
    let write_scope = value("OAUTH_WRITE_SCOPE").unwrap_or_else(|| "ferrisflow:write".to_owned());
    let admin_scope = value("OAUTH_ADMIN_SCOPE").unwrap_or_else(|| "ferrisflow:admin".to_owned());
    let ttl_secs =
        value("OIDC_SESSION_TTL_SECS").and_then(|value| value.parse::<u64>().ok()).unwrap_or(3600);
    let key_bytes = |pem_name: &str, path_name: &str| -> Result<Option<Vec<u8>>, String> {
        if let Some(pem) = value(pem_name) {
            return Ok(Some(pem.into_bytes()));
        }
        if let Some(path) = value(path_name) {
            return std::fs::read(path)
                .map(Some)
                .map_err(|error| format!("could not read {path_name}: {error}"));
        }
        Ok(None)
    };
    let private_key =
        key_bytes("OIDC_SESSION_JWT_PRIVATE_KEY_PEM", "OIDC_SESSION_JWT_PRIVATE_KEY_PATH")?;
    let public_key =
        key_bytes("OIDC_SESSION_JWT_PUBLIC_KEY_PEM", "OIDC_SESSION_JWT_PUBLIC_KEY_PATH")?;
    let codec_options = match (private_key, public_key) {
        (Some(private_key), Some(public_key)) => {
            webgates::codecs::jwt::JsonWebTokenOptions::from_es384_pem(&private_key, &public_key)
                .map_err(|error| format!("invalid OIDC session JWT key pair: {error}"))?
        }
        (None, None) => {
            return Err("OIDC_ENABLED requires OIDC_SESSION_JWT_PRIVATE_KEY_PATH and OIDC_SESSION_JWT_PUBLIC_KEY_PATH (or matching PEM variables)".to_owned());
        }
        _ => return Err("configure both OIDC session JWT private and public keys".to_owned()),
    };
    let codec = Arc::new(OidcCodec::new_with_options(codec_options));
    let secure_cookies = redirect_url.starts_with("https://");
    let cookie_template = |name: &'static str, ttl: u64| {
        let template = webgates::cookie_template::CookieTemplate::recommended()
            .name(name)
            .same_site(webgates::cookie::SameSite::Lax)
            .persistent(webgates::cookie::time::Duration::seconds(ttl.min(i64::MAX as u64) as i64));
        if secure_cookies { template } else { template.insecure_dev_only() }
    };
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("could not create OAuth introspection client: {error}"))?;
    let flow = Arc::new(OidcFlowContext::default());
    let id_token_http = http.clone();
    let id_token_flow = Arc::clone(&flow);
    let validator_issuer = issuer.clone();
    let validator_client_id = client_id.clone();
    let validator_jwks_url = jwks_url.clone();
    let mapper_read_scope = read_scope.clone();
    let mapper_write_scope = write_scope.clone();
    let mapper_admin_scope = admin_scope.clone();
    let gate = webgates_axum::gate::oauth2::OAuth2Gate::<
        webgates::roles::Role,
        webgates::groups::Group,
    >::new()
    .auth_url(auth_url)
    .token_url(token_url)
    .client_id(client_id)
    .client_secret(client_secret)
    .redirect_url(redirect_url)
    .add_scope("openid")
    .add_scope("profile")
    .add_scope("email")
    .with_state_cookie_template(cookie_template("ferrisflow_oidc_state", 300))
    .with_pkce_cookie_template(cookie_template("ferrisflow_oidc_pkce", 300))
    .with_cookie_template(cookie_template(OIDC_SESSION_COOKIE, ttl_secs))
    .with_post_login_redirect("/")
    .with_jwt_codec(&issuer, Arc::clone(&codec), ttl_secs)
    .with_token_exchanger(OidcTokenExchanger { http: http.clone(), flow: Arc::clone(&flow) })
    .with_account_mapper(move |token_response| {
        let http = http.clone();
        let introspection_url = introspection_url.clone();
        let introspection_id = introspection_id.clone();
        let introspection_secret = introspection_secret.clone();
        let read_scope = mapper_read_scope.clone();
        let write_scope = mapper_write_scope.clone();
        let admin_scope = mapper_admin_scope.clone();
        let id_token_http = id_token_http.clone();
        let id_token_flow = Arc::clone(&id_token_flow);
        let issuer = validator_issuer.clone();
        let client_id = validator_client_id.clone();
        let jwks_url = validator_jwks_url.clone();
        Box::pin(async move {
            use oauth2::TokenResponse;
            let access_token = token_response.access_token().secret();
            let (id_token, expected_nonce) = id_token_flow
                .id_token_by_access_token
                .lock()
                .ok()
                .and_then(|mut tokens| tokens.remove(access_token))
                .ok_or_else(|| {
                    webgates::gate::oauth2::errors::OAuth2Error::token_exchange(
                        "OIDC token response did not include a usable ID token",
                    )
                })?;
            let id_claims = validate_oidc_id_token(
                &id_token_http,
                &jwks_url,
                &issuer,
                &client_id,
                &expected_nonce,
                &id_token,
            )
            .await
            .map_err(webgates::gate::oauth2::errors::OAuth2Error::token_exchange)?;
            let response = http
                .post(introspection_url)
                .basic_auth(introspection_id, Some(introspection_secret))
                .form(&[("token", access_token)])
                .send()
                .await
                .map_err(|error| {
                    webgates::gate::oauth2::errors::OAuth2Error::token_exchange(error.to_string())
                })?;
            if !response.status().is_success() {
                return Err(webgates::gate::oauth2::errors::OAuth2Error::token_exchange(
                    "provider token introspection failed",
                ));
            }
            let introspection =
                response.json::<IntrospectionResponse>().await.map_err(|error| {
                    webgates::gate::oauth2::errors::OAuth2Error::token_exchange(error.to_string())
                })?;
            let subject = introspection.sub.filter(|_| introspection.active).ok_or_else(|| {
                webgates::gate::oauth2::errors::OAuth2Error::token_exchange(
                    "provider returned an inactive access token",
                )
            })?;
            if subject != id_claims.sub {
                return Err(webgates::gate::oauth2::errors::OAuth2Error::token_exchange(
                    "ID token subject did not match the introspected access token",
                ));
            }
            let scopes = introspection
                .scope
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut account = OidcAccount::new(&subject);
            for scope in &scopes {
                account.permissions.grant(scope.as_str());
            }
            account.roles = if scopes.iter().any(|scope| scope == &admin_scope) {
                vec![webgates::roles::Role::Admin]
            } else if scopes.iter().any(|scope| scope == &write_scope) {
                vec![webgates::roles::Role::Reporter]
            } else if scopes.iter().any(|scope| scope == &read_scope) {
                vec![webgates::roles::Role::User]
            } else {
                return Err(webgates::gate::oauth2::errors::OAuth2Error::token_exchange(
                    "provider token did not grant Ferrisflow access",
                ));
            };
            Ok(account)
        })
    });
    let cookie_middleware_state =
        OidcCookieMiddlewareState { flow: Arc::clone(&flow), secure: secure_cookies };
    let router = gate
        .into_router("/auth/oidc")
        .map_err(|error| format!("invalid WebGates OIDC configuration: {error}"))?
        .layer(from_fn_with_state(cookie_middleware_state, oidc_cookie_middleware));
    Ok(Some(OidcRouter { router, codec, issuer }))
}

async fn oidc_logout() -> Response {
    let secure =
        std::env::var("OIDC_REDIRECT_URI").ok().is_some_and(|uri| uri.starts_with("https://"));
    let mut response = Redirect::to("/").into_response();
    set_cookie(
        &mut response,
        &format!(
            "{OIDC_SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
            if secure { "; Secure" } else { "" }
        ),
    );
    response
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

fn set_cookie(response: &mut Response, cookie: &str) {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(axum::http::header::SET_COOKIE, value);
    }
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
            value
                .split(';')
                .find_map(|part| part.trim().strip_prefix(&format!("{OIDC_SESSION_COOKIE}=")))
        })
        .filter(|token| !token.is_empty());
    if let (Some(token), Some(codec), Some(issuer)) =
        (cookie_token, state.oidc_codec.as_ref(), state.oidc_issuer.as_deref())
    {
        use webgates::codecs::Codec;
        if let Ok(claims) = codec.decode(token.as_bytes())
            && claims.has_issuer(issuer)
        {
            let permissions = &claims.custom_claims.permissions;
            let mut scopes = Vec::new();
            for scope in [&config.read_scope, &config.write_scope, &config.admin_scope] {
                if permissions.has(scope.as_str()) {
                    scopes.push(scope.clone());
                }
            }
            let principal =
                AuthPrincipal { subject: claims.custom_claims.user_id, username: None, scopes };
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
            return next.run(request).await;
        }
    }
    let token = auth_header
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty());
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
    use axum::{
        body::Body,
        routing::{get, post},
    };
    use sea_orm::{ActiveModelTrait, Database, Set};
    use tower::ServiceExt;

    const USERNAME: &str = "regular-user";
    const USER_PASSWORD: &str = "regular-user-password";
    const ADMIN_PASSWORD: &str = "test-admin-password";
    // This private key is a test fixture and must never be used outside these tests.
    const TEST_ID_TOKEN_PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC6oFLwnLqrwGb5\nWtmviIRZGualzWW8ahcF0T8VBc68Eq3TUvA/rnp0cFMicjf6z6hH0IcK6iXlxRjm\nzi9du7sgGjCWAvTsN8K3pIyZHU41qCElZeZfqxzOsH+H5y5vjrACIy4mDLOhBuSW\nxKZtfk2pqZaili9eYzCNNJny8N26hKFcXTH1nzAnHDgTb9nAnEUtJ87KXzQQcOg9\nUDbPVRKvSk9oA29YT0rrOfG3ZxA/L2JXzpwmpXv6cMyJ7VxzVKfUQwvaoeZrSy9w\nldpI6+1wetXhJbAjs5K/dBHMpDq7MKjPzjoI9SWsTdMDsvFzOxGCZNfWixQdHO0a\nPBclknFfAgMBAAECggEADJUGPed1cmKchYZbyn+uJJRWJWGEYxuxF6srY3Phf0DU\n2eDrvV0V5mwPGjq01zB1uh5TrHCQjKuW/A5QBTPpXaJGY2/gc6ZCwAhCJT/11Xf3\n7mbTVpeQSzVYfOohA8HWPfoTybXJOoS6TpmWn3Ca64c5VoSJ4nMNOjWe8uNfMdXA\n9KM/Kkh5dF4U9BPBUKsO0ylugt7SRE/17ssXQXqJ/SyiMnQrDibkdhoZ9pDm60n3\nE5fQQ9bQYaD9jUW3YNDyA5oqRM3EeBS9aj7ubOfL4yLvmgCprKUm/1Q1BKuLzShE\nW10/iHQgRU/hH2soMXu6MDcVyVxUvJzMYUrTp5vQGQKBgQDgeE08hdoa0hAirwjZ\nQv2L6j03U5ZtSVpGkeYbG6zXA43OahUY1QT6pDDy+4A81oeG4AU5RDPEEZ966ND4\nmo6DgcvxTibEky38jyypHhwaVgSugQ63P3hm/xr9Q40QpAKaU5dL11ECzg8pKzym\npSe7KG8NNWWXW4my/zVGZ7U74wKBgQDU1zRlt4C69u5kUAqMuGX7IB8YXpAg/U5n\niaruVeYPZ8duz34H2JaIxLsBU9FRn1DJZER9iCdk/2iv1NoA9IZPoP7atbtHpVbH\nMGMA2v3ubgDkUYgc7aswIHlGNokvqV97ZAsxPQ7xOvSM92CJsJbP3qQ4q+ZWFtSd\nqiQRyDdlVQKBgQC9Wx6koLGlkHEGISvQw19JyVbMtlmeZZOfFnLLiWQbmablar3T\nTde+bAXS4/CA4eRAFfiyH2Of0dH8CCxIm3D/ZN4a+Hdhulq/xDPDkl/Yo5mdb8dv\nND6ofClTXsnAMOr1LvDhjIiNWjCV5SBr38SNcIWF2faSYSje88UIWHIdSwKBgGX5\n1jkF+r+ZoA6f6u4RS4tOkJd0d0nXE8G3zBX1Qg0PhBYrhA7uTvGokJDKP/gDhQl4\n6PfcTEOvu9DDAtCCV/e/A1x2xgxFw5YcMNMwGYorWnIKPlTd6q9cX76sUn5rgDv2\nawZkYsxhmtoPjElsOlXOLpCYi/ujZKtF4ZJDg4chAoGAUvuEsOA+MOqLARaCc2Oi\nZM1eB9a7CAnscjDUlMFm+UaE4cN1l0fOvH2lEHvsJur61CpxkG6Rlc9BakumbtZ7\nepxWRJmWkLvUO3GHTZoVJKNjQA+dQ9UQeS+zuZc0NJHXEKm6zrNZCtDbQrxnBLr0\nhrNaXGWT87lqFhKKm96pCVE=\n-----END PRIVATE KEY-----\n";
    const TEST_ID_TOKEN_JWKS: &str = r#"{"keys":[{"kty":"RSA","kid":"test-key","use":"sig","alg":"RS256","n":"uqBS8Jy6q8Bm-VrZr4iEWRrmpc1lvGoXBdE_FQXOvBKt01LwP656dHBTInI3-s-oR9CHCuol5cUY5s4vXbu7IBowlgL07DfCt6SMmR1ONaghJWXmX6sczrB_h-cub46wAiMuJgyzoQbklsSmbX5NqamWopYvXmMwjTSZ8vDduoShXF0x9Z8wJxw4E2_ZwJxFLSfOyl80EHDoPVA2z1USr0pPaANvWE9K6znxt2cQPy9iV86cJqV7-nDMie1cc1Sn1EML2qHma0svcJXaSOvtcHrV4SWwI7OSv3QRzKQ6uzCoz846CPUlrE3TA7LxczsRgmTX1osUHRztGjwXJZJxXw","e":"AQAB"}]}"#;

    #[derive(Serialize)]
    struct TestIdTokenClaims {
        iss: String,
        sub: String,
        aud: serde_json::Value,
        exp: usize,
        iat: usize,
        nonce: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        azp: Option<String>,
    }

    async fn test_jwks_url() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let jwks: serde_json::Value = serde_json::from_str(TEST_ID_TOKEN_JWKS).unwrap();
        let app = Router::new().route(
            "/jwks",
            get(move || {
                let jwks = jwks.clone();
                async move { Json(jwks) }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/jwks"), server)
    }

    fn signed_test_id_token(
        algorithm: jsonwebtoken::Algorithm, issuer: &str, audience: serde_json::Value, nonce: &str,
        expires_at: usize, issued_at: usize, authorized_party: Option<&str>,
    ) -> String {
        let mut header = jsonwebtoken::Header::new(algorithm);
        header.kid = Some("test-key".to_owned());
        let claims = TestIdTokenClaims {
            iss: issuer.to_owned(),
            sub: "test-subject".to_owned(),
            aud: audience,
            exp: expires_at,
            iat: issued_at,
            nonce: nonce.to_owned(),
            azp: authorized_party.map(str::to_owned),
        };
        jsonwebtoken::encode(
            &header,
            &claims,
            &jsonwebtoken::EncodingKey::from_rsa_pem(TEST_ID_TOKEN_PRIVATE_KEY.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    fn current_unix_time() -> usize {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
            as usize
    }

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
                AuthMiddlewareState {
                    config: config(required),
                    db,
                    oidc_codec: None,
                    oidc_issuer: None,
                },
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
    fn oidc_provider_urls_are_restricted_to_tls_or_loopback() {
        assert!(safe_provider_url("https://id.example.com/realms/team"));
        assert!(safe_provider_url("http://localhost:3333/realms/team"));
        assert!(!safe_provider_url("http://id.example.com/realms/team"));
        assert!(!safe_provider_url("https://user:pass@id.example.com/realms/team"));
    }

    #[tokio::test]
    async fn valid_signed_oidc_id_token_is_accepted_with_matching_issuer_audience_and_nonce() {
        let (jwks_url, server) = test_jwks_url().await;
        let now = current_unix_time();
        let token = signed_test_id_token(
            jsonwebtoken::Algorithm::RS256,
            "https://issuer.example/realm",
            serde_json::json!("ferrisflow-client"),
            "login-nonce",
            now + 300,
            now,
            None,
        );

        let claims = validate_oidc_id_token(
            &reqwest::Client::new(),
            &jwks_url,
            "https://issuer.example/realm",
            "ferrisflow-client",
            "login-nonce",
            &token,
        )
        .await
        .unwrap();

        assert_eq!(claims.sub, "test-subject");
        assert_eq!(claims.iss, "https://issuer.example/realm");
        server.abort();
    }

    #[tokio::test]
    async fn webgates_token_exchange_captures_the_id_token_and_login_nonce() {
        use oauth2::TokenResponse;
        use webgates::gate::oauth2::TokenExchanger;

        let nonce = "nonce-bound-to-this-login";
        let now = current_unix_time();
        let id_token = signed_test_id_token(
            jsonwebtoken::Algorithm::RS256,
            "https://issuer.example/realm",
            serde_json::json!("ferrisflow-client"),
            nonce,
            now + 300,
            now,
            None,
        );
        let expected_id_token = id_token.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/token",
            post(move || {
                let id_token = id_token.clone();
                async move {
                    Json(serde_json::json!({
                        "access_token": "test-access-token",
                        "token_type": "Bearer",
                        "expires_in": 300,
                        "id_token": id_token
                    }))
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let flow = Arc::new(OidcFlowContext::default());
        let verifier = "test-pkce-verifier-with-at-least-43-characters".to_owned();
        flow.nonce_by_verifier.lock().unwrap().insert(verifier.clone(), nonce.to_owned());
        let exchanger =
            OidcTokenExchanger { http: reqwest::Client::new(), flow: Arc::clone(&flow) };

        let response = exchanger
            .exchange_code(webgates::gate::oauth2::TokenRequest {
                code: "test-authorization-code".to_owned(),
                pkce_verifier: verifier,
                token_url: format!("http://{address}/token"),
                client_id: "ferrisflow-client".to_owned(),
                client_secret: Some("test-client-secret".to_owned()),
                redirect_url: "http://localhost/callback".to_owned(),
            })
            .await
            .unwrap();

        assert_eq!(response.access_token().secret(), "test-access-token");
        let (captured_id_token, captured_nonce) =
            flow.id_token_by_access_token.lock().unwrap().remove("test-access-token").unwrap();
        assert_eq!(captured_id_token, expected_id_token);
        assert_eq!(captured_nonce, nonce);
        server.abort();
    }

    #[tokio::test]
    async fn oidc_middleware_sends_nonce_and_binds_it_to_the_callback_pkce_verifier() {
        let flow = Arc::new(OidcFlowContext::default());
        let callback_flow = Arc::clone(&flow);
        let observed_callback_nonce = Arc::new(Mutex::new(None));
        let nonce_observer = Arc::clone(&observed_callback_nonce);
        let app = Router::new()
            .route(
                "/auth/oidc/login",
                get(|| async { Redirect::to("https://issuer.example/authorize").into_response() }),
            )
            .route(
                "/auth/oidc/callback",
                get(move || {
                    let flow = Arc::clone(&callback_flow);
                    let nonce_observer = Arc::clone(&nonce_observer);
                    async move {
                        let nonce =
                            flow.nonce_by_verifier.lock().unwrap().get("test-verifier").cloned();
                        assert!(nonce.is_some());
                        *nonce_observer.lock().unwrap() = nonce;
                        StatusCode::OK
                    }
                }),
            )
            .layer(from_fn_with_state(
                OidcCookieMiddlewareState { flow: Arc::clone(&flow), secure: true },
                oidc_cookie_middleware,
            ));

        let login_response = app
            .clone()
            .oneshot(Request::builder().uri("/auth/oidc/login").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(login_response.status(), StatusCode::SEE_OTHER);
        let authorization_url =
            url::Url::parse(login_response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let request_nonce = authorization_url
            .query_pairs()
            .find_map(|(key, value)| (key == "nonce").then(|| value.into_owned()))
            .unwrap();
        let nonce_cookie = login_response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|value| {
                value
                    .to_str()
                    .ok()?
                    .strip_prefix("ferrisflow_oidc_nonce=")
                    .and_then(|cookie| cookie.split(';').next())
                    .map(str::to_owned)
            })
            .unwrap();
        assert_eq!(request_nonce, nonce_cookie);
        assert!(login_response.headers().get_all(header::SET_COOKIE).iter().any(|value| {
            value.to_str().unwrap().contains("HttpOnly; SameSite=Lax; Max-Age=300; Secure")
        }));

        let callback_response = app
            .oneshot(
                Request::builder()
                    .uri("/auth/oidc/callback")
                    .header(
                        header::COOKIE,
                        format!(
                            "ferrisflow_oidc_pkce=test-verifier; ferrisflow_oidc_nonce={nonce_cookie}"
                        ),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(callback_response.status(), StatusCode::OK);
        assert!(
            callback_response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .any(|value| value.to_str().unwrap().contains("ferrisflow_oidc_nonce=;"))
        );
        assert_eq!(observed_callback_nonce.lock().unwrap().as_deref(), Some(nonce_cookie.as_str()));
        assert!(flow.nonce_by_verifier.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn oidc_id_token_requires_valid_issuer_audience_nonce_and_times() {
        let (jwks_url, server) = test_jwks_url().await;
        let now = current_unix_time();
        let invalid_claims = [
            (
                "https://wrong-issuer.example",
                serde_json::json!("ferrisflow-client"),
                "login-nonce",
                now + 300,
                now,
            ),
            (
                "https://issuer.example/realm",
                serde_json::json!("another-client"),
                "login-nonce",
                now + 300,
                now,
            ),
            (
                "https://issuer.example/realm",
                serde_json::json!("ferrisflow-client"),
                "wrong-nonce",
                now + 300,
                now,
            ),
            (
                "https://issuer.example/realm",
                serde_json::json!("ferrisflow-client"),
                "login-nonce",
                now - 120,
                now,
            ),
            (
                "https://issuer.example/realm",
                serde_json::json!("ferrisflow-client"),
                "login-nonce",
                now + 300,
                now + 600,
            ),
        ];

        for (issuer, audience, nonce, expires_at, issued_at) in invalid_claims {
            let token = signed_test_id_token(
                jsonwebtoken::Algorithm::RS256,
                issuer,
                audience,
                nonce,
                expires_at,
                issued_at,
                None,
            );
            assert!(
                validate_oidc_id_token(
                    &reqwest::Client::new(),
                    &jwks_url,
                    "https://issuer.example/realm",
                    "ferrisflow-client",
                    "login-nonce",
                    &token,
                )
                .await
                .is_err(),
                "invalid ID-token claims should be rejected"
            );
        }

        server.abort();
    }

    #[tokio::test]
    async fn oidc_id_token_rejects_invalid_signatures_and_requires_azp_for_multiple_audiences() {
        let (jwks_url, server) = test_jwks_url().await;
        let now = current_unix_time();
        let valid_multi_audience = signed_test_id_token(
            jsonwebtoken::Algorithm::RS256,
            "https://issuer.example/realm",
            serde_json::json!(["ferrisflow-client", "another-client"]),
            "login-nonce",
            now + 300,
            now,
            Some("ferrisflow-client"),
        );
        assert!(
            validate_oidc_id_token(
                &reqwest::Client::new(),
                &jwks_url,
                "https://issuer.example/realm",
                "ferrisflow-client",
                "login-nonce",
                &valid_multi_audience,
            )
            .await
            .is_ok()
        );

        let missing_azp = signed_test_id_token(
            jsonwebtoken::Algorithm::RS256,
            "https://issuer.example/realm",
            serde_json::json!(["ferrisflow-client", "another-client"]),
            "login-nonce",
            now + 300,
            now,
            None,
        );
        assert!(
            validate_oidc_id_token(
                &reqwest::Client::new(),
                &jwks_url,
                "https://issuer.example/realm",
                "ferrisflow-client",
                "login-nonce",
                &missing_azp,
            )
            .await
            .is_err()
        );

        let mut invalid_signature = valid_multi_audience;
        let last = invalid_signature.pop().unwrap();
        invalid_signature.push(if last == 'A' { 'B' } else { 'A' });
        assert!(
            validate_oidc_id_token(
                &reqwest::Client::new(),
                &jwks_url,
                "https://issuer.example/realm",
                "ferrisflow-client",
                "login-nonce",
                &invalid_signature,
            )
            .await
            .is_err()
        );

        server.abort();
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
