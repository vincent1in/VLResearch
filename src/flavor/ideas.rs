//! The idea queue: every change worth trying is a typed work item with a
//! forecast, a cost and a kill test, and every experiment can point at the
//! idea it tests. That makes the improvement loop countable: which ideas were
//! tried, what each was forecast to do, what it did, and how well the
//! forecasts hold up.
//!
//! Ids exist for the CLI and the agent; the dashboard shows titles only.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use super::metrics::{self, Direction, PrimaryMetric};
use crate::error::{anyhow, Result};
use crate::store::Store;

/// What an idea is for. Blockers and halts come before any science.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    /// A change expected to move the metric.
    Science,
    /// Something broken in the pipeline that stops ideas from being tested.
    InfraBlocker,
    /// A doubt about the evaluation that must be settled before trusting it.
    EvalBlocker,
    /// Stop and check with the user.
    Halt,
    /// Rerun a promising result with new seeds before promoting it.
    Replicate,
}

/// How an idea relates to what has been tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Class {
    /// A small step from the current best.
    Incremental,
    /// A different direction.
    Orthogonal,
    /// Reproduce a known result.
    Reproduction,
    /// Remove a part to see what it contributes.
    Ablation,
    /// A new or changed dataset.
    Data,
    /// Speed or cost of the pipeline, with output kept equivalent.
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Open,
    Running,
    /// Tested, and it improved on its parent by more than the noise floor.
    Kept,
    /// Tested, and it did not; or its kill test fired.
    Refuted,
    /// Closed without a test.
    Dropped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Author {
    /// The scoping interview.
    Scope,
    /// A coding agent.
    Agent,
    /// A review pass.
    Review,
    /// The person running the project.
    User,
}

