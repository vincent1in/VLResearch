//! A seeded simulation of the improvement loop, run through the loop's own
//! decision code: the selector (`ideas::pick`), the keep rule
//! (`ideas::judge`), the rule for the best (`metrics::best_by_rule`), the
//! scores it compares (`metrics::experiment_reference`) and the ablation
//! checks a new best queues (`ideas::ablation_targets`). The replicate the
//! `orx-improve` skill has the agent queue for a new best is mirrored here.
//!
//! Each campaign draws a research problem whose truth is known: every idea's
//! true effect and cost, and the agent's forecasts of them, which are noisy,
//! optimistic and overconfident. Every strategy works the same problem with
//! the same luck (the same seed noise for the same run of the same idea), so
//! the differences come from the strategies alone. Scores are in units of
//! one run's seed noise (its standard deviation).
//!
//! The full report, written to `flavor/bench/loop.md`:
//!
//! ```sh
//! cargo test --release --bin orx flavor::bench::report -- --ignored --nocapture
//! ```
//!
//! `VLR_BENCH_CAMPAIGNS` sets the campaigns per scenario (default 2000). A
//! smaller run of the typical scenario guards the headline results in CI.

use std::collections::HashMap;
use std::fmt::Write as _;

use super::ideas::{self, Author, Class, Idea, Kind, Status};
use super::metrics::{self, Direction, PathStep, PrimaryMetric, ScoredRun};

/// SplitMix64: small, and the same on every platform.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform on [0, 1).
    fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal, by Box–Muller.
    fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(f64::MIN_POSITIVE);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

/// The seed of one random stream: a campaign, what the stream is for, and
/// which one.
fn stream(campaign: u64, purpose: u64, index: u64) -> u64 {
    let mut rng = Rng::new(
        campaign
            ^ purpose.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ index.wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
    );
    rng.next()
}

/// The normal quantile at 90%: an 80% interval is the mean ± this many SDs.
const Z80: f64 = 1.2816;
/// Seed runs of the baseline, which measure the noise.
const BASELINE_SEEDS: usize = 3;
const BASE: &str = "baseline";

/// A kind of research problem.
#[derive(Debug, Clone, Copy)]
struct Scenario {
    name: &'static str,
    about: &'static str,
    ideas: usize,
    /// GPU-hours for the whole campaign, baseline runs included.
    budget: f64,
    /// Seed-to-seed SD of one run's score.
    noise: f64,
    /// SD of each change's luck on the validation split: part of every run's
    /// score once the change is in, the same for every seed, and not part of
    /// the true (out-of-sample) score. Selecting on validation rewards it,
    /// and no rerun can tell it from a real gain.
    overfit: f64,
    /// True effect of an incremental and of an orthogonal idea: mean, SD.
    incremental: (f64, f64),
    orthogonal: (f64, f64),
    orthogonal_share: f64,
    /// The agent's forecast error: its SD, and its mean (optimism above 0).
    forecast_noise: f64,
    forecast_bias: f64,
    /// The stated 80% half-width over an honest one (below 1: overconfident).
    interval_scale: f64,
    /// GPU-hours of one run: median and log-SD. Orthogonal ideas cost half
    /// as much again.
    cost_median: f64,
    cost_spread: f64,
    /// Log-SD of the agent's cost estimates.
    cost_error: f64,
}

const TYPICAL: Scenario = Scenario {
    name: "Typical",
    about: "true effects about the size of seed noise, and most ideas no help; forecasts \
            informative but optimistic and overconfident",
    ideas: 40,
    budget: 40.0,
    noise: 1.0,
    overfit: 0.0,
    incremental: (-0.1, 0.6),
    orthogonal: (-0.4, 1.4),
    orthogonal_share: 0.3,
    forecast_noise: 0.8,
    forecast_bias: 0.4,
    interval_scale: 0.6,
    cost_median: 1.0,
    cost_spread: 0.7,
    cost_error: 0.3,
};

