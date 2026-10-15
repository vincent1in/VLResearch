//! VLResearch's dashboard endpoints, merged into `orx up`'s router. They sit
//! behind the same loopback guard and remote-auth layers as upstream's routes.
//!
//! A project lists its optimizations; each optimization has its own brief,
//! metrics and ideas. Findings are the project's. The research settings are
//! the machine's.

use std::collections::BTreeMap;

use axum::extract::Path;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::{json, Value};

use super::scope::Brief;
use super::start::{BriefDraft, IdeaDraft};
use crate::local::model::LocalProject;
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
            "/api/vlresearch/projects/{id}/optimizations",
            get(project_optimizations).post(create_optimization),
        )
        .route(
            "/api/vlresearch/optimizations/{id}/brief",
            get(optimization_brief).put(save_brief),
        )
        .route(
            "/api/vlresearch/optimizations/{id}/brief/confirm",
            post(confirm_brief),
        )
        .route(
            "/api/vlresearch/optimizations/{id}/brief/chat",
            put(link_chat),
        )
        .route(
            "/api/vlresearch/optimizations/{id}/ideas",
            get(optimization_ideas).post(add_idea),
        )
        .route(
            "/api/vlresearch/optimizations/{id}/metrics",
            get(optimization_metrics),
        )
        .route(
            "/api/vlresearch/projects/{id}/findings",
            get(project_findings),
        )
        .route("/api/vlresearch/findings/{id}", get(finding_detail))
        .route(
            "/api/vlresearch/settings",
            get(research_settings).put(save_settings),
        )
}

/// Changing settings and confirming briefs are the person's to do, so these
/// requests are taken only from the dashboard page itself, which the browser
/// marks as same-origin. A coding agent's `curl` gets a refusal saying where
/// the person does it.
fn from_dashboard(headers: &HeaderMap, refusal: &str) -> Result<(), (StatusCode, String)> {
    let get = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    let same_site =
        get("sec-fetch-site").is_some_and(|site| site.eq_ignore_ascii_case("same-origin"));
    let same_origin = match (get(header::ORIGIN.as_str()), get(header::HOST.as_str())) {
        (Some(origin), Some(host)) => {
            origin == format!("http://{host}") || origin == format!("https://{host}")
        }
        _ => false,
    };
    if same_site || same_origin {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, refusal.to_string()))
    }
}

const SETTINGS_REFUSED: &str =
    "Research settings change on the dashboard's Settings page, by the person.";
const CONFIRM_REFUSED: &str =
    "The person confirms a brief on the dashboard's Research view. Once they agree in the chat, \
     confirm it with `orx brief confirm`.";

/// Runs a read on a blocking thread: 404 with `missing` when it finds nothing.
async fn read(
    missing: &'static str,
    body: impl FnOnce(&Store) -> crate::error::Result<Option<Value>> + Send + 'static,
) -> ApiResult {
    let body = tokio::task::spawn_blocking(move || {
        let store = Store::open()?;
        body(&store)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    body.map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, missing.to_string()))
}

/// Runs a dashboard write: 404 when its target is gone, 400 with the reason
/// when the write is refused (a missing field, a bad value).
async fn write(
    write: impl FnOnce(&Store) -> crate::error::Result<Option<Value>> + Send + 'static,
) -> ApiResult {
    let outcome = tokio::task::spawn_blocking(move || {
        let store = Store::open().map_err(internal)?;
        write(&store)
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "not found".to_string()))
    })
    .await
    .map_err(internal)?;
    outcome.map(Json)
}

/// The project's optimizations, most recently changed first, each with its
/// brief, metric and what it still needs; and the metric keys runs have
/// reported, for starting another.
async fn project_optimizations(Path(project_id): Path<String>) -> ApiResult {
    read("project not found", move |store| {
        let Some(project) = store.get_local_project(&project_id)? else {
            return Ok(None);
        };
        let mut optimizations = Vec::new();
        for brief in super::scope::list(store, &project.id)? {
            optimizations.push(brief_body(store, &brief)?);
        }
        for run in store.list_runs_by_project(&project.id)? {
            super::metrics::ingest(store, &run)?;
        }
        Ok(Some(json!({
            "optimizations": optimizations,
            "keys": super::metrics::project_keys(store, &project.id)?,
        })))
    })
    .await
}