macro_rules! text_enum {
    ($type:ty { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $type {
            pub(crate) fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
            fn parse(value: &str) -> Option<Self> {
                match value { $($text => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

text_enum!(Kind { Science => "science", InfraBlocker => "infra-blocker", EvalBlocker => "eval-blocker", Halt => "halt", Replicate => "replicate" });
text_enum!(Class { Incremental => "incremental", Orthogonal => "orthogonal", Reproduction => "reproduction", Ablation => "ablation", Data => "data", Process => "process" });
text_enum!(Status { Open => "open", Running => "running", Kept => "kept", Refuted => "refuted", Dropped => "dropped" });
text_enum!(Author { Scope => "scope", Agent => "agent", Review => "review", User => "user" });

impl Kind {
    fn is_blocker(self) -> bool {
        matches!(self, Kind::InfraBlocker | Kind::EvalBlocker | Kind::Halt)
    }
}

impl Status {
    pub(crate) fn is_closed(self) -> bool {
        matches!(self, Status::Kept | Status::Refuted | Status::Dropped)
    }
}

/// One idea, as stored.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Idea {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub kind: Kind,
    pub class: Option<Class>,
    /// 1 is the most urgent, 3 the least.
    pub priority: i64,
    pub hypothesis: Option<String>,
    /// The cheap test whose negative result kills the idea, or the result
    /// that would accept it.
    pub kill_test: Option<String>,
    /// Forecast change in the primary metric, in its own units and sign.
    pub forecast: Option<f64>,
    /// The forecast's 80% interval.
    pub forecast_low: Option<f64>,
    pub forecast_high: Option<f64>,
    /// Expected compute, in GPU-hours (or machine-hours without GPUs).
    pub cost_hours: Option<f64>,
    pub status: Status,
    pub blocked_by: Option<String>,
    pub author: Author,
    pub evidence: Option<String>,
    /// The experiment that tests it, once started.
    pub experiment_id: Option<String>,
    /// Measured change against the parent experiment when closed, in the same
    /// units as the forecast, and the key it was measured on.
    pub outcome: Option<f64>,
    pub outcome_key: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub closed_at: Option<i64>,
}

/// Fields for a new idea or an edit; `None` leaves a field as it is.
#[derive(Debug, Clone, Default)]
pub(crate) struct IdeaFields {
    pub title: Option<String>,
    pub kind: Option<Kind>,
    pub class: Option<Class>,
    pub priority: Option<i64>,
    pub hypothesis: Option<String>,
    pub kill_test: Option<String>,
    pub forecast: Option<f64>,
    pub forecast_low: Option<f64>,
    pub forecast_high: Option<f64>,
    pub cost_hours: Option<f64>,
    pub blocked_by: Option<String>,
    pub author: Option<Author>,
}

pub(crate) fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vlr_ideas (
             id            TEXT PRIMARY KEY,
             project_id    TEXT NOT NULL,
             title         TEXT NOT NULL,
             kind          TEXT NOT NULL,
             class         TEXT,
             priority      INTEGER NOT NULL,
             hypothesis    TEXT,
             kill_test     TEXT,
             forecast      REAL,
             forecast_low  REAL,
             forecast_high REAL,
             cost_hours    REAL,
             status        TEXT NOT NULL,
             blocked_by    TEXT,
             author        TEXT NOT NULL,
             evidence      TEXT,
             experiment_id TEXT,
             outcome       REAL,
             outcome_key   TEXT,
             created_at    INTEGER NOT NULL,
             updated_at    INTEGER NOT NULL,
             started_at    INTEGER,
             closed_at     INTEGER
         );
         CREATE INDEX IF NOT EXISTS idx_vlr_ideas_project ON vlr_ideas(project_id, status);
         CREATE INDEX IF NOT EXISTS idx_vlr_ideas_experiment ON vlr_ideas(experiment_id);",
    )?;
    Ok(())
}

const IDEA_COLUMNS: &str = "id, project_id, title, kind, class, priority, hypothesis, kill_test,
    forecast, forecast_low, forecast_high, cost_hours, status, blocked_by, author, evidence,
    experiment_id, outcome, outcome_key, created_at, started_at, closed_at";

fn from_row(row: &Row) -> rusqlite::Result<Idea> {
    let text = |index: usize| row.get::<_, String>(index);
    Ok(Idea {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        kind: Kind::parse(&text(3)?).unwrap_or(Kind::Science),
        class: row
            .get::<_, Option<String>>(4)?
            .as_deref()
            .and_then(Class::parse),
        priority: row.get(5)?,
        hypothesis: row.get(6)?,
        kill_test: row.get(7)?,
        forecast: row.get(8)?,
        forecast_low: row.get(9)?,
        forecast_high: row.get(10)?,
        cost_hours: row.get(11)?,
        status: Status::parse(&text(12)?).unwrap_or(Status::Open),
        blocked_by: row.get(13)?,
        author: Author::parse(&text(14)?).unwrap_or(Author::User),
        evidence: row.get(15)?,
        experiment_id: row.get(16)?,
        outcome: row.get(17)?,
        outcome_key: row.get(18)?,
        created_at: row.get(19)?,
        started_at: row.get(20)?,
        closed_at: row.get(21)?,
    })
}

fn validate(fields: &IdeaFields) -> Result<()> {
    if let Some(title) = &fields.title {
        if title.trim().is_empty() {
            return Err(anyhow!("An idea needs a title."));
        }
    }
    if let Some(priority) = fields.priority {
        if !(1..=3).contains(&priority) {
            return Err(anyhow!("Priority is 1 (most urgent), 2 or 3."));
        }
    }
    if let (Some(low), Some(high)) = (fields.forecast_low, fields.forecast_high) {
        if low > high {
            return Err(anyhow!("The interval's low end is above its high end."));
        }
    }
    for value in [
        fields.forecast,
        fields.forecast_low,
        fields.forecast_high,
        fields.cost_hours,
    ]
    .into_iter()
    .flatten()
    {
        if !value.is_finite() {
            return Err(anyhow!("Forecasts and costs must be finite numbers."));
        }
    }
    if fields.cost_hours.is_some_and(|hours| hours < 0.0) {
        return Err(anyhow!("A cost can't be negative."));
    }
    Ok(())
}

pub(crate) fn get(store: &Store, id: &str) -> Result<Option<Idea>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    Ok(conn
        .query_row(
            &format!("SELECT {IDEA_COLUMNS} FROM vlr_ideas WHERE id = ?1"),
            [id.trim()],
            from_row,
        )
        .optional()?)
}

pub(crate) fn require(store: &Store, id: &str) -> Result<Idea> {
    get(store, id)?.ok_or_else(|| {
        anyhow!("Idea {id} was not found. List ideas with `orx idea list <projectId> --all`.")
    })
}

/// Every idea in the project, oldest first.
pub(crate) fn list(store: &Store, project_id: &str) -> Result<Vec<Idea>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {IDEA_COLUMNS} FROM vlr_ideas WHERE project_id = ?1 ORDER BY created_at, id"
    ))?;
    let ideas = stmt
        .query_map([project_id], from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ideas)
}

