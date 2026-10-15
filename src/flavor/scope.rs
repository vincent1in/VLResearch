//! The scoping gate. Before compute is spent on improvement work, a brief is
//! agreed with the user: the objective and primary metric, what may and must
//! not change, a budget, and a stop rule. Once a project has a brief, launches
//! are refused until it is confirmed, while an open blocker idea stands, when
//! the snapshot about to run changes a locked file, or once the budget is
//! spent. Projects without a brief launch as upstream does, with a reminder,
//! unless `VLR_SCOPE_GATE=strict`. Probe runs (`VLR_PROBE=1`: smoke tests,
//! baseline reproductions, noise-floor seeds) pass the gate and never count as
//! a best result.
//!
//! Inside a chat, `orx exp run` is forwarded to the `orx up` server, whose
//! environment is not the agent's. So the launching command records a probe
//! request in the store (`before_launch`), and the server's check consumes it.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::ideas;
use crate::error::{anyhow, Result};
use crate::local::model::{LocalExperiment, LocalProject};
use crate::store::Store;

/// `strict` also refuses projects without a brief; `0` turns the gate off.
pub(crate) const GATE_ENV: &str = "VLR_SCOPE_GATE";
/// Set to `1` on a launch to mark it a probe.
pub(crate) const PROBE_ENV: &str = "VLR_PROBE";
/// How long a probe request waits for the launch it was made for.
const PROBE_REQUEST_TTL_MS: i64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateMode {
    Off,
    /// Enforce briefs where a project has one.
    Scoped,
    /// Require a confirmed brief everywhere.
    Strict,
}

fn gate_mode() -> GateMode {
    match std::env::var(GATE_ENV)
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

/// What the user and the agent agreed the work is.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Brief {
    pub project_id: String,
    /// The goal in words; the number that measures it is the project's
    /// primary metric.
    pub objective: Option<String>,
    pub may_change: Option<String>,
    pub must_not_change: Option<String>,
    /// Repository paths (evaluation code, split definitions) that no run may
    /// change, pinned to their git object at confirmation.
    pub locked: Vec<LockedPath>,
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
    pub updated_at: i64,
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
    pub budget_runs: Option<i64>,
    pub budget_gpu_hours: Option<f64>,
    pub budget_days: Option<f64>,
    /// Start the budget over at the next confirmation.
    pub reset_budget: bool,
    pub pointers: Option<String>,
    pub sources: Option<String>,
    pub stop_rule: Option<String>,
}

fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vlr_briefs (
             project_id       TEXT PRIMARY KEY,
             objective        TEXT,
             may_change       TEXT,
             must_not_change  TEXT,
             locked_json      TEXT NOT NULL DEFAULT '[]',
             budget_runs      INTEGER,
             budget_gpu_hours REAL,
             budget_days      REAL,
             pointers         TEXT,
             sources          TEXT,
             stop_rule        TEXT,
             confirmed_at     INTEGER,
             confirmed_by     TEXT,
             updated_at       INTEGER NOT NULL,
             budget_since     INTEGER
         );
         CREATE TABLE IF NOT EXISTS vlr_probe_runs (
             run_id TEXT PRIMARY KEY
         );
         CREATE TABLE IF NOT EXISTS vlr_probe_requests (
             experiment_id TEXT PRIMARY KEY,
             requested_at  INTEGER NOT NULL
         );",
    )?;
    // Briefs written before the budget clock existed.
    let has_budget_since = conn
        .prepare("SELECT 1 FROM pragma_table_info('vlr_briefs') WHERE name = 'budget_since'")?
        .exists([])?;
    if !has_budget_since {
        conn.execute_batch(
            "ALTER TABLE vlr_briefs ADD COLUMN budget_since INTEGER;
             UPDATE vlr_briefs SET budget_since = confirmed_at;",
        )?;
    }
    Ok(())
}

