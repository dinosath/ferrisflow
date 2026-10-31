mod health;
pub mod ai;
pub mod workflow;

use crate::app_state::AppState;
use axum::Router;

/// Returns a Router with all entity controllers and health endpoint merged.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/health", axum::routing::get(health::health))
        .route("/api/ai/workflow-chat", axum::routing::post(ai::workflow_chat))
        .route(
            "/api/ai/providers",
            axum::routing::get(ai::list_providers).post(ai::create_provider),
        )
        .route(
            "/api/ai/providers/{id}",
            axum::routing::put(ai::update_provider).delete(ai::delete_provider),
        )
        .merge(workflow::routes())
}
