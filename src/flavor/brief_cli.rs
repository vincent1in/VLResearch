//! `orx brief`: the scoping brief from the command line.

use serde_json::json;

use super::cli::BriefCommand;
use super::metrics;
use super::scope::{self, Brief, BriefFields};
use crate::error::Result;
use crate::store::Store;

/// Links the brief to the chat whose agent is setting it, so that confirming
/// it in the Research view tells that chat to start the loop. A helper or side
/// chat's top-level chat takes its place. Outside a chat, nothing changes.
fn link_calling_chat(store: &Store, project_id: &str) {
    if !crate::local::chat::in_local_session() {
        return;
    }
    if let Some(session_id) = crate::local::chat::launching_chat_session() {
        let _ = link_chat_or_parent(store, project_id, session_id);
    }
}

fn link_chat_or_parent(store: &Store, project_id: &str, mut session_id: String) -> Result<()> {
    // Helper chats nest, but not deeply; the bound only guards against a cycle.
    for _ in 0..8 {
        let Some(session) = store.get_chat_session(&session_id)? else {
            return Ok(());
        };
        match session.parent_session_id.or(session.side_parent_session_id) {
            Some(parent) => session_id = parent,
            None => return super::start::link_chat(store, project_id, &session.id),
        }
    }
    Ok(())
}

pub(crate) fn run(command: BriefCommand) -> Result<()> {
    let store = Store::open()?;
    match command {
        BriefCommand::Show { project_id, json } => show(&store, &project_id, json),
        BriefCommand::Set {
            project_id,
            objective,
            may_change,
            must_not_change,
            lock,
            unlock,
            budget_runs,
            budget_gpu_hours,
            budget_days,
            reset_budget,
            pointers,
            sources,
            stop_rule,
        } => {
            let project = crate::local::resolve::resolve_project(&store, &project_id)?;
            let brief = scope::set(
                &store,
                &project.id,
                BriefFields {
                    objective,
                    may_change,
                    must_not_change,
                    lock,
                    unlock,
                    budget_runs,
                    budget_gpu_hours,
                    budget_days,
                    clear_budget: false,
                    reset_budget,
                    pointers,
                    sources,
                    stop_rule,
                },
            )?;
            link_calling_chat(&store, &project.id);
            let missing = scope::missing(&store, &brief)?;
            if missing.is_empty() {
                println!(
                    "Brief updated. Show it to the user (`orx brief show {}`); once they agree, \
                     run `orx brief confirm {}`.",
                    project.id, project.id
                );
            } else {
                println!("Brief updated. It still needs {}.", missing.join("; "));
            }
            Ok(())
        }
        BriefCommand::Confirm { project_id } => {
            let project = crate::local::resolve::resolve_project(&store, &project_id)?;
            let by = if super::by_user() { "user" } else { "agent" };
            let brief = scope::confirm(&store, &project, by)?;
            let locked = if brief.locked.is_empty() {
                String::new()
            } else {
                format!(
                    " Locked: {}.",
                    brief
                        .locked
                        .iter()
                        .map(|locked| locked.path.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            println!(
                "Brief for {} confirmed. Launches are now held to it.{locked}",
                project.id
            );
            Ok(())
        }
    }
}

fn show(store: &Store, project_id: &str, json: bool) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let brief = scope::get(store, &project.id)?;
    let primary = metrics::primary(store, &project.id)?;
    let usage = match &brief {
        Some(brief) => Some(scope::usage(store, brief)?),
        None => None,
    };
    let missing = match &brief {
        Some(brief) => scope::missing(store, brief)?,
        None => Vec::new(),
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "brief": brief,
                "primary": primary,
                "usage": usage,
                "missing": missing,
            }))?
        );
        return Ok(());
    }
    let Some(brief) = brief else {
        println!(
            "Project {} has no brief. `orx skill grill` walks through agreeing one with the user.",
            project.id
        );
        return Ok(());
    };
    match (&brief.confirmed_at, &brief.confirmed_by) {
        (Some(_), Some(by)) => println!("Brief for {} (confirmed by {by})", project.id),
        (Some(_), None) => println!("Brief for {} (confirmed)", project.id),
        _ if missing.is_empty() => println!(
            "Brief for {} (not confirmed: show it to the user, then `orx brief confirm {}`)",
            project.id, project.id
        ),
        _ => println!(
            "Brief for {} (not confirmed; still needs {})",
            project.id,
            missing.join("; ")
        ),
    }
    print_fields(&brief, primary.as_ref(), usage.as_ref());
    Ok(())
}

