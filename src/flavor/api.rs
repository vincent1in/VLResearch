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
    Router::new()
        .route(
            "/api/vlresearch/sessions/{id}/progress",
            get(session_progress),
        )
        .route(
            "/api/vlresearch/projects/{id}/metrics",
            get(project_metrics),
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

/// The project's optimization surface: its primary metric, every run scored
/// on it, the running best against the baseline, and the lineage path from
/// the baseline to the best experiment.
async fn project_metrics(Path(project_id): Path<String>) -> ApiResult {
    let body = tokio::task::spawn_blocking(move || -> crate::error::Result<Option<Value>> {
        let store = Store::open()?;
        let Some(project) = store.get_local_project(&project_id)? else {
            return Ok(None);
        };
        let primary = super::metrics::primary(&store, &project.id)?;
        let body = match &primary {
            Some(metric) => {
                let (standing, path, runs) = super::metrics::standing(&store, &project.id, metric)?;
                json!({
                    "primary": metric,
                    "keys": super::metrics::project_keys(&store, &project.id)?,
                    "best": standing.best,
                    "baseline": standing.baseline,
                    "history": standing.history,
                    "path": path,
                    "runs": runs,
                })
            }
            None => {
                for run in store.list_runs_by_project(&project.id)? {
                    super::metrics::ingest(&store, &run)?;
                }
                json!({
                    "primary": null,
                    "keys": super::metrics::project_keys(&store, &project.id)?,
                    "best": null,
                    "baseline": null,
                    "history": [],
                    "path": [],
                    "runs": [],
                })
            }
        };
        Ok(Some(body))
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    body.map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "project not found".to_string()))
}

fn internal(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
