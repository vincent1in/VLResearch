//! Starting auto-research from the dashboard. The Research view's form starts
//! an optimization with its whole brief at once (with the primary metric and
//! guards), adds the seed ideas, links the chat it starts, and lets the user
//! confirm the brief with a button; it edits an optimization's brief the same
//! way.
//! The rules are the CLI's: any change reopens a confirmed brief, confirming
//! needs everything `scope::missing` lists, and nothing here launches compute.

use serde::Deserialize;

use super::ideas::{self, Author, Class, Idea, IdeaFields, Kind};
use super::metrics::{self, Direction, Guard, PrimaryMetric};
use super::scope::{self, Brief, BriefFields};
use crate::error::{anyhow, Result};
use crate::store::Store;

/// The brief as the dashboard's form holds it: every field, so a blank one
/// clears what was there.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BriefDraft {
    pub objective: Option<String>,
    pub may_change: Option<String>,
    pub must_not_change: Option<String>,
    #[serde(default)]
    pub locked: Vec<String>,
    /// The only paths runs may change; empty for the whole repository.
    #[serde(default)]
    pub scope: Vec<String>,
    pub budget_runs: Option<i64>,
    pub budget_gpu_hours: Option<f64>,
    pub budget_days: Option<f64>,
    pub pointers: Option<String>,
    pub sources: Option<String>,
    pub stop_rule: Option<String>,
    pub metric: Option<MetricDraft>,
    /// Every guard the optimization keeps, replacing the ones it had; `None`
    /// leaves them as they are.
    pub guards: Option<Vec<GuardDraft>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MetricDraft {
    pub key: String,
    /// `max` when higher is better, `min` when lower is.
    pub direction: String,
    pub noise_floor: Option<f64>,
    pub split: Option<String>,
}

/// A guard as the form holds it: a key an experiment must keep within an
/// upper bound, a lower bound or both.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GuardDraft {
    pub key: String,
    pub max: Option<f64>,
    pub min: Option<f64>,
    /// The split to read the key in, when runs report it under several.
    pub split: Option<String>,
}

/// A draft's primary metric and guards, checked before anything is written.
struct Checked {
    metric: Option<PrimaryMetric>,
    guards: Option<Vec<Guard>>,
}

/// An idea typed into the dashboard.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdeaDraft {
    pub title: String,
    pub hypothesis: Option<String>,
    pub class: Option<String>,
    pub forecast: Option<f64>,
    pub forecast_low: Option<f64>,
    pub forecast_high: Option<f64>,
    pub cost_hours: Option<f64>,
    pub kill_test: Option<String>,
}

/// Starts an optimization in the project with the brief the form holds. The
/// draft is checked first, so a refused one starts nothing.
pub(crate) fn create(store: &Store, project_id: &str, draft: BriefDraft) -> Result<Brief> {
    let checked = check(store, project_id, &draft, None)?;
    let brief = scope::create(store, project_id, None)?;
    apply(store, &brief, draft, checked)
}

/// Saves an optimization's brief as the form holds it, with its primary metric
/// and guards. The draft is checked first, so a refused one saves nothing.
pub(crate) fn save_brief(store: &Store, optimization_id: &str, draft: BriefDraft) -> Result<Brief> {
    let brief = scope::require(store, optimization_id)?;
    let checked = check(store, &brief.project_id, &draft, Some(&brief))?;
    apply(store, &brief, draft, checked)
}

/// The draft's metric and guards, and its budgets, checked before anything is
/// written.
fn check(
    store: &Store,
    project_id: &str,
    draft: &BriefDraft,
    current: Option<&Brief>,
) -> Result<Checked> {
    if draft.budget_runs.is_some_and(|runs| runs < 1) {
        return Err(anyhow!("A run budget is at least 1."));
    }
    for (value, name) in [
        (draft.budget_gpu_hours, "GPU-hour"),
        (draft.budget_days, "day"),
    ] {
        if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(anyhow!("A {name} budget must be a positive number."));
        }
    }
    let metric = draft
        .metric
        .as_ref()
        .map(|metric| primary_metric(store, project_id, metric, current))
        .transpose()?;
    let guards = draft.guards.as_deref().map(guards).transpose()?;
    Ok(Checked { metric, guards })
}

