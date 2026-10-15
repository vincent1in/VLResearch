//! When each chat last showed signs of life, for the progress strip.
//!
//! `ChatHost::emit` hands every event it broadcasts to [`note`] (one hook
//! line). A streamed transcript update is what "the agent is still doing
//! something" means to the person watching, whichever harness runs the chat,
//! so the time of the latest one is kept here, in memory, for the life of the
//! `orx up` process.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde_json::Value;

/// Events that mean the agent did something: transcript changes, token usage
/// reports, and a turn starting.
const ACTIVITY_EVENTS: [&str; 3] = ["chat.message", "chat.usage", "chat.busy"];

static LAST_ACTIVITY: LazyLock<Mutex<HashMap<String, i64>>> = LazyLock::new(Default::default);

/// Records an event the chat host broadcast.
pub(crate) fn note(name: &str, data: &Value) {
    let Some(session_id) = data.get("sessionId").and_then(Value::as_str) else {
        return;
    };
    let Ok(mut last) = LAST_ACTIVITY.lock() else {
        return;
    };
    if name == "chat.session.deleted" {
        last.remove(session_id);
    } else if ACTIVITY_EVENTS.contains(&name) {
        last.insert(session_id.to_owned(), crate::store::now_ms());
    }
}

/// Unix millis of the chat's latest activity since `orx up` started, if any.
pub(crate) fn last_activity_ms(session_id: &str) -> Option<i64> {
    LAST_ACTIVITY.lock().ok()?.get(session_id).copied()
}

#[cfg(test)]
mod tests {
    use super::{last_activity_ms, note};
    use serde_json::json;

    #[test]
    fn transcript_events_count_as_activity_and_deletion_forgets_the_chat() {
        let session = format!("chat_{}", uuid::Uuid::new_v4());
        note("chat.queued", &json!({ "sessionId": session }));
        assert_eq!(last_activity_ms(&session), None);

        let before = crate::store::now_ms();
        note(
            "chat.message",
            &json!({ "sessionId": session, "message": {} }),
        );
        let seen = last_activity_ms(&session).expect("a transcript update is activity");
        assert!(seen >= before);

        note("chat.session.deleted", &json!({ "sessionId": session }));
        assert_eq!(last_activity_ms(&session), None);
    }
}
