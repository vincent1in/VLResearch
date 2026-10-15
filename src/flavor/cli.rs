//! VLResearch's own `orx` commands. One flattened variant in `src/main.rs`
//! adds all of them to upstream's command list.

use clap::{Args, Subcommand};
use serde_json::json;

use super::metrics::{self, Direction, PrimaryMetric};
use crate::error::{anyhow, Result};
use crate::output::print_table;
use crate::store::Store;

#[derive(Subcommand, Debug)]
pub(crate) enum FlavorCommand {
    /// Record and compare run metrics (`ORX_METRIC {json}` lines in run output).
    Metric(MetricArgs),
}

impl FlavorCommand {
    /// The command's label for orx's command accounting.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            FlavorCommand::Metric(_) => "metric",
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct MetricArgs {
    #[command(subcommand)]
    pub command: MetricCommand,
}

#[derive(Subcommand, Debug)]
pub(crate) enum MetricCommand {
    /// Show the metrics a run has reported so far.
    Show {
        run_id: String,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Rank a project's runs by a metric: its primary metric unless `--key`.
    Runs {
        project_id: String,
        /// Rank by this key instead of the primary metric.
        #[arg(long)]
        key: Option<String>,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Show the project's primary metric, or set it: a key plus `--maximize`
    /// or `--minimize`.
    Primary {
        project_id: String,
        /// The key to optimize, as printed in `ORX_METRIC` lines. Use a
        /// validation metric; keep test splits for final reports.
        key: Option<String>,
        /// Higher is better.
        #[arg(long, conflicts_with = "minimize")]
        maximize: bool,
        /// Lower is better.
        #[arg(long)]
        minimize: bool,
        /// The smallest gain that counts as an improvement rather than noise.
        #[arg(long, value_name = "AMOUNT")]
        noise_floor: Option<f64>,
    },
    /// The best finished run against the baseline, and the experiments on the
    /// path between them with each one's change.
    Best {
        project_id: String,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) async fn run(command: FlavorCommand) -> Result<()> {
    match command {
        FlavorCommand::Metric(args) => tokio::task::spawn_blocking(move || metric(args.command))
            .await
            .map_err(|error| anyhow!("orx metric stopped unexpectedly: {error}"))?,
    }
}

/// Whether a tokenized `orx …` call only reads, for Claude's plan-mode gate.
pub(crate) fn is_readonly(tokens: &[&str]) -> bool {
    use clap::Parser;
    match crate::Cli::try_parse_from(tokens) {
        Ok(cli) => matches!(
            cli.command,
            Some(crate::Command::Flavor(FlavorCommand::Metric(MetricArgs {
                command: MetricCommand::Show { .. }
                    | MetricCommand::Runs { .. }
                    | MetricCommand::Best { .. }
                    | MetricCommand::Primary { key: None, .. },
            })))
        ),
        Err(error) => matches!(error.kind(), clap::error::ErrorKind::DisplayHelp),
    }
}

fn metric(command: MetricCommand) -> Result<()> {
    let store = Store::open()?;
    match command {
        MetricCommand::Show { run_id, json } => show(&store, &run_id, json),
        MetricCommand::Runs {
            project_id,
            key,
            json,
        } => runs(&store, &project_id, key, json),
        MetricCommand::Primary {
            project_id,
            key,
            maximize,
            minimize,
            noise_floor,
        } => primary(&store, &project_id, key, maximize, minimize, noise_floor),
        MetricCommand::Best { project_id, json } => best(&store, &project_id, json),
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

fn describe(metric: &PrimaryMetric) -> String {
    let direction = match metric.direction {
        Direction::Max => "higher is better",
        Direction::Min => "lower is better",
    };
    match metric.noise_floor {
        Some(floor) => format!(
            "{} ({direction}; gains under {} are noise)",
            metric.key,
            metrics::format_value(Some(floor))
        ),
        None => format!("{} ({direction})", metric.key),
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

fn no_primary(store: &Store, project_id: &str) -> Result<anyhow::Error> {
    Ok(anyhow!(
        "Project {project_id} has no primary metric yet. Set one with `orx metric primary \
         {project_id} <key> --maximize` (or `--minimize`). {}",
        reported_keys(store, project_id)?
    ))
}

fn runs(store: &Store, project_id: &str, key: Option<String>, json: bool) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let primary = metrics::primary(store, &project.id)?;
    let metric = match (key, primary) {
        (Some(key), Some(primary)) if key == primary.key => primary,
        (Some(key), primary) => PrimaryMetric {
            key,
            direction: primary.map_or(Direction::Max, |primary| primary.direction),
            noise_floor: None,
        },
        (None, Some(primary)) => primary,
        (None, None) => return Err(no_primary(store, &project.id)?),
    };
    let (standing, _, mut runs) = metrics::standing(store, &project.id, &metric)?;
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
    let baseline_id = standing.baseline.as_ref().map(|run| run.run_id.clone());
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "metric": metric,
                "bestRunId": best_id,
                "baselineRunId": baseline_id,
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
            } else {
                ""
            };
            vec![
                run.run_id.clone(),
                run.experiment.clone(),
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
    print_table(&["RUN", "EXPERIMENT", "STATUS", &column, ""], &rows);
    Ok(())
}

fn primary(
    store: &Store,
    project_id: &str,
    key: Option<String>,
    maximize: bool,
    minimize: bool,
    noise_floor: Option<f64>,
) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let Some(key) = key else {
        if maximize || minimize || noise_floor.is_some() {
            return Err(anyhow!(
                "Name the metric key to set, e.g. `orx metric primary {} val_loss --minimize`.",
                project.id
            ));
        }
        match metrics::primary(store, &project.id)? {
            Some(metric) => println!("Primary metric: {}", describe(&metric)),
            None => println!(
                "No primary metric yet. {}",
                reported_keys(store, &project.id)?
            ),
        }
        return Ok(());
    };
    let direction = match (maximize, minimize) {
        (true, false) => Direction::Max,
        (false, true) => Direction::Min,
        _ => {
            return Err(anyhow!(
                "Say which way is better: add `--maximize` or `--minimize`."
            ))
        }
    };
    let metric = PrimaryMetric {
        key,
        direction,
        noise_floor,
    };
    metrics::set_primary(store, &project.id, &metric)?;
    println!(
        "Primary metric for {} is now {}.",
        project.id,
        describe(&metric)
    );
    Ok(())
}

fn best(store: &Store, project_id: &str, json: bool) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let Some(metric) = metrics::primary(store, &project.id)? else {
        return Err(no_primary(store, &project.id)?);
    };
    let (standing, path, _) = metrics::standing(store, &project.id, &metric)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "metric": metric,
                "best": standing.best,
                "baseline": standing.baseline,
                "path": path,
                "history": standing.history,
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
    match standing
        .baseline
        .as_ref()
        .and_then(|baseline| baseline.score.map(|s| (baseline, s)))
    {
        Some((baseline, base)) if baseline.run_id != best.run_id => println!(
            "Best {} is {} (run {}, {}): {} from the baseline's {} (run {}, {}).",
            metric.key,
            metrics::format_value(Some(score)),
            best.run_id,
            best.experiment,
            signed(score - base),
            metrics::format_value(Some(base)),
            baseline.run_id,
            baseline.experiment,
        ),
        _ => println!(
            "Best {} is {} (run {}, {}), and it is the baseline.",
            metric.key,
            metrics::format_value(Some(score)),
            best.run_id,
            best.experiment
        ),
    }
    if path.len() > 1 {
        println!("\nPath from the baseline:");
        let column = metric.key.to_uppercase();
        let rows: Vec<Vec<String>> = path
            .iter()
            .map(|step| {
                vec![
                    step.experiment.clone(),
                    step.run_id.clone().unwrap_or_else(|| "—".to_string()),
                    step.score.map_or_else(
                        || "—".to_string(),
                        |score| metrics::format_value(Some(score)),
                    ),
                    step.delta.map(signed).unwrap_or_default(),
                ]
            })
            .collect();
        print_table(&["EXPERIMENT", "RUN", &column, "CHANGE"], &rows);
    }
    Ok(())
}

fn signed(value: f64) -> String {
    let text = metrics::format_value(Some(value));
    if value > 0.0 {
        format!("+{text}")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::is_readonly;

    #[test]
    fn only_reading_forms_pass_the_plan_gate() {
        for call in [
            "orx metric show run_1",
            "orx metric runs proj_1 --key val_ic",
            "orx metric best proj_1 --json",
            "orx metric primary proj_1",
            "orx metric --help",
        ] {
            let tokens: Vec<&str> = call.split_whitespace().collect();
            assert!(is_readonly(&tokens), "{call}");
        }
        for call in [
            "orx metric primary proj_1 val_ic --maximize",
            "orx metric bogus",
            "orx exp run exp_1",
        ] {
            let tokens: Vec<&str> = call.split_whitespace().collect();
            assert!(!is_readonly(&tokens), "{call}");
        }
    }
}