pub(crate) fn add(store: &Store, project_id: &str, fields: IdeaFields) -> Result<Idea> {
    let title = fields
        .title
        .clone()
        .ok_or_else(|| anyhow!("An idea needs a title."))?;
    validate(&fields)?;
    if let Some(blocker) = &fields.blocked_by {
        let blocker = require(store, blocker)?;
        if blocker.project_id != project_id {
            return Err(anyhow!("Idea {} belongs to another project.", blocker.id));
        }
    }
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let id = format!("idea_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let now = crate::store::now_ms();
    conn.execute(
        "INSERT INTO vlr_ideas (id, project_id, title, kind, class, priority, hypothesis,
             kill_test, forecast, forecast_low, forecast_high, cost_hours, status, blocked_by,
             author, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'open', ?13, ?14, ?15, ?15)",
        params![
            id,
            project_id,
            title.trim(),
            fields.kind.unwrap_or(Kind::Science).as_str(),
            fields.class.map(Class::as_str),
            fields.priority.unwrap_or(2),
            fields.hypothesis,
            fields.kill_test,
            fields.forecast,
            fields.forecast_low,
            fields.forecast_high,
            fields.cost_hours,
            fields.blocked_by,
            fields.author.unwrap_or(Author::User).as_str(),
            now,
        ],
    )?;
    require(store, &id)
}

pub(crate) fn edit(store: &Store, id: &str, fields: IdeaFields) -> Result<Idea> {
    validate(&fields)?;
    let idea = require(store, id)?;
    if let Some(blocker) = &fields.blocked_by {
        if blocker == &idea.id {
            return Err(anyhow!("An idea can't block itself."));
        }
        require(store, blocker)?;
    }
    let conn = store.flavor_conn();
    conn.execute(
        "UPDATE vlr_ideas SET
             title = COALESCE(?2, title), kind = COALESCE(?3, kind), class = COALESCE(?4, class),
             priority = COALESCE(?5, priority), hypothesis = COALESCE(?6, hypothesis),
             kill_test = COALESCE(?7, kill_test), forecast = COALESCE(?8, forecast),
             forecast_low = COALESCE(?9, forecast_low), forecast_high = COALESCE(?10, forecast_high),
             cost_hours = COALESCE(?11, cost_hours), blocked_by = COALESCE(?12, blocked_by),
             author = COALESCE(?13, author), updated_at = ?14
         WHERE id = ?1",
        params![
            idea.id,
            fields.title.as_deref().map(str::trim),
            fields.kind.map(Kind::as_str),
            fields.class.map(Class::as_str),
            fields.priority,
            fields.hypothesis,
            fields.kill_test,
            fields.forecast,
            fields.forecast_low,
            fields.forecast_high,
            fields.cost_hours,
            fields.blocked_by,
            fields.author.map(Author::as_str),
            crate::store::now_ms(),
        ],
    )?;
    require(store, &idea.id)
}

