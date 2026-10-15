//! Agent settings and events VLResearch adds on top of upstream's harness
//! adapters.

use serde_json::Value;

use crate::local::chat::TurnCtx;

/// Set to `0` to leave Claude Code's task tools as they come.
pub(crate) const CHECKLIST_ENV: &str = "VLR_AGENT_CHECKLIST";

/// Claude Code (2.1) gives its task-tracking tools only to some models, and
/// where it does, it defaults to TaskCreate/TaskUpdate, which refer to tasks by
/// ids from earlier calls. These settings give every model TodoWrite instead,
/// which sends the whole checklist each time, so orx can show the agent's
/// progress from any single call. Each is set only when the variable has no
/// value yet.
const CHECKLIST_ENV_DEFAULTS: &[(&str, &str)] = &[
    ("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1"),
    ("CLAUDE_CODE_ENABLE_TASKS", "0"),
];

/// Adds [`CHECKLIST_ENV_DEFAULTS`] to a coding-agent process.
pub(crate) fn apply_agent_env_defaults(cmd: &mut tokio::process::Command) {
    if std::env::var(CHECKLIST_ENV).is_ok_and(|value| value.trim() == "0") {
        return;
    }
    super::set_env_defaults(cmd, CHECKLIST_ENV_DEFAULTS);
}

/// Seen by the Claude Code adapter before its own handling of each
/// stream-json event; upstream's handling is unchanged.
///
/// Claude Code emits `system/api_retry` before retrying a failed API request.
/// Upstream drops it, so a backoff (rate limit, overload) looked like a hang.
/// It becomes the same retry row Codex and OpenCode turns already show, and
/// the row clears once Claude streams again.
pub(crate) fn claude_event(ctx: &mut TurnCtx, event: &Value) {
    let kind = event.get("type").and_then(Value::as_str);
    if kind == Some("system") && event.get("subtype").and_then(Value::as_str) == Some("api_retry") {
        let attempt = event.get("attempt").and_then(Value::as_i64).unwrap_or(1);
        let max_retries = event.get("max_retries").and_then(Value::as_i64);
        let next_retry_at = event
            .get("retry_delay_ms")
            .and_then(Value::as_i64)
            .map(|delay| crate::store::now_ms() + delay.max(0));
        ctx.show_retry_status(
            "native",
            &claude_retry_reason(event),
            attempt,
            max_retries,
            next_retry_at,
        );
        return;
    }
    if matches!(kind, Some("assistant" | "stream_event" | "user")) && showing_native_retry(ctx) {
        ctx.clear_retry_status();
    }
}

fn claude_retry_reason(event: &Value) -> String {
    let what = match event.get("error").and_then(Value::as_str) {
        Some("rate_limit") => "rate limited",
        Some("overloaded") => "the API is overloaded",
        Some("server_error") => "API server error",
        Some("authentication_failed") => "authentication failed",
        Some("billing_error") => "billing error",
        Some("model_not_found") => "model not found",
        _ if event.get("no_response").is_some() => "no response from the API",
        _ => "the API request failed",
    };
    match event.get("error_status").and_then(Value::as_i64) {
        Some(status) => format!("Claude Code is retrying: {what} (HTTP {status})"),
        None => format!("Claude Code is retrying: {what}"),
    }
}

/// Whether the turn currently shows a retry row the agent itself reported
/// (not one of orx's own restart retries).
fn showing_native_retry(ctx: &TurnCtx) -> bool {
    ctx.assistant.parts.iter().any(|part| {
        part.id == "turn-retry"
            && part
                .state
                .as_ref()
                .and_then(|state| state.input.as_ref())
                .and_then(|input| input.get("retryOwner"))
                .and_then(Value::as_str)
                == Some("native")
    })
}

#[cfg(test)]
mod tests {
    use super::claude_retry_reason;
    use serde_json::json;

    #[test]
    fn retry_reasons_name_the_cause_and_status() {
        assert_eq!(
            claude_retry_reason(&json!({"error": "rate_limit", "error_status": 429})),
            "Claude Code is retrying: rate limited (HTTP 429)"
        );
        assert_eq!(
            claude_retry_reason(&json!({"error": "unknown", "error_status": null,
                "no_response": {"waited_ms": 1}})),
            "Claude Code is retrying: no response from the API"
        );
        assert_eq!(
            claude_retry_reason(&json!({})),
            "Claude Code is retrying: the API request failed"
        );
    }
}
