//! Autocompaction. When a chat's context window is nearly full, orx compacts
//! the chat as soon as the turn that filled it ends and nothing else is
//! waiting to run, instead of leaving the agent to run out of room in the
//! middle of a task. That matters most for harnesses that never compact on
//! their own: OpenResearch turns OpenCode's compaction off, and Cursor has
//! none. Claude Code and Codex also compact mid-turn when they must; this
//! compacts at a quiet moment, before they have to.
//!
//! The compaction is OpenResearch's own (`ChatHost::compact_session`): the
//! harness's native compaction where it has one, otherwise a summary that the
//! next turn is reseeded with. It shows in the chat as a manual one does and
//! runs as a turn. Stop cancels it, and messages sent meanwhile wait for it.

use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::error::Result;
use crate::local::chat::{ChatHost, ContextUsage};
use crate::store::Store;

/// The share of the context window at which a chat is compacted.
pub(crate) const AUTO_COMPACT_ENV: &str = "VLR_AUTO_COMPACT";
const DEFAULT_THRESHOLD: f64 = 0.8;
/// Turns that must run after an automatic compaction before the next one. If
/// a single turn refills the window, compacting after every turn would only
/// double what each turn costs.
const MIN_TURNS_BETWEEN: i64 = 3;
/// How long a chat is left alone after its compaction failed or was stopped.
const RETRY_AFTER_MS: i64 = 30 * 60_000;

/// The threshold from `VLR_AUTO_COMPACT`, or `None` when autocompaction is
/// off.
pub(crate) fn threshold() -> Option<f64> {
    parse_threshold(std::env::var(AUTO_COMPACT_ENV).ok().as_deref())
}

/// A fraction (`0.8`) or a percentage (`80`, `80%`) of the context window;
/// `0` or `off` turns autocompaction off. Unset, `on`, or anything else gives
/// the default, 80%.
fn parse_threshold(value: Option<&str>) -> Option<f64> {
    let value = value.unwrap_or_default().trim().to_ascii_lowercase();
    if matches!(value.as_str(), "off" | "false" | "no") {
        return None;
    }
    let (number, percent) = match value.strip_suffix('%') {
        Some(number) => (number.trim(), true),
        None => (value.as_str(), false),
    };
    let number = number.parse::<f64>().ok();
    if number == Some(0.0) {
        return None;
    }
    let fraction = number.map(|n| if percent || n > 1.0 { n / 100.0 } else { n });
    Some(
        fraction
            .filter(|fraction| *fraction > 0.0 && *fraction < 1.0)
            .unwrap_or(DEFAULT_THRESHOLD),
    )
}

/// Called after a chat turn has ended and any queued message has started.
/// Compacts the chat if its context is at least the threshold full, it is
/// idle, and the turn didn't end on a card waiting for the user's answer.
pub(crate) async fn after_turn(host: &Arc<ChatHost>, session_id: &str, message_id: &str) {
    let Some(threshold) = threshold() else {
        return;
    };
    let now = crate::store::now_ms();
    let due =
        match Store::open().and_then(|store| due(&store, session_id, message_id, threshold, now)) {
            Ok(Some(due)) => due,
            Ok(None) => return,
            Err(err) => {
                eprintln!("vlresearch: autocompaction check for chat {session_id}: {err}");
                return;
            }
        };
    let compacted = host.compact_session(session_id).await;
    let (marker_id, error) = match compacted {
        Ok(marker) => (Some(marker.id), None),
        Err(err) => {
            // A queued message, a wake-up or the user got there first; the end
            // of that turn decides again. (The turn may already be over by
            // the time the slot is checked, hence the refusal's wording too.)
            if host.is_busy(session_id).await || err.to_string().contains("busy") {
                return;
            }
            eprintln!("vlresearch: could not compact chat {session_id}: {err}");
            (None, Some(err.to_string()))
        }
    };
    let recorded = Store::open().and_then(|store| {
        record(
            &store,
            session_id,
            &due,
            now,
            marker_id.as_deref(),
            error.as_deref(),
        )
    });
    if let Err(err) = recorded {
        eprintln!("vlresearch: could not record compacting chat {session_id}: {err}");
    }
}

/// How full a chat's context was when it was chosen for compaction.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Due {
    used_tokens: u64,
    context_window: u64,
}