/// Marks an idea as being tested by an experiment.
pub(crate) fn start(store: &Store, id: &str, experiment_id: &str) -> Result<Idea> {
    let idea = require(store, id)?;
    if idea.status.is_closed() {
        return Err(anyhow!(
            "Idea {} is already {}; add a new idea to try it again.",
            idea.id,
            idea.status.as_str()
        ));
    }
    let experiment = crate::local::resolve::resolve_experiment(store, experiment_id)?;
    if experiment.project_id != idea.project_id {
        return Err(anyhow!(
            "Experiment {} belongs to another project.",
            experiment.id
        ));
    }
    let now = crate::store::now_ms();
    store.flavor_conn().execute(
        "UPDATE vlr_ideas SET status = 'running', experiment_id = ?2,
             started_at = COALESCE(started_at, ?3), updated_at = ?3
         WHERE id = ?1",
        params![idea.id, experiment.id, now],
    )?;
    require(store, &idea.id)
}

/// How an idea is being closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Decide from the measured outcome against the noise floor.
    Measured,
    Kept,
    Refuted,
    Dropped,
}

/// Closes an idea. With an experiment and a primary metric, the outcome is the
/// experiment's best finished score minus its parent's, the "verify" half of
/// forecast-and-verify.
pub(crate) fn close(
    store: &Store,
    id: &str,
    verdict: Verdict,
    evidence: Option<String>,
) -> Result<Idea> {
    let idea = require(store, id)?;
    if idea.status.is_closed() {
        return Err(anyhow!(
            "Idea {} is already {}.",
            idea.id,
            idea.status.as_str()
        ));
    }
    let primary = metrics::primary(store, &idea.project_id)?;
    let measured = match (&idea.experiment_id, &primary) {
        (Some(experiment_id), Some(metric)) if verdict != Verdict::Dropped => {
            measured_change(store, &idea.project_id, experiment_id, metric)?
        }
        _ => None,
    };
    let status = match verdict {
        Verdict::Kept => Status::Kept,
        Verdict::Refuted => Status::Refuted,
        Verdict::Dropped => Status::Dropped,
        Verdict::Measured => {
            let (Some(change), Some(metric)) = (measured, &primary) else {
                return Err(anyhow!(
                    "Idea {} has no measured result to judge it by (it needs a started experiment \
                     with a finished, scored run, its parent's score, and a primary metric). Close \
                     it with --kept, --refuted or --drop instead.",
                    idea.id
                ));
            };
            let gain = match metric.direction {
                Direction::Max => change,
                Direction::Min => -change,
            };
            if gain > metric.noise_floor.unwrap_or(0.0) {
                Status::Kept
            } else {
                Status::Refuted
            }
        }
    };
    let now = crate::store::now_ms();
    store.flavor_conn().execute(
        "UPDATE vlr_ideas SET status = ?2, evidence = COALESCE(?3, evidence), outcome = ?4,
             outcome_key = ?5, closed_at = ?6, updated_at = ?6
         WHERE id = ?1",
        params![
            idea.id,
            status.as_str(),
            evidence,
            measured,
            measured.and(primary.as_ref().map(|metric| metric.key.clone())),
            now
        ],
    )?;
    require(store, &idea.id)
}

