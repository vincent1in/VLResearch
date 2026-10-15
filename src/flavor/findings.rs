//! The findings ledger: what the project has learned, as claims with
//! evidence. A claim ("volume features add IC in high-volatility months") is
//! backed or undercut by runs, linked to related claims, and assessed over
//! time; assessments are append-only, so earlier verdicts stay on record.
//! Refuted claims stay too: they count the attempts behind any result.
//!
//! Ported in spirit from openResearch1's atom knowledge graph, keeping only
//! its claim, evidence, relation and assessment tables.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::error::{anyhow, Result};
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    /// About the data: coverage, quality, leakage, what a source carries.
    Dataset,
    /// About models and features: what helps, where, and how much.
    Model,
    /// About the pipeline: speed, cost, reliability.
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Open,
    Testing,
    Supported,
    Refuted,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Relation {
    Contradicts,
    DependsOn,
    Refines,
    Motivates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Verdict {
    Supported,
    Refuted,
    Inconclusive,
}

macro_rules! text_enum {
    ($type:ty { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $type {
            pub(crate) fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
            fn parse(value: &str) -> Option<Self> {
                match value { $($text => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

text_enum!(Kind { Dataset => "dataset", Model => "model", Process => "process" });
text_enum!(Status { Open => "open", Testing => "testing", Supported => "supported", Refuted => "refuted", Inconclusive => "inconclusive" });
text_enum!(Relation { Contradicts => "contradicts", DependsOn => "depends-on", Refines => "refines", Motivates => "motivates" });
text_enum!(Verdict { Supported => "supported", Refuted => "refuted", Inconclusive => "inconclusive" });

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Finding {
    pub id: String,
    pub project_id: String,
    pub kind: Kind,
    pub statement: String,
    pub status: Status,
    /// Locked findings were settled by the user; agents can't change them.
    pub locked: bool,
    /// The idea whose test produced or tests the claim, if any.
    pub idea_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Evidence {
    pub id: i64,
    pub finding_id: String,
    pub supports: bool,
    pub experiment_id: Option<String>,
    pub run_id: Option<String>,
    pub note: Option<String>,
    /// The run's last reported value of each metric when this was recorded.
    pub metrics: Option<serde_json::Value>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Link {
    pub from_id: String,
    pub relation: Relation,
    pub to_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Assessment {
    pub id: i64,
    pub finding_id: String,
    pub verdict: Verdict,
    /// 0 to 1.
    pub confidence: Option<f64>,
    /// Where the verdict holds: universe, period, regime, model family.
    pub scope: Option<String>,
    pub gaps: Option<String>,
    pub next_evidence: Option<String>,
    pub created_at: i64,
}

pub(crate) fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vlr_findings (
             id         TEXT PRIMARY KEY,
             project_id TEXT NOT NULL,
             kind       TEXT NOT NULL,
             statement  TEXT NOT NULL,
             status     TEXT NOT NULL,
             locked     INTEGER NOT NULL DEFAULT 0,
             idea_id    TEXT,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_vlr_findings_project ON vlr_findings(project_id);
         CREATE TABLE IF NOT EXISTS vlr_finding_evidence (
             id            INTEGER PRIMARY KEY AUTOINCREMENT,
             finding_id    TEXT NOT NULL,
             supports      INTEGER NOT NULL,
             experiment_id TEXT,
             run_id        TEXT,
             note          TEXT,
             metrics_json  TEXT,
             created_at    INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_vlr_finding_evidence ON vlr_finding_evidence(finding_id);
         CREATE TABLE IF NOT EXISTS vlr_finding_relations (
             from_id    TEXT NOT NULL,
             relation   TEXT NOT NULL,
             to_id      TEXT NOT NULL,
             created_at INTEGER NOT NULL,
             PRIMARY KEY (from_id, relation, to_id)
         );
         CREATE TABLE IF NOT EXISTS vlr_finding_assessments (
             id            INTEGER PRIMARY KEY AUTOINCREMENT,
             finding_id    TEXT NOT NULL,
             verdict       TEXT NOT NULL,
             confidence    REAL,
             scope         TEXT,
             gaps          TEXT,
             next_evidence TEXT,
             created_at    INTEGER NOT NULL
         );",
    )?;
    Ok(())
}

const FINDING_COLUMNS: &str =
    "id, project_id, kind, statement, status, locked, idea_id, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<Finding> {
    Ok(Finding {
        id: row.get(0)?,
        project_id: row.get(1)?,
        kind: Kind::parse(&row.get::<_, String>(2)?).unwrap_or(Kind::Model),
        statement: row.get(3)?,
        status: Status::parse(&row.get::<_, String>(4)?).unwrap_or(Status::Open),
        locked: row.get(5)?,
        idea_id: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub(crate) fn get(store: &Store, id: &str) -> Result<Option<Finding>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    Ok(conn
        .query_row(
            &format!("SELECT {FINDING_COLUMNS} FROM vlr_findings WHERE id = ?1"),
            [id.trim()],
            from_row,
        )
        .optional()?)
}

pub(crate) fn require(store: &Store, id: &str) -> Result<Finding> {
    get(store, id)?.ok_or_else(|| {
        anyhow!("Finding {id} was not found. List findings with `orx finding list <projectId>`.")
    })
}

/// Every finding in the project, newest first.
pub(crate) fn list(store: &Store, project_id: &str) -> Result<Vec<Finding>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {FINDING_COLUMNS} FROM vlr_findings WHERE project_id = ?1
         ORDER BY updated_at DESC, id"
    ))?;
    let findings = stmt
        .query_map([project_id], from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(findings)
}

fn writable(finding: &Finding) -> Result<()> {
    if finding.locked {
        return Err(anyhow!(
            "Finding {} is locked: the user settled it. Add evidence if you have new results, \
             and ask the user to unlock it if the claim itself should change.",
            finding.id
        ));
    }
    Ok(())
}

pub(crate) fn add(
    store: &Store,
    project_id: &str,
    kind: Kind,
    statement: &str,
    idea_id: Option<&str>,
) -> Result<Finding> {
    let statement = statement.trim();
    if statement.is_empty() {
        return Err(anyhow!("A finding needs a statement."));
    }
    if let Some(idea_id) = idea_id {
        let idea = super::ideas::require(store, idea_id)?;
        if idea.project_id != project_id {
            return Err(anyhow!("Idea {} belongs to another project.", idea.id));
        }
    }
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let id = format!("fnd_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let now = crate::store::now_ms();
    conn.execute(
        "INSERT INTO vlr_findings (id, project_id, kind, statement, status, locked, idea_id,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'open', 0, ?5, ?6, ?6)",
        params![id, project_id, kind.as_str(), statement, idea_id, now],
    )?;
    require(store, &id)
}

/// Rewords a claim or moves it to `testing`; locked findings refuse.
pub(crate) fn edit(
    store: &Store,
    id: &str,
    statement: Option<&str>,
    status: Option<Status>,
) -> Result<Finding> {
    let finding = require(store, id)?;
    writable(&finding)?;
    if statement.is_some_and(|statement| statement.trim().is_empty()) {
        return Err(anyhow!("A finding needs a statement."));
    }
    store.flavor_conn().execute(
        "UPDATE vlr_findings SET statement = COALESCE(?2, statement),
             status = COALESCE(?3, status), updated_at = ?4
         WHERE id = ?1",
        params![
            finding.id,
            statement.map(str::trim),
            status.map(Status::as_str),
            crate::store::now_ms()
        ],
    )?;
    require(store, &finding.id)
}

/// Records a run or experiment that bears on a claim. With a run, a snapshot
/// of its last reported metric values goes with it.
pub(crate) fn add_evidence(
    store: &Store,
    id: &str,
    supports: bool,
    run_id: Option<&str>,
    experiment_id: Option<&str>,
    note: Option<&str>,
) -> Result<Evidence> {
    let finding = require(store, id)?;
    if run_id.is_none() && experiment_id.is_none() && note.is_none_or(|note| note.trim().is_empty())
    {
        return Err(anyhow!(
            "Evidence needs a run (--run), an experiment (--experiment) or a note (--note)."
        ));
    }
    let mut experiment = experiment_id.map(str::to_owned);
    let mut snapshot = None;
    if let Some(run_id) = run_id {
        let run = crate::local::resolve::resolve_run(store, run_id)?;
        if run.project_id != finding.project_id {
            return Err(anyhow!("Run {} belongs to another project.", run.id));
        }
        super::metrics::ingest(store, &run)?;
        let values: serde_json::Map<String, serde_json::Value> =
            super::metrics::run_summary(store, &run.id)?
                .into_iter()
                .map(|summary| (summary.key, serde_json::json!(summary.last)))
                .collect();
        snapshot = (!values.is_empty()).then_some(serde_json::Value::Object(values));
        experiment.get_or_insert(run.experiment_id);
    }
    if let Some(experiment_id) = &experiment {
        let experiment = crate::local::resolve::resolve_experiment(store, experiment_id)?;
        if experiment.project_id != finding.project_id {
            return Err(anyhow!(
                "Experiment {} belongs to another project.",
                experiment.id
            ));
        }
    }
    let conn = store.flavor_conn();
    let now = crate::store::now_ms();
    conn.execute(
        "INSERT INTO vlr_finding_evidence (finding_id, supports, experiment_id, run_id, note,
             metrics_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            finding.id,
            supports,
            experiment,
            run_id,
            note.map(str::trim).filter(|note| !note.is_empty()),
            snapshot.as_ref().map(serde_json::Value::to_string),
            now
        ],
    )?;
    let evidence_id = conn.last_insert_rowid();
    // An open claim with evidence is being tested.
    conn.execute(
        "UPDATE vlr_findings SET status = CASE WHEN status = 'open' AND locked = 0
             THEN 'testing' ELSE status END, updated_at = ?2
         WHERE id = ?1",
        params![finding.id, now],
    )?;
    Ok(evidence(store, &finding.id)?
        .into_iter()
        .find(|evidence| evidence.id == evidence_id)
        .expect("the evidence just recorded"))
}

pub(crate) fn evidence(store: &Store, id: &str) -> Result<Vec<Evidence>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, finding_id, supports, experiment_id, run_id, note, metrics_json, created_at
         FROM vlr_finding_evidence WHERE finding_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt
        .query_map([id], |row| {
            let metrics: Option<String> = row.get(6)?;
            Ok(Evidence {
                id: row.get(0)?,
                finding_id: row.get(1)?,
                supports: row.get(2)?,
                experiment_id: row.get(3)?,
                run_id: row.get(4)?,
                note: row.get(5)?,
                metrics: metrics.and_then(|text| serde_json::from_str(&text).ok()),
                created_at: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub(crate) fn relate(store: &Store, from: &str, relation: Relation, to: &str) -> Result<Link> {
    let from = require(store, from)?;
    let to = require(store, to)?;
    if from.id == to.id {
        return Err(anyhow!("A finding can't relate to itself."));
    }
    if from.project_id != to.project_id {
        return Err(anyhow!(
            "Findings {} and {} are in different projects.",
            from.id,
            to.id
        ));
    }
    store.flavor_conn().execute(
        "INSERT OR IGNORE INTO vlr_finding_relations (from_id, relation, to_id, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![from.id, relation.as_str(), to.id, crate::store::now_ms()],
    )?;
    Ok(Link {
        from_id: from.id,
        relation,
        to_id: to.id,
    })
}

/// Links that start or end at a finding: its one-hop neighborhood.
pub(crate) fn links(store: &Store, id: &str) -> Result<Vec<Link>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT from_id, relation, to_id FROM vlr_finding_relations
         WHERE from_id = ?1 OR to_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(from_id, relation, to_id)| {
            Some(Link {
                from_id,
                relation: Relation::parse(&relation)?,
                to_id,
            })
        })
        .collect())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Adds an assessment (never edits one) and moves the claim to its verdict.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assess(
    store: &Store,
    id: &str,
    verdict: Verdict,
    confidence: Option<f64>,
    scope: Option<&str>,
    gaps: Option<&str>,
    next_evidence: Option<&str>,
) -> Result<Assessment> {
    let finding = require(store, id)?;
    writable(&finding)?;
    if confidence.is_some_and(|value| !(0.0..=1.0).contains(&value)) {
        return Err(anyhow!("Confidence is between 0 and 1."));
    }
    let conn = store.flavor_conn();
    let now = crate::store::now_ms();
    conn.execute(
        "INSERT INTO vlr_finding_assessments (finding_id, verdict, confidence, scope, gaps,
             next_evidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            finding.id,
            verdict.as_str(),
            confidence,
            non_empty(scope),
            non_empty(gaps),
            non_empty(next_evidence),
            now
        ],
    )?;
    let assessment_id = conn.last_insert_rowid();
    conn.execute(
        "UPDATE vlr_findings SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![finding.id, verdict.as_str(), now],
    )?;
    Ok(assessments(store, &finding.id)?
        .into_iter()
        .find(|assessment| assessment.id == assessment_id)
        .expect("the assessment just recorded"))
}

pub(crate) fn assessments(store: &Store, id: &str) -> Result<Vec<Assessment>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, finding_id, verdict, confidence, scope, gaps, next_evidence, created_at
         FROM vlr_finding_assessments WHERE finding_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt
        .query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<f64>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(
            |(id, finding_id, verdict, confidence, scope, gaps, next_evidence, created_at)| {
                Some(Assessment {
                    id,
                    finding_id,
                    verdict: Verdict::parse(&verdict)?,
                    confidence,
                    scope,
                    gaps,
                    next_evidence,
                    created_at,
                })
            },
        )
        .collect())
}

/// Locks or unlocks a finding. Only the user may unlock: an agent session is
/// refused, so a settled claim can't be reopened by the agent that disputes it.
pub(crate) fn set_locked(store: &Store, id: &str, locked: bool) -> Result<Finding> {
    let finding = require(store, id)?;
    if !locked && finding.locked && crate::local::chat::in_local_session() {
        return Err(anyhow!(
            "Only the user can unlock finding {}. Ask them to run `orx finding unlock {}` in a \
             terminal.",
            finding.id,
            finding.id
        ));
    }
    store.flavor_conn().execute(
        "UPDATE vlr_findings SET locked = ?2, updated_at = ?3 WHERE id = ?1",
        params![finding.id, locked, crate::store::now_ms()],
    )?;
    require(store, &finding.id)
}

/// A finding's evidence as (supporting, refuting) counts, for listings.
pub(crate) fn evidence_counts(store: &Store, id: &str) -> Result<(i64, i64)> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(supports), 0), COALESCE(SUM(1 - supports), 0)
         FROM vlr_finding_evidence WHERE finding_id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("vlr-findings-{}", uuid::Uuid::new_v4()));
        (Store::open_at(dir.clone()).unwrap(), dir)
    }

    #[test]
    fn claims_gather_evidence_relations_and_append_only_assessments() {
        let (store, dir) = temp_store();
        let claim = add(
            &store,
            "p",
            Kind::Model,
            "Volume features add IC in high-volatility months",
            None,
        )
        .unwrap();
        assert_eq!(claim.status, Status::Open);
        let note = add_evidence(
            &store,
            &claim.id,
            true,
            None,
            None,
            Some("2022 fold: +0.004"),
        )
        .unwrap();
        assert!(note.supports);
        assert_eq!(require(&store, &claim.id).unwrap().status, Status::Testing);
        assert!(add_evidence(&store, &claim.id, false, None, None, None).is_err());

        let other = add(
            &store,
            "p",
            Kind::Dataset,
            "The vendor's volume has gaps before 2015",
            None,
        )
        .unwrap();
        relate(&store, &other.id, Relation::Contradicts, &claim.id).unwrap();
        relate(&store, &other.id, Relation::Contradicts, &claim.id).unwrap();
        assert!(relate(&store, &claim.id, Relation::Refines, &claim.id).is_err());
        let around_claim = links(&store, &claim.id).unwrap();
        assert_eq!(around_claim.len(), 1);
        assert_eq!(around_claim[0].relation, Relation::Contradicts);

        assess(
            &store,
            &claim.id,
            Verdict::Inconclusive,
            Some(0.4),
            None,
            Some("one fold"),
            None,
        )
        .unwrap();
        assess(
            &store,
            &claim.id,
            Verdict::Supported,
            Some(0.8),
            Some("2016-2023, top 1000"),
            None,
            None,
        )
        .unwrap();
        let history = assessments(&store, &claim.id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].verdict, Verdict::Inconclusive);
        assert_eq!(
            require(&store, &claim.id).unwrap().status,
            Status::Supported
        );
        assert!(assess(
            &store,
            &claim.id,
            Verdict::Supported,
            Some(1.5),
            None,
            None,
            None
        )
        .is_err());

        // Locked: no more verdicts or rewording, but evidence still lands.
        set_locked(&store, &claim.id, true).unwrap();
        assert!(assess(&store, &claim.id, Verdict::Refuted, None, None, None, None).is_err());
        assert!(edit(&store, &claim.id, Some("Reworded"), None).is_err());
        add_evidence(
            &store,
            &claim.id,
            false,
            None,
            None,
            Some("2024 fold: -0.001"),
        )
        .unwrap();
        assert_eq!(evidence_counts(&store, &claim.id).unwrap(), (1, 1));
        assert_eq!(
            require(&store, &claim.id).unwrap().status,
            Status::Supported
        );
        set_locked(&store, &claim.id, false).unwrap();
        edit(&store, &claim.id, Some("Reworded"), None).unwrap();

        assert_eq!(list(&store, "p").unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
