//! Starting auto-research from the dashboard. The Research view's form saves
//! the whole brief at once (with the primary metric), adds the seed ideas,
//! links the chat it starts, and lets the user confirm the brief with a
//! button. The rules are the CLI's: any change reopens a confirmed brief,
//! confirming needs everything `scope::missing` lists, and nothing here
//! launches compute.

use serde::Deserialize;

use super::ideas::{self, Author, Class, Idea, IdeaFields, Kind};
use super::metrics::{self, Direction, PrimaryMetric};
use super::scope::{self, BriefFields};
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
    pub budget_runs: Option<i64>,
    pub budget_gpu_hours: Option<f64>,
    pub budget_days: Option<f64>,
    pub pointers: Option<String>,
    pub sources: Option<String>,
    pub stop_rule: Option<String>,
    pub metric: Option<MetricDraft>,
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

/// Saves the brief as the form holds it, and its primary metric. The metric
/// is checked first, so a refused one saves nothing.
pub(crate) fn save_brief(store: &Store, project_id: &str, draft: BriefDraft) -> Result<()> {
    let metric = draft
        .metric
        .as_ref()
        .map(|metric| primary_metric(store, project_id, metric))
        .transpose()?;
    let current = scope::get(store, project_id)?;
    let unlock = current
        .map(|brief| brief.locked.into_iter().map(|locked| locked.path).collect())
        .unwrap_or_default();
    let blank = |value: Option<String>| Some(value.unwrap_or_default());
    scope::set(
        store,
        project_id,
        BriefFields {
            objective: blank(draft.objective),
            may_change: blank(draft.may_change),
            must_not_change: blank(draft.must_not_change),
            lock: draft
                .locked
                .into_iter()
                .filter(|path| !path.trim().is_empty())
                .collect(),
            unlock,
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
    if let Some(metric) = metric {
        metrics::set_primary(store, project_id, &metric)?;
    }
    Ok(())
}

/// The primary metric a draft names, checked the way `orx metric primary`
/// checks it.
fn primary_metric(store: &Store, project_id: &str, draft: &MetricDraft) -> Result<PrimaryMetric> {
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
    let current = metrics::primary(store, project_id)?;
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

/// Adds an idea the user typed, as theirs.
pub(crate) fn add_idea(store: &Store, project_id: &str, draft: IdeaDraft) -> Result<Idea> {
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
        project_id,
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

/// Links the brief to the chat the dashboard started for it, which is told
/// when the user confirms the brief there.
pub(crate) fn link_chat(store: &Store, project_id: &str, session_id: &str) -> Result<()> {
    let session = store
        .get_chat_session(session_id)?
        .filter(|session| session.project_id == project_id)
        .ok_or_else(|| anyhow!("Chat {session_id} isn't one of this project's."))?;
    scope::set_chat(store, project_id, &session.id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{add_idea, link_chat, save_brief, BriefDraft, IdeaDraft, MetricDraft};
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
        save_brief(
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
                metric: metric("val_ic", "max"),
                ..BriefDraft::default()
            },
        )
        .unwrap();
        let brief = scope::get(&store, "p").unwrap().unwrap();
        assert_eq!(brief.objective.as_deref(), Some("Raise validation IC"));
        assert_eq!(brief.locked.len(), 1);
        assert_eq!(
            (brief.budget_runs, brief.budget_days),
            (Some(20), Some(14.0))
        );
        let primary = metrics::primary(&store, "p").unwrap().unwrap();
        assert_eq!(
            (primary.key.as_str(), primary.direction),
            ("val_ic", metrics::Direction::Max)
        );

        // Saving again replaces the brief: cleared fields and locks go away.
        metrics::set_primary(
            &store,
            "p",
            &metrics::PrimaryMetric {
                noise_floor: Some(0.002),
                ..primary
            },
        )
        .unwrap();
        save_brief(
            &store,
            "p",
            BriefDraft {
                objective: Some("Raise validation IC".into()),
                locked: vec!["eval/score.py".into()],
                budget_gpu_hours: Some(40.0),
                metric: metric("val_ic", "max"),
                ..BriefDraft::default()
            },
        )
        .unwrap();
        let brief = scope::get(&store, "p").unwrap().unwrap();
        assert_eq!(brief.must_not_change, None);
        assert_eq!(brief.pointers, None);
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
        let primary = metrics::primary(&store, "p").unwrap().unwrap();
        assert_eq!(primary.noise_floor, Some(0.002));
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
        assert!(save_brief(&store, "p", draft(metric("val_ic", "up"), None)).is_err());
        assert!(save_brief(&store, "p", draft(metric(" ", "max"), None)).is_err());
        assert!(save_brief(&store, "p", draft(metric("val_ic", "max"), Some(0))).is_err());
        // A refused metric saves nothing.
        assert!(scope::get(&store, "p").unwrap().is_none());

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
        let refused = save_brief(&store, "p", draft(metric("ic", "max"), None))
            .unwrap_err()
            .to_string();
        assert!(refused.contains("several splits"), "{refused}");
        let chosen = Some(super::MetricDraft {
            split: Some("val".into()),
            ..metric("ic", "max").unwrap()
        });
        save_brief(&store, "p", draft(chosen, None)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typed_ideas_are_the_users_and_the_chat_link_stays_in_the_project() {
        let (store, dir) = temp_store();
        let idea = add_idea(
            &store,
            "p",
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
        assert!(add_idea(&store, "p", wrong_class).is_err());

        save_brief(
            &store,
            "p",
            BriefDraft {
                objective: Some("x".into()),
                ..BriefDraft::default()
            },
        )
        .unwrap();
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
            parent_session_id: None,
            side_parent_session_id: None,
            created_at: 1,
            updated_at: 1,
        };
        store.create_chat_session(&session).unwrap();
        session.id = "chat_elsewhere".into();
        session.project_id = "other".into();
        store.create_chat_session(&session).unwrap();
        link_chat(&store, "p", "chat_1").unwrap();
        assert_eq!(
            scope::get(&store, "p")
                .unwrap()
                .unwrap()
                .chat_session_id
                .as_deref(),
            Some("chat_1")
        );
        assert!(link_chat(&store, "p", "chat_elsewhere").is_err());
        assert!(link_chat(&store, "p", "chat_missing").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
