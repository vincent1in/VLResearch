//! The scoping gate. Before compute is spent on improvement work, a brief is
//! agreed with the user: the objective and primary metric, what may and must
//! not change, a budget, and a stop rule. Once a project has a brief, launches
//! are refused until it is confirmed, while an open blocker idea stands, when
//! a run would change a locked evaluation file, or once the budget is spent.
//! Projects without a brief launch as upstream does, with a reminder, unless
//! `VLR_SCOPE_GATE=strict`. Probe runs (`VLR_PROBE=1`: smoke tests, baseline
//! reproductions, noise-floor seeds) pass the gate and never count as a best
//! result.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::ideas;
use crate::error::{anyhow, Result};
use crate::local::model::{LocalExperiment, LocalProject};
use crate::store::Store;

/// `strict` also refuses projects without a brief; `0` turns the gate off.
pub(crate) const GATE_ENV: &str = "VLR_SCOPE_GATE";
/// Set to `1` on a launch to mark it a probe.
pub(crate) const PROBE_ENV: &str = "VLR_PROBE";

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

/// Whether this launch was marked as a probe.
pub(crate) fn probe_requested() -> bool {
    super::env_flag(PROBE_ENV)
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
    /// change, with their content hash at confirmation.
    pub locked: Vec<LockedPath>,
    pub budget_runs: Option<i64>,
    pub budget_gpu_hours: Option<f64>,
    pub budget_days: Option<f64>,
    pub pointers: Option<String>,
    pub sources: Option<String>,
    pub stop_rule: Option<String>,
    pub confirmed_at: Option<i64>,
    pub confirmed_by: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub(crate) struct LockedPath {
    pub path: String,
    /// sha256 of the file on the baseline branch when the brief was confirmed.
    pub sha256: Option<String>,
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
             updated_at       INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS vlr_probe_runs (
             run_id TEXT PRIMARY KEY
         );",
    )?;
    Ok(())
}

pub(crate) fn get(store: &Store, project_id: &str) -> Result<Option<Brief>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let brief = conn
        .query_row(
            "SELECT project_id, objective, may_change, must_not_change, locked_json, budget_runs,
                    budget_gpu_hours, budget_days, pointers, sources, stop_rule, confirmed_at,
                    confirmed_by, updated_at
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
                })
            },
        )
        .optional()?;
    Ok(brief)
}

