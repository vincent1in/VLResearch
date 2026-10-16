//! Optimizations and their scoping gate. Each auto-research chat runs its own
//! optimization: a brief agreed with the user (the objective and its primary
//! metric, what may and must not change, the paths the work is confined to, a
//! budget, a stop rule) with its own queue of ideas. A project can run several
//! at once, on the whole system or on parts of it.
//!
//! A run belongs to the optimization of the idea that tests its experiment,
//! else of the chat that created the experiment, else of the chat that
//! launched it. Such runs are refused until the brief is confirmed, while one
//! of its blocker ideas is open, when the snapshot about to run changes a
//! locked file or a path outside the brief's scope, or once its budget is
//! spent. Runs outside any optimization launch as upstream does, with a
//! reminder, unless `VLR_SCOPE_GATE=strict`; but the improvement loop itself
//! (`orx idea next`, `orx idea start`) waits for a confirmed brief either way.
//! Probe runs (`VLR_PROBE=1`: smoke tests, baseline reproductions, noise-floor
//! seeds) pass the gate and never count as a best result.
//!
//! Inside a chat, `orx exp run` is forwarded to the `orx up` server, whose
//! environment is not the agent's. So the launching command records a probe
//! request in the store (`before_launch`), and the server's check consumes it.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use super::ideas;
use crate::error::{anyhow, Result};
use crate::local::model::{LocalExperiment, LocalProject};
use crate::store::Store;

/// `strict` also refuses runs outside any optimization; `0` turns the gate off.
/// Also on the Settings page.
pub(crate) const GATE_ENV: &str = "VLR_SCOPE_GATE";
/// Set to `1` on a launch to mark it a probe.
pub(crate) const PROBE_ENV: &str = "VLR_PROBE";
/// How long a probe request waits for the launch it was made for.
const PROBE_REQUEST_TTL_MS: i64 = 120_000;
/// Marks the one-time move of project-wide briefs into optimizations.
const MIGRATED_KEY: &str = "optimizations";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateMode {
    Off,
    /// Enforce briefs on the runs of optimizations.
    Scoped,
    /// Require a confirmed brief for every run.
    Strict,
}

impl GateMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            GateMode::Off => "off",
            GateMode::Scoped => "scoped",
            GateMode::Strict => "strict",
        }
    }
}

fn gate_mode(store: &Store) -> GateMode {
    parse_gate(super::settings::value(store, GATE_ENV).as_deref())
}

/// `strict`, or `0`/`off`; anything else, or unset, is `scoped`.
pub(crate) fn parse_gate(value: Option<&str>) -> GateMode {
    match value
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "0" | "off" | "false" | "no" => GateMode::Off,
        "strict" => GateMode::Strict,
        _ => GateMode::Scoped,
    }
}

/// One optimization: what the user and the agent agreed to improve, how it is
/// measured (its primary metric, kept by `metrics`), its limits, and the chat
/// that runs it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Brief {
    /// The optimization's id (`opt_…`).
    pub id: String,
    pub project_id: String,
    /// The goal in words; the number that measures it is the primary metric.
    pub objective: Option<String>,
    pub may_change: Option<String>,
    pub must_not_change: Option<String>,
    /// Repository paths (evaluation code, split definitions) that no run may
    /// change, pinned to their git object at confirmation.
    pub locked: Vec<LockedPath>,
    /// When set, the only repository paths runs may change: the part of the
    /// system this optimization works on.
    pub scope: Vec<String>,
    pub budget_runs: Option<i64>,
    pub budget_gpu_hours: Option<f64>,
    pub budget_days: Option<f64>,
    pub pointers: Option<String>,
    pub sources: Option<String>,
    pub stop_rule: Option<String>,
    pub confirmed_at: Option<i64>,
    pub confirmed_by: Option<String>,
    /// When the budget started counting: the first confirmation, kept across
    /// later edits and confirmations until the user asks for a fresh budget.
    pub budget_since: Option<i64>,
    /// The baseline branch's commit at confirmation; the scope check measures
    /// a run's changes from it.
    pub base_commit: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// The chat that runs this optimization, told when the user confirms the
    /// brief in the dashboard.
    pub chat_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub(crate) struct LockedPath {
    pub path: String,
    /// The git object (a file's blob or a folder's tree) on the baseline
    /// branch when the brief was confirmed.
    #[serde(alias = "sha256")]
    pub object: Option<String>,
}

/// A change to a brief; `None` keeps a field.
#[derive(Debug, Clone, Default)]
pub(crate) struct BriefFields {
    pub objective: Option<String>,
    pub may_change: Option<String>,
    pub must_not_change: Option<String>,
    pub lock: Vec<String>,
    pub unlock: Vec<String>,
    /// Paths to add to the scope, and to take out of it.
    pub scope: Vec<String>,
    pub unscope: Vec<String>,
    pub budget_runs: Option<i64>,
    pub budget_gpu_hours: Option<f64>,
    pub budget_days: Option<f64>,
    /// Clear all three budgets before applying the ones given.
    pub clear_budget: bool,
    /// Start the budget over at the next confirmation.
    pub reset_budget: bool,
    pub pointers: Option<String>,
    pub sources: Option<String>,
    pub stop_rule: Option<String>,
}

