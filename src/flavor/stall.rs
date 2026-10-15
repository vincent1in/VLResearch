//! Stall alerts. A run can sit in "running" for hours after it has stopped
//! being useful: hung on a dead worker, diverged into NaNs, or limping after an
//! out-of-memory error. For each live run a chat is waiting on, orx up's run
//! watcher looks at the run's log every half minute and records what it finds
//! here; `chat_host.rs` tells the waiting chats once, and the progress strip
//! shows the alerts.
//!
//! Detection only reads the local log (`run-logs/<id>.log`, which the run's
//! supervisor appends to) and the run row, so it costs no remote calls.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::store::{RunWakeup, Store, StoredRun};

/// Minutes a running job may print nothing before it is flagged; `0` turns
/// stall alerts off.
pub(crate) const STALL_MINUTES_ENV: &str = "VLR_STALL_MINUTES";
const DEFAULT_STALL_MINUTES: i64 = 30;
/// How often the watcher's 3-second tick actually scans.
const SCAN_INTERVAL_MS: i64 = 30_000;
/// How much of the end of a log is searched for NaNs and memory errors.
const TAIL_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum AlertKind {
    /// No output for longer than the stall window.
    Quiet,
    /// A metric was logged as NaN or infinite.
    NonFinite,
    /// The log shows an out-of-memory error.
    OutOfMemory,
}

impl AlertKind {
    const ALL: [AlertKind; 3] = [
        AlertKind::Quiet,
        AlertKind::NonFinite,
        AlertKind::OutOfMemory,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AlertKind::Quiet => "quiet",
            AlertKind::NonFinite => "nonfinite",
            AlertKind::OutOfMemory => "oom",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// What the person sees in the progress strip.
    pub(crate) fn summary(self) -> &'static str {
        match self {
            AlertKind::Quiet => "Possibly stuck: it has gone quiet for a long time",
            AlertKind::NonFinite => "A metric went NaN or infinite",
            AlertKind::OutOfMemory => "Out-of-memory error in the log",
        }
    }
}

/// The stall window in minutes (`VLR_STALL_MINUTES`, default 30; 0 is off).
pub(crate) fn stall_minutes() -> i64 {
    std::env::var(STALL_MINUTES_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|minutes| *minutes >= 0)
        .unwrap_or(DEFAULT_STALL_MINUTES)
}

static LAST_SCAN: AtomicI64 = AtomicI64::new(0);

/// Whether the watcher should scan on this tick (at most every 30 seconds).
pub(crate) fn due(now: i64) -> bool {
    let last = LAST_SCAN.load(Ordering::Relaxed);
    now - last >= SCAN_INTERVAL_MS
        && LAST_SCAN
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}

fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vlr_run_alerts (
             run_id      TEXT NOT NULL,
             kind        TEXT NOT NULL,
             detected_at INTEGER NOT NULL,
             PRIMARY KEY (run_id, kind)
         );
         CREATE TABLE IF NOT EXISTS vlr_run_alert_deliveries (
             run_id          TEXT NOT NULL,
             kind            TEXT NOT NULL,
             chat_session_id TEXT NOT NULL,
             delivered_at    INTEGER NOT NULL,
             PRIMARY KEY (run_id, kind, chat_session_id)
         );",
    )?;
    Ok(())
}

/// What the end of a log shows, in a fixed order.
pub(crate) fn scan_tail(text: &str) -> Vec<AlertKind> {
    let mut found = Vec::new();
    // Progress bars redraw with carriage returns; each redraw is a line.
    let lines = text.split(['\n', '\r']);
    let (mut nonfinite, mut oom) = (false, false);
    for line in lines {
        nonfinite = nonfinite || line_has_nonfinite_metric(line);
        oom = oom || line_has_out_of_memory(line);
    }
    if nonfinite {
        found.push(AlertKind::NonFinite);
    }
    if oom {
        found.push(AlertKind::OutOfMemory);
    }
    found
}

/// Words that name a metric in training and evaluation logs.
const METRIC_WORDS: &[&str] = &[
    "loss",
    "acc",
    "accuracy",
    "auc",
    "f1",
    "rmse",
    "mae",
    "mse",
    "mape",
    "smape",
    "r2",
    "sharpe",
    "ic",
    "ir",
    "score",
    "metric",
    "metrics",
    "objective",
    "reward",
    "return",
    "returns",
    "ppl",
    "perplexity",
    "lr",
    "logits",
    "kl",
    "entropy",
    "nll",
    "bpb",
    "bpc",
    "error",
    "err",
];

