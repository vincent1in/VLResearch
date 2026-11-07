//! VLResearch glue that needs the chat host's private turn machinery. It is
//! compiled as a child of `local::chat` (the `#[path]` module declaration
//! there), which lets it start hidden turns the way upstream's run notices
//! do, while the file itself stays with the rest of the flavor.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex};

use super::{
    AnnotatedText, ChatHost, SendTurnRequest, TranscriptDisplay, TurnAdmission, TurnAttachments,
    TurnGuard, TurnOverrides, TurnState, TurnSubmission,
};
use crate::flavor::stall;
use crate::store::{Store, StoredRun};

/// A chat whose turns fail to start (its project folder is gone, say) is not
/// retried forever: after this many failed attempts its alerts count as told,
/// and the progress strip is left to show them.
const MAX_FAILED_DELIVERIES: u32 = 3;

static FAILED_DELIVERIES: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(Default::default);

/// Records a failed attempt; true once the chat has failed too often.
fn give_up_after_failure(session_id: &str) -> bool {
    let Ok(mut failures) = FAILED_DELIVERIES.lock() else {
        return true;
    };
    let count = failures.entry(session_id.to_owned()).or_default();
    *count += 1;
    if *count >= MAX_FAILED_DELIVERIES {
        failures.remove(session_id);
        return true;
    }
    false
}

fn forget_failures(session_id: &str) {
    if let Ok(mut failures) = FAILED_DELIVERIES.lock() {
        failures.remove(session_id);
    }
}

/// Called on every run-watcher tick: records stall alerts for the live runs
/// chats are waiting on, and tells each idle waiting chat about new ones. A
/// busy chat is told once its turn ends; the progress strip shows the alert to
/// the user straight away either way.
pub(super) async fn process_stall_alerts(chat: &Arc<ChatHost>) {
    let now = crate::store::now_ms();
    if !stall::due(now) {
        return;
    }
    let Ok(store) = Store::open() else {
        return;
    };
    if stall::stall_minutes(&store) == 0 {
        return;
    }
    if let Err(err) = deliver_stall_alerts(chat, store, now).await {
        eprintln!("vlresearch: stall check: {err}");
    }
}

async fn deliver_stall_alerts(
    chat: &Arc<ChatHost>,
    store: Store,
    now: i64,
) -> crate::error::Result<()> {
    let watched = store.chat_active_runs()?;
    stall::scan(&store, &watched, now)?;
    let mut runs_by_session: BTreeMap<String, Vec<StoredRun>> = BTreeMap::new();
    for (session_id, run) in watched {
        runs_by_session.entry(session_id).or_default().push(run);
    }
    for (session_id, runs) in runs_by_session {
        let pending = stall::undelivered(&store, &session_id, &runs)?;
        if pending.is_empty() {
            continue;
        }
        let Some(guard) =
            TurnGuard::claim_idle(chat, &session_id, TurnState::Reserved { turn_id: None }).await
        else {
            continue;
        };
        let text = stall::agent_text(&pending, stall::stall_minutes(&store));
        let delivered = match chat
            .send_message_showing(&session_id, hidden_turn(text, guard))
            .await
        {
            Ok(TurnSubmission::Started(_)) => {
                forget_failures(&session_id);
                true
            }
            // Not started (the chat got busy first): try again next scan.
            Ok(_) => false,
            Err(err) => {
                eprintln!(
                    "vlresearch: could not tell chat {session_id} about a stalled run: {err}"
                );
                give_up_after_failure(&session_id)
            }
        };
        if delivered {
            for (run, kind) in &pending {
                stall::mark_delivered(&store, &run.id, *kind, &session_id, now)?;
            }
        }
    }
    Ok(())
}

/// A turn the agent sees and the transcript doesn't.
fn hidden_turn(text: String, guard: TurnGuard) -> SendTurnRequest {
    SendTurnRequest {
        messages: vec![AnnotatedText {
            text,
            annotations: Vec::new(),
        }],
        prepared_input: None,
        replace_settings: false,
        transcript: TranscriptDisplay {
            text: Some(String::new()),
            annotations: None,
            orx: Vec::new(),
            record_user_message: false,
        },
        overrides: TurnOverrides::default(),
        images: TurnAttachments::Uploaded(Vec::new()),
        admission: TurnAdmission::Preclaimed(guard),
        client_turn_id: None,
        request_hash: None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{ChatHost, TurnState};
    use super::{deliver_stall_alerts, forget_failures, give_up_after_failure};
    use crate::flavor::stall;
    use crate::store::{Store, StoredRun};

    #[test]
    fn a_chat_that_cannot_start_turns_is_given_up_on_after_three_tries() {
        let session = format!("chat_{}", uuid::Uuid::new_v4());
        assert!(!give_up_after_failure(&session));
        assert!(!give_up_after_failure(&session));
        assert!(give_up_after_failure(&session));
        // The count starts over afterwards, and a success clears it.
        assert!(!give_up_after_failure(&session));
        forget_failures(&session);
        assert!(!give_up_after_failure(&session));
        assert!(!give_up_after_failure(&session));
    }

    #[tokio::test]
    async fn a_busy_chat_keeps_its_stall_alert_until_it_is_free() {
        let dir = std::env::temp_dir().join(format!("vlr-stall-host-{}", uuid::Uuid::new_v4()));
        let store = Store::open_at(dir.clone()).unwrap();
        let now = crate::store::now_ms();
        let started = now - 45 * 60_000;
        let run = StoredRun {
            id: "run_quiet".into(),
            experiment_id: "exp_1".into(),
            project_id: "proj_1".into(),
            status: "running".into(),
            backend_json: "{}".into(),
            command: "python train.py".into(),
            created_at: started,
            updated_at: started,
            ended_at: None,
            exit_code: None,
            commit_sha: None,
            result_markdown: None,
            cancel_requested: false,
            cancel_reason: None,
            chat_session_id: Some("chat_A".into()),
        };
        store.upsert_run(&run).unwrap();
        let host = Arc::new(ChatHost::new(
            Arc::new(crate::local::opencode::AgentHost::new(None)),
            Arc::new(crate::local::codex::CodexHost::new()),
            Arc::new(crate::local::claude::ClaudeHost::new()),
        ));
        host.turns
            .lock()
            .await
            .insert("chat_A".into(), TurnState::Draining);

        deliver_stall_alerts(&host, Store::open_at(dir.clone()).unwrap(), now)
            .await
            .unwrap();

        // Recorded for the strip, not yet told to the busy chat.
        let pending = stall::undelivered(&store, "chat_A", [&run]).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1, stall::AlertKind::Quiet);
        assert_eq!(stall::alerts_for_run(&store, "run_quiet").unwrap().len(), 1);

        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