pub(crate) fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vlr_optimizations (
             id               TEXT PRIMARY KEY,
             project_id       TEXT NOT NULL,
             chat_session_id  TEXT,
             objective        TEXT,
             may_change       TEXT,
             must_not_change  TEXT,
             locked_json      TEXT NOT NULL DEFAULT '[]',
             scope_json       TEXT NOT NULL DEFAULT '[]',
             budget_runs      INTEGER,
             budget_gpu_hours REAL,
             budget_days      REAL,
             pointers         TEXT,
             sources          TEXT,
             stop_rule        TEXT,
             confirmed_at     INTEGER,
             confirmed_by     TEXT,
             budget_since     INTEGER,
             base_commit      TEXT,
             metric_key       TEXT,
             metric_direction TEXT,
             noise_floor      REAL,
             metric_split     TEXT,
             created_at       INTEGER NOT NULL,
             updated_at       INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_vlr_optimizations_project
             ON vlr_optimizations(project_id, updated_at);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_vlr_optimizations_chat
             ON vlr_optimizations(chat_session_id) WHERE chat_session_id IS NOT NULL;
         CREATE TABLE IF NOT EXISTS vlr_optimization_runs (
             run_id          TEXT PRIMARY KEY,
             optimization_id TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_vlr_optimization_runs
             ON vlr_optimization_runs(optimization_id);
         CREATE TABLE IF NOT EXISTS vlr_probe_runs (
             run_id TEXT PRIMARY KEY
         );
         CREATE TABLE IF NOT EXISTS vlr_probe_requests (
             experiment_id TEXT PRIMARY KEY,
             requested_at  INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS vlr_flavor_meta (
             key   TEXT PRIMARY KEY,
             value TEXT
         );",
    )?;
    migrate(conn)
}

/// Moves the project-wide briefs of earlier versions, with their metric,
/// ideas, findings and runs, into one optimization per project. Runs once.
fn migrate(conn: &Connection) -> Result<()> {
    let done = |conn: &Connection| -> Result<bool> {
        Ok(conn
            .prepare("SELECT 1 FROM vlr_flavor_meta WHERE key = ?1")?
            .exists([MIGRATED_KEY])?)
    };
    if done(conn)? {
        return Ok(());
    }
    ideas::create_tables(conn)?;
    super::findings::create_tables(conn)?;
    super::metrics::ensure_tables(conn)?;
    let legacy_briefs = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'vlr_briefs'")?
        .exists([])?;
    if legacy_briefs {
        super::metrics::add_column(conn, "vlr_briefs", "budget_since", "INTEGER")?;
        super::metrics::add_column(conn, "vlr_briefs", "chat_session_id", "TEXT")?;
    }
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    // Another process may have moved them while this one waited for the lock.
    if done(&tx)? {
        return Ok(());
    }
    let mut sources = vec![
        "SELECT project_id FROM vlr_project_metric",
        "SELECT project_id FROM vlr_ideas WHERE optimization_id IS NULL",
    ];
    if legacy_briefs {
        sources.push("SELECT project_id FROM vlr_briefs");
    }
    let projects: Vec<String> = tx
        .prepare(&sources.join(" UNION "))?
        .query_map([], |row| row.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let now = crate::store::now_ms();
    for project_id in projects {
        let id = new_id();
        tx.execute(
            "INSERT INTO vlr_optimizations (id, project_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)",
            params![id, project_id, now],
        )?;
        if legacy_briefs {
            tx.execute(
                "UPDATE vlr_optimizations SET (objective, may_change, must_not_change,
                     locked_json, budget_runs, budget_gpu_hours, budget_days, pointers, sources,
                     stop_rule, confirmed_at, confirmed_by, budget_since, chat_session_id,
                     created_at, updated_at) =
                 (SELECT objective, may_change, must_not_change, locked_json, budget_runs,
                     budget_gpu_hours, budget_days, pointers, sources, stop_rule, confirmed_at,
                     confirmed_by, budget_since, chat_session_id, updated_at, updated_at
                  FROM vlr_briefs WHERE project_id = ?2)
                 WHERE id = ?1 AND EXISTS (SELECT 1 FROM vlr_briefs WHERE project_id = ?2)",
                params![id, project_id],
            )?;
        }
        tx.execute(
            "UPDATE vlr_optimizations SET (metric_key, metric_direction, noise_floor, metric_split) =
                 (SELECT key, direction, noise_floor, split FROM vlr_project_metric
                  WHERE project_id = ?2)
             WHERE id = ?1 AND EXISTS (SELECT 1 FROM vlr_project_metric WHERE project_id = ?2)",
            params![id, project_id],
        )?;
        tx.execute(
            "UPDATE vlr_ideas SET optimization_id = ?1
             WHERE project_id = ?2 AND optimization_id IS NULL",
            params![id, project_id],
        )?;
        tx.execute(
            "UPDATE vlr_findings SET optimization_id = ?1
             WHERE project_id = ?2 AND optimization_id IS NULL",
            params![id, project_id],
        )?;
        // A project-wide brief held every run in the project.
        tx.execute(
            "INSERT OR IGNORE INTO vlr_optimization_runs (run_id, optimization_id)
             SELECT id, ?1 FROM runs WHERE project_id = ?2",
            params![id, project_id],
        )?;
    }
    tx.execute(
        "INSERT INTO vlr_flavor_meta (key, value) VALUES (?1, ?2)",
        params![MIGRATED_KEY, now.to_string()],
    )?;
    tx.commit()?;
    Ok(())
}

fn new_id() -> String {
    format!("opt_{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

const COLUMNS: &str = "id, project_id, objective, may_change, must_not_change, locked_json,
    scope_json, budget_runs, budget_gpu_hours, budget_days, pointers, sources, stop_rule,
    confirmed_at, confirmed_by, budget_since, base_commit, created_at, updated_at,
    chat_session_id";

fn from_row(row: &Row) -> rusqlite::Result<Brief> {
    let locked: String = row.get(5)?;
    let scope: String = row.get(6)?;
    Ok(Brief {
        id: row.get(0)?,
        project_id: row.get(1)?,
        objective: row.get(2)?,
        may_change: row.get(3)?,
        must_not_change: row.get(4)?,
        locked: serde_json::from_str(&locked).unwrap_or_default(),
        scope: serde_json::from_str(&scope).unwrap_or_default(),
        budget_runs: row.get(7)?,
        budget_gpu_hours: row.get(8)?,
        budget_days: row.get(9)?,
        pointers: row.get(10)?,
        sources: row.get(11)?,
        stop_rule: row.get(12)?,
        confirmed_at: row.get(13)?,
        confirmed_by: row.get(14)?,
        budget_since: row.get(15)?,
        base_commit: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
        chat_session_id: row.get(19)?,
    })
}

/// One optimization by id.
pub(crate) fn get(store: &Store, id: &str) -> Result<Option<Brief>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM vlr_optimizations WHERE id = ?1"),
            [id.trim()],
            from_row,
        )
        .optional()?)
}

pub(crate) fn require(store: &Store, id: &str) -> Result<Brief> {
    get(store, id)?.ok_or_else(|| {
        anyhow!("Optimization {id} was not found. List them with `orx brief list <projectId>`.")
    })
}

/// A project's optimizations, most recently changed first.
pub(crate) fn list(store: &Store, project_id: &str) -> Result<Vec<Brief>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM vlr_optimizations WHERE project_id = ?1
         ORDER BY updated_at DESC, id"
    ))?;
    let briefs = stmt
        .query_map([project_id], from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(briefs)
}

/// The optimization a chat runs, if any.
pub(crate) fn for_chat(store: &Store, session_id: &str) -> Result<Option<Brief>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM vlr_optimizations WHERE chat_session_id = ?1"),
            [session_id],
            from_row,
        )
        .optional()?)
}

/// A new, empty optimization, run by `chat` when given.
pub(crate) fn create(store: &Store, project_id: &str, chat: Option<&str>) -> Result<Brief> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let id = new_id();
    let now = crate::store::now_ms();
    if let Some(chat) = chat {
        unlink_chat(conn, chat)?;
    }
    conn.execute(
        "INSERT INTO vlr_optimizations (id, project_id, chat_session_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![id, project_id, chat, now],
    )?;
    require(store, &id)
}

/// A chat runs one optimization at a time.
fn unlink_chat(conn: &Connection, chat: &str) -> Result<()> {
    conn.execute(
        "UPDATE vlr_optimizations SET chat_session_id = NULL WHERE chat_session_id = ?1",
        [chat],
    )?;
    Ok(())
}

/// Hands the optimization to a chat, which stops running any other.
pub(crate) fn set_chat(store: &Store, id: &str, session_id: &str) -> Result<Brief> {
    let brief = require(store, id)?;
    let conn = store.flavor_conn();
    unlink_chat(conn, session_id)?;
    conn.execute(
        "UPDATE vlr_optimizations SET chat_session_id = ?2 WHERE id = ?1",
        params![brief.id, session_id],
    )?;
    require(store, &brief.id)
}

/// The chat a side chat branched from (or the chat itself): a side chat asks
/// about its parent's optimization.
pub(crate) fn top_level_chat(store: &Store, session_id: &str) -> String {
    let mut current = session_id.to_owned();
    // Side chats don't nest deeply; the bound only guards against a cycle.
    for _ in 0..8 {
        match store.get_chat_session(&current) {
            Ok(Some(session)) => match session.side_parent_session_id {
                Some(parent) => current = parent,
                None => break,
            },
            _ => break,
        }
    }
    current
}