fn print_fields(
    brief: &Brief,
    primary: Option<&metrics::PrimaryMetric>,
    usage: Option<&scope::Usage>,
) {
    let line = |label: &str, value: Option<String>| {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            println!("  {label:<16} {value}");
        }
    };
    line("objective", brief.objective.clone());
    line(
        "primary metric",
        primary.map(|metric| {
            let direction = match metric.direction {
                metrics::Direction::Max => "higher is better",
                metrics::Direction::Min => "lower is better",
            };
            match metric.noise_floor {
                Some(floor) => format!(
                    "{} ({direction}; noise floor {})",
                    metric.key,
                    metrics::format_value(Some(floor))
                ),
                None => format!("{} ({direction})", metric.key),
            }
        }),
    );
    line("may change", brief.may_change.clone());
    line("must not change", brief.must_not_change.clone());
    line(
        "locked files",
        (!brief.locked.is_empty()).then(|| {
            brief
                .locked
                .iter()
                .map(|locked| locked.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }),
    );
    let mut budget = Vec::new();
    if let Some(runs) = brief.budget_runs {
        budget.push(format!("{runs} runs"));
    }
    if let Some(hours) = brief.budget_gpu_hours {
        budget.push(format!("{} GPU-hours", metrics::format_value(Some(hours))));
    }
    if let Some(days) = brief.budget_days {
        budget.push(format!("{} days", metrics::format_value(Some(days))));
    }
    let used = usage.filter(|_| brief.confirmed_at.is_some()).map(|usage| {
        format!(
            " (used: {} runs, {} run-hours, {} days)",
            usage.runs,
            metrics::format_value(Some((usage.run_hours * 10.0).round() / 10.0)),
            metrics::format_value(Some((usage.days * 10.0).round() / 10.0))
        )
    });
    line(
        "budget",
        (!budget.is_empty()).then(|| format!("{}{}", budget.join(" · "), used.unwrap_or_default())),
    );
    line("pointers", brief.pointers.clone());
    line("sources", brief.sources.clone());
    line("stop rule", brief.stop_rule.clone());
}

#[cfg(test)]
mod tests {
    use super::link_chat_or_parent;
    use crate::flavor::scope::{self, BriefFields};
    use crate::store::{Store, StoredChatSession};

    fn chat(id: &str, project_id: &str, parent: Option<&str>) -> StoredChatSession {
        StoredChatSession {
            id: id.into(),
            project_id: project_id.into(),
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
            parent_session_id: parent.map(str::to_owned),
            side_parent_session_id: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn the_brief_links_the_top_level_chat_that_set_it() {
        let dir = std::env::temp_dir().join(format!("vlr-brief-cli-{}", uuid::Uuid::new_v4()));
        let store = Store::open_at(dir.clone()).unwrap();
        scope::set(
            &store,
            "p",
            BriefFields {
                objective: Some("Raise IC".into()),
                ..BriefFields::default()
            },
        )
        .unwrap();
        for session in [
            chat("chat_main", "p", None),
            chat("chat_helper", "p", Some("chat_main")),
            chat("chat_other", "q", None),
        ] {
            store.create_chat_session(&session).unwrap();
        }
        let linked = || scope::get(&store, "p").unwrap().unwrap().chat_session_id;

        link_chat_or_parent(&store, "p", "chat_helper".into()).unwrap();
        assert_eq!(linked().as_deref(), Some("chat_main"));
        // Another project's chat, or one orx doesn't know, leaves the link alone.
        assert!(link_chat_or_parent(&store, "p", "chat_other".into()).is_err());
        link_chat_or_parent(&store, "p", "chat_gone".into()).unwrap();
        assert_eq!(linked().as_deref(), Some("chat_main"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