pub(crate) fn get(store: &Store, project_id: &str) -> Result<Option<Brief>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let brief = conn
        .query_row(
            "SELECT project_id, objective, may_change, must_not_change, locked_json, budget_runs,
                    budget_gpu_hours, budget_days, pointers, sources, stop_rule, confirmed_at,
                    confirmed_by, updated_at, budget_since
             FROM vlr_briefs WHERE project_id = ?1",
            [project_id],
            |row| {
                let locked: String = row.get(4)?;
                Ok(Brief {
                    project_id: row.get(0)?,
                    objective: row.get(1)?,
                    may_change: row.get(2)?,
                    must_not_change: row.get(3)?,
                    locked: serde_json::from_str(&locked).unwrap_or_default(),
                    budget_runs: row.get(5)?,
                    budget_gpu_hours: row.get(6)?,
                    budget_days: row.get(7)?,
                    pointers: row.get(8)?,
                    sources: row.get(9)?,
                    stop_rule: row.get(10)?,
                    confirmed_at: row.get(11)?,
                    confirmed_by: row.get(12)?,
                    updated_at: row.get(13)?,
                    budget_since: row.get(14)?,
                })
            },
        )
        .optional()?;
    Ok(brief)
}

/// Normalizes a repository-relative path for locking.
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
            "Lock files or folders by their path from the repository root, without `..`: {path:?}"
        ));
    }
    Ok(trimmed)
}

/// Applies changes to the brief. Any change reopens a confirmed brief, so the
/// user agrees to the brief as it now stands.
pub(crate) fn set(store: &Store, project_id: &str, fields: BriefFields) -> Result<Brief> {
    let mut brief = get(store, project_id)?.unwrap_or(Brief {
        project_id: project_id.to_owned(),
        ..Brief::default()
    });
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
        "INSERT INTO vlr_briefs (project_id, objective, may_change, must_not_change, locked_json,
             budget_runs, budget_gpu_hours, budget_days, pointers, sources, stop_rule,
             confirmed_at, confirmed_by, updated_at, budget_since)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(project_id) DO UPDATE SET objective = excluded.objective,
             may_change = excluded.may_change, must_not_change = excluded.must_not_change,
             locked_json = excluded.locked_json, budget_runs = excluded.budget_runs,
             budget_gpu_hours = excluded.budget_gpu_hours, budget_days = excluded.budget_days,
             pointers = excluded.pointers, sources = excluded.sources,
             stop_rule = excluded.stop_rule, confirmed_at = excluded.confirmed_at,
             confirmed_by = excluded.confirmed_by, updated_at = excluded.updated_at,
             budget_since = excluded.budget_since",
        params![
            brief.project_id,
            brief.objective,
            brief.may_change,
            brief.must_not_change,
            serde_json::to_string(&brief.locked)?,
            brief.budget_runs,
            brief.budget_gpu_hours,
            brief.budget_days,
            brief.pointers,
            brief.sources,
            brief.stop_rule,
            brief.confirmed_at,
            brief.confirmed_by,
            brief.updated_at,
            brief.budget_since,
        ],
    )?;
    Ok(())
}