/// Whether the chat should be compacted now. Not when it is a delegated
/// helper, whose parent reads its last reply; not when the turn that just
/// ended left a plan, question or permission card for the user, since
/// answering it starts a turn that a running compaction would refuse; not when
/// the harness hasn't said how large the window is; and not while an earlier
/// automatic compaction holds it back.
fn due(
    store: &Store,
    session_id: &str,
    message_id: &str,
    threshold: f64,
    now: i64,
) -> Result<Option<Due>> {
    let Some(session) = store.get_chat_session(session_id)? else {
        return Ok(None);
    };
    if session.parent_session_id.is_some() {
        return Ok(None);
    }
    let Some(usage) = session
        .context_usage_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<ContextUsage>(json).ok())
    else {
        return Ok(None);
    };
    let Some(window) = usage.context_window.filter(|window| *window > 0) else {
        return Ok(None);
    };
    if (usage.used_tokens as f64) < threshold * window as f64 {
        return Ok(None);
    }
    if awaits_answer(store, message_id)? {
        return Ok(None);
    }
    if previous(store, session_id)?.is_some_and(|previous| previous.holds_back(now)) {
        return Ok(None);
    }
    Ok(Some(Due {
        used_tokens: usage.used_tokens,
        context_window: window,
    }))
}

/// Whether a turn's reply holds a card the user hasn't answered yet.
fn awaits_answer(store: &Store, message_id: &str) -> Result<bool> {
    let Some(message) = store.get_chat_message(message_id)? else {
        return Ok(false);
    };
    let parts = serde_json::from_str::<Value>(&message.parts_json).unwrap_or(Value::Null);
    Ok(has_open_prompt(&parts))
}

