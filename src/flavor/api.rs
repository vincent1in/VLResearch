//! VLResearch's dashboard endpoints, merged into `orx up`'s router. They sit
//! behind the same loopback guard and remote-auth layers as upstream's routes.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::store::Store;

type ApiResult = std::result::Result<Json<Value>, (StatusCode, String)>;

pub(crate) fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route(
        "/api/vlresearch/sessions/{id}/progress",
        get(session_progress),
    )
}

/// What the chat's progress strip shows beyond the transcript: when the agent
/// last did anything, and the runs this chat will be woken for.
async fn session_progress(Path(session_id): Path<String>) -> ApiResult {
    let last_activity = super::activity::last_activity_ms(&session_id);
    let waiting = tokio::task::spawn_blocking(move || {
        let store = Store::open()?;
        super::wake::waiting_runs(&store, &session_id)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    Ok(Json(json!({
        "lastActivityAt": last_activity,
        "waiting": waiting,
    })))
}

fn internal(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
