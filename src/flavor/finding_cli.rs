//! `orx finding`: the findings ledger from the command line.

use serde_json::json;

use super::cli::FindingCommand;
use super::findings::{self, Finding};
use super::metrics;
use crate::error::Result;
use crate::output::print_table;
use crate::store::Store;

pub(crate) fn run(command: FindingCommand) -> Result<()> {
    let store = Store::open()?;
    match command {
        FindingCommand::Add {
            project_id,
            statement,
            kind,
            idea,
        } => {
            let project = crate::local::resolve::resolve_project(&store, &project_id)?;
            let finding = findings::add(&store, &project.id, kind, &statement, idea.as_deref())?;
            println!("Added finding {}: {}", finding.id, finding.statement);
            Ok(())
        }
        FindingCommand::List {
            project_id,
            status,
            kind,
            json,
        } => list(&store, &project_id, status, kind, json),
        FindingCommand::Show { finding_id, json } => show(&store, &finding_id, json),
        FindingCommand::Evidence {
            finding_id,
            supports,
            refutes: _,
            run,
            experiment,
            note,
        } => {
            let evidence = findings::add_evidence(
                &store,
                &finding_id,
                supports,
                run.as_deref(),
                experiment.as_deref(),
                note.as_deref(),
            )?;
            println!(
                "Recorded evidence {} finding {}.",
                if evidence.supports {
                    "supporting"
                } else {
                    "against"
                },
                evidence.finding_id
            );
            Ok(())
        }
        FindingCommand::Relate {
            finding_id,
            relation,
            other_id,
        } => {
            let link = findings::relate(&store, &finding_id, relation, &other_id)?;
            println!(
                "{} {} {}.",
                link.from_id,
                link.relation.as_str(),
                link.to_id
            );
            Ok(())
        }
        FindingCommand::Assess {
            finding_id,
            verdict,
            confidence,
            scope,
            gaps,
            next,
        } => {
            let assessment = findings::assess(
                &store,
                &finding_id,
                verdict,
                confidence,
                scope.as_deref(),
                gaps.as_deref(),
                next.as_deref(),
            )?;
            println!(
                "Finding {} is now {}.",
                assessment.finding_id,
                assessment.verdict.as_str()
            );
            Ok(())
        }
        FindingCommand::Edit {
            finding_id,
            statement,
        } => {
            let finding = findings::edit(&store, &finding_id, Some(&statement), None)?;
            println!("Reworded finding {}.", finding.id);
            Ok(())
        }
        FindingCommand::Lock { finding_id } => {
            let finding = findings::set_locked(&store, &finding_id, true, super::by_user())?;
            println!("Finding {} is locked.", finding.id);
            Ok(())
        }
        FindingCommand::Unlock { finding_id } => {
            let finding = findings::set_locked(&store, &finding_id, false, super::by_user())?;
            println!("Finding {} is unlocked.", finding.id);
            Ok(())
        }
    }
}

fn list(
    store: &Store,
    project_id: &str,
    status: Option<findings::Status>,
    kind: Option<findings::Kind>,
    json: bool,
) -> Result<()> {
    let project = crate::local::resolve::resolve_project(store, project_id)?;
    let findings: Vec<Finding> = findings::list(store, &project.id)?
        .into_iter()
        .filter(|finding| status.is_none_or(|status| finding.status == status))
        .filter(|finding| kind.is_none_or(|kind| finding.kind == kind))
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&findings)?);
        return Ok(());
    }
    if findings.is_empty() {
        println!(
            "No findings yet. Record one with `orx finding add {} \"<claim>\" --kind model`.",
            project.id
        );
        return Ok(());
    }
    let mut rows = Vec::with_capacity(findings.len());
    for finding in &findings {
        let (supporting, against) = findings::evidence_counts(store, &finding.id)?;
        rows.push(vec![
            finding.id.clone(),
            finding.kind.as_str().to_string(),
            if finding.locked {
                format!("{} (locked)", finding.status.as_str())
            } else {
                finding.status.as_str().to_string()
            },
            format!("+{supporting} −{against}"),
            finding.statement.clone(),
        ]);
    }
    print_table(&["ID", "KIND", "STATUS", "EVIDENCE", "CLAIM"], &rows);
    Ok(())
}

fn show(store: &Store, id: &str, json: bool) -> Result<()> {
    let finding = findings::require(store, id)?;
    let evidence = findings::evidence(store, &finding.id)?;
    let links = findings::links(store, &finding.id)?;
    let assessments = findings::assessments(store, &finding.id)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "finding": finding,
                "evidence": evidence,
                "links": links,
                "assessments": assessments,
            }))?
        );
        return Ok(());
    }
    println!("{} {}", finding.id, finding.statement);
    println!(
        "  {} claim, {}{}",
        finding.kind.as_str(),
        finding.status.as_str(),
        if finding.locked { ", locked" } else { "" }
    );
    if let Some(idea) = &finding.idea_id {
        println!("  from idea {idea}");
    }
    if !evidence.is_empty() {
        println!("\nEvidence:");
        for item in &evidence {
            let mut parts = Vec::new();
            if let Some(run) = &item.run_id {
                parts.push(format!("run {run}"));
            } else if let Some(experiment) = &item.experiment_id {
                parts.push(format!("experiment {experiment}"));
            }
            if let Some(note) = &item.note {
                parts.push(note.clone());
            }
            if let Some(serde_json::Value::Object(values)) = &item.metrics {
                let shown: Vec<String> = values
                    .iter()
                    .take(4)
                    .map(|(key, value)| format!("{key} {}", metrics::format_value(value.as_f64())))
                    .collect();
                parts.push(shown.join(", "));
            }
            println!(
                "  {} {}",
                if item.supports { "+" } else { "−" },
                parts.join(" · ")
            );
        }
    }
    if !links.is_empty() {
        println!("\nLinks:");
        for link in &links {
            let (other, phrase) = if link.from_id == finding.id {
                (&link.to_id, link.relation.as_str().to_string())
            } else {
                (&link.from_id, format!("{} this ←", link.relation.as_str()))
            };
            let statement = findings::get(store, other)?
                .map(|other| other.statement)
                .unwrap_or_default();
            println!("  {phrase} {other}: {statement}");
        }
    }
    if !assessments.is_empty() {
        println!("\nAssessments, oldest first:");
        for assessment in &assessments {
            let mut parts = vec![assessment.verdict.as_str().to_string()];
            if let Some(confidence) = assessment.confidence {
                parts.push(format!(
                    "confidence {}",
                    metrics::format_value(Some(confidence))
                ));
            }
            if let Some(scope) = &assessment.scope {
                parts.push(format!("holds for {scope}"));
            }
            if let Some(gaps) = &assessment.gaps {
                parts.push(format!("gaps: {gaps}"));
            }
            if let Some(next) = &assessment.next_evidence {
                parts.push(format!("next: {next}"));
            }
            println!("  {}", parts.join(" · "));
        }
    }
    Ok(())
}
