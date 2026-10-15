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
        .route("/api/vlresearch/projects/{id}/ideas", get(project_ideas))
        .route("/api/vlresearch/projects/{id}/brief", get(project_brief))
        .route(
            "/api/vlresearch/projects/{id}/findings",
            get(project_findings),
        )
        .route("/api/vlresearch/findings/{id}", get(finding_detail))
}

/// The project's findings with their evidence counts and latest verdict, for
/// the ledger table.
async fn project_findings(Path(project_id): Path<String>) -> ApiResult {
    let body = tokio::task::spawn_blocking(move || -> crate::error::Result<Option<Value>> {
        let store = Store::open()?;
        let Some(project) = store.get_local_project(&project_id)? else {
            return Ok(None);
        };
        let mut rows = Vec::new();
        for finding in super::findings::list(&store, &project.id)? {
            let (supporting, against) = super::findings::evidence_counts(&store, &finding.id)?;
            let latest = super::findings::assessments(&store, &finding.id)?.pop();
            rows.push(json!({
                "finding": finding,
                "supporting": supporting,
                "against": against,
                "latest": latest,
            }));
        }
        Ok(Some(json!({ "findings": rows })))
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    body.map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "project not found".to_string()))
}

/// One finding's page: the claim, its evidence, its one-hop links (with the
/// linked claims' statements) and its assessments.
async fn finding_detail(Path(finding_id): Path<String>) -> ApiResult {
    let body = tokio::task::spawn_blocking(move || -> crate::error::Result<Option<Value>> {
        let store = Store::open()?;
        let Some(finding) = super::findings::get(&store, &finding_id)? else {
            return Ok(None);
        };
        let mut neighbors = Vec::new();
        for link in super::findings::links(&store, &finding.id)? {
            let other = if link.from_id == finding.id {
                &link.to_id
            } else {
                &link.from_id
            };
            let statement = super::findings::get(&store, other)?.map(|other| other.statement);
            neighbors.push(json!({ "link": link, "statement": statement }));
        }
        Ok(Some(json!({
            "evidence": super::findings::evidence(&store, &finding.id)?,
            "assessments": super::findings::assessments(&store, &finding.id)?,
            "links": neighbors,
            "finding": finding,
        })))
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    body.map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "finding not found".to_string()))
}

/// The project's brief, what it still lacks, and the budget used so far.
async fn project_brief(Path(project_id): Path<String>) -> ApiResult {
    let body = tokio::task::spawn_blocking(move || -> crate::error::Result<Option<Value>> {
        let store = Store::open()?;
        let Some(project) = store.get_local_project(&project_id)? else {
            return Ok(None);
        };
        let brief = super::scope::get(&store, &project.id)?;
        let (usage, missing) = match &brief {
            Some(brief) => (
                Some(super::scope::usage(&store, brief)?),
                super::scope::missing(&store, brief)?,
            ),
            None => (None, Vec::new()),
        };
        Ok(Some(json!({
            "brief": brief,
            "primary": super::metrics::primary(&store, &project.id)?,
            "usage": usage,
            "missing": missing,
        })))
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    body.map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "project not found".to_string()))
}

/// The project's idea queue, the selector's next pick, how well forecasts
/// have held up, and the idea-class scoreboard. Ideas carry ids for the
/// dashboard's own use; it shows titles.
async fn project_ideas(Path(project_id): Path<String>) -> ApiResult {
    let body = tokio::task::spawn_blocking(move || -> crate::error::Result<Option<Value>> {
        let store = Store::open()?;
        let Some(project) = store.get_local_project(&project_id)? else {
            return Ok(None);
        };
        let ideas = super::ideas::list(&store, &project.id)?;
        let direction = super::metrics::primary(&store, &project.id)?
            .map_or(super::metrics::Direction::Max, |metric| metric.direction);
        // Each idea also carries its status as people read it ("resolved",
        // "confirmed"), so the dashboard doesn't restate the rule.
        let learned = super::ideas::adjustment(&ideas);
        let labeled = |idea: &super::ideas::Idea| -> crate::error::Result<Value> {
            let mut value = serde_json::to_value(idea)?;
            value["statusLabel"] = json!(idea.status_label());
            // Open ideas also carry their forecast as past misses suggest
            // reading it.
            if !idea.status.is_closed() {
                if let Some((forecast, low, high)) = learned
                    .as_ref()
                    .and_then(|learned| super::ideas::adjusted(idea, learned))
                {
                    value["adjusted"] = json!({ "forecast": forecast, "low": low, "high": high });
                }
            }
            Ok(value)
        };
        let next = match super::ideas::next(&store, &project.id)? {
            Some(pick) => json!({
                "idea": labeled(&pick.idea)?,
                "reason": pick.reason,
                "score": pick.score,
            }),
            None => Value::Null,
        };
        Ok(Some(json!({
            "next": next,
            "calibration": super::ideas::calibration(&ideas),
            "byClass": super::ideas::scoreboard(&ideas, direction),
            "adjustment": learned,
            "ideas": ideas.iter().map(labeled).collect::<crate::error::Result<Vec<_>>>()?,
        })))
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    body.map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "project not found".to_string()))
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
                // Each step carries its ablation check, when one was queued.
                let ablations = super::ideas::ablations_by_step(&store, &project.id)?;
                let mut steps = Vec::with_capacity(path.len());
                for step in &path {
                    let mut value = serde_json::to_value(step)?;
                    if let Some(check) = ablations.get(&step.experiment_id) {
                        value["ablation"] = json!({
                            "status": check.status_label(),
                            "forecast": check.forecast,
                            "outcome": check.outcome,
                        });
                    }
                    steps.push(value);
                }
                json!({
                    "primary": metric,
                    "keys": super::metrics::project_keys(&store, &project.id)?,
                    "best": standing.best,
                    "baseline": standing.baseline,
                    "history": standing.history,
                    "path": steps,
                    "trials": super::ideas::trials(&store, &project.id, &runs)?,
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
