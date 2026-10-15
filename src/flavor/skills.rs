//! Agent skills VLResearch adds to upstream's bundled set. Each is a complete
//! skill package under `agent-skills/`; `local::agent_skills::skills` hands
//! its lists to [`add_to`] (one hook line).

use crate::local::agent_skills::{AgentSkill, AgentSkillResource};

pub(crate) const METRICS: AgentSkill = AgentSkill {
    name: "orx-metrics",
    description: "Report run metrics as ORX_METRIC lines and compare runs on this chat's optimization's primary metric with `orx metric`. Use when writing or editing code that reports losses or scores, before launching a run whose result will be compared, when choosing what to optimize, or before saying which experiment is best.",
    content: include_str!("../../agent-skills/orx-metrics/SKILL.md"),
    resources: &[AgentSkillResource {
        path: "assets/orx_metrics.py",
        content: include_str!("../../agent-skills/orx-metrics/assets/orx_metrics.py"),
    }],
};

pub(crate) const IDEAS: AgentSkill = AgentSkill {
    name: "orx-ideas",
    description: "Queue and track ideas with `orx idea`: forecasts with 80% intervals, GPU-hour costs, kill tests, blockers and measured outcomes. Use before proposing or starting an experiment, when deciding what to try next, after a run finishes, and when the user asks what has been tried or what worked.",
    content: include_str!("../../agent-skills/orx-ideas/SKILL.md"),
    resources: &[],
};

pub(crate) const GRILL: AgentSkill = AgentSkill {
    name: "orx-grill",
    description: "Scope research work with the user before spending compute: interview in numbered rounds with a recommended answer to each question, check the data and baseline, review for leakage, propose ideas with kill tests, and record the brief with `orx brief`. Use at the start of a research goal, when a launch says the brief is missing or unconfirmed, or when the goal changes.",
    content: include_str!("../../agent-skills/orx-grill/SKILL.md"),
    resources: &[],
};

pub(crate) const IMPROVE: AgentSkill = AgentSkill {
    name: "orx-improve",
    description: "Run the forecast-and-verify improvement loop on this chat's scoped optimization: pick the next idea, check its forecast, build it on a child of the best experiment, run it, measure it against the forecast, and confirm each new best with a rerun. Use when asked to improve a model, dataset or process over several experiments, or to keep going on an optimization with a confirmed brief.",
    content: include_str!("../../agent-skills/orx-improve/SKILL.md"),
    resources: &[],
};

pub(crate) const FINDINGS: AgentSkill = AgentSkill {
    name: "orx-findings",
    description: "Record what the project has learned as claims with evidence using `orx finding`: back or undercut a claim with runs, link related claims, and add verdicts with confidence and scope. Use after an idea closes, when a result changes what you believe about the data, model or pipeline, and before summarizing what is known.",
    content: include_str!("../../agent-skills/orx-findings/SKILL.md"),
    resources: &[],
};

/// VLResearch's skills, in the order they are listed.
const SKILLS: [&AgentSkill; 5] = [&METRICS, &IDEAS, &FINDINGS, &GRILL, &IMPROVE];

/// Adds VLResearch's skills to an upstream skill list, right after
/// `orx-results` (or at the end if upstream renames it).
pub(crate) fn add_to(skills: &mut Vec<&'static AgentSkill>) {
    let at = skills
        .iter()
        .position(|skill| skill.name == "orx-results")
        .map_or(skills.len(), |index| index + 1);
    for (offset, skill) in SKILLS.into_iter().enumerate() {
        if skills.iter().all(|existing| existing.name != skill.name) {
            skills.insert(at + offset, skill);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::local::agent_skills::{skills, SkillSet};

    #[test]
    fn both_sets_list_the_skills_after_results() {
        for set in [SkillSet::Local, SkillSet::Full] {
            let names: Vec<_> = skills(set).iter().map(|skill| skill.name).collect();
            let results = names
                .iter()
                .position(|name| *name == "orx-results")
                .unwrap();
            assert_eq!(
                names[results + 1..results + 6],
                [
                    "orx-metrics",
                    "orx-ideas",
                    "orx-findings",
                    "orx-grill",
                    "orx-improve"
                ]
            );
        }
    }

    #[test]
    fn the_metrics_helper_prints_what_the_ledger_parses() {
        // The helper's own format string, applied to a typical record.
        let helper = super::METRICS.resources[0].content;
        assert!(helper.contains("print(\"ORX_METRIC \" + json.dumps(record)"));
        let line = r#"ORX_METRIC {"step": 3, "train_loss": 0.42, "lr": 0.0003}"#;
        let points = crate::flavor::metrics::parse_line(line);
        let keys: Vec<_> = points.iter().map(|point| point.key.as_str()).collect();
        assert_eq!(keys, ["train_loss", "lr"]);
        assert!(points.iter().all(|point| point.step == Some(3)));
    }
}