/// The form's guards, checked the way `orx metric guard` checks one, and one
/// per key.
fn guards(drafts: &[GuardDraft]) -> Result<Vec<Guard>> {
    let mut guards: Vec<Guard> = Vec::new();
    for draft in drafts {
        let key = draft.key.trim();
        if key.is_empty() {
            return Err(anyhow!("Name the metric each guard bounds."));
        }
        if guards.iter().any(|guard| guard.key == key) {
            return Err(anyhow!(
                "{key} has two guards. Give it one, with both bounds if it needs them."
            ));
        }
        if draft.max.is_none() && draft.min.is_none() {
            return Err(anyhow!(
                "Give the guard on {key} an upper bound, a lower bound or both."
            ));
        }
        if let (Some(min), Some(max)) = (draft.min, draft.max) {
            if min > max {
                return Err(anyhow!(
                    "The guard on {key} has its lower bound above its upper bound."
                ));
            }
        }
        guards.push(Guard {
            key: key.to_owned(),
            split: draft
                .split
                .as_deref()
                .map(str::trim)
                .filter(|split| !split.is_empty())
                .map(str::to_owned),
            max: draft.max,
            min: draft.min,
        });
    }
    Ok(guards)
}

fn apply(store: &Store, brief: &Brief, draft: BriefDraft, checked: Checked) -> Result<Brief> {
    let blank = |value: Option<String>| Some(value.unwrap_or_default());
    let paths = |paths: Vec<String>| -> Vec<String> {
        paths
            .into_iter()
            .filter(|path| !path.trim().is_empty())
            .collect()
    };
    let saved = scope::set(
        store,
        &brief.id,
        BriefFields {
            objective: blank(draft.objective),
            may_change: blank(draft.may_change),
            must_not_change: blank(draft.must_not_change),
            lock: paths(draft.locked),
            unlock: brief
                .locked
                .iter()
                .map(|locked| locked.path.clone())
                .collect(),
            scope: paths(draft.scope),
            unscope: brief.scope.clone(),
            budget_runs: draft.budget_runs,
            budget_gpu_hours: draft.budget_gpu_hours,
            budget_days: draft.budget_days,
            clear_budget: true,
            reset_budget: false,
            pointers: blank(draft.pointers),
            sources: blank(draft.sources),
            stop_rule: blank(draft.stop_rule),
        },
    )?;
    if let Some(metric) = checked.metric {
        metrics::set_primary(store, &brief.id, &metric)?;
    }
    if let Some(guards) = checked.guards {
        for current in metrics::guards(store, &brief.id)? {
            if !guards.iter().any(|guard| guard.key == current.key) {
                metrics::remove_guard(store, &brief.id, &current.key)?;
            }
        }
        for guard in &guards {
            metrics::set_guard(store, &brief.id, guard)?;
        }
    }
    Ok(saved)
}

/// The primary metric a draft names, checked the way `orx metric primary`
/// checks it.
fn primary_metric(
    store: &Store,
    project_id: &str,
    draft: &MetricDraft,
    current: Option<&Brief>,
) -> Result<PrimaryMetric> {
    let key = draft.key.trim();
    if key.is_empty() {
        return Err(anyhow!("Name the metric that measures the goal."));
    }
    let direction = match draft.direction.as_str() {
        "max" => Direction::Max,
        "min" => Direction::Min,
        other => {
            return Err(anyhow!(
                "Say whether higher or lower is better, not {other:?}."
            ))
        }
    };
    if let Some(floor) = draft.noise_floor {
        if !floor.is_finite() || floor < 0.0 {
            return Err(anyhow!("The noise floor is a change of zero or more."));
        }
    }
    let split = draft
        .split
        .as_deref()
        .map(str::trim)
        .filter(|split| !split.is_empty())
        .map(str::to_owned);
    for run in store.list_runs_by_project(project_id)? {
        metrics::ingest(store, &run)?;
    }
    if split.is_none() {
        let splits = metrics::key_splits(store, project_id, key)?;
        if splits.len() > 1 {
            let names: Vec<String> = splits
                .iter()
                .map(|split| split.clone().unwrap_or_else(|| "(none)".to_string()))
                .collect();
            return Err(anyhow!(
                "Runs report {key} under several splits ({}). Choose the one to score; select \
                 on validation, never on test.",
                names.join(", ")
            ));
        }
    }
    // Saving the same key again keeps its measured noise floor.
    let current = match current {
        Some(brief) => metrics::primary(store, &brief.id)?,
        None => None,
    };
    let noise_floor = draft.noise_floor.or_else(|| {
        current
            .filter(|current| current.key == key)
            .and_then(|current| current.noise_floor)
    });
    Ok(PrimaryMetric {
        key: key.to_owned(),
        direction,
        noise_floor,
        split,
    })
}

