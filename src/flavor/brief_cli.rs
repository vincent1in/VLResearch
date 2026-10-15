//! `orx brief`: the scoping brief from the command line.

use serde_json::json;

use super::cli::BriefCommand;
use super::metrics;
use super::scope::{self, Brief, BriefFields};
use crate::error::Result;
use crate::store::Store;

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
                    reset_budget,
                    pointers,
                    sources,
                    stop_rule,
                },
            )?;
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
