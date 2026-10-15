//! `orx metric`: the metrics ledger from the command line. Run metrics are the
//! project's; the primary metric, the ranking on it and the best belong to an
//! optimization: the one named with `--opt`, else the one this chat runs, else
//! the project's only one.

use serde_json::json;

use super::cli::MetricCommand;
use super::metrics::{self, Direction, PrimaryMetric};
use super::scope::{self, Resolve};
use crate::error::{anyhow, Result};
use crate::output::print_table;
use crate::store::Store;

pub(crate) fn run(command: MetricCommand) -> Result<()> {
    let store = Store::open()?;
    match command {
        MetricCommand::Show { run_id, json } => show(&store, &run_id, json),
        MetricCommand::Runs {
            project_id,
            opt,
            key,
            split,
            maximize,
            minimize,
            json,
        } => runs(
            &store,
            &project_id,
            opt.as_deref(),
            key,
            split,
            direction_flag(maximize, minimize),
            json,
        ),
        MetricCommand::Primary {
            project_id,
            opt,
            key,
            maximize,
            minimize,
            noise_floor,
            split,
        } => primary(
            &store,
            &project_id,
            opt.as_deref(),
            key,
            direction_flag(maximize, minimize),
            noise_floor,
            split,
        ),
        MetricCommand::Best {
            project_id,
            opt,
            json,
        } => best(&store, &project_id, opt.as_deref(), json),
    }
}

fn direction_flag(maximize: bool, minimize: bool) -> Option<Direction> {
    match (maximize, minimize) {
        (true, _) => Some(Direction::Max),
        (_, true) => Some(Direction::Min),
        _ => None,
    }
}

const HOW_TO_REPORT: &str = "Print lines like `ORX_METRIC {\"key\": \"val_loss\", \"value\": 0.42, \"step\": 100}` from the run's code; `orx skill metrics` has the details.";

fn show(store: &Store, run_id: &str, json: bool) -> Result<()> {
    let run = crate::local::resolve::resolve_run(store, run_id)?;
    metrics::ingest(store, &run)?;
    let summary = metrics::run_summary(store, &run.id)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "runId": run.id, "metrics": summary }))?
        );
        return Ok(());
    }
    if summary.is_empty() {
        println!(
            "Run {} has reported no metrics yet. {HOW_TO_REPORT}",
            run.id
        );
        return Ok(());
    }
    let with_split = summary.iter().any(|row| row.split.is_some());
    let with_non_finite = summary.iter().any(|row| row.non_finite > 0);
    let mut headers = vec!["KEY"];
    if with_split {
        headers.push("SPLIT");
    }
    headers.extend(["POINTS", "LAST STEP", "LAST", "MIN", "MAX"]);
    if with_non_finite {
        headers.push("NAN/INF");
    }
    let rows: Vec<Vec<String>> = summary
        .iter()
        .map(|row| {
            let mut cells = vec![row.key.clone()];
            if with_split {
                cells.push(row.split.clone().unwrap_or_default());
            }
            cells.extend([
                row.points.to_string(),
                row.last_step
                    .map(|step| step.to_string())
                    .unwrap_or_default(),
                metrics::format_value(row.last),
                optional_value(row.min),
                optional_value(row.max),
            ]);
            if with_non_finite {
                cells.push(row.non_finite.to_string());
            }
            cells
        })
        .collect();
    print_table(&headers, &rows);
    Ok(())
}

fn optional_value(value: Option<f64>) -> String {
    value.map_or_else(
        || "—".to_string(),
        |value| metrics::format_value(Some(value)),
    )
}

pub(crate) fn signed(value: f64) -> String {
    let text = metrics::format_value(Some(value));
    if value > 0.0 {
        format!("+{text}")
    } else {
        text
    }
}

fn describe(metric: &PrimaryMetric) -> String {
    let direction = match metric.direction {
        Direction::Max => "higher is better",
        Direction::Min => "lower is better",
    };
    let key = match &metric.split {
        Some(split) => format!("{} ({split} split)", metric.key),
        None => metric.key.clone(),
    };
    match metric.noise_floor {
        Some(floor) => format!(
            "{key}, {direction}; gains under {} are noise",
            metrics::format_value(Some(floor))
        ),
        None => format!("{key}, {direction}"),
    }
}

fn reported_keys(store: &Store, project_id: &str) -> Result<String> {
    for run in store.list_runs_by_project(project_id)? {
        metrics::ingest(store, &run)?;
    }
    let keys = metrics::project_keys(store, project_id)?;
    Ok(if keys.is_empty() {
        format!("No run has reported metrics yet. {HOW_TO_REPORT}")
    } else {
        format!("Keys reported so far: {}.", keys.join(", "))
    })
}