fn has_open_prompt(parts: &Value) -> bool {
    parts.as_array().is_some_and(|parts| {
        parts.iter().any(|part| {
            part.get("prompt")
                .is_some_and(|prompt| prompt.get("resolved").and_then(Value::as_bool) != Some(true))
                || part.get("children").is_some_and(has_open_prompt)
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Completed,
    /// Failed, stopped, or never recorded as finished.
    Unfinished,
}

/// The chat's most recent automatic compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Previous {
    started_at: i64,
    outcome: Outcome,
    turns_since: i64,
}

impl Previous {
    fn holds_back(&self, now: i64) -> bool {
        match self.outcome {
            Outcome::Completed => self.turns_since < MIN_TURNS_BETWEEN,
            Outcome::Unfinished => now.saturating_sub(self.started_at) < RETRY_AFTER_MS,
        }
    }
}

fn previous(store: &Store, session_id: &str) -> Result<Option<Previous>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let row = conn
        .query_row(
            "SELECT started_at, marker_id, error FROM vlr_auto_compactions
             WHERE session_id = ?1 ORDER BY started_at DESC, rowid DESC LIMIT 1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((started_at, marker_id, error)) = row else {
        return Ok(None);
    };
    let outcome = match (marker_id, error) {
        (Some(marker_id), None) => marker_outcome(store, &marker_id)?,
        _ => Outcome::Unfinished,
    };
    let turns_since = conn.query_row(
        "SELECT COUNT(*) FROM chat_turns WHERE session_id = ?1 AND created_at > ?2",
        params![session_id, started_at],
        |row| row.get(0),
    )?;
    Ok(Some(Previous {
        started_at,
        outcome,
        turns_since,
    }))
}

/// How a compaction ended, read off its row in the chat: `completed` once it
/// has landed; `running` or `error` until then, or when it failed or was
/// stopped.
fn marker_outcome(store: &Store, marker_id: &str) -> Result<Outcome> {
    let Some(message) = store.get_chat_message(marker_id)? else {
        // Gone with its branch; nothing says it failed.
        return Ok(Outcome::Completed);
    };
    let completed = serde_json::from_str::<Value>(&message.parts_json)
        .ok()
        .and_then(|parts| {
            parts
                .get(0)?
                .pointer("/state/status")?
                .as_str()
                .map(|status| status == "completed")
        })
        .unwrap_or(false);
    Ok(if completed {
        Outcome::Completed
    } else {
        Outcome::Unfinished
    })
}

fn record(
    store: &Store,
    session_id: &str,
    due: &Due,
    now: i64,
    marker_id: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    conn.execute(
        "INSERT INTO vlr_auto_compactions
             (session_id, started_at, marker_id, used_tokens, context_window, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            session_id,
            now,
            marker_id,
            i64::try_from(due.used_tokens).unwrap_or(i64::MAX),
            i64::try_from(due.context_window).unwrap_or(i64::MAX),
            error,
        ],
    )?;
    Ok(())
}

fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vlr_auto_compactions (
             session_id     TEXT NOT NULL,
             started_at     INTEGER NOT NULL,
             marker_id      TEXT,
             used_tokens    INTEGER NOT NULL,
             context_window INTEGER NOT NULL,
             error          TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_vlr_auto_compactions_session
             ON vlr_auto_compactions(session_id, started_at);",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use serde_json::json;

    use super::{
        due, has_open_prompt, parse_threshold, previous, record, Due, Outcome, Previous,
        DEFAULT_THRESHOLD, MIN_TURNS_BETWEEN, RETRY_AFTER_MS,
    };
    use crate::store::{Store, StoredChatMessage, StoredChatSession};

    const NOW: i64 = 1_800_000_000_000;

    struct Fixture {
        dir: std::path::PathBuf,
        store: Store,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("vlr-compact-{}", uuid::Uuid::new_v4()));
            let store = Store::open_at(dir.clone()).unwrap();
            Self { dir, store }
        }

        fn chat(&self, id: &str, usage: Option<&str>, parent: Option<&str>) {
            self.store
                .create_chat_session(&StoredChatSession {
                    id: id.into(),
                    project_id: "proj_1".into(),
                    harness: "opencode".into(),
                    native_session_id: Some("native".into()),
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
                    parent_session_id: parent.map(str::to_owned),
                    side_parent_session_id: None,
                    created_at: 1,
                    updated_at: 1,
                })
                .unwrap();
            // Written as a turn's end writes it; creating a chat leaves it out.
            if let Some(usage) = usage {
                self.store
                    .set_chat_session_context_usage(id, usage)
                    .unwrap();
            }
        }

        fn message(&self, id: &str, session_id: &str, parts: serde_json::Value) {
            self.store
                .upsert_chat_message(&StoredChatMessage {
                    id: id.into(),
                    session_id: session_id.into(),
                    role: "assistant".into(),
                    parts_json: parts.to_string(),
                    created_at: 1,
                    completed_at: Some(2),
                    parent_id: None,
                    base_native_session_id: None,
                    result_native_session_id: None,
                })
                .unwrap();
        }

        fn turn(&self, session_id: &str, created_at: i64) {
            let id = format!("turn_{}", uuid::Uuid::new_v4());
            self.store
                .flavor_conn()
                .execute(
                    "INSERT INTO chat_turns (id, session_id, assistant_message_id, client_turn_id,
                         request_hash, prepared_input, settings_json, state, delivery_state,
                         created_at, updated_at)
                     VALUES (?1, ?2, 'msg', ?1, 'hash', 'text', '{}', 'completed', 'sent', ?3, ?3)",
                    params![id, session_id, created_at],
                )
                .unwrap();
        }

        fn due(&self, session_id: &str, now: i64) -> Option<Due> {
            due(&self.store, session_id, "msg_reply", 0.8, now).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn compaction_row(status: &str) -> serde_json::Value {
        json!([{"id": "compacted", "type": "tool", "tool": "compacted", "state": {"status": status}}])
    }

    #[test]
    fn the_threshold_reads_fractions_and_percentages() {
        for (value, expected) in [
            (None, Some(DEFAULT_THRESHOLD)),
            (Some(""), Some(DEFAULT_THRESHOLD)),
            (Some("on"), Some(DEFAULT_THRESHOLD)),
            (Some("1"), Some(DEFAULT_THRESHOLD)),
            (Some("0.7"), Some(0.7)),
            (Some("70"), Some(0.7)),
            (Some(" 70% "), Some(0.7)),
            (Some("85.5%"), Some(0.855)),
            (Some("100"), Some(DEFAULT_THRESHOLD)),
            (Some("-5"), Some(DEFAULT_THRESHOLD)),
            (Some("lots"), Some(DEFAULT_THRESHOLD)),
            (Some("0"), None),
            (Some("0%"), None),
            (Some("OFF"), None),
            (Some("false"), None),
        ] {
            assert_eq!(parse_threshold(value), expected, "value {value:?}");
        }
    }

    #[test]
    fn a_chat_is_due_once_its_context_reaches_the_threshold() {
        let fixture = Fixture::new();
        fixture.chat(
            "full",
            Some(r#"{"usedTokens":160000,"contextWindow":200000}"#),
            None,
        );
        fixture.chat(
            "roomy",
            Some(r#"{"usedTokens":120000,"contextWindow":200000}"#),
            None,
        );
        fixture.chat("no_window", Some(r#"{"usedTokens":190000}"#), None);
        fixture.chat("fresh", None, None);
        fixture.chat(
            "helper",
            Some(r#"{"usedTokens":199000,"contextWindow":200000}"#),
            Some("full"),
        );

        assert_eq!(
            fixture.due("full", NOW),
            Some(Due {
                used_tokens: 160_000,
                context_window: 200_000
            })
        );
        for session in ["roomy", "no_window", "fresh", "helper", "missing"] {
            assert_eq!(fixture.due(session, NOW), None, "session {session}");
        }
    }

    #[test]
    fn a_turn_waiting_on_the_users_answer_is_left_alone() {
        let fixture = Fixture::new();
        fixture.chat(
            "chat_1",
            Some(r#"{"usedTokens":190000,"contextWindow":200000}"#),
            None,
        );
        fixture.message(
            "msg_reply",
            "chat_1",
            json!([{"id": "p1", "type": "prompt", "prompt": {"kind": "plan", "resolved": false}}]),
        );
        assert_eq!(fixture.due("chat_1", NOW), None);

        fixture.message(
            "msg_reply",
            "chat_1",
            json!([{"id": "p1", "type": "prompt", "prompt": {"kind": "plan", "resolved": true}}]),
        );
        assert!(fixture.due("chat_1", NOW).is_some());

        // A card nested under a sub-agent's part counts too.
        assert!(has_open_prompt(
            &json!([{"id": "spawn", "type": "tool", "children": [
                {"id": "q", "type": "prompt", "prompt": {"kind": "question"}}
            ]}])
        ));
        assert!(!has_open_prompt(
            &json!([{"id": "t", "type": "text", "text": "done"}])
        ));
    }

    #[test]
    fn after_a_compaction_a_few_turns_must_run_before_the_next() {
        let fixture = Fixture::new();
        fixture.chat(
            "chat_1",
            Some(r#"{"usedTokens":170000,"contextWindow":200000}"#),
            None,
        );
        let due = fixture.due("chat_1", NOW).unwrap();
        record(
            &fixture.store,
            "chat_1",
            &due,
            NOW,
            Some("msg_compact"),
            None,
        )
        .unwrap();
        fixture.message("msg_compact", "chat_1", compaction_row("completed"));

        for turn in 1..MIN_TURNS_BETWEEN {
            fixture.turn("chat_1", NOW + turn);
            assert_eq!(fixture.due("chat_1", NOW + 10), None, "after {turn} turns");
        }
        fixture.turn("chat_1", NOW + MIN_TURNS_BETWEEN);
        // A turn from before the compaction doesn't count.
        fixture.turn("chat_1", NOW - 1);
        assert!(fixture.due("chat_1", NOW + 10).is_some());

        let previous = previous(&fixture.store, "chat_1").unwrap().unwrap();
        assert_eq!(previous.outcome, Outcome::Completed);
        assert_eq!(previous.turns_since, MIN_TURNS_BETWEEN);
    }

    #[test]
    fn a_failed_or_stopped_compaction_waits_before_another_try() {
        let fixture = Fixture::new();
        fixture.chat(
            "stopped",
            Some(r#"{"usedTokens":170000,"contextWindow":200000}"#),
            None,
        );
        fixture.chat(
            "refused",
            Some(r#"{"usedTokens":170000,"contextWindow":200000}"#),
            None,
        );
        let due = Due {
            used_tokens: 170_000,
            context_window: 200_000,
        };
        record(
            &fixture.store,
            "stopped",
            &due,
            NOW,
            Some("msg_stopped"),
            None,
        )
        .unwrap();
        fixture.message("msg_stopped", "stopped", compaction_row("error"));
        record(
            &fixture.store,
            "refused",
            &due,
            NOW,
            None,
            Some("unknown harness"),
        )
        .unwrap();

        for session in ["stopped", "refused"] {
            for turn in 1..=MIN_TURNS_BETWEEN {
                fixture.turn(session, NOW + turn);
            }
            assert_eq!(fixture.due(session, NOW + RETRY_AFTER_MS - 1), None);
            assert!(fixture.due(session, NOW + RETRY_AFTER_MS).is_some());
        }
    }

    #[test]
    fn holding_back_depends_on_how_the_last_compaction_ended() {
        let completed = |turns_since| Previous {
            started_at: NOW,
            outcome: Outcome::Completed,
            turns_since,
        };
        assert!(completed(0).holds_back(NOW + RETRY_AFTER_MS * 10));
        assert!(!completed(MIN_TURNS_BETWEEN).holds_back(NOW));
        let unfinished = Previous {
            started_at: NOW,
            outcome: Outcome::Unfinished,
            turns_since: 50,
        };
        assert!(unfinished.holds_back(NOW + 1));
        assert!(!unfinished.holds_back(NOW + RETRY_AFTER_MS));
    }
}