/// `loss=nan`, `val_loss: NaN`, `sharpe -inf`: a NaN or infinite value within
/// three words after a metric name. Gradient norms are left out: with mixed
/// precision an infinite gradient just skips a step. `ORX_METRIC` lines are
/// read exactly, whatever their keys are called.
fn line_has_nonfinite_metric(line: &str) -> bool {
    if line.contains(super::metrics::MARKER) {
        return super::metrics::has_non_finite(line);
    }
    let lower = line.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    words.iter().enumerate().any(|(index, word)| {
        matches!(*word, "nan" | "inf" | "infinity")
            && words[index.saturating_sub(3)..index]
                .iter()
                .any(|before| METRIC_WORDS.contains(before))
    })
}

fn line_has_out_of_memory(line: &str) -> bool {
    const MARKERS: &[&str] = &[
        "out of memory",
        "outofmemoryerror",
        "memoryerror",
        "std::bad_alloc",
        "resource_exhausted",
        "oom-kill",
        "oom_kill",
        "oom killer",
    ];
    let lower = line.trim().to_ascii_lowercase();
    // `Killed` alone on a line is how a shell reports a SIGKILLed child, which
    // on a training box is nearly always the kernel's OOM killer.
    lower == "killed" || MARKERS.iter().any(|marker| lower.contains(marker))
}

/// The last `TAIL_BYTES` of a file, as text.
fn read_tail(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// When the run last showed signs of life: its log's last write, or the last
/// change to its row (it starting to run, say), whichever is later.
fn quiet_since(run: &StoredRun, log_modified: Option<i64>) -> i64 {
    log_modified
        .unwrap_or(0)
        .max(run.updated_at)
        .max(run.created_at)
}

/// Whether upstream already reports this run as unmonitored, in which case its
/// silence is expected and upstream's own alert covers it.
fn unmonitored(run: &StoredRun) -> bool {
    crate::jobs::BackendDescriptor::parse(&run.backend_json)
        .is_ok_and(|descriptor| descriptor.monitoring_error.is_some())
}

/// Updates the alerts for the live runs these wake-ups wait on.
pub(crate) fn scan(store: &Store, wakeups: &[RunWakeup], now: i64) -> Result<()> {
    let minutes = stall_minutes();
    if minutes == 0 {
        return Ok(());
    }
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut seen = std::collections::HashSet::new();
    for wakeup in wakeups {
        let run = &wakeup.run;
        if run.status != "running" || !seen.insert(run.id.clone()) {
            continue;
        }
        let log = super::wake::run_log_path(store, &run.id);
        let since = quiet_since(run, super::wake::modified_ms(&log));
        let mut found = Vec::new();
        if now - since >= minutes.saturating_mul(60_000) && !unmonitored(run) {
            found.push(AlertKind::Quiet);
        } else {
            // Output resumed (or never stopped): a past quiet alert no longer
            // holds, and a later silence should be reported afresh.
            clear(conn, &run.id, AlertKind::Quiet, since)?;
        }
        if let Some(tail) = read_tail(&log) {
            found.extend(scan_tail(&tail));
        }
        for kind in found {
            conn.execute(
                "INSERT OR IGNORE INTO vlr_run_alerts (run_id, kind, detected_at)
                 VALUES (?1, ?2, ?3)",
                params![run.id, kind.as_str(), now],
            )?;
        }
    }
    Ok(())
}

/// Drops an alert detected before `before`, with its deliveries.
fn clear(conn: &Connection, run_id: &str, kind: AlertKind, before: i64) -> Result<()> {
    let detected: Option<i64> = conn
        .query_row(
            "SELECT detected_at FROM vlr_run_alerts WHERE run_id = ?1 AND kind = ?2",
            params![run_id, kind.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if detected.is_some_and(|detected| detected < before) {
        conn.execute(
            "DELETE FROM vlr_run_alerts WHERE run_id = ?1 AND kind = ?2",
            params![run_id, kind.as_str()],
        )?;
        conn.execute(
            "DELETE FROM vlr_run_alert_deliveries WHERE run_id = ?1 AND kind = ?2",
            params![run_id, kind.as_str()],
        )?;
    }
    Ok(())
}

/// Alerts on these runs that `session` has not been told about yet.
pub(crate) fn undelivered<'a>(
    store: &Store,
    session: &str,
    runs: impl IntoIterator<Item = &'a StoredRun>,
) -> Result<Vec<(StoredRun, AlertKind)>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT a.kind FROM vlr_run_alerts a
         WHERE a.run_id = ?1 AND NOT EXISTS (
             SELECT 1 FROM vlr_run_alert_deliveries d
             WHERE d.run_id = a.run_id AND d.kind = a.kind AND d.chat_session_id = ?2)
         ORDER BY a.detected_at, a.kind",
    )?;
    let mut pending = Vec::new();
    for run in runs {
        let kinds = stmt
            .query_map(params![run.id, session], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        pending.extend(
            kinds
                .iter()
                .filter_map(|kind| AlertKind::parse(kind))
                .map(|kind| (run.clone(), kind)),
        );
    }
    Ok(pending)
}

pub(crate) fn mark_delivered(
    store: &Store,
    run_id: &str,
    kind: AlertKind,
    session: &str,
    now: i64,
) -> Result<()> {
    store.flavor_conn().execute(
        "INSERT OR IGNORE INTO vlr_run_alert_deliveries
             (run_id, kind, chat_session_id, delivered_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![run_id, kind.as_str(), session, now],
    )?;
    Ok(())
}

/// A run's alerts for the progress endpoint, oldest first.
pub(crate) fn alerts_for_run(store: &Store, run_id: &str) -> Result<Vec<serde_json::Value>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT kind, detected_at FROM vlr_run_alerts WHERE run_id = ?1
         ORDER BY detected_at, kind",
    )?;
    let rows = stmt
        .query_map([run_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(kind, at)| {
            let kind = AlertKind::parse(&kind)?;
            Some(serde_json::json!({
                "kind": kind.as_str(),
                "message": kind.summary(),
                "at": at,
            }))
        })
        .collect())
}