/// Normalizes a repository-relative path for locking.
fn clean_path(path: &str) -> Result<String> {
    let trimmed = path.trim().trim_start_matches("./").replace('\\', "/");
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.split('/').any(|part| part == "..")
    {
        return Err(anyhow!(
            "Lock paths relative to the repository root, without `..`: {path:?}"
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
    for path in fields.unlock {
        let path = clean_path(&path)?;
        brief.locked.retain(|locked| locked.path != path);
    }
    for path in fields.lock {
        let path = clean_path(&path)?;
        if brief.locked.iter().all(|locked| locked.path != path) {
            brief.locked.push(LockedPath { path, sha256: None });
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
             confirmed_at, confirmed_by, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(project_id) DO UPDATE SET objective = excluded.objective,
             may_change = excluded.may_change, must_not_change = excluded.must_not_change,
             locked_json = excluded.locked_json, budget_runs = excluded.budget_runs,
             budget_gpu_hours = excluded.budget_gpu_hours, budget_days = excluded.budget_days,
             pointers = excluded.pointers, sources = excluded.sources,
             stop_rule = excluded.stop_rule, confirmed_at = excluded.confirmed_at,
             confirmed_by = excluded.confirmed_by, updated_at = excluded.updated_at",
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

/// sha256 of a file on a branch, or `None` when it doesn't exist there.
fn file_hash(repo: &Path, branch: &str, path: &str) -> Result<Option<String>> {
    let output = crate::local::git::git_command()
        .arg("-C")
        .arg(repo)
        .args(["show", &format!("{branch}:{path}")])
        .output()
        .map_err(|error| anyhow!("Could not run git to check {path}: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(format!("{:x}", Sha256::digest(&output.stdout))))
}

/// Confirms the brief as it stands: every required field is present, and each
/// locked file is pinned to its content on the project's baseline branch.
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
    for locked in &mut brief.locked {
        locked.sha256 = Some(
            file_hash(repo, &project.baseline_branch, &locked.path)?.ok_or_else(|| {
                anyhow!(
                    "Locked file {} is not on the baseline branch {}.",
                    locked.path,
                    project.baseline_branch
                )
            })?,
        );
    }
    brief.confirmed_at = Some(crate::store::now_ms());
    brief.confirmed_by = Some(by.to_owned());
    save(store, &brief)?;
    Ok(brief)
}

/// What the brief's budget has used so far: non-probe runs launched since it
/// was confirmed, and their wall-clock hours.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Usage {
    pub runs: i64,
    pub run_hours: f64,
    pub days: f64,
}

pub(crate) fn usage(store: &Store, brief: &Brief) -> Result<Usage> {
    let Some(since) = brief.confirmed_at else {
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

pub(crate) fn mark_probe(store: &Store, run_id: &str) -> Result<()> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO vlr_probe_runs (run_id) VALUES (?1)",
        [run_id],
    )?;
    Ok(())
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

/// Called before any compute is spent on a launch. `Err` refuses it with a
/// message that says what to do instead.
pub(crate) fn check_launch(
    store: &Store,
    project: &LocalProject,
    experiment: &LocalExperiment,
) -> Result<()> {
    let mode = gate_mode();
    if mode == GateMode::Off || probe_requested() {
        return Ok(());
    }
    let probe_hint = format!(
        "A smoke test or baseline check can run as a probe, which never counts as a result: \
         `VLR_PROBE=1 orx exp run {}`.",
        experiment.id
    );
    let brief = match get(store, &project.id)? {
        None if mode == GateMode::Scoped => {
            eprintln!(
                "Note: project {} has no brief. For improvement work, agree one with the user \
                 first (`orx skill grill`); orx then holds runs to its budget and locked files.",
                project.id
            );
            return Ok(());
        }
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
            "Open blockers come first: {}. Resolve each and close it with `orx idea done` or \
             `orx idea drop`. {probe_hint}",
            blockers.join("; ")
        ));
    }
    let used = usage(store, &brief)?;
    if brief.budget_runs.is_some_and(|budget| used.runs >= budget) {
        return Err(anyhow!(
            "The brief's budget of {} runs is spent. Report where things stand and ask the user \
             whether to raise it (`orx brief set {} --budget-runs <n>`, then confirm again).",
            brief.budget_runs.unwrap_or_default(),
            project.id
        ));
    }
    if brief.budget_days.is_some_and(|budget| used.days >= budget) {
        return Err(anyhow!(
            "The brief's time budget of {} days is spent. Report where things stand and ask the \
             user whether to extend it.",
            brief.budget_days.unwrap_or_default()
        ));
    }
    let repo = Path::new(&project.repo_path);
    for locked in &brief.locked {
        let Some(expected) = &locked.sha256 else {
            continue;
        };
        let actual = file_hash(repo, &experiment.branch_name, &locked.path)?;
        if actual.as_ref() != Some(expected) {
            return Err(anyhow!(
                "Experiment {} changes {}, which the brief locks (evaluation code stays fixed so \
                 scores stay comparable). Revert that file on the experiment's branch, or ask the \
                 user to change the brief.",
                experiment.id,
                locked.path
            ));
        }
    }
    Ok(())
}

/// Bookkeeping after a launch: a probe run is recorded as one.
pub(crate) fn after_launch(store: &Store, run_id: &str) {
    if probe_requested() {
        if let Err(error) = mark_probe(store, run_id) {
            eprintln!("vlresearch: could not record run {run_id} as a probe: {error}");
        }
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

    fn git(repo: &Path, args: &[&str]) {
        let status = crate::local::git::git_command()
            .arg("-C")
            .arg(repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
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

    fn experiment(branch: &str) -> LocalExperiment {
        LocalExperiment {
            id: "exp_1".into(),
            project_id: "p".into(),
            parent_experiment_id: None,
            slug: "e".into(),
            branch_name: branch.into(),
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

    #[test]
    fn launches_wait_for_a_confirmed_brief_and_respect_locked_files() {
        let (store, dir) = temp_store();
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join("eval")).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("eval/score.py"), "def score(): ...\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "baseline"]);
        git(&repo, &["branch", "orx/same"]);
        git(&repo, &["checkout", "-q", "-b", "orx/tampered"]);
        std::fs::write(repo.join("eval/score.py"), "def score(): return 1.0\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "better score"]);
        let project = project(&repo);

        // Without a brief, launches go ahead (with a reminder).
        check_launch(&store, &project, &experiment("orx/same")).unwrap();

        let brief = set(
            &store,
            "p",
            BriefFields {
                objective: Some("Raise validation IC".into()),
                lock: vec!["./eval/score.py".into()],
                budget_runs: Some(2),
                stop_rule: Some("No gain in 5 runs".into()),
                ..BriefFields::default()
            },
        )
        .unwrap();
        assert_eq!(brief.locked[0].path, "eval/score.py");
        // A brief that exists but isn't confirmed holds launches back.
        let refused = check_launch(&store, &project, &experiment("orx/same"))
            .unwrap_err()
            .to_string();
        assert!(refused.contains("not confirmed"), "{refused}");
        assert!(refused.contains("a primary metric"), "{refused}");
        assert!(
            refused.contains("VLR_PROBE=1 orx exp run exp_1"),
            "{refused}"
        );
        let incomplete = confirm(&store, &project, "user").unwrap_err().to_string();
        assert!(incomplete.contains("a primary metric"), "{incomplete}");
        metrics::set_primary(
            &store,
            "p",
            &PrimaryMetric {
                key: "val_ic".into(),
                direction: Direction::Max,
                noise_floor: None,
            },
        )
        .unwrap();
        let confirmed = confirm(&store, &project, "user").unwrap();
        assert!(confirmed.locked[0].sha256.is_some());

        check_launch(&store, &project, &experiment("orx/same")).unwrap();
        let tampered = check_launch(&store, &project, &experiment("orx/tampered"))
            .unwrap_err()
            .to_string();
        assert!(tampered.contains("eval/score.py"), "{tampered}");

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
        assert!(check_launch(&store, &project, &experiment("orx/same"))
            .unwrap_err()
            .to_string()
            .contains("Validation overlaps training"));
        ideas::close(&store, &blocker.id, ideas::Verdict::Dropped, None).unwrap();
        check_launch(&store, &project, &experiment("orx/same")).unwrap();

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
        assert!(check_launch(&store, &project, &experiment("orx/same")).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_paths_stay_inside_the_repository() {
        for bad in ["", "/etc/passwd", "../outside.py", "eval/../../x"] {
            assert!(clean_path(bad).is_err(), "{bad}");
        }
        assert_eq!(clean_path(" ./eval\\score.py ").unwrap(), "eval/score.py");
    }
}