/// The chat this `orx` command runs in, as its top-level chat.
pub(crate) fn current_chat(store: &Store) -> Option<String> {
    if !crate::local::chat::in_local_session() {
        return None;
    }
    let session = crate::local::chat::launching_chat_session()?;
    Some(top_level_chat(store, &session))
}

/// Which optimization a command means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resolve {
    /// One that exists.
    Existing,
    /// Start one when the chat runs none (or, outside a chat, when the
    /// project has none): commands that fill in a brief, a metric or ideas.
    OrCreate,
}

/// The optimization a command acts on: the one named with `--opt`; else the
/// one this chat runs; else the project's only one.
pub(crate) fn resolve(
    store: &Store,
    project_id: &str,
    explicit: Option<&str>,
    want: Resolve,
) -> Result<Brief> {
    if let Some(id) = explicit {
        let brief = require(store, id)?;
        if brief.project_id != project_id {
            return Err(anyhow!(
                "Optimization {} belongs to project {}, not {project_id}.",
                brief.id,
                brief.project_id
            ));
        }
        return Ok(brief);
    }
    if let Some(chat) = current_chat(store) {
        match for_chat(store, &chat)? {
            Some(brief) if brief.project_id == project_id => return Ok(brief),
            // A chat runs one optimization; another project's leaves it be.
            Some(_) => {}
            None if want == Resolve::OrCreate => {
                return create(store, project_id, Some(&chat));
            }
            None => {}
        }
    }
    let mut all = list(store, project_id)?;
    match all.len() {
        0 if want == Resolve::OrCreate => create(store, project_id, None),
        0 => Err(anyhow!(
            "Project {project_id} has no optimization yet. Scope one with the user \
             (`orx skill grill`), or start one from the dashboard."
        )),
        1 => Ok(all.remove(0)),
        _ => Err(anyhow!(
            "Project {project_id} runs several optimizations, and this command doesn't say \
             which: {}. Pass `--opt <optId>`, or run it from the chat that runs the one you \
             mean.",
            all.iter().map(label).collect::<Vec<_>>().join("; ")
        )),
    }
}

/// An optimization as a one-line reference: its id and objective.
pub(crate) fn label(brief: &Brief) -> String {
    match &brief.objective {
        Some(objective) => format!("{} ({objective})", brief.id),
        None => brief.id.clone(),
    }
}

/// Normalizes a repository-relative path for locking or scoping.
fn clean_path(path: &str) -> Result<String> {
    let trimmed = path
        .trim()
        .trim_start_matches("./")
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_owned();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed.starts_with('/')
        || trimmed
            .split('/')
            .any(|part| part == ".." || part.is_empty())
    {
        return Err(anyhow!(
            "Name files or folders by their path from the repository root, without `..`: {path:?}"
        ));
    }
    Ok(trimmed)
}

/// Applies changes to the brief. Any change reopens a confirmed brief, so the
/// user agrees to the brief as it now stands.
pub(crate) fn set(store: &Store, id: &str, fields: BriefFields) -> Result<Brief> {
    let mut brief = require(store, id)?;
    let text = |value: Option<String>, current: Option<String>| match value
        .map(|value| value.trim().to_owned())
    {
        Some(value) if value.is_empty() => None,
        Some(value) => Some(value),
        None => current,
    };
    brief.objective = text(fields.objective, brief.objective);
    brief.may_change = text(fields.may_change, brief.may_change);
    brief.must_not_change = text(fields.must_not_change, brief.must_not_change);
    brief.pointers = text(fields.pointers, brief.pointers);
    brief.sources = text(fields.sources, brief.sources);
    brief.stop_rule = text(fields.stop_rule, brief.stop_rule);
    if fields.clear_budget {
        brief.budget_runs = None;
        brief.budget_gpu_hours = None;
        brief.budget_days = None;
    }
    if let Some(runs) = fields.budget_runs {
        if runs < 1 {
            return Err(anyhow!("A run budget is at least 1."));
        }
        brief.budget_runs = Some(runs);
    }
    for (value, slot, name) in [
        (
            fields.budget_gpu_hours,
            &mut brief.budget_gpu_hours,
            "GPU-hour",
        ),
        (fields.budget_days, &mut brief.budget_days, "day"),
    ] {
        if let Some(value) = value {
            if !value.is_finite() || value <= 0.0 {
                return Err(anyhow!("A {name} budget must be a positive number."));
            }
            *slot = Some(value);
        }
    }
    if fields.reset_budget {
        brief.budget_since = None;
    }
    for path in fields.unlock {
        let path = clean_path(&path)?;
        brief.locked.retain(|locked| locked.path != path);
    }
    for path in fields.lock {
        let path = clean_path(&path)?;
        if brief.locked.iter().all(|locked| locked.path != path) {
            brief.locked.push(LockedPath { path, object: None });
        }
    }
    for path in fields.unscope {
        let path = clean_path(&path)?;
        brief.scope.retain(|scoped| *scoped != path);
    }
    for path in fields.scope {
        let path = clean_path(&path)?;
        if !brief.scope.contains(&path) {
            brief.scope.push(path);
        }
    }
    brief.confirmed_at = None;
    brief.confirmed_by = None;
    brief.updated_at = crate::store::now_ms();
    save(store, &brief)?;
    Ok(brief)
}

fn save(store: &Store, brief: &Brief) -> Result<()> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    conn.execute(
        "UPDATE vlr_optimizations SET objective = ?2, may_change = ?3, must_not_change = ?4,
             locked_json = ?5, scope_json = ?6, budget_runs = ?7, budget_gpu_hours = ?8,
             budget_days = ?9, pointers = ?10, sources = ?11, stop_rule = ?12,
             confirmed_at = ?13, confirmed_by = ?14, budget_since = ?15, base_commit = ?16,
             updated_at = ?17
         WHERE id = ?1",
        params![
            brief.id,
            brief.objective,
            brief.may_change,
            brief.must_not_change,
            serde_json::to_string(&brief.locked)?,
            serde_json::to_string(&brief.scope)?,
            brief.budget_runs,
            brief.budget_gpu_hours,
            brief.budget_days,
            brief.pointers,
            brief.sources,
            brief.stop_rule,
            brief.confirmed_at,
            brief.confirmed_by,
            brief.budget_since,
            brief.base_commit,
            brief.updated_at,
        ],
    )?;
    Ok(())
}

/// The improvement loop's entry points (`orx idea next`, `orx idea start`)
/// wait for a confirmed brief: what to improve and how it is measured, its
/// limits, a budget, a stop rule and ideas to start from, agreed with the
/// user. Even where launches are allowed without one. Off with the gate.
pub(crate) fn require_confirmed(store: &Store, brief: &Brief) -> Result<()> {
    if gate_mode(store) == GateMode::Off || brief.confirmed_at.is_some() {
        return Ok(());
    }
    let missing = missing(store, brief)?;
    let next_step = if missing.is_empty() {
        format!(
            "Show it to the user; they confirm it in the Research view, or once they agree, run \
             `orx brief confirm {} --opt {}`.",
            brief.project_id, brief.id
        )
    } else {
        format!("It still needs {}.", missing.join("; "))
    };
    Err(anyhow!(
        "The improvement loop starts from a confirmed brief: what to improve and how it is \
         measured, what may and must not change, a budget, a stop rule and ideas to start from. \
         Optimization {} isn't confirmed yet. {next_step}",
        brief.id
    ))
}