/// What a brief still lacks before it can be confirmed.
pub(crate) fn missing(store: &Store, brief: &Brief) -> Result<Vec<&'static str>> {
    let mut missing = Vec::new();
    if brief.objective.is_none() {
        missing.push("an objective (--objective)");
    }
    if super::metrics::primary(store, &brief.project_id)?.is_none() {
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

/// Confirms the brief as it stands: every required field is present, and each
/// locked path is pinned to its object on the project's baseline branch. The
/// budget starts counting at the first confirmation.
pub(crate) fn confirm(store: &Store, project: &LocalProject, by: &str) -> Result<Brief> {
    let mut brief = get(store, &project.id)?.ok_or_else(|| {
        anyhow!(
            "Project {} has no brief yet. Fill it with `orx brief set {} ...` first.",
            project.id,
            project.id
        )
    })?;
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
    let now = crate::store::now_ms();
    brief.confirmed_at = Some(now);
    brief.confirmed_by = Some(by.to_owned());
    brief.budget_since.get_or_insert(now);
    save(store, &brief)?;
    Ok(brief)
}

/// What the brief's budget has used so far: non-probe runs launched since the
/// budget started, and their wall-clock hours.
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
        "SELECT COUNT(*), SUM(COALESCE(ended_at, ?3) - created_at) FROM runs
         WHERE project_id = ?1 AND created_at >= ?2
           AND id NOT IN (SELECT run_id FROM vlr_probe_runs)",
        params![brief.project_id, since, now],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(Usage {
        runs,
        run_hours: millis.unwrap_or(0).max(0) as f64 / 3_600_000.0,
        days: (now - since).max(0) as f64 / 86_400_000.0,
    })
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
/// about the brief where the gate would only remind.
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
    if gate_mode() != GateMode::Scoped {
        return request;
    }
    let project = store
        .get_local_experiment(experiment_id)
        .ok()
        .flatten()
        .map(|experiment| experiment.project_id);
    if let Some(project_id) = project {
        if matches!(get(&store, &project_id), Ok(None)) {
            eprintln!(
                "Note: project {project_id} has no brief. For improvement work, agree one with \
                 the user first (`orx skill grill`); orx then holds runs to its budget and locked \
                 files."
            );
        }
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
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Launch {
    /// A probe: allowed past the gate, recorded as one, never a result.
    pub probe: bool,
    /// Whether the snapshot still has to pass the locked-path check.
    check_locked: bool,
}

/// Called in `compute::submit` before any compute is spent. `Err` refuses the
/// launch with a message that says what to do instead. A forwarded launch
/// (from a chat, through `orx up`) is a probe only if its command asked.
pub(crate) fn check_launch(
    store: &Store,
    project: &LocalProject,
    experiment: &LocalExperiment,
    forwarded: bool,
) -> Result<Launch> {
    let probe =
        take_probe_request(store, &experiment.id)? || (!forwarded && super::env_flag(PROBE_ENV));
    let mode = gate_mode();
    if probe || mode == GateMode::Off {
        return Ok(Launch {
            probe,
            check_locked: false,
        });
    }
    let probe_hint = format!(
        "A smoke test or baseline check can run as a probe, which never counts as a result: \
         `VLR_PROBE=1 orx exp run {}`.",
        experiment.id
    );
    let brief = match get(store, &project.id)? {
        None if mode == GateMode::Scoped => return Ok(Launch::default()),
        None => {
            return Err(anyhow!(
                "orx won't spend compute on project {} until its brief is agreed with the user. \
                 Scope it first: `orx skill grill` walks through the interview, which ends with \
                 `orx brief confirm {}`. {probe_hint}",
                project.id,
                project.id
            ))
        }
        Some(brief) if brief.confirmed_at.is_none() => {
            let missing = missing(store, &brief)?;
            let next_step = if missing.is_empty() {
                format!(
                    "Show it to the user and, once they agree, run `orx brief confirm {}`.",
                    project.id
                )
            } else {
                format!("It still needs {}.", missing.join("; "))
            };
            return Err(anyhow!(
                "Project {}'s brief is not confirmed, so orx won't spend compute on it yet. \
                 {next_step} {probe_hint}",
                project.id
            ));
        }
        Some(brief) => brief,
    };
    let blockers: Vec<String> = ideas::list(store, &project.id)?
        .into_iter()
        .filter(|idea| {
            matches!(
                idea.kind,
                ideas::Kind::InfraBlocker | ideas::Kind::EvalBlocker | ideas::Kind::Halt
            ) && !idea.status.is_closed()
        })
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
    Ok(Launch {
        probe: false,
        check_locked: !brief.locked.is_empty(),
    })
}

/// Called in `compute::submit` once the source snapshot is staged: the
/// commit about to run must leave every locked path as the brief pinned it.
pub(crate) fn check_snapshot(
    store: &Store,
    project: &LocalProject,
    launch: &Launch,
    revision: &str,
) -> Result<()> {
    if !launch.check_locked {
        return Ok(());
    }
    let Some(brief) = get(store, &project.id)? else {
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
    Ok(())
}

/// Bookkeeping once a launch is accepted: a probe run is recorded as one.
pub(crate) fn after_launch(store: &Store, run_id: &str, launch: &Launch) {
    if !launch.probe {
        return;
    }
    let recorded = ensure_tables(store.flavor_conn()).and_then(|()| {
        store
            .flavor_conn()
            .execute(
                "INSERT OR IGNORE INTO vlr_probe_runs (run_id) VALUES (?1)",
                [run_id],
            )
            .map_err(Into::into)
    });
    if let Err(error) = recorded {
        eprintln!("vlresearch: could not record run {run_id} as a probe: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flavor::metrics::{self, Direction, PrimaryMetric};

    fn temp_store() -> (Store, std::path::PathBuf) {
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

    fn experiment(id: &str) -> LocalExperiment {
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
            chat_session_id: None,
            archived: false,
        }
    }

    fn primary(store: &Store) {
        metrics::set_primary(
            store,
            "p",
            &PrimaryMetric {
                key: "val_ic".into(),
                direction: Direction::Max,
                noise_floor: None,
                split: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn launches_wait_for_a_confirmed_brief_and_the_snapshot_keeps_locked_paths() {
        let (store, dir) = temp_store();
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join("eval")).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("eval/score.py"), "def score(): ...\n").unwrap();
        std::fs::write(repo.join("eval/splits.yaml"), "valid: 2023\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "baseline"]);
        let same = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "-q", "-b", "orx/tampered"]);
        std::fs::write(repo.join("eval/splits.yaml"), "valid: 2022\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "easier split"]);
        let tampered = git(&repo, &["rev-parse", "HEAD"]);
        let project = project(&repo);
        let launch = |forwarded| check_launch(&store, &project, &experiment("e"), forwarded);

        // Without a brief, launches go ahead.
        assert!(!launch(false).unwrap().probe);

        set(
            &store,
            "p",
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
        assert!(refused.contains("VLR_PROBE=1 orx exp run e"), "{refused}");
        assert!(confirm(&store, &project, "user").is_err());
        primary(&store);
        let confirmed = confirm(&store, &project, "user").unwrap();
        assert_eq!(confirmed.locked[0].path, "eval");
        assert!(confirmed.locked[0].object.is_some());
        assert!(confirmed.budget_since.is_some());

        let accepted = launch(false).unwrap();
        check_snapshot(&store, &project, &accepted, &same).unwrap();
        let tampering = check_snapshot(&store, &project, &accepted, &tampered)
            .unwrap_err()
            .to_string();
        assert!(tampering.contains("eval"), "{tampering}");

        // An open blocker stops launches until it is closed.
        let blocker = ideas::add(
            &store,
            "p",
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
            "p",
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
        set(
            &store,
            "p",
            BriefFields {
                objective: Some("x".into()),
                ..BriefFields::default()
            },
        )
        .unwrap();
        let project = project(&dir);
        // The brief is unconfirmed, so only a probe gets through.
        assert!(check_launch(&store, &project, &experiment("e"), true).is_err());
        store
            .flavor_conn()
            .execute(
                "INSERT INTO vlr_probe_requests (experiment_id, requested_at) VALUES ('e', ?1)",
                [crate::store::now_ms()],
            )
            .unwrap();
        let launch = check_launch(&store, &project, &experiment("e"), true).unwrap();
        assert!(launch.probe);
        after_launch(&store, "run_probe", &launch);
        assert!(is_probe(&store, "run_probe").unwrap());
        // The request was used up, and a stale one doesn't count.
        assert!(check_launch(&store, &project, &experiment("e"), true).is_err());
        store
            .flavor_conn()
            .execute(
                "INSERT INTO vlr_probe_requests (experiment_id, requested_at) VALUES ('e', ?1)",
                [crate::store::now_ms() - 10 * PROBE_REQUEST_TTL_MS],
            )
            .unwrap();
        assert!(check_launch(&store, &project, &experiment("e"), true).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_budget_keeps_counting_across_reconfirmations() {
        let (store, dir) = temp_store();
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "start"]);
        let project = project(&repo);
        primary(&store);
        set(
            &store,
            "p",
            BriefFields {
                objective: Some("x".into()),
                must_not_change: Some("eval".into()),
                budget_runs: Some(1),
                stop_rule: Some("y".into()),
                ..BriefFields::default()
            },
        )
        .unwrap();
        let first = confirm(&store, &project, "user").unwrap().budget_since;
        set(
            &store,
            "p",
            BriefFields {
                budget_runs: Some(2),
                ..BriefFields::default()
            },
        )
        .unwrap();
        let again = confirm(&store, &project, "user").unwrap();
        assert_eq!(again.budget_since, first);
        set(
            &store,
            "p",
            BriefFields {
                reset_budget: true,
                ..BriefFields::default()
            },
        )
        .unwrap();
        assert!(get(&store, "p").unwrap().unwrap().budget_since.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_paths_stay_inside_the_repository() {
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
    }
}