fn no_primary(store: &Store, brief: &scope::Brief) -> Result<anyhow::Error> {
    Ok(anyhow!(
        "Optimization {} has no primary metric yet. Set one with `orx metric primary {} <key> \
         --maximize` (or `--minimize`). {}",
        brief.id,
        brief.project_id,
        reported_keys(store, &brief.project_id)?
    ))
}

/// Refuses a key reported under several splits unless one is chosen, so a
/// test-split number can't stand in for the validation score.
fn require_split(store: &Store, project_id: &str, key: &str, split: Option<&str>) -> Result<()> {
    if split.is_some() {
        return Ok(());
    }
    let splits = metrics::key_splits(store, project_id, key)?;
    if splits.len() > 1 {
        let names: Vec<String> = splits
            .iter()
            .map(|split| split.clone().unwrap_or_else(|| "(none)".to_string()))
            .collect();
        return Err(anyhow!(
            "{key} is reported under several splits ({}). Choose the one to score with \
             `--split <name>`; select on validation, never on test.",
            names.join(", ")
        ));
    }
    Ok(())
}

fn runs(
    store: &Store,
    project_id: &str,
    opt: Option<&str>,
    key: Option<String>,
    split: Option<String>,
    direction: Option<Direction>,
    json: bool,
) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    // `--key` ranks the whole project's runs; otherwise the optimization's
    // runs on its primary metric.
    let brief = match (&key, opt) {
        (Some(_), None) => None,
        _ => Some(scope::resolve(store, &project.id, opt, Resolve::Existing)?),
    };
    let primary = match &brief {
        Some(brief) => metrics::primary(store, &brief.id)?,
        None => None,
    };
    let metric = match (key, primary) {
        (None, Some(primary)) => primary,
        (None, None) => match &brief {
            Some(brief) => return Err(no_primary(store, brief)?),
            None => unreachable!("a run ranking without --key resolves an optimization"),
        },
        (Some(key), Some(primary)) if key == primary.key && direction.is_none() => PrimaryMetric {
            split: split.or(primary.split),
            ..primary
        },
        (Some(key), _) => {
            let Some(direction) = direction else {
                return Err(anyhow!(
                    "Say which way is better for {key}: add `--maximize` or `--minimize`."
                ));
            };
            for run in store.list_runs_by_project(&project.id)? {
                metrics::ingest(store, &run)?;
            }
            require_split(store, &project.id, &key, split.as_deref())?;
            PrimaryMetric {
                key,
                direction,
                noise_floor: None,
                split,
            }
        }
    };
    let members = match &brief {
        Some(brief) => Some(scope::run_ids(store, brief)?),
        None => None,
    };
    let (standing, _, mut runs) =
        metrics::standing_among(store, &project.id, &metric, members.as_ref())?;
    // Scored runs best first, then the rest newest first.
    runs.sort_by(|a, b| match (a.score, b.score) {
        (Some(x), Some(y)) => metric
            .direction
            .gain(y, x)
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.created_at.cmp(&a.created_at),
    });
    let best_id = standing.best.as_ref().map(|run| run.run_id.clone());
    let baseline_id = standing
        .baseline
        .as_ref()
        .and_then(|baseline| baseline.run_id.clone());
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "metric": metric,
                "bestRunId": best_id,
                "baseline": standing.baseline,
                "runs": runs,
            }))?
        );
        return Ok(());
    }
    if runs.is_empty() {
        println!("No runs found.");
        return Ok(());
    }
    println!("Ranked by {}", describe(&metric));
    let rows: Vec<Vec<String>> = runs
        .iter()
        .map(|run| {
            let mark = if Some(&run.run_id) == best_id.as_ref() {
                "best"
            } else if Some(&run.run_id) == baseline_id.as_ref() {
                "baseline"
            } else if run.probe {
                "probe"
            } else {
                ""
            };
            vec![
                run.run_id.clone(),
                run.experiment.clone(),
                run.experiment_id.clone(),
                run.status.clone(),
                run.score.map_or_else(
                    || "—".to_string(),
                    |score| metrics::format_value(Some(score)),
                ),
                mark.to_string(),
            ]
        })
        .collect();
    let column = metric.key.to_uppercase();
    print_table(
        &["RUN", "EXPERIMENT", "EXP ID", "STATUS", &column, ""],
        &rows,
    );
    Ok(())
}