/// What a brief still lacks before it can be confirmed.
pub(crate) fn missing(store: &Store, brief: &Brief) -> Result<Vec<&'static str>> {
    let mut missing = Vec::new();
    if brief.objective.is_none() {
        missing.push("an objective (--objective)");
    }
    if super::metrics::primary(store, &brief.id)?.is_none() {
        missing
            .push("a primary metric (orx metric primary <projectId> <key> --maximize|--minimize)");
    }
    if brief.must_not_change.is_none() && brief.locked.is_empty() {
        missing.push("what must not change (--must-not-change, and --lock for evaluation files)");
    }
    if brief.budget_runs.is_none()
        && brief.budget_gpu_hours.is_none()
        && brief.budget_days.is_none()
    {
        missing.push("a budget (--budget-runs, --budget-gpu-hours or --budget-days)");
    }
    if brief.stop_rule.is_none() {
        missing.push("a stop rule (--stop-rule)");
    }
    // The loop needs somewhere to start: the user's pointers and the
    // interview's proposals, queued as ideas.
    let queued = ideas::list(store, &brief.id)?.into_iter().any(|idea| {
        matches!(idea.status, ideas::Status::Open | ideas::Status::Running) && !idea.is_blocker()
    });
    if !queued {
        missing.push("at least one idea to start from (orx idea add)");
    }
    Ok(missing)
}