/// The hidden message that tells a waiting chat. Only local identifiers go in:
/// the log is written by the job, so none of it may ride inside an `[orx]`
/// instruction; the agent reads it with `orx logs` as command output instead.
pub(crate) fn agent_text(alerts: &[(StoredRun, AlertKind)], minutes: i64) -> String {
    let lines = alerts
        .iter()
        .map(|(run, kind)| {
            let problem = match kind {
                AlertKind::Quiet => format!("has printed nothing for over {minutes} minutes"),
                AlertKind::NonFinite => {
                    "logged a NaN or infinite metric value, so it has probably diverged".to_string()
                }
                AlertKind::OutOfMemory => {
                    "printed an out-of-memory error, so a worker may have died".to_string()
                }
            };
            format!(
                "- run `{}` of experiment `{}` (still **running**) {problem}",
                run.id, run.experiment_id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "[orx] Stall check on runs you are waiting for:\n{lines}\nLook at each with `orx logs \
         <runId>`. If one is stuck or wasting compute, cancel it with `orx exp cancel <expId>` and \
         tell the user what you found. If it is fine, end your turn: orx still wakes you when it \
         finishes, and won't raise the same alert again."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoredRun;

    fn run(id: &str, status: &str, created_at: i64) -> StoredRun {
        StoredRun {
            id: id.into(),
            experiment_id: "exp_1".into(),
            project_id: "proj_1".into(),
            status: status.into(),
            backend_json: "{}".into(),
            command: "python train.py".into(),
            created_at,
            updated_at: created_at,
            ended_at: None,
            exit_code: None,
            commit_sha: None,
            result_markdown: None,
            cancel_requested: false,
            cancel_reason: None,
            chat_session_id: Some("chat_A".into()),
        }
    }

    fn wakeup(run: StoredRun) -> RunWakeup {
        RunWakeup {
            run,
            chat_session_id: "chat_A".into(),
            state: "pending".into(),
            monitoring_alerted: false,
        }
    }

    #[test]
    fn nonfinite_metrics_are_found_in_common_log_formats() {
        for line in [
            "epoch 3 | loss=nan | acc=0.51",
            "step 1200 val_loss: NaN",
            "{\"sharpe\": Infinity, \"turnover\": 0.3}",
            "ORX_METRIC {\"ic\": -inf}",
            "ORX_METRIC {\"sharpe_ratio_annualized_net\": NaN}",
        ] {
            assert_eq!(scan_tail(line), [AlertKind::NonFinite], "{line}");
        }
        for line in [
            "Running inference on 10 batches",
            "loss=0.42 acc=0.91",
            "nan values dropped from the price table",
            "Info: banana count 3",
            // Mixed precision skips a step when gradients overflow.
            "train/grad_norm tensor(inf, device='cuda:0')",
            "ORX_METRIC {\"loss\": 0.4, \"tag\": \"nan\"}",
        ] {
            assert!(scan_tail(line).is_empty(), "{line}");
        }
    }

    #[test]
    fn out_of_memory_errors_are_found() {
        for line in [
            "torch.OutOfMemoryError: CUDA out of memory. Tried to allocate 2.00 GiB",
            "RuntimeError: CUDA error: out of memory",
            "MemoryError",
            "Killed",
            "terminate called after throwing an instance of 'std::bad_alloc'",
            "RESOURCE_EXHAUSTED: Out of memory while trying to allocate",
        ] {
            assert_eq!(scan_tail(line), [AlertKind::OutOfMemory], "{line}");
        }
        assert!(scan_tail("Process killed the old checkpoint writer").is_empty());
        assert_eq!(
            scan_tail("loss=nan\r100%|#####| 10/10\nKilled\n"),
            [AlertKind::NonFinite, AlertKind::OutOfMemory]
        );
    }

    #[test]
    fn quiet_runs_are_flagged_once_and_cleared_when_output_resumes() {
        let dir = std::env::temp_dir().join(format!("vlr-stall-{}", uuid::Uuid::new_v4()));
        let store = Store::open_at(dir.clone()).unwrap();
        let minute = 60_000;
        let now = 1_000 * minute;
        let quiet = run("run_quiet", "running", now - 45 * minute);
        let queued = run("run_queued", "starting", now - 45 * minute);
        let wakeups = [wakeup(quiet.clone()), wakeup(queued)];

        scan(&store, &wakeups, now).unwrap();
        let alerts = alerts_for_run(&store, "run_quiet").unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0]["kind"], "quiet");
        // A job still waiting for a machine is not stalled.
        assert!(alerts_for_run(&store, "run_queued").unwrap().is_empty());

        let pending = undelivered(&store, "chat_A", [&quiet]).unwrap();
        assert_eq!(pending.len(), 1);
        mark_delivered(&store, "run_quiet", AlertKind::Quiet, "chat_A", now).unwrap();
        assert!(undelivered(&store, "chat_A", [&quiet]).unwrap().is_empty());
        // Another chat waiting on the same run still hears about it.
        assert_eq!(undelivered(&store, "chat_B", [&quiet]).unwrap().len(), 1);

        // The job prints again: the alert lifts, so a later silence is news.
        let log = super::super::wake::run_log_path(&store, "run_quiet");
        std::fs::write(&log, "epoch 4 loss=0.40\n").unwrap();
        let later = crate::store::now_ms();
        scan(&store, &wakeups, later).unwrap();
        assert!(alerts_for_run(&store, "run_quiet").unwrap().is_empty());
        assert!(undelivered(&store, "chat_A", [&quiet]).unwrap().is_empty());

        // A diverged metric is recorded even while output flows.
        std::fs::write(&log, "epoch 5 loss=nan\n").unwrap();
        scan(&store, &wakeups, crate::store::now_ms()).unwrap();
        let kinds: Vec<_> = alerts_for_run(&store, "run_quiet")
            .unwrap()
            .iter()
            .map(|alert| alert["kind"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(kinds, ["nonfinite"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_agent_message_names_runs_but_quotes_no_log_text() {
        let text = agent_text(
            &[
                (run("run_1", "running", 0), AlertKind::Quiet),
                (run("run_2", "running", 0), AlertKind::NonFinite),
            ],
            30,
        );
        assert!(text.starts_with("[orx] "));
        assert!(text.contains("run `run_1` of experiment `exp_1` (still **running**) has printed nothing for over 30 minutes"));
        assert!(text.contains("run `run_2`"));
        assert!(text.contains("orx exp cancel <expId>"));
    }
}