/// Adds an idea the user typed to an optimization's queue, as theirs.
pub(crate) fn add_idea(store: &Store, optimization_id: &str, draft: IdeaDraft) -> Result<Idea> {
    let class = match draft
        .class
        .as_deref()
        .map(str::trim)
        .filter(|class| !class.is_empty())
    {
        Some(text) => {
            Some(Class::from_text(text).ok_or_else(|| anyhow!("{text:?} is not an idea class."))?)
        }
        None => None,
    };
    ideas::add(
        store,
        optimization_id,
        IdeaFields {
            title: Some(draft.title),
            kind: Some(Kind::Science),
            class,
            hypothesis: draft.hypothesis.filter(|text| !text.trim().is_empty()),
            kill_test: draft.kill_test.filter(|text| !text.trim().is_empty()),
            forecast: draft.forecast,
            forecast_low: draft.forecast_low,
            forecast_high: draft.forecast_high,
            cost_hours: draft.cost_hours,
            author: Some(Author::User),
            ..IdeaFields::default()
        },
    )
}

/// Hands the optimization to the chat the dashboard started for it, which is
/// told when the user confirms the brief there.
pub(crate) fn link_chat(store: &Store, optimization_id: &str, session_id: &str) -> Result<Brief> {
    let brief = scope::require(store, optimization_id)?;
    let session = store
        .get_chat_session(session_id)?
        .filter(|session| session.project_id == brief.project_id)
        .ok_or_else(|| anyhow!("Chat {session_id} isn't one of this project's."))?;
    scope::set_chat(store, &brief.id, &session.id)
}