/// The git object a path names at a revision (a blob for a file, a tree for a
/// folder, covering everything inside it), or `None` when it doesn't exist.
fn object_at(repo: &Path, revision: &str, path: &str) -> Result<Option<String>> {
    let output = crate::local::git::git_command()
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{revision}:{path}"))
        .output()
        .map_err(|error| anyhow!("Could not run git to check {path}: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let object = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!object.is_empty()).then_some(object))
}

/// The commit a revision names, or `None`.
fn commit_at(repo: &Path, revision: &str) -> Result<Option<String>> {
    let output = crate::local::git::git_command()
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{revision}^{{commit}}"))
        .output()
        .map_err(|error| anyhow!("Could not run git to read {revision}: {error}"))?;
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((output.status.success() && !commit.is_empty()).then_some(commit))
}

/// Files a revision changes since it branched from `base`.
fn changed_paths(repo: &Path, base: &str, revision: &str) -> Result<Vec<String>> {
    let output = crate::local::git::git_command()
        .arg("-C")
        .arg(repo)
        .args(["diff", "--name-only", "--no-renames"])
        .arg(format!("{base}...{revision}"))
        .output()
        .map_err(|error| anyhow!("Could not run git to compare {revision} with {base}: {error}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "Could not compare {revision} with {base}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .filter(|line| !line.is_empty())
        .collect())
}

fn within(path: &str, scope: &[String]) -> bool {
    scope
        .iter()
        .any(|allowed| path == allowed || path.starts_with(&format!("{allowed}/")))
}

/// Confirms the brief as it stands: every required field is present, each
/// locked path is pinned to its object on the project's baseline branch, and
/// the scope check's starting commit is that branch's. The budget starts
/// counting at the first confirmation.
pub(crate) fn confirm(store: &Store, project: &LocalProject, id: &str, by: &str) -> Result<Brief> {
    let mut brief = require(store, id)?;
    if brief.project_id != project.id {
        return Err(anyhow!(
            "Optimization {} belongs to project {}.",
            brief.id,
            brief.project_id
        ));
    }
    let missing = missing(store, &brief)?;
    if !missing.is_empty() {
        return Err(anyhow!("The brief still needs {}.", missing.join("; ")));
    }
    let repo = Path::new(&project.repo_path);
    let baseline = format!("refs/heads/{}", project.baseline_branch);
    for locked in &mut brief.locked {
        locked.object = Some(object_at(repo, &baseline, &locked.path)?.ok_or_else(|| {
            anyhow!(
                "Locked path {} is not on the baseline branch {}.",
                locked.path,
                project.baseline_branch
            )
        })?);
    }
    if !brief.scope.is_empty() {
        brief.base_commit = Some(commit_at(repo, &baseline)?.ok_or_else(|| {
            anyhow!(
                "The baseline branch {} has no commit to measure the scope from.",
                project.baseline_branch
            )
        })?);
    }
    let now = crate::store::now_ms();
    brief.confirmed_at = Some(now);
    brief.confirmed_by = Some(by.to_owned());
    brief.budget_since.get_or_insert(now);
    brief.updated_at = now;
    save(store, &brief)?;
    Ok(brief)
}

/// What the brief's budget has used so far: the optimization's non-probe runs
/// launched since the budget started, and their wall-clock hours.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Usage {
    pub runs: i64,
    pub run_hours: f64,
    pub days: f64,
}

pub(crate) fn usage(store: &Store, brief: &Brief) -> Result<Usage> {
    let Some(since) = brief.budget_since else {
        return Ok(Usage::default());
    };
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let now = crate::store::now_ms();
    let (runs, millis): (i64, Option<i64>) = conn.query_row(
        "SELECT COUNT(*), SUM(COALESCE(r.ended_at, ?3) - r.created_at) FROM runs r
         JOIN vlr_optimization_runs o ON o.run_id = r.id
         WHERE o.optimization_id = ?1 AND r.created_at >= ?2
           AND r.id NOT IN (SELECT run_id FROM vlr_probe_runs)",
        params![brief.id, since, now],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(Usage {
        runs,
        run_hours: millis.unwrap_or(0).max(0) as f64 / 3_600_000.0,
        days: (now - since).max(0) as f64 / 86_400_000.0,
    })
}

/// The runs an optimization's results come from: those launched for it, and
/// any run of an experiment one of its ideas tests.
pub(crate) fn run_ids(store: &Store, brief: &Brief) -> Result<HashSet<String>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT run_id FROM vlr_optimization_runs WHERE optimization_id = ?1
         UNION
         SELECT r.id FROM runs r JOIN vlr_ideas i ON i.experiment_id = r.experiment_id
         WHERE i.optimization_id = ?1",
    )?;
    let ids = stmt
        .query_map([&brief.id], |row| row.get(0))?
        .collect::<std::result::Result<HashSet<String>, _>>()?;
    Ok(ids)
}

/// The optimization a run of `experiment` belongs to: that of the idea testing
/// the experiment, else of the chat that created it, else of the chat
/// launching the run.
pub(crate) fn for_launch(
    store: &Store,
    experiment: &LocalExperiment,
    launching_chat: Option<&str>,
) -> Result<Option<Brief>> {
    if let Some(id) = ideas::optimization_of_experiment(store, &experiment.id)? {
        if let Some(brief) = get(store, &id)? {
            return Ok(Some(brief));
        }
    }
    for chat in [experiment.chat_session_id.as_deref(), launching_chat]
        .into_iter()
        .flatten()
    {
        let chat = top_level_chat(store, chat);
        if let Some(brief) = for_chat(store, &chat)? {
            if brief.project_id == experiment.project_id {
                return Ok(Some(brief));
            }
        }
    }
    Ok(None)
}

pub(crate) fn is_probe(store: &Store, run_id: &str) -> Result<bool> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    Ok(conn
        .query_row(
            "SELECT 1 FROM vlr_probe_runs WHERE run_id = ?1",
            [run_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Held by the launching command while its launch is under way; drops any
/// probe request the launch didn't use.
pub(crate) struct LaunchRequest {
    experiment_id: Option<String>,
}

impl Drop for LaunchRequest {
    fn drop(&mut self) {
        let Some(experiment_id) = &self.experiment_id else {
            return;
        };
        if let Ok(store) = Store::open() {
            let _ = store.flavor_conn().execute(
                "DELETE FROM vlr_probe_requests WHERE experiment_id = ?1",
                [experiment_id],
            );
        }
    }
}

/// Called by `orx exp run` before it launches, in the agent's (or user's)
/// own process: records a probe request when `VLR_PROBE` is set, and reminds
/// about briefs where the gate would only remind.
pub(crate) fn before_launch(experiment_id: &str) -> LaunchRequest {
    let mut request = LaunchRequest {
        experiment_id: None,
    };
    let Ok(store) = Store::open() else {
        return request;
    };
    if super::env_flag(PROBE_ENV) {
        let recorded = ensure_tables(store.flavor_conn()).and_then(|()| {
            store
                .flavor_conn()
                .execute(
                    "INSERT OR REPLACE INTO vlr_probe_requests (experiment_id, requested_at)
                     VALUES (?1, ?2)",
                    params![experiment_id, crate::store::now_ms()],
                )
                .map_err(Into::into)
        });
        match recorded {
            Ok(_) => request.experiment_id = Some(experiment_id.to_owned()),
            Err(error) => eprintln!("vlresearch: could not record the probe request: {error}"),
        }
        return request;
    }
    if gate_mode(&store) != GateMode::Scoped {
        return request;
    }
    let Ok(Some(experiment)) = store.get_local_experiment(experiment_id) else {
        return request;
    };
    let chat = current_chat(&store);
    if matches!(for_launch(&store, &experiment, chat.as_deref()), Ok(None)) {
        eprintln!(
            "Note: this run isn't part of an optimization, so no brief holds it. For improvement \
             work, agree one with the user first (`orx skill grill`); orx then holds its runs to \
             the budget, the locked files and the scope."
        );
    }
    request
}

/// Takes a fresh probe request for the experiment, if there is one.
fn take_probe_request(store: &Store, experiment_id: &str) -> Result<bool> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let fresh_after = crate::store::now_ms() - PROBE_REQUEST_TTL_MS;
    let taken = conn
        .prepare("DELETE FROM vlr_probe_requests WHERE experiment_id = ?1 RETURNING requested_at")?
        .query_map([experiment_id], |row| row.get::<_, i64>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(taken.into_iter().any(|requested| requested >= fresh_after))
}

/// What the gate decided about a launch.
#[derive(Debug, Clone, Default)]
pub(crate) struct Launch {
    /// A probe: allowed past the gate, recorded as one, never a result.
    pub probe: bool,
    /// The optimization the run belongs to.
    pub optimization_id: Option<String>,
    /// Whether the snapshot still has to pass the locked-path and scope checks.
    pub check_snapshot: bool,
}

/// Called in `compute::submit` before any compute is spent. `Err` refuses the
/// launch with a message that says what to do instead. A forwarded launch
/// (from a chat, through `orx up`) is a probe only if its command asked.
pub(crate) fn check_launch(
    store: &Store,
    project: &LocalProject,
    experiment: &LocalExperiment,
    forwarded: bool,
    launching_chat: Option<&str>,
) -> Result<Launch> {
    let probe =
        take_probe_request(store, &experiment.id)? || (!forwarded && super::env_flag(PROBE_ENV));
    let brief = for_launch(store, experiment, launching_chat)?;
    let mut launch = Launch {
        probe,
        optimization_id: brief.as_ref().map(|brief| brief.id.clone()),
        check_snapshot: false,
    };
    let mode = gate_mode(store);
    if probe || mode == GateMode::Off {
        return Ok(launch);
    }
    let probe_hint = format!(
        "A smoke test or baseline check can run as a probe, which never counts as a result: \
         `VLR_PROBE=1 orx exp run {}`.",
        experiment.id
    );
    let brief = match brief {
        None if mode == GateMode::Scoped => return Ok(launch),
        None => {
            return Err(anyhow!(
                "orx won't spend compute on this run until it belongs to an optimization whose \
                 brief the user agreed. Scope one first: `orx skill grill` walks through the \
                 interview, which ends with `orx brief confirm {}`. {probe_hint}",
                project.id
            ))
        }
        Some(brief) if brief.confirmed_at.is_none() => {
            let missing = missing(store, &brief)?;
            let next_step = if missing.is_empty() {
                format!(
                    "Show it to the user and, once they agree, run `orx brief confirm {} --opt {}`.",
                    project.id, brief.id
                )
            } else {
                format!("It still needs {}.", missing.join("; "))
            };
            return Err(anyhow!(
                "This run belongs to optimization {}, whose brief is not confirmed, so orx won't \
                 spend compute on it yet. {next_step} {probe_hint}",
                brief.id
            ));
        }
        Some(brief) => brief,
    };
    let blockers: Vec<String> = ideas::list(store, &brief.id)?
        .into_iter()
        .filter(|idea| idea.is_blocker() && !idea.status.is_closed())
        .map(|idea| format!("{} ({})", idea.title, idea.id))
        .collect();
    if !blockers.is_empty() {
        return Err(anyhow!(
            "Open blockers come first: {}. Resolve each and close it with `orx idea done <ideaId>`, \
             or `orx idea drop <ideaId>` if it no longer applies. {probe_hint}",
            blockers.join("; ")
        ));
    }
    let used = usage(store, &brief)?;
    if let Some(budget) = brief.budget_runs.filter(|budget| used.runs >= *budget) {
        return Err(anyhow!(
            "The brief's budget of {budget} runs is spent. Report where things stand and ask the \
             user whether to raise it (`orx brief set {} --budget-runs <total>`, then confirm \
             again; the count keeps running).",
            project.id
        ));
    }
    if let Some(budget) = brief.budget_days.filter(|budget| used.days >= *budget) {
        return Err(anyhow!(
            "The brief's time budget of {budget} days is spent. Report where things stand and \
             ask the user whether to extend it."
        ));
    }
    launch.check_snapshot = !brief.locked.is_empty() || !brief.scope.is_empty();
    Ok(launch)
}

/// Called in `compute::submit` once the source snapshot is staged: the commit
/// about to run must leave every locked path as the brief pinned it, and
/// change nothing outside the brief's scope.
pub(crate) fn check_snapshot(
    store: &Store,
    project: &LocalProject,
    launch: &Launch,
    revision: &str,
) -> Result<()> {
    if !launch.check_snapshot {
        return Ok(());
    }
    let Some(brief) = launch
        .optimization_id
        .as_deref()
        .map(|id| get(store, id))
        .transpose()?
        .flatten()
    else {
        return Ok(());
    };
    let repo = Path::new(&project.repo_path);
    for locked in &brief.locked {
        let Some(expected) = &locked.object else {
            continue;
        };
        if object_at(repo, revision, &locked.path)?.as_ref() != Some(expected) {
            return Err(anyhow!(
                "This run's code changes {}, which the brief locks (evaluation code stays fixed \
                 so scores stay comparable). Revert it on the experiment's branch, or ask the \
                 user to change the brief.",
                locked.path
            ));
        }
    }
    if let (false, Some(base)) = (brief.scope.is_empty(), &brief.base_commit) {
        let outside: Vec<String> = changed_paths(repo, base, revision)?
            .into_iter()
            .filter(|path| !within(path, &brief.scope))
            .collect();
        if !outside.is_empty() {
            let shown = outside
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let more = outside.len().saturating_sub(5);
            return Err(anyhow!(
                "This run's code changes {shown}{}, outside what this optimization works on ({}). \
                 Keep the change inside those paths, or ask the user to widen the brief's scope.",
                if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                },
                brief.scope.join(", ")
            ));
        }
    }
    Ok(())
}

/// Bookkeeping once a launch is accepted: the run joins its optimization, and
/// a probe run is recorded as one.
pub(crate) fn after_launch(store: &Store, run_id: &str, launch: &Launch) {
    let conn = store.flavor_conn();
    let recorded = ensure_tables(conn).and_then(|()| {
        if let Some(optimization_id) = &launch.optimization_id {
            conn.execute(
                "INSERT OR REPLACE INTO vlr_optimization_runs (run_id, optimization_id)
                 VALUES (?1, ?2)",
                params![run_id, optimization_id],
            )?;
        }
        if launch.probe {
            conn.execute(
                "INSERT OR IGNORE INTO vlr_probe_runs (run_id) VALUES (?1)",
                [run_id],
            )?;
        }
        Ok(())
    });
    if let Err(error) = recorded {
        eprintln!("vlresearch: could not record run {run_id}: {error}");
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::flavor::metrics::{self, Direction, PrimaryMetric};
    use crate::store::StoredChatSession;

    pub(crate) fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("vlr-scope-{}", uuid::Uuid::new_v4()));
        (Store::open_at(dir.clone()).unwrap(), dir)
    }

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = crate::local::git::git_command()
            .arg("-C")
            .arg(repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn project(repo: &Path) -> LocalProject {
        LocalProject {
            id: "p".into(),
            name: "P".into(),
            slug: "p".into(),
            github_owner: String::new(),
            github_repo: String::new(),
            github_sync_enabled: false,
            baseline_branch: "main".into(),
            repo_path: repo.to_string_lossy().into_owned(),
            run_command: None,
            paper_id: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    pub(crate) fn experiment(id: &str, chat: Option<&str>) -> LocalExperiment {
        LocalExperiment {
            id: id.into(),
            project_id: "p".into(),
            parent_experiment_id: None,
            slug: id.into(),
            branch_name: format!("orx/{id}"),
            title: None,
            description: None,
            run_command: "python train.py".into(),
            agent_status: "idle".into(),
            created_at: 1,
            updated_at: 1,
            chat_session_id: chat.map(str::to_owned),
            archived: false,
        }
    }

    pub(crate) fn chat(id: &str, side_parent: Option<&str>) -> StoredChatSession {
        StoredChatSession {
            id: id.into(),
            project_id: "p".into(),
            harness: "claude-code".into(),
            native_session_id: None,
            title: None,
            title_source: None,
            model: None,
            service_tier: None,
            permission_mode: None,
            plan_mode: false,
            plan_reset_pending: false,
            reasoning_level: None,
            archived: false,
            context_usage_json: None,
            bootstrap_context: None,
            goal: None,
            autonomy: None,
            active_leaf_id: None,
            side_parent_session_id: side_parent.map(str::to_owned),
            created_at: 1,
            updated_at: 1,
        }
    }

    /// An optimization with a chosen id in project `p`, holding every run the
    /// project has when this is called, for tests that name it.
    pub(crate) fn named(store: &Store, id: &str) -> Brief {
        let conn = store.flavor_conn();
        ensure_tables(conn).unwrap();
        conn.execute(
            "INSERT INTO vlr_optimizations (id, project_id, created_at, updated_at)
             VALUES (?1, 'p', 1, 1)",
            [id],
        )
        .unwrap();
        adopt_runs(store, id);
        get(store, id).unwrap().unwrap()
    }

    /// Makes every run in project `p` one of the optimization's, as runs
    /// launched for it are.
    pub(crate) fn adopt_runs(store: &Store, id: &str) {
        store
            .flavor_conn()
            .execute(
                "INSERT OR IGNORE INTO vlr_optimization_runs (run_id, optimization_id)
                 SELECT id, ?1 FROM runs WHERE project_id = 'p'",
                [id],
            )
            .unwrap();
    }

    /// An idea to start the loop from.
    pub(crate) fn seed(store: &Store, brief: &Brief) {
        ideas::add(
            store,
            &brief.id,
            ideas::IdeaFields {
                title: Some("Volume features".into()),
                ..ideas::IdeaFields::default()
            },
        )
        .unwrap();
    }

    pub(crate) fn primary(store: &Store, brief: &Brief, key: &str) {
        metrics::set_primary(
            store,
            &brief.id,
            &PrimaryMetric {
                key: key.into(),
                direction: Direction::Max,
                noise_floor: None,
                split: None,
            },
        )
        .unwrap();
    }

    fn complete(store: &Store, brief: &Brief, scope: &[&str]) -> Brief {
        primary(store, brief, "val_ic");
        seed(store, brief);
        set(
            store,
            &brief.id,
            BriefFields {
                objective: Some("Raise validation IC".into()),
                must_not_change: Some("eval".into()),
                scope: scope.iter().map(|path| (*path).to_owned()).collect(),
                budget_runs: Some(5),
                stop_rule: Some("No gain in 3 runs".into()),
                ..BriefFields::default()
            },
        )
        .unwrap()
    }

    fn repo_with_baseline(dir: &Path) -> std::path::PathBuf {
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join("eval")).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("eval/score.py"), "def score(): ...\n").unwrap();
        std::fs::write(repo.join("eval/splits.yaml"), "valid: 2023\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "baseline"]);
        repo
    }

    #[test]
    fn launches_wait_for_a_confirmed_brief_and_the_snapshot_keeps_locked_paths() {
        let (store, dir) = temp_store();
        let repo = repo_with_baseline(&dir);
        let same = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "-q", "-b", "orx/tampered"]);
        std::fs::write(repo.join("eval/splits.yaml"), "valid: 2022\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "easier split"]);
        let tampered = git(&repo, &["rev-parse", "HEAD"]);
        let project = project(&repo);
        let launch = |forwarded| {
            check_launch(
                &store,
                &project,
                &experiment("e", Some("chat_loop")),
                forwarded,
                None,
            )
        };

        // Outside any optimization, launches go ahead.
        let free = launch(false).unwrap();
        assert!(!free.probe && free.optimization_id.is_none());

        let opt = create(&store, "p", Some("chat_loop")).unwrap();
        set(
            &store,
            &opt.id,
            BriefFields {
                objective: Some("Raise validation IC".into()),
                // A whole folder can be locked.
                lock: vec!["./eval/".into()],
                budget_runs: Some(2),
                stop_rule: Some("No gain in 5 runs".into()),
                ..BriefFields::default()
            },
        )
        .unwrap();
        let refused = launch(true).unwrap_err().to_string();
        assert!(refused.contains("not confirmed"), "{refused}");
        assert!(refused.contains("a primary metric"), "{refused}");
        assert!(refused.contains("at least one idea"), "{refused}");
        assert!(refused.contains("VLR_PROBE=1 orx exp run e"), "{refused}");
        assert!(confirm(&store, &project, &opt.id, "user").is_err());
        primary(&store, &opt, "val_ic");
        // A blocker isn't an idea to start from.
        ideas::add(
            &store,
            &opt.id,
            ideas::IdeaFields {
                title: Some("Check the label join".into()),
                kind: Some(ideas::Kind::EvalBlocker),
                ..ideas::IdeaFields::default()
            },
        )
        .map(|blocker| ideas::close(&store, &blocker.id, ideas::Verdict::Dropped, None))
        .unwrap()
        .unwrap();
        let still = confirm(&store, &project, &opt.id, "user")
            .unwrap_err()
            .to_string();
        assert!(still.contains("at least one idea"), "{still}");
        seed(&store, &opt);
        let confirmed = confirm(&store, &project, &opt.id, "user").unwrap();
        assert_eq!(confirmed.locked[0].path, "eval");
        assert!(confirmed.locked[0].object.is_some());
        assert!(confirmed.budget_since.is_some());

        let accepted = launch(false).unwrap();
        assert_eq!(accepted.optimization_id.as_deref(), Some(opt.id.as_str()));
        check_snapshot(&store, &project, &accepted, &same).unwrap();
        let tampering = check_snapshot(&store, &project, &accepted, &tampered)
            .unwrap_err()
            .to_string();
        assert!(tampering.contains("eval"), "{tampering}");

        // An open blocker stops launches until it is closed.
        let blocker = ideas::add(
            &store,
            &opt.id,
            ideas::IdeaFields {
                title: Some("Validation overlaps training".into()),
                kind: Some(ideas::Kind::EvalBlocker),
                ..ideas::IdeaFields::default()
            },
        )
        .unwrap();
        assert!(launch(false)
            .unwrap_err()
            .to_string()
            .contains("Validation overlaps training"));
        ideas::close(&store, &blocker.id, ideas::Verdict::Dropped, None).unwrap();
        launch(false).unwrap();

        // Changing the brief reopens it.
        set(
            &store,
            &opt.id,
            BriefFields {
                pointers: Some("Start with volume".into()),
                ..BriefFields::default()
            },
        )
        .unwrap();
        assert!(launch(false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_probe_request_from_the_launching_command_reaches_a_forwarded_launch() {
        let (store, dir) = temp_store();
        let opt = create(&store, "p", Some("chat_loop")).unwrap();
        set(
            &store,
            &opt.id,
            BriefFields {
                objective: Some("x".into()),
                ..BriefFields::default()
            },
        )
        .unwrap();
        let project = project(&dir);
        let e = experiment("e", Some("chat_loop"));
        // The brief is unconfirmed, so only a probe gets through.
        assert!(check_launch(&store, &project, &e, true, None).is_err());
        store
            .flavor_conn()
            .execute(
                "INSERT INTO vlr_probe_requests (experiment_id, requested_at) VALUES ('e', ?1)",
                [crate::store::now_ms()],
            )
            .unwrap();
        let launch = check_launch(&store, &project, &e, true, None).unwrap();
        assert!(launch.probe);
        after_launch(&store, "run_probe", &launch);
        assert!(is_probe(&store, "run_probe").unwrap());
        // The request was used up, and a stale one doesn't count.
        assert!(check_launch(&store, &project, &e, true, None).is_err());
        store
            .flavor_conn()
            .execute(
                "INSERT INTO vlr_probe_requests (experiment_id, requested_at) VALUES ('e', ?1)",
                [crate::store::now_ms() - 10 * PROBE_REQUEST_TTL_MS],
            )
            .unwrap();
        assert!(check_launch(&store, &project, &e, true, None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_budget_keeps_counting_across_reconfirmations() {
        let (store, dir) = temp_store();
        let repo = repo_with_baseline(&dir);
        let project = project(&repo);
        let opt = create(&store, "p", None).unwrap();
        complete(&store, &opt, &[]);
        let first = confirm(&store, &project, &opt.id, "user")
            .unwrap()
            .budget_since;
        set(
            &store,
            &opt.id,
            BriefFields {
                budget_runs: Some(2),
                ..BriefFields::default()
            },
        )
        .unwrap();
        let again = confirm(&store, &project, &opt.id, "user").unwrap();
        assert_eq!(again.budget_since, first);
        set(
            &store,
            &opt.id,
            BriefFields {
                reset_budget: true,
                ..BriefFields::default()
            },
        )
        .unwrap();
        assert!(get(&store, &opt.id)
            .unwrap()
            .unwrap()
            .budget_since
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_loop_waits_for_a_confirmed_brief() {
        let (store, dir) = temp_store();
        let repo = repo_with_baseline(&dir);
        let project = project(&repo);
        let opt = create(&store, "p", None).unwrap();
        primary(&store, &opt, "val_ic");
        set(
            &store,
            &opt.id,
            BriefFields {
                objective: Some("Raise validation IC".into()),
                must_not_change: Some("eval".into()),
                budget_runs: Some(5),
                stop_rule: Some("No gain in 3 runs".into()),
                ..BriefFields::default()
            },
        )
        .unwrap();
        let get_opt = || get(&store, &opt.id).unwrap().unwrap();
        let unseeded = require_confirmed(&store, &get_opt())
            .unwrap_err()
            .to_string();
        assert!(unseeded.contains("at least one idea"), "{unseeded}");
        seed(&store, &opt);
        let unconfirmed = require_confirmed(&store, &get_opt())
            .unwrap_err()
            .to_string();
        assert!(unconfirmed.contains("Research view"), "{unconfirmed}");
        confirm(&store, &project, &opt.id, "user").unwrap();
        require_confirmed(&store, &get_opt()).unwrap();

        // The dashboard links the chat it started, and the link survives edits.
        set_chat(&store, &opt.id, "chat_research").unwrap();
        let edited = set(
            &store,
            &opt.id,
            BriefFields {
                clear_budget: true,
                budget_days: Some(7.0),
                ..BriefFields::default()
            },
        )
        .unwrap();
        assert_eq!(edited.chat_session_id.as_deref(), Some("chat_research"));
        assert_eq!((edited.budget_runs, edited.budget_days), (None, Some(7.0)));
        assert!(edited.confirmed_at.is_none());
        assert!(set_chat(&store, "opt_missing", "chat_x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_chat_runs_its_own_optimization() {
        let (store, dir) = temp_store();
        // Outside a chat, with none yet, a reading command asks for one and a
        // writing one starts one.
        let none = resolve(&store, "p", None, Resolve::Existing)
            .unwrap_err()
            .to_string();
        assert!(none.contains("no optimization yet"), "{none}");
        let first = resolve(&store, "p", None, Resolve::OrCreate).unwrap();
        assert_eq!(
            resolve(&store, "p", None, Resolve::Existing).unwrap().id,
            first.id
        );

        let speed = create(&store, "p", Some("chat_speed")).unwrap();
        let several = resolve(&store, "p", None, Resolve::Existing)
            .unwrap_err()
            .to_string();
        assert!(
            several.contains("several") && several.contains(&speed.id),
            "{several}"
        );
        assert_eq!(
            resolve(&store, "p", Some(&speed.id), Resolve::Existing)
                .unwrap()
                .id,
            speed.id
        );
        assert!(resolve(&store, "q", Some(&speed.id), Resolve::Existing).is_err());

        // A chat runs one optimization; handing it another takes the first back.
        assert_eq!(
            for_chat(&store, "chat_speed").unwrap().unwrap().id,
            speed.id
        );
        set_chat(&store, &first.id, "chat_speed").unwrap();
        assert_eq!(
            for_chat(&store, "chat_speed").unwrap().unwrap().id,
            first.id
        );
        assert!(get(&store, &speed.id)
            .unwrap()
            .unwrap()
            .chat_session_id
            .is_none());

        // A side chat stands for the chat it branched from.
        store
            .create_chat_session(&chat("chat_speed", None))
            .unwrap();
        store
            .create_chat_session(&chat("chat_side", Some("chat_speed")))
            .unwrap();
        assert_eq!(top_level_chat(&store, "chat_side"), "chat_speed");
        assert_eq!(top_level_chat(&store, "chat_unknown"), "chat_unknown");

        // Runs belong to the optimization of the idea testing their
        // experiment, else of the chat that made it.
        let idea = ideas::add(
            &store,
            &speed.id,
            ideas::IdeaFields {
                title: Some("Cache features".into()),
                ..ideas::IdeaFields::default()
            },
        )
        .unwrap();
        store
            .flavor_conn()
            .execute(
                "UPDATE vlr_ideas SET experiment_id = 'e_cache' WHERE id = ?1",
                [&idea.id],
            )
            .unwrap();
        let owned = |id: &str, chat: Option<&str>, launching: Option<&str>| {
            for_launch(&store, &experiment(id, chat), launching)
                .unwrap()
                .map(|brief| brief.id)
        };
        assert_eq!(
            owned("e_cache", Some("chat_speed"), None),
            Some(speed.id.clone())
        );
        assert_eq!(
            owned("e_any", Some("chat_side"), None),
            Some(first.id.clone())
        );
        assert_eq!(
            owned("e_any", None, Some("chat_speed")),
            Some(first.id.clone())
        );
        assert_eq!(owned("e_any", Some("chat_other"), None), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_scoped_optimization_refuses_changes_outside_its_paths() {
        let (store, dir) = temp_store();
        let repo = repo_with_baseline(&dir);
        std::fs::create_dir_all(repo.join("features")).unwrap();
        std::fs::write(repo.join("features/volume.py"), "WINDOW = 10\n").unwrap();
        std::fs::write(repo.join("model.py"), "DEPTH = 3\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "code"]);
        let project = project(&repo);
        let opt = create(&store, "p", Some("chat_features")).unwrap();
        complete(&store, &opt, &["features/"]);
        let confirmed = confirm(&store, &project, &opt.id, "user").unwrap();
        assert_eq!(confirmed.scope, ["features"]);
        assert!(confirmed.base_commit.is_some());

        git(&repo, &["checkout", "-q", "-b", "orx/inside"]);
        std::fs::write(repo.join("features/volume.py"), "WINDOW = 20\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "wider window"]);
        let inside = git(&repo, &["rev-parse", "HEAD"]);
        // A child of an accepted step, which also touches the model.
        git(&repo, &["checkout", "-q", "-b", "orx/outside"]);
        std::fs::write(repo.join("model.py"), "DEPTH = 4\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "deeper"]);
        let outside = git(&repo, &["rev-parse", "HEAD"]);

        let launch = check_launch(
            &store,
            &project,
            &experiment("e", Some("chat_features")),
            false,
            None,
        )
        .unwrap();
        check_snapshot(&store, &project, &launch, &inside).unwrap();
        let refused = check_snapshot(&store, &project, &launch, &outside)
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("model.py") && refused.contains("features"),
            "{refused}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_wide_briefs_become_one_optimization_each() {
        let dir = std::env::temp_dir().join(format!("vlr-migrate-{}", uuid::Uuid::new_v4()));
        let store = Store::open_at(dir.clone()).unwrap();
        let conn = store.flavor_conn();
        // The tables as earlier versions left them.
        conn.execute_batch(
            "CREATE TABLE vlr_briefs (
                 project_id TEXT PRIMARY KEY, objective TEXT, may_change TEXT,
                 must_not_change TEXT, locked_json TEXT NOT NULL DEFAULT '[]', budget_runs INTEGER,
                 budget_gpu_hours REAL, budget_days REAL, pointers TEXT, sources TEXT,
                 stop_rule TEXT, confirmed_at INTEGER, confirmed_by TEXT,
                 updated_at INTEGER NOT NULL, budget_since INTEGER, chat_session_id TEXT);
             INSERT INTO vlr_briefs (project_id, objective, budget_runs, confirmed_at,
                 confirmed_by, updated_at, budget_since, chat_session_id)
             VALUES ('p', 'Raise IC', 20, 5, 'user', 5, 5, 'chat_old');
             CREATE TABLE vlr_project_metric (project_id TEXT PRIMARY KEY, key TEXT NOT NULL,
                 direction TEXT NOT NULL, noise_floor REAL, updated_at INTEGER NOT NULL,
                 split TEXT);
             INSERT INTO vlr_project_metric VALUES ('p', 'val_ic', 'max', 0.002, 5, 'val');",
        )
        .unwrap();
        ideas::create_tables(conn).unwrap();
        conn.execute(
            "INSERT INTO vlr_ideas (id, project_id, title, kind, priority, status, author,
                 created_at, updated_at)
             VALUES ('idea_old', 'p', 'Volume', 'science', 2, 'open', 'agent', 5, 5)",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE vlr_ideas SET optimization_id = NULL WHERE id = 'idea_old'",
            [],
        )
        .unwrap();

        let migrated = list(&store, "p").unwrap();
        assert_eq!(migrated.len(), 1);
        let opt = &migrated[0];
        assert_eq!(opt.objective.as_deref(), Some("Raise IC"));
        assert_eq!(opt.budget_runs, Some(20));
        assert_eq!(opt.confirmed_by.as_deref(), Some("user"));
        assert_eq!(opt.chat_session_id.as_deref(), Some("chat_old"));
        let metric = metrics::primary(&store, &opt.id).unwrap().unwrap();
        assert_eq!(
            (
                metric.key.as_str(),
                metric.noise_floor,
                metric.split.as_deref()
            ),
            ("val_ic", Some(0.002), Some("val"))
        );
        assert_eq!(ideas::list(&store, &opt.id).unwrap()[0].id, "idea_old");
        // Once only.
        conn.execute("DELETE FROM vlr_optimizations", []).unwrap();
        assert!(list(&store, "p").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paths_stay_inside_the_repository() {
        for bad in [
            "",
            ".",
            "/etc/passwd",
            "../outside.py",
            "eval/../../x",
            "eval//x",
        ] {
            assert!(clean_path(bad).is_err(), "{bad}");
        }
        assert_eq!(clean_path(" ./eval\\score.py ").unwrap(), "eval/score.py");
        assert_eq!(clean_path("eval/").unwrap(), "eval");
        let scope = ["features".to_owned()];
        assert!(within("features/volume.py", &scope));
        assert!(within("features", &scope));
        assert!(!within("features_old/x.py", &scope));
    }
}