const SCENARIOS: [Scenario; 7] = [
    TYPICAL,
    Scenario {
        name: "Clear signal",
        about: "seed noise a third of the typical, so true effects stand out",
        noise: 0.35,
        ..TYPICAL
    },
    Scenario {
        name: "Overfitting the validation split",
        about: "each change also has luck on the validation split, half the seed noise",
        overfit: 0.5,
        ..TYPICAL
    },
    Scenario {
        name: "Mostly harmful ideas",
        about: "most ideas make things worse",
        incremental: (-0.4, 0.5),
        orthogonal: (-0.8, 1.2),
        ..TYPICAL
    },
    Scenario {
        name: "Uninformative forecasts",
        about: "the agent's forecasts are mostly noise",
        forecast_noise: 2.5,
        ..TYPICAL
    },
    Scenario {
        name: "Calibrated forecaster",
        about: "forecasts unbiased, intervals honest",
        forecast_noise: 0.6,
        forecast_bias: 0.0,
        interval_scale: 1.0,
        ..TYPICAL
    },
    Scenario {
        name: "Tight budget",
        about: "15 GPU-hours instead of 40",
        budget: 15.0,
        ..TYPICAL
    },
];

/// One idea the agent could try, with the truth about it.
#[derive(Debug, Clone)]
struct Candidate {
    class: Class,
    effect: f64,
    /// Its luck on the validation split.
    luck: f64,
    cost: f64,
    forecast: f64,
    half_width: f64,
    cost_estimate: f64,
}