#[cfg(test)]
mod tests {
    use super::{
        add_idea, create, link_chat, save_brief, BriefDraft, GuardDraft, IdeaDraft, MetricDraft,
    };
    use crate::flavor::{ideas, metrics, scope};
    use crate::store::{Store, StoredChatSession, StoredRun};

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("vlr-start-{}", uuid::Uuid::new_v4()));
        (Store::open_at(dir.clone()).unwrap(), dir)
    }

    fn metric(key: &str, direction: &str) -> Option<MetricDraft> {
        Some(MetricDraft {
            key: key.into(),
            direction: direction.into(),
            noise_floor: None,
            split: None,
        })
    }

    #[test]
    fn the_form_saves_the_whole_brief_and_its_metric() {
        let (store, dir) = temp_store();
        let opt = create(
            &store,
            "p",
            BriefDraft {
                objective: Some("Raise validation IC".into()),
                must_not_change: Some("evaluation".into()),
                locked: vec!["eval".into(), " ".into()],
                budget_runs: Some(20),
                budget_days: Some(14.0),
                stop_rule: Some("No gain in 5 runs".into()),
                pointers: Some("Volume features first".into()),
                scope: vec!["features/".into()],
                metric: metric("val_ic", "max"),
                ..BriefDraft::default()
            },
        )
        .unwrap();
        let brief = scope::get(&store, &opt.id).unwrap().unwrap();
        assert_eq!(brief.objective.as_deref(), Some("Raise validation IC"));
        assert_eq!(brief.locked.len(), 1);
        assert_eq!(brief.scope, ["features"]);
        assert_eq!(
            (brief.budget_runs, brief.budget_days),
            (Some(20), Some(14.0))
        );
        let primary = metrics::primary(&store, &opt.id).unwrap().unwrap();
        assert_eq!(
            (primary.key.as_str(), primary.direction),
            ("val_ic", metrics::Direction::Max)
        );

        // Saving again replaces the brief: cleared fields, locks and scope go.
        metrics::set_primary(
            &store,
            &opt.id,
            &metrics::PrimaryMetric {
                noise_floor: Some(0.002),
                ..primary
            },
        )
        .unwrap();
        save_brief(
            &store,
            &opt.id,
            BriefDraft {
                objective: Some("Raise validation IC".into()),
                locked: vec!["eval/score.py".into()],
                budget_gpu_hours: Some(40.0),
                metric: metric("val_ic", "max"),
                ..BriefDraft::default()
            },
        )
        .unwrap();
        let brief = scope::get(&store, &opt.id).unwrap().unwrap();
        assert_eq!(brief.must_not_change, None);
        assert_eq!(brief.pointers, None);
        assert!(brief.scope.is_empty());
        let paths: Vec<&str> = brief
            .locked
            .iter()
            .map(|locked| locked.path.as_str())
            .collect();
        assert_eq!(paths, ["eval/score.py"]);
        assert_eq!(
            (brief.budget_runs, brief.budget_gpu_hours, brief.budget_days),
            (None, Some(40.0), None)
        );
        // The measured noise floor stays with the same key.
        let primary = metrics::primary(&store, &opt.id).unwrap().unwrap();
        assert_eq!(primary.noise_floor, Some(0.002));

        // A second optimization has its own brief and metric.
        let speed = create(
            &store,
            "p",
            BriefDraft {
                objective: Some("Cut backtest time".into()),
                metric: metric("backtest_seconds", "min"),
                ..BriefDraft::default()
            },
        )
        .unwrap();
        assert_ne!(speed.id, opt.id);
        assert_eq!(scope::list(&store, "p").unwrap().len(), 2);
        assert_eq!(
            metrics::primary(&store, &speed.id).unwrap().unwrap().key,
            "backtest_seconds"
        );
        assert_eq!(
            metrics::primary(&store, &opt.id).unwrap().unwrap().key,
            "val_ic"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_metric_or_budget_is_refused() {
        let (store, dir) = temp_store();
        let draft = |metric, runs| BriefDraft {
            objective: Some("x".into()),
            budget_runs: runs,
            metric,
            ..BriefDraft::default()
        };
        assert!(create(&store, "p", draft(metric("val_ic", "up"), None)).is_err());
        assert!(create(&store, "p", draft(metric(" ", "max"), None)).is_err());
        assert!(create(&store, "p", draft(metric("val_ic", "max"), Some(0))).is_err());
        // A refused draft starts nothing.
        assert!(scope::list(&store, "p").unwrap().is_empty());

        // A key reported under two splits needs one chosen.
        store
            .upsert_run(&StoredRun {
                id: "r1".into(),
                experiment_id: "e".into(),
                project_id: "p".into(),
                status: "done".into(),
                backend_json: "{}".into(),
                command: "python train.py".into(),
                created_at: 1,
                updated_at: 2,
                ended_at: Some(2),
                exit_code: Some(0),
                commit_sha: None,
                result_markdown: None,
                cancel_requested: false,
                cancel_reason: None,
                chat_session_id: None,
            })
            .unwrap();
        let log = crate::flavor::wake::run_log_path(&store, "r1");
        std::fs::write(
            log,
            "ORX_METRIC {\"key\": \"ic\", \"value\": 0.03, \"split\": \"val\"}\n\
             ORX_METRIC {\"key\": \"ic\", \"value\": 0.02, \"split\": \"test\"}\n",
        )
        .unwrap();
        let refused = create(&store, "p", draft(metric("ic", "max"), None))
            .unwrap_err()
            .to_string();
        assert!(refused.contains("several splits"), "{refused}");
        let chosen = Some(super::MetricDraft {
            split: Some("val".into()),
            ..metric("ic", "max").unwrap()
        });
        let opt = create(&store, "p", draft(chosen, None)).unwrap();
        // An edit is checked the same way, and a refused one changes nothing.
        assert!(save_brief(&store, &opt.id, draft(metric("ic", "max"), None)).is_err());
        assert_eq!(
            metrics::primary(&store, &opt.id)
                .unwrap()
                .unwrap()
                .split
                .as_deref(),
            Some("val")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_form_sets_and_replaces_the_guards() {
        let (store, dir) = temp_store();
        let guard = |key: &str, max: Option<f64>, min: Option<f64>| GuardDraft {
            key: key.into(),
            max,
            min,
            split: None,
        };
        let draft = |guards: Option<Vec<GuardDraft>>| BriefDraft {
            objective: Some("Cut backtest time".into()),
            metric: metric("backtest_seconds", "min"),
            guards,
            ..BriefDraft::default()
        };
        let opt = create(
            &store,
            "p",
            draft(Some(vec![
                guard(" output_diff ", Some(0.0), None),
                GuardDraft {
                    split: Some(" val ".into()),
                    ..guard("val_ic", None, Some(0.02))
                },
            ])),
        )
        .unwrap();
        let saved = metrics::guards(&store, &opt.id).unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(
            (saved[0].key.as_str(), saved[0].max, saved[0].min),
            ("output_diff", Some(0.0), None)
        );
        assert_eq!(saved[1].split.as_deref(), Some("val"));

        // A save that doesn't list guards leaves them; one that does replaces them.
        save_brief(&store, &opt.id, draft(None)).unwrap();
        assert_eq!(metrics::guards(&store, &opt.id).unwrap().len(), 2);
        let both = guard("val_ic", Some(0.05), Some(0.01));
        save_brief(&store, &opt.id, draft(Some(vec![both]))).unwrap();
        let saved = metrics::guards(&store, &opt.id).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(
            (saved[0].max, saved[0].min, saved[0].split.as_deref()),
            (Some(0.05), Some(0.01), None)
        );
        save_brief(&store, &opt.id, draft(Some(Vec::new()))).unwrap();
        assert!(metrics::guards(&store, &opt.id).unwrap().is_empty());

        // A bad guard is refused, and a refused save changes nothing.
        let turnover = guard("turnover", Some(2.0), None);
        save_brief(&store, &opt.id, draft(Some(vec![turnover]))).unwrap();
        for bad in [
            vec![guard(" ", Some(1.0), None)],
            vec![guard("turnover", None, None)],
            vec![guard("turnover", Some(1.0), Some(2.0))],
            vec![
                guard("turnover", Some(1.0), None),
                guard("turnover", None, Some(0.0)),
            ],
        ] {
            assert!(save_brief(&store, &opt.id, draft(Some(bad))).is_err());
        }
        let saved = metrics::guards(&store, &opt.id).unwrap();
        assert_eq!((saved.len(), saved[0].max), (1, Some(2.0)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typed_ideas_are_the_users_and_the_chat_link_stays_in_the_project() {
        let (store, dir) = temp_store();
        let opt = create(
            &store,
            "p",
            BriefDraft {
                objective: Some("x".into()),
                ..BriefDraft::default()
            },
        )
        .unwrap();
        let idea = add_idea(
            &store,
            &opt.id,
            IdeaDraft {
                title: "Volume features".into(),
                class: Some("data".into()),
                forecast: Some(0.003),
                hypothesis: Some(" ".into()),
                ..IdeaDraft::default()
            },
        )
        .unwrap();
        assert_eq!(idea.author, ideas::Author::User);
        assert_eq!(idea.class, Some(ideas::Class::Data));
        assert_eq!(idea.hypothesis, None);
        let wrong_class = IdeaDraft {
            title: "x".into(),
            class: Some("magic".into()),
            ..IdeaDraft::default()
        };
        assert!(add_idea(&store, &opt.id, wrong_class).is_err());
        assert_eq!(idea.optimization_id, opt.id);

        let mut session = StoredChatSession {
            id: "chat_1".into(),
            project_id: "p".into(),
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
            side_parent_session_id: None,
            side_parent_leaf_id: None,
            created_at: 1,
            updated_at: 1,
        };
        store.create_chat_session(&session).unwrap();
        session.id = "chat_elsewhere".into();
        session.project_id = "other".into();
        store.create_chat_session(&session).unwrap();
        link_chat(&store, &opt.id, "chat_1").unwrap();
        assert_eq!(
            scope::get(&store, &opt.id)
                .unwrap()
                .unwrap()
                .chat_session_id
                .as_deref(),
            Some("chat_1")
        );
        assert!(link_chat(&store, &opt.id, "chat_elsewhere").is_err());
        assert!(link_chat(&store, &opt.id, "chat_missing").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
