//! `orx idea`: the idea queue from the command line.

use serde_json::json;

use super::cli::{IdeaCommand, IdeaFieldArgs};
use super::ideas::{self, Author, Idea, IdeaFields, Status, Verdict};
use super::metrics;
use crate::error::{anyhow, Result};
use crate::output::print_table;
use crate::store::Store;

pub(crate) fn run(command: IdeaCommand) -> Result<()> {
    let store = Store::open()?;
    match command {
        IdeaCommand::Add {
            project_id,
            title,
            fields,
        } => {
            let project = crate::local::resolve::resolve_project(&store, &project_id)?;
            let mut fields = to_fields(fields)?;
            fields.title = Some(title);
            fields.author = fields.author.or(Some(default_author()));
            let idea = ideas::add(&store, &project.id, fields)?;
            println!("Added idea {}: {}", idea.id, idea.title);
            Ok(())
        }
        IdeaCommand::Edit {
            idea_id,
            title,
            fields,
        } => {
            let mut fields = to_fields(fields)?;
            fields.title = title;
            let idea = ideas::edit(&store, &idea_id, fields)?;
            println!("Updated idea {}: {}", idea.id, idea.title);
            Ok(())
        }
        IdeaCommand::List {
            project_id,
            all,
            json,
        } => list(&store, &project_id, all, json),
        IdeaCommand::Show { idea_id, json } => {
            let idea = ideas::require(&store, &idea_id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&idea)?);
            } else {
                print_idea(&idea);
            }
            Ok(())
        }
        IdeaCommand::Next { project_id, json } => next(&store, &project_id, json),
        IdeaCommand::Start {
            idea_id,
            experiment_id,
        } => {
            let idea = ideas::start(&store, &idea_id, &experiment_id)?;
            println!(
                "Idea {} is now tested by experiment {}. Close it with `orx idea done {}` once \
                 its runs have finished.",
                idea.id, experiment_id, idea.id
            );
            Ok(())
        }
        IdeaCommand::Done {
            idea_id,
            evidence,
            kept,
            refuted,
        } => {
            let verdict = match (kept, refuted) {
                (true, _) => Verdict::Kept,
                (_, true) => Verdict::Refuted,
                _ => Verdict::Measured,
            };
            report_closed(&ideas::close(&store, &idea_id, verdict, evidence)?);
            Ok(())
        }
        IdeaCommand::Kill { idea_id, evidence } => {
            report_closed(&ideas::close(
                &store,
                &idea_id,
                Verdict::Refuted,
                Some(evidence),
            )?);
            Ok(())
        }
        IdeaCommand::Drop { idea_id, reason } => {
            report_closed(&ideas::close(&store, &idea_id, Verdict::Dropped, reason)?);
            Ok(())
        }
        IdeaCommand::Calibration { project_id, json } => calibration(&store, &project_id, json),
    }
}

fn default_author() -> Author {
    if crate::local::chat::in_local_session() {
        Author::Agent
    } else {
        Author::User
    }
}

fn to_fields(args: IdeaFieldArgs) -> Result<IdeaFields> {
    let (forecast_low, forecast_high) = match args.interval.as_deref() {
        None => (None, None),
        Some([low, high]) => (Some(*low), Some(*high)),
        Some(_) => return Err(anyhow!("--interval takes two numbers: LOW HIGH.")),
    };
    Ok(IdeaFields {
        title: None,
        kind: args.kind,
        class: args.class,
        priority: args.priority,
        hypothesis: args.hypothesis,
        kill_test: args.kill_test,
        forecast: args.forecast,
        forecast_low,
        forecast_high,
        cost_hours: args.cost_hours,
        blocked_by: args.blocked_by,
        author: args.author,
    })
}

fn signed(value: f64) -> String {
    let text = metrics::format_value(Some(value));
    if value > 0.0 {
        format!("+{text}")
    } else {
        text
    }
}

fn forecast_text(idea: &Idea) -> String {
    match (idea.forecast, idea.forecast_low, idea.forecast_high) {
        (Some(forecast), Some(low), Some(high)) => {
            format!("{} [{}, {}]", signed(forecast), signed(low), signed(high))
        }
        (Some(forecast), _, _) => signed(forecast),
        _ => "—".to_string(),
    }
}

fn status_order(status: Status) -> u8 {
    match status {
        Status::Running => 0,
        Status::Open => 1,
        Status::Kept => 2,
        Status::Refuted => 3,
        Status::Dropped => 4,
    }
}

