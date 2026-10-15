//! Agent skills VLResearch adds to upstream's bundled set. Each is a complete
//! skill package under `agent-skills/`, registered in
//! `local::agent_skills::skills` by a hook line.

use crate::local::agent_skills::{AgentSkill, AgentSkillResource};

pub(crate) const METRICS: AgentSkill = AgentSkill {
    name: "orx-metrics",
    description: "Report run metrics as ORX_METRIC lines and compare runs on the project's primary metric with `orx metric`. Use when writing or editing code that reports losses or scores, before launching a run whose result will be compared, when choosing what to optimize, or before saying which experiment is best.",
    content: include_str!("../../agent-skills/orx-metrics/SKILL.md"),
    resources: &[AgentSkillResource {
        path: "assets/orx_metrics.py",
        content: include_str!("../../agent-skills/orx-metrics/assets/orx_metrics.py"),
    }],
};

#[cfg(test)]
mod tests {
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