fn problem(scenario: &Scenario, campaign: u64) -> Vec<Candidate> {
    let mut rng = Rng::new(stream(campaign, 1, 0));
    (0..scenario.ideas)
        .map(|_| {
            let orthogonal = rng.uniform() < scenario.orthogonal_share;
            let (mean, sd) = if orthogonal {
                scenario.orthogonal
            } else {
                scenario.incremental
            };
            let effect = mean + sd * rng.normal();
            let size = if orthogonal { 1.5 } else { 1.0 };
            let cost = size * scenario.cost_median * (scenario.cost_spread * rng.normal()).exp();
            let forecast = effect + scenario.forecast_bias + scenario.forecast_noise * rng.normal();
            Candidate {
                class: if orthogonal {
                    Class::Orthogonal
                } else {
                    Class::Incremental
                },
                effect,
                luck: scenario.overfit * rng.normal(),
                cost,
                forecast,
                half_width: Z80 * scenario.forecast_noise * scenario.interval_scale,
                cost_estimate: cost * (scenario.cost_error * rng.normal()).exp(),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Order {
    /// `ideas::pick`, as `orx idea next` does.
    Selector,
    /// The highest forecast first, whatever it costs.
    Forecast,
    /// Any open idea.
    Random,
}

/// How much an attempt has to gain to be kept, and to take over the best.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Floor {
    Zero,
    /// This many standard deviations of the baseline's seed runs.
    Deviations(f64),
    /// The spread (highest minus lowest) of the baseline's seed runs.
    Spread,
}

#[derive(Debug, Clone, Copy)]
struct Strategy {
    name: &'static str,
    order: Order,
    /// The baseline is measured with seed runs (probes); otherwise with one
    /// run.
    seeded: bool,
    floor: Floor,
    /// Rerun each new best with a new seed before building on it.
    replicate: bool,
    /// Queue ablation checks for the steps behind each new best.
    ablate: bool,
}

/// The loop as VLResearch runs it by default: the noise floor at the spread
/// of the baseline's seed runs, as `orx-grill` has it measured, each new best
/// replicated, and ablation checks on.
const VLRESEARCH: Strategy = Strategy {
    name: "VLResearch loop",
    order: Order::Selector,
    seeded: true,
    floor: Floor::Spread,
    replicate: true,
    ablate: true,
};
/// A plain autoresearch loop: try the most promising idea, keep any gain.
const KEEP_IF_HIGHER: Strategy = Strategy {
    name: "Keep if higher, best forecast first",
    order: Order::Forecast,
    seeded: false,
    floor: Floor::Zero,
    replicate: false,
    ablate: false,
};
const RANDOM: Strategy = Strategy {
    name: "Keep if higher, random order",
    order: Order::Random,
    ..KEEP_IF_HIGHER
};

const STRATEGIES: [Strategy; 7] = [
    VLRESEARCH,
    Strategy {
        name: "  without replicates",
        replicate: false,
        ..VLRESEARCH
    },
    Strategy {
        name: "  without ablation checks",
        ablate: false,
        ..VLRESEARCH
    },
    Strategy {
        name: "  floor at one SD of the seed runs",
        floor: Floor::Deviations(1.0),
        ..VLRESEARCH
    },
    Strategy {
        name: "  floor at zero",
        floor: Floor::Zero,
        ..VLRESEARCH
    },
    KEEP_IF_HIGHER,
    RANDOM,
];

/// The experiments and runs of one campaign, with the truth behind them.
struct Lab {
    campaign: u64,
    noise: f64,
    runs: Vec<ScoredRun>,
    truth: HashMap<String, f64>,
    /// The validation split's luck, summed over an experiment's changes.
    luck: HashMap<String, f64>,
    parent: HashMap<String, String>,
    /// Which noise stream an experiment's runs draw from.
    origin: HashMap<String, u64>,
    /// GPU-hours of one run of each experiment.
    cost: HashMap<String, f64>,
    hours: f64,
}

impl Lab {
    fn new(campaign: u64, noise: f64) -> Self {
        Lab {
            campaign,
            noise,
            runs: Vec::new(),
            truth: HashMap::new(),
            luck: HashMap::new(),
            parent: HashMap::new(),
            origin: HashMap::new(),
            cost: HashMap::new(),
            hours: 0.0,
        }
    }

    fn experiment(
        &mut self,
        id: &str,
        parent: Option<&str>,
        (truth, luck): (f64, f64),
        origin: u64,
        cost: f64,
    ) {
        self.truth.insert(id.to_string(), truth);
        self.luck.insert(id.to_string(), luck);
        if let Some(parent) = parent {
            self.parent.insert(id.to_string(), parent.to_string());
        }
        self.origin.insert(id.to_string(), origin);
        self.cost.insert(id.to_string(), cost);
    }

    /// One run of an experiment, with the next seed of its noise stream.
    fn run(&mut self, id: &str, probe: bool) {
        let index = self
            .runs
            .iter()
            .filter(|run| run.experiment_id == id)
            .count() as u64;
        let noise = Rng::new(stream(self.campaign, self.origin[id], index)).normal();
        let tick = self.runs.len() as i64 + 1;
        self.runs.push(ScoredRun {
            run_id: format!("run{tick}"),
            experiment_id: id.to_string(),
            experiment: id.to_string(),
            status: "done".to_string(),
            created_at: tick,
            ended_at: Some(tick),
            score: Some(self.truth[id] + self.luck[id] + self.noise * noise),
            probe,
        });
        self.hours += self.cost[id];
    }

    /// The best run by the loop's rule.
    fn best(&self, metric: &PrimaryMetric) -> Option<&ScoredRun> {
        metrics::best_by_rule(self.runs.iter(), metric)
    }

    /// What `orx metric best` names as the experiment to build on.
    fn best_experiment(&self, metric: &PrimaryMetric) -> String {
        self.best(metric)
            .map_or_else(|| BASE.to_string(), |run| run.experiment_id.clone())
    }

    /// What closing an idea measures: its experiment against its parent.
    fn measured(&self, experiment: &str, parent: &str, metric: &PrimaryMetric) -> Option<f64> {
        let tested = metrics::experiment_reference(&self.runs, experiment, metric, false)?;
        let reference = metrics::experiment_reference(&self.runs, parent, metric, true)?;
        Some(tested.score - reference.score)
    }

    fn path(&self, metric: &PrimaryMetric) -> Vec<PathStep> {
        let Some(best) = self.best(metric) else {
            return Vec::new();
        };
        let chain = metrics::ancestry(&best.experiment_id, |id| self.parent.get(id).cloned());
        metrics::path_steps(&chain, str::to_string, best, &self.runs, metric)
    }

    /// The best's score over the baseline's, as measured: what is reported.
    fn claimed_gain(&self, metric: &PrimaryMetric) -> f64 {
        let baseline = metrics::experiment_reference(&self.runs, BASE, metric, true)
            .map_or(0.0, |reference| reference.score);
        self.best(metric)
            .and_then(|run| run.score)
            .map_or(0.0, |score| score - baseline)
    }
}

fn blank_idea(id: String, created_at: i64) -> Idea {
    Idea {
        id,
        project_id: "project".to_string(),
        optimization_id: "optimization".to_string(),
        title: String::new(),
        kind: Kind::Science,
        class: None,
        priority: 2,
        hypothesis: None,
        kill_test: None,
        forecast: None,
        forecast_low: None,
        forecast_high: None,
        cost_hours: None,
        status: Status::Open,
        blocked_by: None,
        author: Author::Agent,
        evidence: None,
        experiment_id: None,
        outcome: None,
        outcome_key: None,
        created_at,
        started_at: None,
        closed_at: None,
        ablates: None,
    }
}

/// The next idea to work on, by the strategy's order.
fn choose(ideas: &[Idea], order: Order, rng: &mut Rng) -> Option<usize> {
    if order == Order::Selector {
        let pick = ideas::pick(ideas, Direction::Max)?;
        return ideas.iter().position(|idea| idea.id == pick.idea.id);
    }
    let open: Vec<usize> = (0..ideas.len())
        .filter(|&index| ideas[index].status == Status::Open)
        .collect();
    // Checks on the best go first, as the selector has them.
    let check = open
        .iter()
        .find(|&&index| ideas[index].kind == Kind::Replicate)
        .or_else(|| open.iter().find(|&&index| ideas[index].ablates.is_some()));
    if let Some(&check) = check {
        return Some(check);
    }
    if open.is_empty() {
        return None;
    }
    if order == Order::Random {
        let draw = (rng.uniform() * open.len() as f64) as usize;
        return Some(open[draw.min(open.len() - 1)]);
    }
    let forecast = |index: usize| ideas[index].forecast.unwrap_or(f64::NEG_INFINITY);
    open.into_iter()
        .max_by(|&a, &b| forecast(a).total_cmp(&forecast(b)).then(b.cmp(&a)))
}

/// How one campaign ended.
#[derive(Debug, Clone, Copy, Default)]
struct Outcome {
    /// True score of the experiment the strategy ends on, over the baseline's.
    true_gain: f64,
    /// The gain it reports: its best score over the baseline's, as measured.
    claimed_gain: f64,
    hours: f64,
    /// Steps on the final best's lineage, and those that truly did no good.
    steps: usize,
    false_steps: usize,
    /// GPU-hours spent on replicates and ablation checks.
    check_hours: f64,
}

fn campaign(scenario: &Scenario, campaign: u64, strategy: &Strategy) -> Outcome {
    let candidates = problem(scenario, campaign);
    let mut lab = Lab::new(campaign, scenario.noise);
    lab.experiment(BASE, None, (0.0, 0.0), 1_000, scenario.cost_median);
    if strategy.seeded {
        for _ in 0..BASELINE_SEEDS {
            lab.run(BASE, true);
        }
    } else {
        lab.run(BASE, false);
    }
    let seeds: Vec<f64> = lab.runs.iter().filter_map(|run| run.score).collect();
    let deviation = || {
        let mean = seeds.iter().sum::<f64>() / seeds.len() as f64;
        let square = seeds
            .iter()
            .map(|score| (score - mean).powi(2))
            .sum::<f64>();
        (square / (seeds.len() as f64 - 1.0).max(1.0)).sqrt()
    };
    let floor = match strategy.floor {
        Floor::Zero => 0.0,
        Floor::Deviations(multiple) => multiple * deviation(),
        Floor::Spread => {
            let high = seeds.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let low = seeds.iter().copied().fold(f64::INFINITY, f64::min);
            high - low
        }
    };
    let metric = PrimaryMetric {
        key: "score".to_string(),
        direction: Direction::Max,
        noise_floor: Some(floor),
        split: None,
    };
    let mut ideas: Vec<Idea> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let mut idea = blank_idea(format!("idea{index}"), index as i64);
            idea.class = Some(candidate.class);
            idea.forecast = Some(candidate.forecast);
            idea.forecast_low = Some(candidate.forecast - candidate.half_width);
            idea.forecast_high = Some(candidate.forecast + candidate.half_width);
            idea.cost_hours = Some(candidate.cost_estimate);
            idea
        })
        .collect();
    let mut order = Rng::new(stream(campaign, 2, 0));
    let mut replicate_of: HashMap<String, String> = HashMap::new();
    let mut check_hours = 0.0;
    let mut clock = scenario.ideas as i64;
    while let Some(index) = choose(&ideas, strategy.order, &mut order) {
        clock += 1;
        let best = lab.best_experiment(&metric);
        let is_check = ideas[index].kind == Kind::Replicate || ideas[index].ablates.is_some();
        // What the idea runs and on top of what: a rerun of the best for a
        // replicate, a child of the best for anything else.
        let (experiment, parent, cost) = if ideas[index].kind == Kind::Replicate {
            let experiment = replicate_of[&ideas[index].id].clone();
            let parent = lab.parent[&experiment].clone();
            let cost = lab.cost[&experiment];
            (experiment, parent, cost)
        } else if let Some(step) = ideas[index].ablates.clone() {
            let step_effect = lab.truth[&step] - lab.truth[&lab.parent[&step]];
            let step_luck = lab.luck[&step] - lab.luck[&lab.parent[&step]];
            let scores = (lab.truth[&best] - step_effect, lab.luck[&best] - step_luck);
            let cost = lab.cost[&step];
            let experiment = format!("ablation{clock}");
            if lab.hours + cost <= scenario.budget {
                lab.experiment(
                    &experiment,
                    Some(&best),
                    scores,
                    100_000 + clock as u64,
                    cost,
                );
            }
            (experiment, best, cost)
        } else {
            let candidate = &candidates[index];
            let experiment = format!("experiment{index}");
            if lab.hours + candidate.cost <= scenario.budget {
                let scores = (
                    lab.truth[&best] + candidate.effect,
                    lab.luck[&best] + candidate.luck,
                );
                lab.experiment(
                    &experiment,
                    Some(&best),
                    scores,
                    2_000 + index as u64,
                    candidate.cost,
                );
            }
            (experiment, best, candidate.cost)
        };
        if lab.hours + cost > scenario.budget {
            // Not affordable any more: set aside, and something cheaper may fit.
            ideas[index].status = Status::Dropped;
            ideas[index].closed_at = Some(clock);
            continue;
        }
        ideas[index].status = Status::Running;
        ideas[index].started_at = Some(clock);
        ideas[index].experiment_id = Some(experiment.clone());
        lab.run(&experiment, false);
        if is_check {
            check_hours += cost;
        }
        let change = lab.measured(&experiment, &parent, &metric);
        let status = change.map_or(Status::Refuted, |change| {
            ideas::judge(&ideas[index], change, &metric)
        });
        ideas[index].status = status;
        ideas[index].outcome = change;
        ideas[index].closed_at = Some(clock);
        if ideas[index].kind == Kind::Replicate {
            // As `orx-improve` has the agent do: a best that doesn't hold
            // drops its ablation checks.
            for other in ideas.iter_mut() {
                if status == Status::Refuted
                    && other.ablates.is_some()
                    && other.status == Status::Open
                {
                    other.status = Status::Dropped;
                    other.closed_at = Some(clock);
                }
            }
        } else if !is_check && status == Status::Kept && lab.best_experiment(&metric) == experiment
        {
            // A replicate, as `orx-improve` has the agent queue, and the
            // ablation checks `ideas::close` queues.
            if strategy.replicate {
                let mut replicate = blank_idea(format!("replicate{clock}"), clock);
                replicate.kind = Kind::Replicate;
                replicate.priority = 1;
                replicate_of.insert(replicate.id.clone(), experiment.clone());
                ideas.push(replicate);
            }
            if strategy.ablate {
                let path = lab.path(&metric);
                let checks: Vec<Idea> = ideas::ablation_targets(&path, &ideas, &metric)
                    .into_iter()
                    .map(|target| {
                        let id = format!("ablate{clock}-{}", target.step.experiment_id);
                        let mut check = blank_idea(id, clock);
                        check.class = Some(Class::Ablation);
                        check.forecast = Some(target.forecast);
                        check.forecast_low = Some(target.low);
                        check.forecast_high = Some(target.high);
                        check.cost_hours = target.cost_hours;
                        check.ablates = Some(target.step.experiment_id.clone());
                        check
                    })
                    .collect();
                ideas.extend(checks);
            }
        }
    }
    let best = lab.best_experiment(&metric);
    let chain = metrics::ancestry(&best, |id| lab.parent.get(id).cloned());
    let effects: Vec<f64> = chain
        .windows(2)
        .map(|pair| lab.truth[&pair[1]] - lab.truth[&pair[0]])
        .collect();
    Outcome {
        true_gain: lab.truth[&best],
        claimed_gain: lab.claimed_gain(&metric),
        hours: lab.hours,
        steps: effects.len(),
        false_steps: effects.iter().filter(|effect| **effect <= 0.0).count(),
        check_hours,
    }
}

/// A mean with its 95% confidence half-width.
#[derive(Debug, Clone, Copy)]
struct Estimate {
    mean: f64,
    margin: f64,
}

fn estimate(values: impl Iterator<Item = f64>) -> Estimate {
    let values: Vec<f64> = values.collect();
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / (n - 1.0);
    Estimate {
        mean,
        margin: 1.96 * (variance / n).sqrt(),
    }
}

/// Every strategy's outcomes on the same campaigns of one scenario.
struct Results {
    scenario: Scenario,
    outcomes: Vec<Vec<Outcome>>,
}

fn simulate(scenario: &Scenario, strategies: &[Strategy], campaigns: u64, seed: u64) -> Results {
    let outcomes = strategies
        .iter()
        .map(|strategy| {
            (0..campaigns)
                .map(|index| campaign(scenario, stream(seed, 3, index), strategy))
                .collect()
        })
        .collect();
    Results {
        scenario: *scenario,
        outcomes,
    }
}

impl Results {
    fn of(&self, strategy: usize) -> &[Outcome] {
        &self.outcomes[strategy]
    }

    fn mean(&self, strategy: usize, value: impl Fn(&Outcome) -> f64) -> Estimate {
        estimate(self.of(strategy).iter().map(value))
    }

    fn true_gain(&self, strategy: usize) -> f64 {
        self.mean(strategy, |outcome| outcome.true_gain).mean
    }

    fn overclaim(&self, strategy: usize) -> f64 {
        self.mean(strategy, |outcome| outcome.claimed_gain - outcome.true_gain)
            .mean
    }

    /// The paired difference in true gain between two strategies.
    fn difference(&self, a: usize, b: usize) -> Estimate {
        estimate(
            self.of(a)
                .iter()
                .zip(self.of(b))
                .map(|(a, b)| a.true_gain - b.true_gain),
        )
    }

    /// How often a strategy ends worse than the baseline it started from.
    fn worse_than_baseline(&self, strategy: usize) -> f64 {
        let outcomes = self.of(strategy);
        outcomes
            .iter()
            .filter(|outcome| outcome.true_gain < 0.0)
            .count() as f64
            / outcomes.len() as f64
    }

    /// True gain per GPU-hour, over all campaigns.
    fn gain_per_hour(&self, strategy: usize) -> f64 {
        let outcomes = self.of(strategy);
        outcomes
            .iter()
            .map(|outcome| outcome.true_gain)
            .sum::<f64>()
            / outcomes.iter().map(|outcome| outcome.hours).sum::<f64>()
    }
}

fn signed(value: f64) -> String {
    format!("{value:+.2}")
}

fn with_margin(estimate: Estimate) -> String {
    format!("{} ± {:.2}", signed(estimate.mean), estimate.margin)
}

/// The full comparison on one scenario.
fn strategy_table(results: &Results, strategies: &[Strategy]) -> String {
    let mut out = String::from(
        "| Strategy | True gain | Claimed gain | Overclaim | Ends below baseline | Steps kept that hurt or did nothing | GPU-hours on checks | True gain per 100 GPU-hours |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for (index, strategy) in strategies.iter().enumerate() {
        let name = strategy.name.trim_start();
        let name = if strategy.name.starts_with(' ') {
            format!("↳ {name}")
        } else {
            format!("**{name}**")
        };
        let false_steps = results.mean(index, |outcome| outcome.false_steps as f64);
        let steps = results.mean(index, |outcome| outcome.steps as f64);
        let _ = writeln!(
            out,
            "| {name} | {} | {} | {} | {:.0}% | {:.1} of {:.1} | {:.1} | {:+.1} |",
            with_margin(results.mean(index, |outcome| outcome.true_gain)),
            signed(results.mean(index, |outcome| outcome.claimed_gain).mean),
            signed(results.overclaim(index)),
            100.0 * results.worse_than_baseline(index),
            false_steps.mean,
            steps.mean,
            results.mean(index, |outcome| outcome.check_hours).mean,
            100.0 * results.gain_per_hour(index),
        );
    }
    out
}

fn campaigns() -> u64 {
    std::env::var("VLR_BENCH_CAMPAIGNS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|campaigns| *campaigns >= 10)
        .unwrap_or(2000)
}

/// Seed of the whole report.
const SEED: u64 = 20_261_015;

fn position(strategy: &Strategy) -> usize {
    STRATEGIES
        .iter()
        .position(|candidate| candidate.name == strategy.name)
        .expect("a reported strategy")
}

fn render(campaigns: u64) -> String {
    let all: Vec<Results> = SCENARIOS
        .iter()
        .map(|scenario| simulate(scenario, &STRATEGIES, campaigns, SEED))
        .collect();
    let (vlr, naive, random) = (
        position(&VLRESEARCH),
        position(&KEEP_IF_HIGHER),
        position(&RANDOM),
    );
    let mut out = String::new();
    let _ = write!(
        out,
        "# Improvement-loop benchmark\n\n\
         Generated by `src/flavor/bench.rs` with seed {SEED} and {campaigns} campaigns per \
         scenario. To reproduce it:\n\n\
         ```sh\n\
         cargo test --release --bin orx flavor::bench::report -- --ignored --nocapture\n\
         ```\n\n\
         ## What it measures\n\n\
         Each campaign is a research problem whose truth is known: {} ideas, each with a true \
         effect on the metric and a true cost in GPU-hours, and the agent's forecast of each \
         with an 80% interval and a cost estimate. A run's score is the true score of its \
         experiment plus seed noise, plus, in one scenario, luck on the validation split. Every \
         idea builds on the current best, and a campaign ends when its budget is spent. Every \
         strategy works the same campaigns with the same luck, so the differences between them \
         come from the strategies alone.\n\n\
         The VLResearch strategies go through the loop's own code: `ideas::pick` chooses (what \
         `orx idea next` prints), `ideas::judge` keeps or refutes, and `metrics::best_by_rule` decides the best to \
         build on (what `orx metric best` prints). Each new best gets a replicate, as the \
         `orx-improve` skill says, and the ablation checks `orx idea done` queues. Their baseline is measured with {BASELINE_SEEDS} seed runs; \
         the plain loops measure it once. Seed runs count against the budget.\n\n\
         Scores are in units of one run's seed noise (its standard deviation). **True gain** is \
         the true, out-of-sample score of the experiment a strategy ends on, minus the \
         baseline's. **Claimed gain** is what it reports, its best's score minus the \
         baseline's as measured, and **overclaim** is the difference. ± is a 95% confidence \
         interval.\n",
        TYPICAL.ideas,
    );
    let typical = &all[0];
    let (mut ahead, mut behind, mut honest) = (0, 0, 0);
    for results in &all {
        let difference = results.difference(vlr, naive);
        if difference.mean - difference.margin > 0.0 {
            ahead += 1;
        } else if difference.mean + difference.margin < 0.0 {
            behind += 1;
        }
        if results.overclaim(vlr) < results.overclaim(naive) {
            honest += 1;
        }
    }
    let level = all.len() - ahead - behind;
    let _ = write!(
        out,
        "\n## Result\n\n\
         In the typical scenario the VLResearch loop finds {} of true gain against {} for a \
         plain loop that keeps any gain, trying the most promising idea first (paired \
         difference {}), and overstates its gain by {} against {}. Across the {} scenarios it \
         is ahead on true gain in {ahead}, level in {level} and behind in {behind}, and it \
         overstates less in {honest}.\n",
        signed(typical.true_gain(vlr)),
        signed(typical.true_gain(naive)),
        with_margin(typical.difference(vlr, naive)),
        signed(typical.overclaim(vlr)),
        signed(typical.overclaim(naive)),
        all.len(),
    );
    out.push_str(
        "\n## Across scenarios\n\n\
         True gain of each, the VLResearch loop's paired advantage over keeping any gain in \
         forecast order, and how much each overstates its gain.\n\n\
         | Scenario | VLResearch loop | Keep if higher | Random order | VLResearch advantage | VLResearch overclaim | Keep-if-higher overclaim |\n\
         | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for results in &all {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} |",
            results.scenario.name,
            signed(results.true_gain(vlr)),
            signed(results.true_gain(naive)),
            signed(results.true_gain(random)),
            with_margin(results.difference(vlr, naive)),
            signed(results.overclaim(vlr)),
            signed(results.overclaim(naive)),
        );
    }
    for results in &all {
        let scenario = &results.scenario;
        let _ = write!(
            out,
            "\n## {}\n\n{}: {} ideas, {} GPU-hours; true effects of incremental ideas N({}, {}), \
             orthogonal ideas ({:.0}% of them) N({}, {}); seed noise {}, validation luck {}; \
             forecast error mean {:+}, SD {}; stated intervals {:.0}% of honest width.\n\n{}",
            scenario.name,
            capitalized(scenario.about),
            scenario.ideas,
            scenario.budget,
            scenario.incremental.0,
            scenario.incremental.1,
            100.0 * scenario.orthogonal_share,
            scenario.orthogonal.0,
            scenario.orthogonal.1,
            scenario.noise,
            scenario.overfit,
            scenario.forecast_bias,
            scenario.forecast_noise,
            100.0 * scenario.interval_scale,
            strategy_table(results, &STRATEGIES),
        );
    }
    out.push_str(
        "\n## What the simulation can't show\n\n\
         - Effects add up and don't depend on what came before, so there are no local optima. \
         The selector's rule of trying an orthogonal idea after three incremental ones exists \
         for local optima, and here it can only cost a little.\n\
         - Changes don't interact, so ablation checks always find the steps adding up; their \
         use, finding steps that don't, can't show.\n\
         - Kill tests, which end an expensive idea early on a cheap negative result, aren't \
         modeled.\n\
         - The agent's forecasts and costs are drawn from the stated distributions; a real \
         agent's are whatever they are. The end-to-end check of the loop is running it on your \
         own problem with an out-of-sample split.\n",
    );
    out
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[test]
#[ignore = "the full benchmark; run it with --release"]
fn report() {
    let text = render(campaigns());
    print!("{text}");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("flavor/bench/loop.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
}

#[test]
fn campaigns_are_reproducible() {
    let a = campaign(&TYPICAL, 7, &VLRESEARCH);
    let b = campaign(&TYPICAL, 7, &VLRESEARCH);
    assert_eq!(a.true_gain, b.true_gain);
    assert_eq!(a.claimed_gain, b.claimed_gain);
    assert!(a.hours <= TYPICAL.budget + 1e-9);
}

/// The benchmark's headline, on fewer campaigns: as it stands, the loop
/// finds less than keeping any gain does.
#[test]
fn the_loop_finds_less_than_keeping_any_gain() {
    let strategies = [VLRESEARCH, KEEP_IF_HIGHER];
    let results = simulate(&TYPICAL, &strategies, 150, SEED);
    assert!(results.true_gain(0) < results.true_gain(1));
    assert!(results.overclaim(0) < results.overclaim(1));
}
