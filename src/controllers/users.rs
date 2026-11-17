use crate::{app_state::AppState, models::user};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/users", get(list).post(create))
        .route("/api/users/{id}", get(read).put(update).delete(remove))
}

#[derive(Debug, Deserialize)]
pub struct UserInput {
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    pub display_name: String,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: i32,
    pub username: String,
    pub email: Option<String>,
    pub display_name: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

impl From<user::Model> for UserResponse {
    fn from(model: user::Model) -> Self {
        Self {
            id: model.id,
            username: model.username,
            email: model.email,
            display_name: model.display_name,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<UserResponse>>, ApiError> {
    let users = user::Entity::find().all(&state.db).await.map_err(ApiError::database)?;
    Ok(Json(users.into_iter().map(Into::into).collect()))
}

async fn read(
    State(state): State<AppState>, Path(id): Path<i32>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiError::database)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(user.into()))
}

async fn create(
    State(state): State<AppState>, Json(input): Json<UserInput>,
) -> Result<impl IntoResponse, ApiError> {
    let (username, email, display_name) = validate(input)?;
    let now = chrono::Utc::now().naive_utc();
    let active = user::ActiveModel {
        username: Set(username),
        email: Set(email),
        display_name: Set(display_name),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let created = active.insert(&state.db).await.map_err(ApiError::database)?;
    Ok((StatusCode::CREATED, Json(UserResponse::from(created))))
}

async fn update(
    State(state): State<AppState>, Path(id): Path<i32>, Json(input): Json<UserInput>,
) -> Result<Json<UserResponse>, ApiError> {
    let (username, email, display_name) = validate(input)?;
    let user = user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiError::database)?
        .ok_or_else(ApiError::not_found)?;
    let mut active = user.into_active_model();
    active.username = Set(username);
    active.email = Set(email);
    active.display_name = Set(display_name);
    active.updated_at = Set(chrono::Utc::now().naive_utc());
    let updated = active.update(&state.db).await.map_err(ApiError::database)?;
    Ok(Json(updated.into()))
}

async fn remove(
    State(state): State<AppState>, Path(id): Path<i32>,
) -> Result<StatusCode, ApiError> {
    let user = user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiError::database)?
        .ok_or_else(ApiError::not_found)?;
    user.into_active_model().delete(&state.db).await.map_err(ApiError::database)?;
    Ok(StatusCode::NO_CONTENT)
}

fn validate(input: UserInput) -> Result<(String, Option<String>, String), ApiError> {
    let username = input.username.trim().to_owned();
    let display_name = input.display_name.trim().to_owned();
    let email = input.email.map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
    if username.is_empty() || username.len() > 128 {
        return Err(ApiError::bad_request("username must contain 1 to 128 characters"));
    }
    if display_name.is_empty() || display_name.len() > 256 {
        return Err(ApiError::bad_request("display_name must contain 1 to 256 characters"));
    }
    if email.as_ref().is_some_and(|value| value.len() > 320) {
        return Err(ApiError::bad_request("email must be at most 320 characters"));
    }
    Ok((username, email, display_name))
}

struct ApiError(StatusCode, String);

impl ApiError {
    fn bad_request(message: &str) -> Self {
        Self(StatusCode::BAD_REQUEST, message.to_owned())
    }

    fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "user not found".to_owned())
    }

    fn database(error: impl std::fmt::Display) -> Self {
        let detail = error.to_string();
        let normalized = detail.to_ascii_lowercase();
        if normalized.contains("unique") || normalized.contains("duplicate") {
            return Self(StatusCode::CONFLICT, "username already exists".to_owned());
        }
        tracing::error!(error = %detail, "user API database operation failed");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "user operation failed".to_owned())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}