fn brief_body(store: &Store, brief: &Brief) -> crate::error::Result<Value> {
    Ok(json!({
        "brief": brief,
        "primary": super::metrics::primary(store, &brief.id)?,
        "usage": super::scope::usage(store, brief)?,
        "missing": super::scope::missing(store, brief)?,
    }))
}

/// An optimization with its project, or `None` when either is gone.
fn load(
    store: &Store,
    optimization_id: &str,
) -> crate::error::Result<Option<(Brief, LocalProject)>> {
    let Some(brief) = super::scope::get(store, optimization_id)? else {
        return Ok(None);
    };
    Ok(store
        .get_local_project(&brief.project_id)?
        .map(|project| (brief, project)))
}

/// Starts an optimization from the Research view's form.
async fn create_optimization(
    Path(project_id): Path<String>,
    Json(draft): Json<BriefDraft>,
) -> ApiResult {
    write(move |store| {
        let Some(project) = store.get_local_project(&project_id)? else {
            return Ok(None);
        };
        let brief = super::start::create(store, &project.id, draft)?;
        brief_body(store, &brief).map(Some)
    })
    .await
}

/// An optimization's brief, what it still lacks, and the budget used so far.
async fn optimization_brief(Path(optimization_id): Path<String>) -> ApiResult {
    read(
        "optimization not found",
        move |store| match super::scope::get(store, &optimization_id)? {
            Some(brief) => brief_body(store, &brief).map(Some),
            None => Ok(None),
        },
    )
    .await
}

/// Saves the brief from the Research view's form, with its primary metric.
async fn save_brief(
    Path(optimization_id): Path<String>,
    Json(draft): Json<BriefDraft>,
) -> ApiResult {
    write(move |store| {
        if super::scope::get(store, &optimization_id)?.is_none() {
            return Ok(None);
        }
        let brief = super::start::save_brief(store, &optimization_id, draft)?;
        brief_body(store, &brief).map(Some)
    })
    .await
}