fn primary(
    store: &Store,
    project_id: &str,
    opt: Option<&str>,
    key: Option<String>,
    direction: Option<Direction>,
    noise_floor: Option<f64>,
    split: Option<String>,
) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let Some(key) = key else {
        if direction.is_some() || noise_floor.is_some() || split.is_some() {
            return Err(anyhow!(
                "Name the metric key to set, e.g. `orx metric primary {} val_loss --minimize`.",
                project.id
            ));
        }
        let brief = scope::resolve(store, &project.id, opt, Resolve::Existing)?;
        match metrics::primary(store, &brief.id)? {
            Some(metric) => println!(
                "Primary metric of optimization {}: {}",
                brief.id,
                describe(&metric)
            ),
            None => println!(
                "Optimization {} has no primary metric yet. {}",
                brief.id,
                reported_keys(store, &project.id)?
            ),
        }
        return Ok(());
    };
    let brief = scope::resolve(store, &project.id, opt, Resolve::OrCreate)?;
    let current = metrics::primary(store, &brief.id)?;
    let Some(direction) = direction else {
        return Err(anyhow!(
            "Say which way is better: add `--maximize` or `--minimize`."
        ));
    };
    for run in store.list_runs_by_project(&project.id)? {
        metrics::ingest(store, &run)?;
    }
    require_split(store, &project.id, &key, split.as_deref())?;
    // Re-setting the same key keeps its noise floor unless a new one is given.
    let noise_floor = noise_floor.or_else(|| {
        current
            .as_ref()
            .filter(|current| current.key == key)
            .and_then(|current| current.noise_floor)
    });
    let metric = PrimaryMetric {
        key,
        direction,
        noise_floor,
        split,
    };
    metrics::set_primary(store, &brief.id, &metric)?;
    println!(
        "Primary metric for optimization {} is now {}.",
        brief.id,
        describe(&metric)
    );
    Ok(())
}

fn best(store: &Store, project_id: &str, opt: Option<&str>, json: bool) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let brief = scope::resolve(store, &project.id, opt, Resolve::Existing)?;
    let Some(metric) = metrics::primary(store, &brief.id)? else {
        return Err(no_primary(store, &brief)?);
    };
    let (standing, path, runs) = metrics::standing(store, &brief, &metric)?;
    let trials = super::ideas::trials(store, &brief.id, &runs)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "metric": metric,
                "best": standing.best,
                "baseline": standing.baseline,
                "path": path,
                "history": standing.history,
                "trials": trials,
            }))?
        );
        return Ok(());
    }
    let Some(best) = standing.best else {
        println!(
            "No finished run has reported {} yet. {}",
            metric.key, HOW_TO_REPORT
        );
        return Ok(());
    };
    let score = best.score.unwrap_or_default();
    let best_text = format!(
        "Best {} is {} (run {}, experiment {} [{}])",
        metric.key,
        metrics::format_value(Some(score)),
        best.run_id,
        best.experiment,
        best.experiment_id
    );
    match &standing.baseline {
        Some(baseline) if baseline.run_id.as_ref() == Some(&best.run_id) => {
            println!("{best_text}, and it is the baseline.")
        }
        Some(baseline) => {
            let source = match &baseline.run_id {
                Some(run) => format!("run {run}"),
                None => format!(
                    "mean of {} probe run{}",
                    baseline.probe_runs,
                    if baseline.probe_runs == 1 { "" } else { "s" }
                ),
            };
            println!(
                "{best_text}: {} from the baseline's {} ({source}, {}).",
                signed(score - baseline.score),
                metrics::format_value(Some(baseline.score)),
                baseline.experiment
            );
        }
        None => println!(
            "{best_text}. Its baseline has no scored run yet, so there is no gain to report."
        ),
    }
    println!(
        "Trials behind it: {} finished run{}, {} tested idea{} ({} kept).",
        trials.runs,
        if trials.runs == 1 { "" } else { "s" },
        trials.ideas_tested,
        if trials.ideas_tested == 1 { "" } else { "s" },
        trials.ideas_kept
    );
    if path.len() > 1 {
        println!("\nPath from the baseline:");
        let column = metric.key.to_uppercase();
        let with_ideas = path.iter().any(|step| step.idea.is_some());
        let rows: Vec<Vec<String>> = path
            .iter()
            .map(|step| {
                let mut row = vec![step.experiment.clone(), step.experiment_id.clone()];
                if with_ideas {
                    row.push(step.idea.clone().unwrap_or_default());
                }
                row.extend([
                    match (&step.run_id, step.probe_runs) {
                        (Some(run), _) => run.clone(),
                        (None, 0) => "—".to_string(),
                        (None, probes) => format!("mean of {probes} probes"),
                    },
                    step.score.map_or_else(
                        || "—".to_string(),
                        |score| metrics::format_value(Some(score)),
                    ),
                    step.delta.map(signed).unwrap_or_default(),
                ]);
                row
            })
            .collect();
        let mut headers = vec!["EXPERIMENT", "EXP ID"];
        if with_ideas {
            headers.push("IDEA");
        }
        headers.extend(["RUN", &column, "CHANGE"]);
        print_table(&headers, &rows);
    }
    Ok(())
}
