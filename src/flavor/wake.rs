//! Run wake-ups. Upstream resumes a chat after a run only if the agent
//! remembered `orx exp wake`; VLResearch registers the wake-up whenever a chat
//! launches a run, so an agent that ends its turn always hears back.
//!
//! Under Claude Code, long `orx exp wait` calls are also cut short: Claude
//! Code moves a Bash command to the background once it passes its timeout
//! (2 minutes by default), after which orx loses what the agent says next.

use crate::error::{anyhow, Result};
use crate::store::Store;

/// Set to `0` to keep upstream's opt-in wake-ups.
pub(crate) const AUTO_WAKE_ENV: &str = "VLR_AUTO_WAKE";

/// Under Claude Code: return before its default 120 s Bash timeout.
const CLAUDE_WAIT_SECS: u64 = 100;
/// Upstream's default for every other caller.
const DEFAULT_WAIT_SECS: u64 = 1800;

fn auto_wake_enabled() -> bool {
    std::env::var(AUTO_WAKE_ENV).map_or(true, |value| value.trim() != "0")
}

fn in_claude_chat() -> bool {
    crate::local::chat::in_local_session()
        && crate::local::chat::launching_chat_harness().as_deref() == Some("claude-code")
}

/// Registers a wake-up for a run a chat just launched. Never fails the launch:
/// a problem here is reported and the run goes on.
pub(crate) fn register_on_submit(store: &Store, run_id: &str, chat_session_id: Option<&str>) {
    let Some(session) = chat_session_id else {
        return;
    };
    if !auto_wake_enabled() {
        return;
    }
    let registered = store.get_chat_session(session).and_then(|chat| match chat {
        Some(_) => store.register_run_wakeup(run_id, session).map(|_| ()),
        None => Ok(()),
    });
    if let Err(error) = registered {
        eprintln!("vlresearch: could not schedule a wake-up for run {run_id}: {error}");
    }
}

/// `orx exp wait` showed this chat the finished run, so drop its pending
/// wake-up rather than waking the chat a second time for the same result.
pub(crate) fn consume(store: &Store, run_id: &str) {
    if !crate::local::chat::in_local_session() {
        return;
    }
    let Some(session) = crate::local::chat::launching_chat_session() else {
        return;
    };
    if let Err(error) = store.flavor_conn().execute(
        "UPDATE chat_run_wakeups SET state = 'delivered', delivered_at = ?3
         WHERE run_id = ?1 AND chat_session_id = ?2 AND state = 'pending'",
        rusqlite::params![run_id, session, crate::store::now_ms()],
    ) {
        eprintln!("vlresearch: could not settle the wake-up for run {run_id}: {error}");
    }
}

/// Default `orx exp wait` timeout in seconds.
pub(crate) fn default_wait_secs() -> u64 {
    if in_claude_chat() {
        CLAUDE_WAIT_SECS
    } else {
        DEFAULT_WAIT_SECS
    }
}

/// Turns a timed-out `orx exp wait` in a chat into a pointer to its wake-up:
/// the runs it was waiting on get wake-ups (if they had none), and the agent
/// is told to end its turn instead of waiting again.
pub(crate) fn after_wait(
    result: Result<()>,
    timed_out: bool,
    exp_id: Option<&str>,
    project_id: Option<&str>,
) -> Result<()> {
    let Err(error) = result else {
        return Ok(());
    };
    if !timed_out || !auto_wake_enabled() || !crate::local::chat::in_local_session() {
        return Err(error);
    }
    let Some(session) = crate::local::chat::launching_chat_session() else {
        return Err(error);
    };
    let registered = Store::open().and_then(|store| {
        if store.get_chat_session(&session)?.is_none() {
            return Ok(0);
        }
        let mut runs = Vec::new();
        if let Some(exp_id) = exp_id {
            runs.extend(store.latest_run_for_experiment(exp_id)?);
        }
        if let Some(project_id) = project_id {
            runs.extend(store.list_runs_by_project(project_id)?);
        }
        let mut count = 0;
        for run in runs
            .iter()
            .filter(|run| !crate::local::is_terminal(&run.status))
        {
            store.register_run_wakeup(&run.id, &session)?;
            count += 1;
        }
        Ok(count)
    });
    match registered {
        Ok(count) if count > 0 => Err(anyhow!(
            "{error}\nThe run is still going and this chat has a wake-up for it. End your turn \
             now: orx will resume this chat when it finishes, so there is no need to wait again."
        )),
        _ => Err(error),
    }
}