/// The user confirms the brief from the Research view.
async fn confirm_brief(headers: HeaderMap, Path(optimization_id): Path<String>) -> ApiResult {
    from_dashboard(&headers, CONFIRM_REFUSED)?;
    write(move |store| {
        let Some((brief, project)) = load(store, &optimization_id)? else {
            return Ok(None);
        };
        let brief = super::scope::confirm(store, &project, &brief.id, "user")?;
        brief_body(store, &brief).map(Some)
    })
    .await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatLink {
    session_id: String,
}

/// Hands the optimization to the chat the Research view started for it.
async fn link_chat(Path(optimization_id): Path<String>, Json(link): Json<ChatLink>) -> ApiResult {
    write(move |store| {
        if super::scope::get(store, &optimization_id)?.is_none() {
            return Ok(None);
        }
        let brief = super::start::link_chat(store, &optimization_id, &link.session_id)?;
        brief_body(store, &brief).map(Some)
    })
    .await
}

/// Adds an idea typed into the Research view, as the user's.
async fn add_idea(Path(optimization_id): Path<String>, Json(draft): Json<IdeaDraft>) -> ApiResult {
    write(move |store| {
        if super::scope::get(store, &optimization_id)?.is_none() {
            return Ok(None);
        }
        let idea = super::start::add_idea(store, &optimization_id, draft)?;
        let mut value = serde_json::to_value(&idea)?;
        value["statusLabel"] = json!(idea.status_label());
        Ok(Some(value))
    })
    .await
}

/// An optimization's idea queue, the selector's next pick, how well forecasts
/// have held up, and the idea-class scoreboard. Ideas carry ids for the
/// dashboard's own use; it shows titles.
async fn optimization_ideas(Path(optimization_id): Path<String>) -> ApiResult {
    read("optimization not found", move |store| {
        let Some(brief) = super::scope::get(store, &optimization_id)? else {
            return Ok(None);
        };
        let ideas = super::ideas::list(store, &brief.id)?;
        let direction = super::metrics::primary(store, &brief.id)?
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
        let next = match super::ideas::pick(&ideas, direction) {
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

/// An optimization's surface: its primary metric, its runs scored on it, the
/// running best against the baseline, and the lineage path from the baseline
/// to the best experiment.
async fn optimization_metrics(Path(optimization_id): Path<String>) -> ApiResult {
    read("optimization not found", move |store| {
        let Some(brief) = super::scope::get(store, &optimization_id)? else {
            return Ok(None);
        };
        let keys = super::metrics::project_keys(store, &brief.project_id)?;
        let guards = super::metrics::guards(store, &brief.id)?;
        let Some(metric) = super::metrics::primary(store, &brief.id)? else {
            return Ok(Some(json!({
                "primary": null,
                "guards": guards,
                "breaches": [],
                "keys": keys,
                "best": null,
                "baseline": null,
                "history": [],
                "path": [],
                "runs": [],
            })));
        };
        let (standing, path, runs) = super::metrics::standing(store, &brief, &metric)?;
        // Each step carries its ablation check, when one was queued.
        let ablations = super::ideas::ablations_by_step(store, &brief.id)?;
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
        Ok(Some(json!({
            "primary": metric,
            "guards": guards,
            "breaches": standing.breaches,
            "keys": keys,
            "best": standing.best,
            "baseline": standing.baseline,
            "history": standing.history,
            "path": steps,
            "trials": super::ideas::trials(store, &brief.id, &runs)?,
            "runs": runs,
        })))
    })
    .await
}

/// The project's findings with their evidence counts and latest verdict, for
/// the ledger table.
async fn project_findings(Path(project_id): Path<String>) -> ApiResult {
    read("project not found", move |store| {
        let Some(project) = store.get_local_project(&project_id)? else {
            return Ok(None);
        };
        let mut rows = Vec::new();
        for finding in super::findings::list(store, &project.id)? {
            let (supporting, against) = super::findings::evidence_counts(store, &finding.id)?;
            let latest = super::findings::assessments(store, &finding.id)?.pop();
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
}

/// One finding's page: the claim, its evidence, its one-hop links (with the
/// linked claims' statements) and its assessments.
async fn finding_detail(Path(finding_id): Path<String>) -> ApiResult {
    read("finding not found", move |store| {
        let Some(finding) = super::findings::get(store, &finding_id)? else {
            return Ok(None);
        };
        let mut neighbors = Vec::new();
        for link in super::findings::links(store, &finding.id)? {
            let other = if link.from_id == finding.id {
                &link.to_id
            } else {
                &link.from_id
            };
            let statement = super::findings::get(store, other)?.map(|other| other.statement);
            neighbors.push(json!({ "link": link, "statement": statement }));
        }
        Ok(Some(json!({
            "evidence": super::findings::evidence(store, &finding.id)?,
            "assessments": super::findings::assessments(store, &finding.id)?,
            "links": neighbors,
            "finding": finding,
        })))
    })
    .await
}

/// The research settings with where each value comes from, and the network
/// policy, for the Settings page.
async fn research_settings() -> ApiResult {
    read("settings not found", |store| {
        Ok(Some(serde_json::to_value(super::settings::report(store)?)?))
    })
    .await
}

/// Saves settings from the Settings page: each key with its new value, or
/// `null` to return it to its default.
async fn save_settings(
    headers: HeaderMap,
    Json(changes): Json<BTreeMap<String, Option<String>>>,
) -> ApiResult {
    from_dashboard(&headers, SETTINGS_REFUSED)?;
    write(move |store| {
        super::settings::save(store, &changes)?;
        Ok(Some(serde_json::to_value(super::settings::report(store)?)?))
    })
    .await
}

fn internal(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, value.parse().unwrap());
        }
        headers
    }

    #[test]
    fn only_the_dashboard_page_may_change_settings_or_confirm() {
        let host = ("host", "127.0.0.1:4791");
        for allowed in [
            headers(&[host, ("sec-fetch-site", "same-origin")]),
            headers(&[host, ("origin", "http://127.0.0.1:4791")]),
            headers(&[host, ("origin", "https://127.0.0.1:4791")]),
        ] {
            assert!(from_dashboard(&allowed, "no").is_ok(), "{allowed:?}");
        }
        for refused in [
            headers(&[host]),
            headers(&[host, ("sec-fetch-site", "none")]),
            headers(&[host, ("sec-fetch-site", "cross-site")]),
            headers(&[host, ("origin", "http://localhost:5173")]),
            headers(&[("origin", "http://127.0.0.1:4791")]),
        ] {
            let (status, message) = from_dashboard(&refused, "refused here").unwrap_err();
            assert_eq!(status, StatusCode::FORBIDDEN, "{refused:?}");
            assert_eq!(message, "refused here");
        }
    }
}