fn list(store: &Store, project_id: &str, all: bool, json: bool) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let mut ideas: Vec<Idea> = ideas::list(store, &project.id)?
        .into_iter()
        .filter(|idea| all || !idea.status.is_closed())
        .collect();
    ideas.sort_by_key(|idea| {
        (
            status_order(idea.status),
            idea.priority,
            std::cmp::Reverse(idea.closed_at),
            idea.created_at,
        )
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&ideas)?);
        return Ok(());
    }
    if ideas.is_empty() {
        println!(
            "No {}ideas yet. Add one with `orx idea add {} \"<title>\" --forecast <change> \
             --interval <low> <high> --cost-hours <hours> --kill-test \"<test>\"`.",
            if all { "" } else { "open " },
            project.id
        );
        return Ok(());
    }
    let rows: Vec<Vec<String>> = ideas
        .iter()
        .map(|idea| {
            vec![
                idea.id.clone(),
                idea.title.clone(),
                idea.kind.as_str().to_string(),
                idea.class
                    .map(|class| class.as_str())
                    .unwrap_or("")
                    .to_string(),
                idea.priority.to_string(),
                idea.status.as_str().to_string(),
                forecast_text(idea),
                idea.cost_hours
                    .map(|hours| metrics::format_value(Some(hours)))
                    .unwrap_or_default(),
                idea.outcome.map(signed).unwrap_or_default(),
            ]
        })
        .collect();
    print_table(
        &[
            "ID", "TITLE", "KIND", "CLASS", "P", "STATUS", "FORECAST", "GPU-H", "OUTCOME",
        ],
        &rows,
    );
    Ok(())
}

fn print_idea(idea: &Idea) {
    println!("{} {}", idea.id, idea.title);
    let line = |label: &str, value: Option<String>| {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            println!("  {label:<11} {value}");
        }
    };
    line("status", Some(idea.status.as_str().to_string()));
    line(
        "kind",
        Some(match idea.class {
            Some(class) => format!("{} ({})", idea.kind.as_str(), class.as_str()),
            None => idea.kind.as_str().to_string(),
        }),
    );
    line("priority", Some(idea.priority.to_string()));
    line("author", Some(idea.author.as_str().to_string()));
    line("hypothesis", idea.hypothesis.clone());
    line("kill test", idea.kill_test.clone());
    line(
        "forecast",
        idea.forecast
            .map(|_| format!("{} (80% interval)", forecast_text(idea))),
    );
    line(
        "cost",
        idea.cost_hours
            .map(|hours| format!("{} GPU-hours", metrics::format_value(Some(hours)))),
    );
    line("blocked by", idea.blocked_by.clone());
    line("experiment", idea.experiment_id.clone());
    line(
        "outcome",
        idea.outcome.map(|outcome| match &idea.outcome_key {
            Some(key) => format!("{} {key}", signed(outcome)),
            None => signed(outcome),
        }),
    );
    line("evidence", idea.evidence.clone());
}

fn report_closed(idea: &Idea) {
    let outcome = match (idea.outcome, &idea.outcome_key) {
        (Some(outcome), Some(key)) => format!(" Measured change: {} {key}.", signed(outcome)),
        _ => String::new(),
    };
    println!("Idea {} is {}.{outcome}", idea.id, idea.status.as_str());
}

fn next(store: &Store, project_id: &str, json: bool) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let pick = ideas::next(store, &project.id)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "pick": pick }))?
        );
        return Ok(());
    }
    match pick {
        Some(pick) => {
            println!("Next: {} ({})", pick.idea.title, pick.idea.id);
            println!("Why: {}.", pick.reason);
            if let Some(test) = &pick.idea.kill_test {
                println!("Kill test first: {test}");
            }
        }
        None => println!(
            "No open idea is ready (none queued, or all are blocked). Add ideas with `orx idea \
             add {} \"<title>\"`.",
            project.id
        ),
    }
    Ok(())
}

fn calibration(store: &Store, project_id: &str, json: bool) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let report = ideas::calibration(&ideas::list(store, &project.id)?);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if report.tested == 0 {
        println!("No idea has been tested and closed yet.");
        return Ok(());
    }
    println!(
        "Tested {} idea{}; kept {} ({}%).",
        report.tested,
        if report.tested == 1 { "" } else { "s" },
        report.kept,
        100 * report.kept / report.tested
    );
    if let (Some(bias), Some(error)) = (report.bias, report.mean_abs_error) {
        let leaning = if bias > 0.0 {
            "outcomes ran above forecasts"
        } else if bias < 0.0 {
            "outcomes ran below forecasts"
        } else {
            "no lean"
        };
        println!(
            "Forecasts vs outcomes on {}: mean error {} ({leaning}), mean absolute error {}.",
            report.scored,
            signed(bias),
            metrics::format_value(Some(error))
        );
    }
    if report.with_interval > 0 {
        println!(
            "{} of {} outcomes fell inside their 80% interval ({}%); calibrated intervals hold \
             about 80%.",
            report.inside_interval,
            report.with_interval,
            100 * report.inside_interval / report.with_interval
        );
    }
    Ok(())
}