/// Runs this chat is waiting on: pending wake-ups whose run is still live,
/// each with how long comparable runs usually take, for the progress strip.
pub(crate) fn waiting_runs(store: &Store, chat_session_id: &str) -> Result<Vec<serde_json::Value>> {
    let conn = store.flavor_conn();
    let mut stmt = conn.prepare(
        "SELECT r.id, r.experiment_id, r.project_id, r.command, r.status, r.created_at,
                e.slug, e.title
         FROM chat_run_wakeups w
         JOIN runs r ON r.id = w.run_id
         LEFT JOIN local_experiments e ON e.id = r.experiment_id
         WHERE w.chat_session_id = ?1 AND w.state = 'pending'
           AND r.status IN ('starting', 'running')
         ORDER BY r.created_at, r.id",
    )?;
    let rows = stmt
        .query_map([chat_session_id], |row| {
            Ok(WaitingRow {
                run_id: row.get(0)?,
                experiment_id: row.get(1)?,
                project_id: row.get(2)?,
                command: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                slug: row.get(6)?,
                title: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|row| {
            let typical = typical_duration_ms(store, &row)?;
            let last_output = modified_ms(&run_log_path(store, &row.run_id));
            let alerts = super::stall::alerts_for_run(store, &row.run_id)?;
            let experiment = row
                .title
                .filter(|title| !title.trim().is_empty())
                .or(row.slug);
            Ok(serde_json::json!({
                "runId": row.run_id,
                "experimentId": row.experiment_id,
                "experiment": experiment,
                "status": row.status,
                "startedAt": row.created_at,
                "typicalMs": typical,
                "lastOutputAt": last_output,
                "alerts": alerts,
            }))
        })
        .collect()
}

/// The run's log file (`run-logs/<id>.log`) beside this store's database.
pub(crate) fn run_log_path(store: &Store, run_id: &str) -> std::path::PathBuf {
    let default = crate::store::log_path(run_id);
    let dir = store
        .flavor_conn()
        .path()
        .filter(|path| !path.is_empty())
        .and_then(|path| std::path::Path::new(path).parent());
    match (dir, default.file_name()) {
        (Some(dir), Some(name)) => dir.join("run-logs").join(name),
        _ => default,
    }
}

/// Unix millis of the file's last modification, if it exists.
pub(crate) fn modified_ms(path: &std::path::Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}

struct WaitingRow {
    run_id: String,
    experiment_id: String,
    project_id: String,
    command: String,
    status: String,
    created_at: i64,
    slug: Option<String>,
    title: Option<String>,
}

/// Median wall time of earlier successful runs of the same experiment, or else
/// of the project's runs with the same command, once there are three of them.
fn typical_duration_ms(store: &Store, row: &WaitingRow) -> Result<Option<i64>> {
    const MIN_SAMPLES: usize = 3;
    let durations = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Result<Vec<i64>> {
        let mut stmt = store.flavor_conn().prepare(sql)?;
        let values = stmt
            .query_map(params, |r| r.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(values)
    };
    let mut samples = durations(
        "SELECT ended_at - created_at FROM runs
         WHERE experiment_id = ?1 AND status = 'done' AND ended_at > created_at
         ORDER BY 1",
        &[&row.experiment_id],
    )?;
    if samples.len() < MIN_SAMPLES && !row.command.trim().is_empty() {
        samples = durations(
            "SELECT ended_at - created_at FROM runs
             WHERE project_id = ?1 AND command = ?2 AND status = 'done' AND ended_at > created_at
             ORDER BY 1",
            &[&row.project_id, &row.command],
        )?;
    }
    Ok((samples.len() >= MIN_SAMPLES).then(|| samples[samples.len() / 2]))
}

/// The line printed after a launch, when the launching chat will be woken.
pub(crate) fn follow_up(orx: &str, experiment_id: &str, run_id: &str) -> Option<String> {
    (auto_wake_enabled() && crate::local::chat::in_local_session()).then(|| {
        format!(
            "  This chat is woken automatically when the run finishes, so you can end your turn. \
             To check on it: `{orx} logs {run_id}` or `{orx} exp status {experiment_id}`."
        )
    })
}

#[cfg(test)]
mod tests {
    use super::waiting_runs;
    use crate::local::model::LocalExperiment;
    use crate::store::{Store, StoredRun};

    fn run(
        id: &str,
        experiment: &str,
        status: &str,
        created: i64,
        ended: Option<i64>,
    ) -> StoredRun {
        StoredRun {
            id: id.into(),
            experiment_id: experiment.into(),
            project_id: "proj_1".into(),
            status: status.into(),
            backend_json: "{}".into(),
            command: "python train.py".into(),
            created_at: created,
            updated_at: created,
            ended_at: ended,
            exit_code: ended.map(|_| 0),
            commit_sha: None,
            result_markdown: None,
            cancel_requested: false,
            cancel_reason: None,
            chat_session_id: Some("chat_A".into()),
        }
    }

    #[test]
    fn waiting_runs_lists_live_wakeups_with_a_typical_duration() {
        let dir = std::env::temp_dir().join(format!("vlr-wake-{}", uuid::Uuid::new_v4()));
        let store = Store::open_at(dir.clone()).unwrap();
        store
            .create_local_experiment(&LocalExperiment {
                id: "exp_1".into(),
                project_id: "proj_1".into(),
                parent_experiment_id: None,
                slug: "volume-features".into(),
                branch_name: "orx/volume-features".into(),
                title: Some("Volume features, 20-day window".into()),
                description: None,
                run_command: "python train.py".into(),
                agent_status: "idle".into(),
                created_at: 1,
                updated_at: 1,
                chat_session_id: None,
                archived: false,
            })
            .unwrap();
        // Three finished runs of 10, 20 and 40 minutes: the median is 20.
        for (id, minutes) in [("done_1", 10), ("done_2", 20), ("done_3", 40)] {
            store
                .upsert_run(&run(
                    id,
                    "exp_1",
                    "done",
                    1_000,
                    Some(1_000 + minutes * 60_000),
                ))
                .unwrap();
        }
        store
            .upsert_run(&run("live", "exp_1", "running", 5_000, None))
            .unwrap();
        store
            .upsert_run(&run("other_exp", "exp_2", "starting", 6_000, None))
            .unwrap();
        store
            .upsert_run(&run("finished", "exp_1", "done", 7_000, Some(8_000)))
            .unwrap();
        for id in ["live", "other_exp", "finished"] {
            store.register_run_wakeup(id, "chat_A").unwrap();
        }
        store.register_run_wakeup("live", "chat_B").unwrap();

        // The live run has written output; the other has not yet.
        let log = super::run_log_path(&store, "live");
        assert_eq!(log, dir.join("run-logs").join("live.log"));
        std::fs::write(&log, "epoch 1 loss 0.42\n").unwrap();

        let waiting = waiting_runs(&store, "chat_A").unwrap();
        let ids: Vec<_> = waiting
            .iter()
            .map(|w| w["runId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["live", "other_exp"]);
        assert_eq!(waiting[0]["experiment"], "Volume features, 20-day window");
        assert_eq!(waiting[0]["typicalMs"], 20 * 60_000);
        assert_eq!(waiting[0]["startedAt"], 5_000);
        assert!(waiting[0]["lastOutputAt"].as_i64().unwrap() > 0);
        assert!(waiting[1]["lastOutputAt"].is_null());
        // exp_2 has no history of its own, but the project's runs with the same
        // command do.
        assert_eq!(waiting[1]["typicalMs"], 20 * 60_000);
        assert!(waiting[1]["experiment"].is_null());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
