//! VLResearch's own `orx` commands. One flattened variant in `src/main.rs`
//! adds all of them to upstream's command list.

use clap::{Args, Subcommand};

use crate::error::Result;

#[derive(Subcommand, Debug)]
pub(crate) enum FlavorCommand {
    /// Record and compare run metrics (`ORX_METRIC {json}` lines in run output).
    Metric(MetricArgs),
    /// Queue ideas to try, each with a forecast, a cost and a kill test, and
    /// record what each did.
    Idea(IdeaArgs),
    /// Optimizations and their briefs. Each chat runs its own: the objective,
    /// limits, scope, budget and stop rule agreed with the user, which its
    /// runs are held to once confirmed.
    Brief(BriefArgs),
    /// Record what the project has learned: claims, the runs that back or
    /// undercut them, how claims relate, and verdicts over time.
    Finding(FindingArgs),
}

#[derive(Args, Debug)]
pub(crate) struct FindingArgs {
    #[command(subcommand)]
    pub command: FindingCommand,
}

#[derive(Subcommand, Debug)]
pub(crate) enum FindingCommand {
    /// Add a claim.
    Add {
        project_id: String,
        statement: String,
        /// What the claim is about.
        #[arg(long, value_enum)]
        kind: super::findings::Kind,
        /// The idea whose test produced or tests it.
        #[arg(long, value_name = "IDEA_ID")]
        idea: Option<String>,
        /// The optimization whose work produced it. Defaults to the idea's,
        /// or the one this chat runs.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,
    },
    /// A project's findings, most recently changed first.
    List {
        project_id: String,
        #[arg(long, value_enum)]
        status: Option<super::findings::Status>,
        #[arg(long, value_enum)]
        kind: Option<super::findings::Kind>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// One finding with its evidence, links and assessments.
    Show {
        finding_id: String,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Record a run, experiment or observation that backs or undercuts a
    /// claim. A run brings a snapshot of its metrics.
    Evidence {
        finding_id: String,
        #[arg(long, conflicts_with = "refutes", required_unless_present = "refutes")]
        supports: bool,
        #[arg(long)]
        refutes: bool,
        #[arg(long, value_name = "RUN_ID")]
        run: Option<String>,
        #[arg(long, value_name = "EXPERIMENT_ID")]
        experiment: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Link two findings: `<findingId> <relation> <otherId>`.
    Relate {
        finding_id: String,
        #[arg(value_enum)]
        relation: super::findings::Relation,
        other_id: String,
    },
    /// Add a verdict. Earlier verdicts stay on record.
    Assess {
        finding_id: String,
        #[arg(long, value_enum)]
        verdict: super::findings::Verdict,
        /// From 0 to 1.
        #[arg(long)]
        confidence: Option<f64>,
        /// Where the verdict holds: universe, period, regime, model family.
        #[arg(long)]
        scope: Option<String>,
        /// What the evidence doesn't cover yet.
        #[arg(long)]
        gaps: Option<String>,
        /// The evidence that would settle it.
        #[arg(long)]
        next: Option<String>,
    },
    /// Reword a claim.
    Edit {
        finding_id: String,
        #[arg(long)]
        statement: String,
    },
    /// Mark a finding settled: agents can still add evidence, but can't
    /// reword or re-judge it.
    Lock { finding_id: String },
    /// Reopen a locked finding. Only the user can, from a terminal.
    Unlock { finding_id: String },
}

#[derive(Args, Debug)]
pub(crate) struct BriefArgs {
    #[command(subcommand)]
    pub command: BriefCommand,
}

// Parsed once per process, so the size of `Set` doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
pub(crate) enum BriefCommand {
    /// Show the brief, what it still needs, and the budget used so far.
    Show {
        project_id: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// The project's optimizations: each one's objective, metric, chat and
    /// state.
    List {
        project_id: String,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Run an existing optimization in this chat (it stops running any other).
    Use { opt_id: String },
    /// Fill in or change the brief, starting an optimization for this chat
    /// if it runs none. Any change reopens a confirmed brief.
    Set {
        project_id: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,

        /// The goal in words. The number that measures it is the primary
        /// metric (`orx metric primary`).
        #[arg(long)]
        objective: Option<String>,
        /// What runs may change: feature code, model code, loaders, …
        #[arg(long)]
        may_change: Option<String>,
        /// What must stay fixed: evaluation code, split dates, the
        /// out-of-sample window, …
        #[arg(long)]
        must_not_change: Option<String>,
        /// A repository file no run may change, checked at every launch
        /// (repeatable).
        #[arg(long, value_name = "PATH")]
        lock: Vec<String>,
        /// Stop locking a file (repeatable).
        #[arg(long, value_name = "PATH")]
        unlock: Vec<String>,
        /// Confine runs to a part of the system: once any path is in the
        /// scope, a run may change only files inside it, checked at every
        /// launch (repeatable).
        #[arg(long, value_name = "PATH")]
        scope: Vec<String>,
        /// Take a path out of the scope (repeatable).
        #[arg(long, value_name = "PATH")]
        unscope: Vec<String>,
        /// How many runs (probes excluded) the brief allows.
        #[arg(long, value_name = "RUNS")]
        budget_runs: Option<i64>,
        /// GPU-hours the brief allows (reported, not enforced).
        #[arg(long, value_name = "HOURS")]
        budget_gpu_hours: Option<f64>,
        /// Days from confirmation the brief allows.
        #[arg(long, value_name = "DAYS")]
        budget_days: Option<f64>,
        /// Start the budget over at the next confirmation. Without it, runs
        /// and days keep counting from the first confirmation.
        #[arg(long)]
        reset_budget: bool,
        /// Where the user wants the search to start.
        #[arg(long)]
        pointers: Option<String>,
        /// Datasets, earlier experiments and papers to draw on.
        #[arg(long)]
        sources: Option<String>,
        /// When to stop, e.g. no validation gain in 5 runs.
        #[arg(long)]
        stop_rule: Option<String>,
    },
    /// Confirm the brief once the user has agreed to it. Locked files are
    /// pinned to their content on the baseline branch.
    Confirm {
        project_id: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,
    },
}

impl FlavorCommand {
    /// The command's label for orx's command accounting.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            FlavorCommand::Metric(_) => "metric",
            FlavorCommand::Idea(_) => "idea",
            FlavorCommand::Brief(_) => "brief",
            FlavorCommand::Finding(_) => "finding",
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct IdeaArgs {
    #[command(subcommand)]
    pub command: IdeaCommand,
}

/// Fields shared by `idea add` and `idea edit`.
#[derive(Args, Debug, Default)]
pub(crate) struct IdeaFieldArgs {
    /// What kind of work it is.
    #[arg(long, value_enum)]
    pub kind: Option<super::ideas::Kind>,
    /// How it relates to what has been tried.
    #[arg(long, value_enum)]
    pub class: Option<super::ideas::Class>,
    /// 1 (most urgent) to 3.
    #[arg(long)]
    pub priority: Option<i64>,
    /// What should happen and why.
    #[arg(long)]
    pub hypothesis: Option<String>,
    /// The cheap test (under about a GPU-hour) whose negative result kills it.
    #[arg(long)]
    pub kill_test: Option<String>,
    /// Forecast change in the primary metric, in its units and sign.
    #[arg(long, allow_negative_numbers = true)]
    pub forecast: Option<f64>,
    /// The forecast's 80% interval.
    #[arg(long, num_args = 2, value_names = ["LOW", "HIGH"], allow_negative_numbers = true)]
    pub interval: Option<Vec<f64>>,
    /// Expected compute in GPU-hours.
    #[arg(long, value_name = "HOURS")]
    pub cost_hours: Option<f64>,
    /// An idea that has to close before this one can start.
    #[arg(long, value_name = "IDEA_ID")]
    pub blocked_by: Option<String>,
    /// Who proposed it. Defaults to agent inside a chat, user otherwise.
    #[arg(long, value_enum)]
    pub author: Option<super::ideas::Author>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum IdeaCommand {
    /// Add an idea to an optimization's queue.
    Add {
        project_id: String,
        title: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,

        #[command(flatten)]
        fields: IdeaFieldArgs,
    },
    /// Change an idea's fields; anything not given stays as it is.
    Edit {
        idea_id: String,
        #[arg(long)]
        title: Option<String>,
        #[command(flatten)]
        fields: IdeaFieldArgs,
    },
    /// An optimization's open and running ideas; `--all` adds closed ones.
    List {
        project_id: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,
        #[arg(long)]
        all: bool,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// One idea in full.
    Show {
        idea_id: String,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// The idea to work on next, and why.
    Next {
        project_id: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,

        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Record that an experiment tests the idea.
    Start {
        idea_id: String,
        experiment_id: String,
    },
    /// Close a tested idea. It is kept when its experiment beat the parent by
    /// more than the noise floor and refuted otherwise, unless `--kept` or
    /// `--refuted` says so.
    Done {
        idea_id: String,
        /// What showed it: run ids, numbers, a sentence.
        #[arg(long)]
        evidence: Option<String>,
        #[arg(long, conflicts_with = "refuted")]
        kept: bool,
        #[arg(long)]
        refuted: bool,
    },
    /// Close an idea whose kill test fired.
    Kill {
        idea_id: String,
        /// What the kill test showed.
        #[arg(long)]
        evidence: String,
    },
    /// Close an idea without testing it.
    Drop {
        idea_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// How well tested ideas' forecasts held up.
    Calibration {
        project_id: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,

        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
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
    /// Rank an optimization's runs by its primary metric, or the project's
    /// runs by `--key`.
    Runs {
        project_id: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,

        /// Rank by this key instead of the primary metric (say which way is
        /// better with `--maximize` or `--minimize`).
        #[arg(long)]
        key: Option<String>,
        /// Score the key in this split only.
        #[arg(long)]
        split: Option<String>,
        /// Higher is better.
        #[arg(long, conflicts_with = "minimize")]
        maximize: bool,
        /// Lower is better.
        #[arg(long)]
        minimize: bool,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Show the optimization's primary metric, or set it: a key plus
    /// `--maximize` or `--minimize`.
    Primary {
        project_id: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,

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
        /// Kept when the same key is set again without it.
        #[arg(long, value_name = "AMOUNT")]
        noise_floor: Option<f64>,
        /// The split to score, when runs report the key under several.
        #[arg(long)]
        split: Option<String>,
    },
    /// The optimization's best finished run against the baseline, and the
    /// experiments on the path between them with each one's change.
    Best {
        project_id: String,
        /// The optimization to act on. Defaults to the one this chat runs,
        /// or the project's only one.
        #[arg(long = "opt", value_name = "OPT_ID")]
        opt: Option<String>,

        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
}

/// Runs a command. They only read and write the local store, so they run in
/// place, like upstream's `orx runs`.
pub(crate) async fn run(command: FlavorCommand) -> Result<()> {
    match command {
        FlavorCommand::Metric(args) => super::metric_cli::run(args.command),
        FlavorCommand::Idea(args) => super::idea_cli::run(args.command),
        FlavorCommand::Brief(args) => super::brief_cli::run(args.command),
        FlavorCommand::Finding(args) => super::finding_cli::run(args.command),
    }
}

/// Whether a tokenized `orx …` call only reads, for Claude's plan-mode gate.
pub(crate) fn is_readonly(tokens: &[&str]) -> bool {
    use clap::Parser;
    match crate::Cli::try_parse_from(tokens) {
        Ok(cli) => matches!(
            cli.command,
            Some(crate::Command::Flavor(
                FlavorCommand::Metric(MetricArgs {
                    command: MetricCommand::Show { .. }
                        | MetricCommand::Runs { .. }
                        | MetricCommand::Best { .. }
                        | MetricCommand::Primary { key: None, .. },
                }) | FlavorCommand::Idea(IdeaArgs {
                    command: IdeaCommand::List { .. }
                        | IdeaCommand::Show { .. }
                        | IdeaCommand::Next { .. }
                        | IdeaCommand::Calibration { .. },
                }) | FlavorCommand::Brief(BriefArgs {
                    command: BriefCommand::Show { .. } | BriefCommand::List { .. },
                }) | FlavorCommand::Finding(FindingArgs {
                    command: FindingCommand::List { .. } | FindingCommand::Show { .. },
                })
            ))
        ),
        Err(error) => matches!(error.kind(), clap::error::ErrorKind::DisplayHelp),
    }
}

#[cfg(test)]
mod tests {
    use super::is_readonly;

    #[test]
    fn plan_mode_runs_the_reading_forms_without_approval() {
        use crate::local::harness::command_is_readonly;
        for call in [
            "orx metric best p",
            "orx idea next p",
            "orx brief show p",
            "orx finding list p",
        ] {
            assert!(command_is_readonly(call), "{call}");
        }
        for call in [
            "orx idea add p x",
            "orx brief confirm p",
            "orx finding lock f",
        ] {
            assert!(!command_is_readonly(call), "{call}");
        }
    }

    #[test]
    fn only_reading_forms_pass_the_plan_gate() {
        for call in [
            "orx metric show run_1",
            "orx metric runs proj_1 --key val_ic",
            "orx metric best proj_1 --json",
            "orx metric primary proj_1",
            "orx metric --help",
            "orx idea list proj_1 --all",
            "orx idea show idea_1",
            "orx idea next proj_1",
            "orx idea calibration proj_1 --json",
            "orx brief show proj_1",
            "orx brief show proj_1 --opt opt_1",
            "orx brief list proj_1",
            "orx finding list proj_1 --status supported",
            "orx finding show fnd_1 --json",
        ] {
            let tokens: Vec<&str> = call.split_whitespace().collect();
            assert!(is_readonly(&tokens), "{call}");
        }
        for call in [
            "orx metric primary proj_1 val_ic --maximize",
            "orx metric bogus",
            "orx exp run exp_1",
            "orx idea add proj_1 Volume",
            "orx idea done idea_1",
            "orx idea start idea_1 exp_1",
            "orx brief confirm proj_1",
            "orx brief set proj_1 --objective x",
            "orx brief use opt_1",
            "orx finding unlock fnd_1",
        ] {
            let tokens: Vec<&str> = call.split_whitespace().collect();
            assert!(!is_readonly(&tokens), "{call}");
        }
    }
}
