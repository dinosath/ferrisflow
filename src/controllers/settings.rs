use axum::{Json, extract::State, http::StatusCode};
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{app_state::AppState, models::app_setting};

const SYNC_SETTINGS_KEY: &str = "sync";
const HTTP_SERVER_SETTINGS_KEY: &str = "http_server";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SyncSettings {
    pub enabled: bool,
    pub endpoint: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HttpServerSettings {
    pub enabled: bool,
}

pub async fn http_server_enabled(state: &AppState) -> bool {
    app_setting::Entity::find_by_id(HTTP_SERVER_SETTINGS_KEY)
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .and_then(|record| serde_json::from_str::<HttpServerSettings>(&record.value).ok())
        .is_some_and(|settings| settings.enabled)
}

pub async fn read_http_server_settings(State(state): State<AppState>) -> Json<HttpServerSettings> {
    Json(HttpServerSettings { enabled: http_server_enabled(&state).await })
}

pub async fn save_http_server_settings(
    State(state): State<AppState>, Json(input): Json<HttpServerSettings>,
) -> Result<Json<HttpServerSettings>, (StatusCode, Json<Value>)> {
    save_setting(&state, HTTP_SERVER_SETTINGS_KEY, &input).await?;
    crate::set_http_server_enabled(input.enabled);
    Ok(Json(input))
}

pub async fn read_sync_settings(State(state): State<AppState>) -> Json<SyncSettings> {
    let settings = app_setting::Entity::find_by_id(SYNC_SETTINGS_KEY)
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .and_then(|record| serde_json::from_str::<SyncSettings>(&record.value).ok())
        .unwrap_or_default();
    Json(settings)
}

pub async fn save_sync_settings(
    State(state): State<AppState>, Json(mut input): Json<SyncSettings>,
) -> Result<Json<SyncSettings>, (StatusCode, Json<Value>)> {
    input.endpoint = input.endpoint.trim().trim_end_matches('/').to_owned();
    if input.enabled {
        let endpoint = url::Url::parse(&input.endpoint).map_err(|_| {
            bad_request("Enter a valid sync endpoint URL, such as https://ferrisflow.example.com")
        })?;
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host().is_none() {
            return Err(bad_request("The sync endpoint must use HTTP or HTTPS"));
        }
        let is_local = endpoint.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host.parse::<std::net::IpAddr>().is_ok_and(|address| address.is_loopback())
        });
        if endpoint.scheme() != "https" && !is_local {
            return Err(bad_request("Use HTTPS for sync endpoints outside this device"));
        }
    }

    save_setting(&state, SYNC_SETTINGS_KEY, &input).await?;

    Ok(Json(input))
}

async fn save_setting<T: Serialize>(
    state: &AppState, key: &str, input: &T,
) -> Result<(), (StatusCode, Json<Value>)> {
    let value = serde_json::to_string(input).map_err(internal_error)?;
    match app_setting::Entity::find_by_id(key).one(&state.db).await.map_err(internal_error)? {
        Some(record) => {
            let mut active = record.into_active_model();
            active.value = Set(value);
            active.update(&state.db).await.map_err(internal_error)?;
        }
        None => {
            app_setting::ActiveModel { key: Set(key.to_owned()), value: Set(value) }
                .insert(&state.db)
                .await
                .map_err(internal_error)?;
        }
    }

    Ok(())
}

fn bad_request(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"error": message})))
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": error.to_string()})))
}
