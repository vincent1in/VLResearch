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

/// The line printed after a launch, when the launching chat will be woken.
pub(crate) fn follow_up(orx: &str, experiment_id: &str, run_id: &str) -> Option<String> {
    (auto_wake_enabled() && crate::local::chat::in_local_session()).then(|| {
        format!(
            "  This chat is woken automatically when the run finishes, so you can end your turn. \
             To check on it: `{orx} logs {run_id}` or `{orx} exp status {experiment_id}`."
        )
    })
}
