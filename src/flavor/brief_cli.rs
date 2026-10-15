//! `orx brief`: optimizations and their briefs from the command line. A
//! command acts on the optimization named with `--opt`, else the one the chat
//! it runs in runs, else the project's only one; `orx brief set` in a chat
//! that runs none starts one for it.

use serde_json::json;

use super::cli::BriefCommand;
use super::metrics;
use super::scope::{self, Brief, BriefFields, Resolve};
use crate::error::{anyhow, Result};
use crate::store::Store;

pub(crate) fn run(command: BriefCommand) -> Result<()> {
    let store = Store::open()?;
    match command {
        BriefCommand::Show {
            project_id,
            opt,
            json,
        } => show(&store, &project_id, opt.as_deref(), json),
        BriefCommand::List { project_id, json } => list(&store, &project_id, json),
        BriefCommand::Use { opt_id } => {
            let chat = scope::current_chat(&store).ok_or_else(|| {
                anyhow!(
                    "`orx brief use` hands an optimization to the chat it runs in; run it from \
                     that chat."
                )
            })?;
            let brief = scope::set_chat(&store, &opt_id, &chat)?;
            println!(
                "This chat now runs optimization {}. Its brief: `orx brief show {}`.",
                scope::label(&brief),
                brief.project_id
            );
            Ok(())
        }
        BriefCommand::Set {
            project_id,
            opt,
            objective,
            may_change,
            must_not_change,
            lock,
            unlock,
            scope: scope_paths,
            unscope,
            budget_runs,
            budget_gpu_hours,
            budget_days,
            reset_budget,
            pointers,
            sources,
            stop_rule,
        } => {
            let project = crate::local::resolve::resolve_project(&store, &project_id)?;
            let current = scope::resolve(&store, &project.id, opt.as_deref(), Resolve::OrCreate)?;
            let brief = scope::set(
                &store,
                &current.id,
                BriefFields {
                    objective,
                    may_change,
                    must_not_change,
                    lock,
                    unlock,
                    scope: scope_paths,
                    unscope,
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
            let missing = scope::missing(&store, &brief)?;
            if missing.is_empty() {
                println!(
                    "Brief for optimization {} updated. Show it to the user (`orx brief show {}`); \
                     once they agree, run `orx brief confirm {}`.",
                    brief.id, project.id, project.id
                );
            } else {
                println!(
                    "Brief for optimization {} updated. It still needs {}.",
                    brief.id,
                    missing.join("; ")
                );
            }
            Ok(())
        }
        BriefCommand::Confirm { project_id, opt } => {
            let project = crate::local::resolve::resolve_project(&store, &project_id)?;
            let current = scope::resolve(&store, &project.id, opt.as_deref(), Resolve::Existing)?;
            let by = if super::by_user() { "user" } else { "agent" };
            let brief = scope::confirm(&store, &project, &current.id, by)?;
            let paths = |paths: Vec<&str>, label: &str| {
                if paths.is_empty() {
                    String::new()
                } else {
                    format!(" {label}: {}.", paths.join(", "))
                }
            };
            println!(
                "Brief for optimization {} confirmed. Its runs are now held to it.{}{}",
                brief.id,
                paths(
                    brief
                        .locked
                        .iter()
                        .map(|locked| locked.path.as_str())
                        .collect(),
                    "Locked"
                ),
                paths(brief.scope.iter().map(String::as_str).collect(), "Scope")
            );
            Ok(())
        }
    }
}

fn show(store: &Store, project_id: &str, opt: Option<&str>, json: bool) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let brief = match scope::resolve(store, &project.id, opt, Resolve::Existing) {
        Ok(brief) => brief,
        // With none at all, say how to start one rather than fail.
        Err(_) if opt.is_none() && scope::list(store, &project.id)?.is_empty() => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "brief": null, "primary": null,
                        "usage": null, "missing": [] }))?
                );
            } else {
                println!(
                    "Project {} has no optimization yet. `orx skill grill` walks through agreeing \
                     a brief with the user.",
                    project.id
                );
            }
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let primary = metrics::primary(store, &brief.id)?;
    let usage = scope::usage(store, &brief)?;
    let missing = scope::missing(store, &brief)?;
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
    let title = format!("Optimization {} in {}", brief.id, project.id);
    match (&brief.confirmed_at, &brief.confirmed_by) {
        (Some(_), Some(by)) => println!("{title} (confirmed by {by})"),
        (Some(_), None) => println!("{title} (confirmed)"),
        _ if missing.is_empty() => println!(
            "{title} (not confirmed: show it to the user, then `orx brief confirm {}`)",
            project.id
        ),
        _ => println!(
            "{title} (not confirmed; still needs {})",
            missing.join("; ")
        ),
    }
    print_fields(&brief, primary.as_ref(), Some(&usage));
    Ok(())
}

fn list(store: &Store, project_id: &str, json: bool) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let briefs = scope::list(store, &project.id)?;
    let current = scope::current_chat(store);
    let mut rows = Vec::with_capacity(briefs.len());
    for brief in &briefs {
        rows.push((brief, metrics::primary(store, &brief.id)?));
    }
    if json {
        let entries: Vec<_> = rows
            .iter()
            .map(|(brief, primary)| {
                json!({
                    "brief": brief,
                    "primary": primary,
                    "thisChat": current.is_some() && brief.chat_session_id == current,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!(
            "Project {} has no optimization yet. `orx skill grill` walks through agreeing a brief \
             with the user.",
            project.id
        );
        return Ok(());
    }
    for (brief, primary) in rows {
        let state = if brief.confirmed_at.is_some() {
            "confirmed"
        } else {
            "not confirmed"
        };
        let metric = primary.map_or_else(|| "no metric yet".to_owned(), |metric| metric.key);
        let here = if current.is_some() && brief.chat_session_id == current {
            " · this chat"
        } else {
            ""
        };
        println!(
            "{}  {}  [{metric}; {state}{here}]",
            brief.id,
            brief.objective.as_deref().unwrap_or("(no objective yet)")
        );
    }
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
    line(
        "scope",
        (!brief.scope.is_empty()).then(|| format!("only {}", brief.scope.join(", "))),
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
    line("chat", brief.chat_session_id.clone());
}