/// The experiment's best finished score minus its parent's, on the primary
/// metric, in the metric's own units.
fn measured_change(
    store: &Store,
    project_id: &str,
    experiment_id: &str,
    metric: &PrimaryMetric,
) -> Result<Option<f64>> {
    let experiment = crate::local::resolve::resolve_experiment(store, experiment_id)?;
    let Some(parent) = experiment.parent_experiment_id else {
        return Ok(None);
    };
    let runs = metrics::scored_runs(store, project_id, &metric.key)?;
    let best = |experiment: &str| {
        runs.iter()
            .filter(|run| run.experiment_id == experiment && run.status == "done")
            .filter_map(|run| run.score)
            .max_by(|a, b| {
                metric
                    .direction
                    .gain(*a, *b)
                    .partial_cmp(&0.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    };
    Ok(match (best(&experiment.id), best(&parent)) {
        (Some(score), Some(parent_score)) => Some(score - parent_score),
        _ => None,
    })
}

/// The idea titles behind experiments, for labeling the lineage path.
pub(crate) fn titles_by_experiment(
    store: &Store,
    project_id: &str,
) -> Result<std::collections::HashMap<String, String>> {
    Ok(list(store, project_id)?
        .into_iter()
        .filter_map(|idea| Some((idea.experiment_id?, idea.title)))
        .collect())
}

/// Expected gain from trying an idea: E[max(0, X)] for X normal with the
/// forecast as its mean and its 80% interval as ±1.2816 standard deviations,
/// counted in the metric's improving direction.
pub(crate) fn expected_gain(idea: &Idea, direction: Direction) -> Option<f64> {
    let forecast = idea.forecast?;
    let mean = match direction {
        Direction::Max => forecast,
        Direction::Min => -forecast,
    };
    let sigma = match (idea.forecast_low, idea.forecast_high) {
        (Some(low), Some(high)) if high > low => (high - low) / (2.0 * 1.2816),
        _ => 0.0,
    };
    if sigma <= 0.0 {
        return Some(mean.max(0.0));
    }
    let z = mean / sigma;
    Some(sigma * normal_pdf(z) + mean * normal_cdf(z))
}

fn normal_pdf(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// Standard normal CDF via the Abramowitz–Stegun erf approximation (error
/// under 1.5e-7), which is plenty for ranking ideas.
fn normal_cdf(z: f64) -> f64 {
    let x = z / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    0.5 * (1.0 + if x >= 0.0 { erf } else { -erf })
}

/// Why the selector picked an idea.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pick {
    pub idea: Idea,
    pub reason: String,
    /// Expected gain per GPU-hour, when the idea has a forecast.
    pub score: Option<f64>,
}

/// After this many incremental ideas in a row, an orthogonal one goes next.
const INCREMENTAL_RUN: usize = 3;
/// After this many tested ideas without a kept one, the search widens.
const STAGNANT_STREAK: usize = 3;
/// Cost assumed for an idea without one, and the floor for any cost.
const DEFAULT_COST_HOURS: f64 = 1.0;
const MIN_COST_HOURS: f64 = 0.1;

/// The next idea to work on. Open blockers come first (most urgent first).
/// Among the rest, the highest expected gain per GPU-hour wins, except that
/// after three incremental ideas in a row, or three tested ideas that kept
/// nothing, an orthogonal idea is preferred when there is one.
pub(crate) fn next(store: &Store, project_id: &str) -> Result<Option<Pick>> {
    let ideas = list(store, project_id)?;
    let closed: std::collections::HashSet<&str> = ideas
        .iter()
        .filter(|idea| idea.status.is_closed())
        .map(|idea| idea.id.as_str())
        .collect();
    let ready: Vec<&Idea> = ideas
        .iter()
        .filter(|idea| idea.status == Status::Open)
        .filter(|idea| {
            idea.blocked_by
                .as_deref()
                .is_none_or(|blocker| closed.contains(blocker))
        })
        .collect();
    if let Some(blocker) = ready
        .iter()
        .filter(|idea| idea.kind.is_blocker())
        .min_by_key(|idea| (idea.priority, idea.created_at))
    {
        return Ok(Some(Pick {
            idea: (*blocker).clone(),
            reason: format!(
                "it is an open {} and blockers come before other ideas",
                blocker.kind.as_str()
            ),
            score: None,
        }));
    }
    let direction =
        metrics::primary(store, project_id)?.map_or(Direction::Max, |metric| metric.direction);
    // Started ideas, newest first, for the diversity rules.
    let mut started: Vec<&Idea> = ideas
        .iter()
        .filter(|idea| idea.started_at.is_some())
        .collect();
    started.sort_by_key(|idea| std::cmp::Reverse(idea.started_at));
    let incremental_run = started
        .iter()
        .take_while(|idea| idea.class == Some(Class::Incremental))
        .count();
    let mut tested: Vec<&Idea> = ideas
        .iter()
        .filter(|idea| matches!(idea.status, Status::Kept | Status::Refuted))
        .collect();
    tested.sort_by_key(|idea| std::cmp::Reverse(idea.closed_at));
    let stagnant = tested.len() >= STAGNANT_STREAK
        && tested
            .iter()
            .take(STAGNANT_STREAK)
            .all(|idea| idea.status == Status::Refuted);
    let rate = |idea: &Idea| {
        expected_gain(idea, direction).map(|gain| {
            gain / idea
                .cost_hours
                .unwrap_or(DEFAULT_COST_HOURS)
                .max(MIN_COST_HOURS)
        })
    };
    let best_of = |pool: &[&Idea]| -> Option<(Idea, Option<f64>)> {
        pool.iter()
            .map(|idea| (*idea, rate(idea)))
            .max_by(|(a, rate_a), (b, rate_b)| {
                // Forecast ideas beat unforecast ones; then by rate; then by
                // priority and age.
                rate_a
                    .is_some()
                    .cmp(&rate_b.is_some())
                    .then(
                        rate_a
                            .unwrap_or(0.0)
                            .partial_cmp(&rate_b.unwrap_or(0.0))
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
                    .then(b.priority.cmp(&a.priority))
                    .then(b.created_at.cmp(&a.created_at))
            })
            .map(|(idea, rate)| (idea.clone(), rate))
    };
    let candidates: Vec<&Idea> = ready
        .iter()
        .copied()
        .filter(|idea| !idea.kind.is_blocker())
        .collect();
    let orthogonal: Vec<&Idea> = candidates
        .iter()
        .copied()
        .filter(|idea| idea.class == Some(Class::Orthogonal))
        .collect();
    let widen = if incremental_run >= INCREMENTAL_RUN {
        Some(format!(
            "the last {incremental_run} ideas tried were incremental, so an orthogonal one goes next"
        ))
    } else if stagnant {
        Some(format!(
            "the last {STAGNANT_STREAK} tested ideas kept nothing, so the search widens"
        ))
    } else {
        None
    };
    if let (Some(reason), Some((idea, score))) = (widen, best_of(&orthogonal)) {
        return Ok(Some(Pick {
            idea,
            reason,
            score,
        }));
    }
    Ok(best_of(&candidates).map(|(idea, score)| {
        let reason = match score {
            Some(score) => format!(
                "highest expected gain per GPU-hour ({}) among open ideas",
                metrics::format_value(Some(score))
            ),
            None => "no open idea has a forecast yet, so the most urgent goes first".to_string(),
        };
        Pick {
            idea,
            reason,
            score,
        }
    }))
}

/// How well the forecasts of tested ideas held up.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Calibration {
    /// Tested ideas with both a forecast and a measured outcome.
    pub scored: usize,
    /// Mean of outcome minus forecast: positive means forecasts ran low.
    pub bias: Option<f64>,
    pub mean_abs_error: Option<f64>,
    /// Ideas with an interval, and how many outcomes fell inside it. A
    /// calibrated 80% interval holds about 80% of them.
    pub with_interval: usize,
    pub inside_interval: usize,
    /// Every tested idea, for the hit rate: kept / tested.
    pub tested: usize,
    pub kept: usize,
}

pub(crate) fn calibration(ideas: &[Idea]) -> Calibration {
    let mut result = Calibration::default();
    let mut errors = Vec::new();
    for idea in ideas {
        if matches!(idea.status, Status::Kept | Status::Refuted) {
            result.tested += 1;
            if idea.status == Status::Kept {
                result.kept += 1;
            }
        }
        let (Some(forecast), Some(outcome)) = (idea.forecast, idea.outcome) else {
            continue;
        };
        errors.push(outcome - forecast);
        if let (Some(low), Some(high)) = (idea.forecast_low, idea.forecast_high) {
            result.with_interval += 1;
            if (low..=high).contains(&outcome) {
                result.inside_interval += 1;
            }
        }
    }
    result.scored = errors.len();
    if !errors.is_empty() {
        let count = errors.len() as f64;
        result.bias = Some(errors.iter().sum::<f64>() / count);
        result.mean_abs_error = Some(errors.iter().map(|error| error.abs()).sum::<f64>() / count);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::model::LocalExperiment;
    use crate::store::StoredRun;

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("vlr-ideas-{}", uuid::Uuid::new_v4()));
        (Store::open_at(dir.clone()).unwrap(), dir)
    }

    fn fields(title: &str) -> IdeaFields {
        IdeaFields {
            title: Some(title.into()),
            ..IdeaFields::default()
        }
    }

    #[test]
    fn expected_gain_rewards_upside_and_respects_direction() {
        let idea = |forecast: f64, low: f64, high: f64| Idea {
            id: "i".into(),
            project_id: "p".into(),
            title: "t".into(),
            kind: Kind::Science,
            class: None,
            priority: 2,
            hypothesis: None,
            kill_test: None,
            forecast: Some(forecast),
            forecast_low: Some(low),
            forecast_high: Some(high),
            cost_hours: None,
            status: Status::Open,
            blocked_by: None,
            author: Author::Agent,
            evidence: None,
            experiment_id: None,
            outcome: None,
            outcome_key: None,
            created_at: 0,
            started_at: None,
            closed_at: None,
        };
        let sure = expected_gain(&idea(0.002, 0.002, 0.002), Direction::Max).unwrap();
        assert!((sure - 0.002).abs() < 1e-12);
        // Same mean, wider interval: more upside, so a higher expected gain.
        let risky = expected_gain(&idea(0.002, -0.006, 0.010), Direction::Max).unwrap();
        assert!(risky > sure);
        // A forecast in the wrong direction for a minimized metric still has
        // some upside when its interval crosses zero.
        let wrong_way = expected_gain(&idea(0.01, -0.002, 0.022), Direction::Min).unwrap();
        assert!(wrong_way > 0.0 && wrong_way < 0.001);
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-7);
        assert!((normal_cdf(1.2816) - 0.9).abs() < 1e-4);
    }

    #[test]
    fn blockers_go_first_then_gain_per_hour_with_a_diversity_rule() {
        let (store, dir) = temp_store();
        let cheap = add(
            &store,
            "p",
            IdeaFields {
                forecast: Some(0.002),
                cost_hours: Some(0.5),
                class: Some(Class::Incremental),
                ..fields("Cheap tweak")
            },
        )
        .unwrap();
        let costly = add(
            &store,
            "p",
            IdeaFields {
                forecast: Some(0.004),
                cost_hours: Some(8.0),
                class: Some(Class::Orthogonal),
                ..fields("New model family")
            },
        )
        .unwrap();
        assert_eq!(next(&store, "p").unwrap().unwrap().idea.id, cheap.id);

        let blocker = add(
            &store,
            "p",
            IdeaFields {
                kind: Some(Kind::EvalBlocker),
                priority: Some(1),
                ..fields("Validation window overlaps training")
            },
        )
        .unwrap();
        let blocked = add(
            &store,
            "p",
            IdeaFields {
                forecast: Some(0.05),
                blocked_by: Some(blocker.id.clone()),
                ..fields("Waits on the blocker")
            },
        )
        .unwrap();
        let pick = next(&store, "p").unwrap().unwrap();
        assert_eq!(pick.idea.id, blocker.id);
        assert!(pick.reason.contains("blockers come before"));
        close(&store, &blocker.id, Verdict::Dropped, None).unwrap();
        // Unblocked, the big forecast wins.
        assert_eq!(next(&store, "p").unwrap().unwrap().idea.id, blocked.id);
        close(&store, &blocked.id, Verdict::Dropped, None).unwrap();

        // Three incremental ideas in a row: the orthogonal one goes next even
        // though the cheap tweak has the better rate.
        for title in ["a", "b", "c"] {
            let idea = add(
                &store,
                "p",
                IdeaFields {
                    class: Some(Class::Incremental),
                    ..fields(title)
                },
            )
            .unwrap();
            store
                .flavor_conn()
                .execute(
                    "UPDATE vlr_ideas SET status = 'refuted', started_at = ?2, closed_at = ?2 WHERE id = ?1",
                    params![idea.id, crate::store::now_ms()],
                )
                .unwrap();
        }
        let pick = next(&store, "p").unwrap().unwrap();
        assert_eq!(pick.idea.id, costly.id);
        assert!(pick.reason.contains("orthogonal"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn closing_measures_the_change_against_the_parent_experiment() {
        let (store, dir) = temp_store();
        for (id, parent) in [("base", None), ("child", Some("base"))] {
            store
                .create_local_experiment(&LocalExperiment {
                    id: id.into(),
                    project_id: "p".into(),
                    parent_experiment_id: parent.map(Into::into),
                    slug: id.into(),
                    branch_name: format!("orx/{id}"),
                    title: None,
                    description: None,
                    run_command: "python train.py".into(),
                    agent_status: "idle".into(),
                    created_at: 1,
                    updated_at: 1,
                    chat_session_id: None,
                    archived: false,
                })
                .unwrap();
        }
        for (run_id, experiment, value) in [("r_base", "base", 0.020), ("r_child", "child", 0.023)]
        {
            store
                .upsert_run(&StoredRun {
                    id: run_id.into(),
                    experiment_id: experiment.into(),
                    project_id: "p".into(),
                    status: "done".into(),
                    backend_json: "{}".into(),
                    command: "python train.py".into(),
                    created_at: 1,
                    updated_at: 2,
                    ended_at: Some(2),
                    exit_code: Some(0),
                    commit_sha: None,
                    result_markdown: None,
                    cancel_requested: false,
                    cancel_reason: None,
                    chat_session_id: None,
                })
                .unwrap();
            let log = super::super::wake::run_log_path(&store, run_id);
            std::fs::write(log, format!("ORX_METRIC {{\"val_ic\": {value}}}\n")).unwrap();
        }
        metrics::set_primary(
            &store,
            "p",
            &PrimaryMetric {
                key: "val_ic".into(),
                direction: Direction::Max,
                noise_floor: Some(0.001),
            },
        )
        .unwrap();
        let idea = add(
            &store,
            "p",
            IdeaFields {
                forecast: Some(0.002),
                forecast_low: Some(0.0),
                forecast_high: Some(0.004),
                ..fields("Volume features")
            },
        )
        .unwrap();
        assert!(close(&store, &idea.id, Verdict::Measured, None).is_err());
        start(&store, &idea.id, "child").unwrap();
        let closed = close(
            &store,
            &idea.id,
            Verdict::Measured,
            Some("runs r_child, r_base".into()),
        )
        .unwrap();
        assert_eq!(closed.status, Status::Kept);
        assert!((closed.outcome.unwrap() - 0.003).abs() < 1e-12);
        assert_eq!(closed.outcome_key.as_deref(), Some("val_ic"));
        assert!(start(&store, &idea.id, "child").is_err());

        let report = calibration(&list(&store, "p").unwrap());
        assert_eq!(
            (report.scored, report.with_interval, report.inside_interval),
            (1, 1, 1)
        );
        assert_eq!((report.tested, report.kept), (1, 1));
        assert!((report.bias.unwrap() - 0.001).abs() < 1e-12);
        let titles = titles_by_experiment(&store, "p").unwrap();
        assert_eq!(
            titles.get("child").map(String::as_str),
            Some("Volume features")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_fields_are_refused() {
        let (store, dir) = temp_store();
        assert!(add(&store, "p", fields("  ")).is_err());
        assert!(add(
            &store,
            "p",
            IdeaFields {
                priority: Some(4),
                ..fields("x")
            }
        )
        .is_err());
        assert!(add(
            &store,
            "p",
            IdeaFields {
                forecast_low: Some(0.2),
                forecast_high: Some(0.1),
                ..fields("x")
            }
        )
        .is_err());
        assert!(add(
            &store,
            "p",
            IdeaFields {
                blocked_by: Some("idea_nope".into()),
                ..fields("x")
            }
        )
        .is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
