//! The metrics ledger. Training and evaluation code prints lines such as
//!
//! ```text
//! ORX_METRIC {"key": "val_ic", "value": 0.031, "step": 1200}
//! ORX_METRIC {"step": 1200, "val_ic": 0.031, "train_loss": 0.42}
//! ```
//!
//! and orx records them per run from the run's local log, which the run's
//! supervisor appends every line of job output to. Reading is incremental: a
//! run's byte offset is kept, so each call parses only what is new.
//!
//! A project names one primary metric and its direction. That defines "best"
//! everywhere: the leaderboard, the running best against the baseline, and the
//! lineage path from the baseline to the best experiment, whose steps add up
//! exactly to the total gain.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{anyhow, Result};
use crate::local::model::LocalExperiment;
use crate::store::{Store, StoredRun};

/// What marks a metrics line in run output.
pub(crate) const MARKER: &str = "ORX_METRIC";

/// Fields of a multi-metric line that are not metrics themselves.
const RESERVED_FIELDS: [&str; 4] = ["step", "split", "key", "value"];
const MAX_KEY_CHARS: usize = 200;
const MAX_METRICS_PER_LINE: usize = 64;
/// Longer lines are never metrics records.
const MAX_LINE_BYTES: u64 = 1 << 20;

/// One reported value. `value` is `None` when the code reported NaN or an
/// infinity, which SQLite cannot hold and which never counts as a score.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MetricPoint {
    pub key: String,
    pub value: Option<f64>,
    pub step: Option<i64>,
    pub split: Option<String>,
}

/// Every metric on one line of run output.
pub(crate) fn parse_line(line: &str) -> Vec<MetricPoint> {
    let mut points = Vec::new();
    let mut rest = line;
    while let Some(index) = rest.find(MARKER) {
        rest = &rest[index + MARKER.len()..];
        let Some(open) = rest.find('{') else {
            break;
        };
        // Anything other than spacing or a separator between the marker and
        // the object means this was not a metrics record after all.
        if !rest[..open]
            .chars()
            .all(|c| c.is_whitespace() || c == ':' || c == '=')
        {
            continue;
        }
        let (json, _) = sanitize_non_finite(&rest[open..]);
        let mut stream = serde_json::Deserializer::from_str(&json).into_iter::<Record>();
        if let Some(Ok(record)) = stream.next() {
            points.extend(points_from_record(&record));
        }
    }
    points.truncate(MAX_METRICS_PER_LINE);
    points
}

/// A JSON object's fields in the order they were written (serde_json's own
/// map sorts them, and the order a line lists its metrics is worth keeping).
struct Record(Vec<(String, serde_json::Value)>);

impl Record {
    fn get(&self, name: &str) -> Option<&serde_json::Value> {
        self.0
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
    }
}

impl<'de> serde::Deserialize<'de> for Record {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> serde::de::Visitor<'de> for Fields {
            type Value = Record;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Record, A::Error> {
                let mut fields = Vec::new();
                while let Some(entry) = map.next_entry::<String, serde_json::Value>()? {
                    fields.push(entry);
                }
                Ok(Record(fields))
            }
        }
        deserializer.deserialize_map(Fields)
    }
}

fn points_from_record(record: &Record) -> Vec<MetricPoint> {
    let step = record.get("step").and_then(|step| {
        step.as_i64().or_else(|| {
            step.as_f64()
                .filter(|value| value.fract() == 0.0 && value.is_finite())
                .map(|value| value as i64)
        })
    });
    let split = record
        .get("split")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|split| !split.is_empty())
        .map(str::to_owned);
    let point = |key: &str, value: &serde_json::Value| -> Option<MetricPoint> {
        let key = key.trim();
        if key.is_empty()
            || key.chars().count() > MAX_KEY_CHARS
            || key.chars().any(char::is_control)
        {
            return None;
        }
        let value = match value {
            serde_json::Value::Number(number) => number.as_f64().filter(|v| v.is_finite()),
            // NaN and the infinities arrive as null (see `sanitize_non_finite`).
            serde_json::Value::Null => None,
            _ => return None,
        };
        Some(MetricPoint {
            key: key.to_owned(),
            value,
            step,
            split: split.clone(),
        })
    };
    if let Some(key) = record.get("key").and_then(serde_json::Value::as_str) {
        return record
            .get("value")
            .and_then(|value| point(key, value))
            .into_iter()
            .collect();
    }
    record
        .0
        .iter()
        .filter(|(name, _)| !RESERVED_FIELDS.contains(&name.as_str()))
        .filter_map(|(name, value)| point(name, value))
        .collect()
}

/// Spellings of non-finite numbers that code prints where JSON wants a number:
/// Python's `json.dumps` writes `NaN` and `Infinity`, and an f-string writes
/// `nan` and `inf`. Longer spellings come first so they win.
const NON_FINITE: [&str; 8] = [
    "-infinity",
    "+infinity",
    "infinity",
    "-inf",
    "+inf",
    "inf",
    "-nan",
    "nan",
];

/// Replaces non-finite number tokens outside strings with `null`. Returns the
/// text and whether anything was replaced.
fn sanitize_non_finite(text: &str) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut replaced = false;
    let mut previous: Option<char> = None;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            previous = Some(c);
            rest = &rest[c.len_utf8()..];
            continue;
        }
        if c == '"' {
            in_string = true;
        } else if !previous.is_some_and(|p| p.is_ascii_alphanumeric()) {
            let token = NON_FINITE.into_iter().find(|token| {
                rest.get(..token.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(token))
                    && !rest[token.len()..]
                        .chars()
                        .next()
                        .is_some_and(|next| next.is_ascii_alphanumeric())
            });
            if let Some(token) = token {
                out.push_str("null");
                replaced = true;
                previous = Some('l');
                rest = &rest[token.len()..];
                continue;
            }
        }
        out.push(c);
        previous = Some(c);
        rest = &rest[c.len_utf8()..];
    }
    (out, replaced)
}

/// Whether a metrics line reports a NaN or an infinity.
pub(crate) fn has_non_finite(line: &str) -> bool {
    let mut rest = line;
    while let Some(index) = rest.find(MARKER) {
        rest = &rest[index + MARKER.len()..];
        if let Some(open) = rest.find('{') {
            if sanitize_non_finite(&rest[open..]).1 {
                return true;
            }
        }
    }
    false
}

pub(crate) fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vlr_run_metrics (
             run_id      TEXT NOT NULL,
             seq         INTEGER NOT NULL,
             key         TEXT NOT NULL,
             step        INTEGER,
             value       REAL,
             split       TEXT,
             recorded_at INTEGER NOT NULL,
             PRIMARY KEY (run_id, seq)
         );
         CREATE INDEX IF NOT EXISTS idx_vlr_run_metrics_key ON vlr_run_metrics(run_id, key);
         CREATE TABLE IF NOT EXISTS vlr_metric_offsets (
             run_id    TEXT PRIMARY KEY,
             offset    INTEGER NOT NULL,
             next_seq  INTEGER NOT NULL,
             head_hash TEXT
         );
         CREATE TABLE IF NOT EXISTS vlr_project_metric (
             project_id  TEXT PRIMARY KEY,
             key         TEXT NOT NULL,
             direction   TEXT NOT NULL,
             noise_floor REAL,
             updated_at  INTEGER NOT NULL,
             split       TEXT
         );",
    )?;
    add_column(conn, "vlr_metric_offsets", "head_hash", "TEXT")?;
    add_column(conn, "vlr_project_metric", "split", "TEXT")?;
    Ok(())
}

/// Adds a column that tables made by an earlier VLResearch version lack.
pub(crate) fn add_column(conn: &Connection, table: &str, column: &str, kind: &str) -> Result<()> {
    let exists = conn
        .prepare(&format!(
            "SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"
        ))?
        .exists([column])?;
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))?;
    }
    Ok(())
}

/// Runs `body` inside `BEGIN IMMEDIATE`, so two processes reading the same log
/// can't both record its new lines.
fn immediate<T>(conn: &Connection, body: impl FnOnce() -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        return body();
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match body() {
        Ok(value) => {
            conn.execute_batch("COMMIT")?;
            Ok(value)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

/// How much of a log's start identifies it: a supervisor that restarts
/// rewrites its log from the top, and a changed start means a new log.
const HEAD_BYTES: u64 = 4096;

fn head_hash(mut file: &std::fs::File, length: u64) -> Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut head = Vec::new();
    Read::take(&mut file, length.min(HEAD_BYTES)).read_to_end(&mut head)?;
    Ok(format!("{:x}", Sha256::digest(&head)))
}

/// Records the metrics a run has printed since the last call. A finished run's
/// last line counts even without a trailing newline.
pub(crate) fn ingest(store: &Store, run: &StoredRun) -> Result<usize> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let path = super::wake::run_log_path(store, &run.id);
    let Ok(file) = std::fs::File::open(&path) else {
        return Ok(0);
    };
    let finished = crate::store::is_terminal_status(&run.status);
    immediate(conn, || {
        // Measured inside the transaction, so a reader that waited for
        // another one sees the log as it is now.
        let length = file.metadata()?.len();
        let stored: Option<(u64, i64, Option<String>)> = conn
            .query_row(
                "SELECT offset, next_seq, head_hash FROM vlr_metric_offsets WHERE run_id = ?1",
                [&run.id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?.max(0) as u64,
                        row.get(1)?,
                        row.get(2)?,
                    ))
                },
            )
            .optional()?;
        let (mut offset, mut seq) = stored
            .as_ref()
            .map_or((0, 0), |(offset, seq, _)| (*offset, *seq));
        let replaced = match &stored {
            Some((stored_offset, _, _)) if length < *stored_offset => true,
            Some((stored_offset, _, Some(hash))) if *stored_offset > 0 => {
                *hash != head_hash(&file, *stored_offset)?
            }
            _ => false,
        };
        if replaced {
            conn.execute("DELETE FROM vlr_run_metrics WHERE run_id = ?1", [&run.id])?;
            (offset, seq) = (0, 0);
        }
        if length == offset && !replaced {
            return Ok(0);
        }
        let mut reader = BufReader::new(&file);
        reader.seek(SeekFrom::Start(offset))?;
        let now = crate::store::now_ms();
        let mut recorded = 0;
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = Read::take(&mut reader, MAX_LINE_BYTES).read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            // A progress bar can redraw for hours without a newline; past the
            // cap a "line" is skipped in pieces rather than held and reread.
            let overlong = !line.ends_with(b"\n") && read as u64 == MAX_LINE_BYTES;
            if !line.ends_with(b"\n") && !overlong && !finished {
                // A line still being written.
                break;
            }
            offset += read as u64;
            if overlong || !line.windows(MARKER.len()).any(|w| w == MARKER.as_bytes()) {
                continue;
            }
            for point in parse_line(&String::from_utf8_lossy(&line)) {
                conn.execute(
                    "INSERT OR REPLACE INTO vlr_run_metrics
                         (run_id, seq, key, step, value, split, recorded_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        run.id,
                        seq,
                        point.key,
                        point.step,
                        point.value,
                        point.split,
                        now
                    ],
                )?;
                seq += 1;
                recorded += 1;
            }
        }
        let hash = head_hash(&file, offset)?;
        conn.execute(
            "INSERT INTO vlr_metric_offsets (run_id, offset, next_seq, head_hash)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(run_id) DO UPDATE SET offset = excluded.offset,
                 next_seq = excluded.next_seq, head_hash = excluded.head_hash",
            params![run.id, offset as i64, seq, hash],
        )?;
        Ok(recorded)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    Max,
    Min,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Direction::Max => "max",
            Direction::Min => "min",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "max" => Some(Direction::Max),
            "min" => Some(Direction::Min),
            _ => None,
        }
    }

    /// How much better `candidate` is than `incumbent` (positive is better).
    pub(crate) fn gain(self, candidate: f64, incumbent: f64) -> f64 {
        match self {
            Direction::Max => candidate - incumbent,
            Direction::Min => incumbent - candidate,
        }
    }
}

/// The metric an optimization improves.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrimaryMetric {
    pub key: String,
    pub direction: Direction,
    /// The smallest gain that counts as an improvement rather than noise.
    pub noise_floor: Option<f64>,
    /// The split the key is scored on, when runs report it under several.
    pub split: Option<String>,
}

/// An optimization's primary metric, once it has one.
pub(crate) fn primary(store: &Store, optimization_id: &str) -> Result<Option<PrimaryMetric>> {
    let conn = store.flavor_conn();
    super::scope::ensure_tables(conn)?;
    let row = conn
        .query_row(
            "SELECT metric_key, metric_direction, noise_floor, metric_split
             FROM vlr_optimizations WHERE id = ?1",
            [optimization_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<f64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(row.and_then(|(key, direction, noise_floor, split)| {
        Some(PrimaryMetric {
            key: key?,
            direction: Direction::parse(&direction?)?,
            noise_floor,
            split,
        })
    }))
}

pub(crate) fn set_primary(
    store: &Store,
    optimization_id: &str,
    metric: &PrimaryMetric,
) -> Result<()> {
    if metric.key.trim().is_empty() {
        return Err(anyhow!("The metric key can't be empty."));
    }
    if metric
        .noise_floor
        .is_some_and(|floor| !floor.is_finite() || floor < 0.0)
    {
        return Err(anyhow!("The noise floor must be a non-negative number."));
    }
    let conn = store.flavor_conn();
    super::scope::ensure_tables(conn)?;
    let updated = conn.execute(
        "UPDATE vlr_optimizations SET metric_key = ?2, metric_direction = ?3, noise_floor = ?4,
             metric_split = ?5, updated_at = ?6
         WHERE id = ?1",
        params![
            optimization_id,
            metric.key.trim(),
            metric.direction.as_str(),
            metric.noise_floor,
            metric
                .split
                .as_deref()
                .map(str::trim)
                .filter(|split| !split.is_empty()),
            crate::store::now_ms(),
        ],
    )?;
    if updated == 0 {
        return Err(anyhow!("Optimization {optimization_id} was not found."));
    }
    Ok(())
}

/// A bound an optimization's experiments must keep on a metric besides the
/// primary one: a speed-up whose output must still match the baseline's
/// (`output_diff` at most 0), or a model change that must keep turnover under
/// a cap. An experiment that breaks a guard, or doesn't report its key, can't
/// be kept or become the best. Baselines are what guards are measured
/// against, so they are exempt.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Guard {
    pub key: String,
    pub split: Option<String>,
    pub max: Option<f64>,
    pub min: Option<f64>,
}

impl Guard {
    /// Why a value breaks the guard, or `None` when it keeps it.
    pub(crate) fn breach(&self, value: Option<f64>) -> Option<String> {
        let Some(value) = value else {
            return Some(format!("it didn't report {}", self.key));
        };
        let shown = |number: f64| format_value(Some(number));
        if let Some(max) = self.max.filter(|max| value > *max) {
            return Some(format!(
                "{} is {}, above {}",
                self.key,
                shown(value),
                shown(max)
            ));
        }
        if let Some(min) = self.min.filter(|min| value < *min) {
            return Some(format!(
                "{} is {}, below {}",
                self.key,
                shown(value),
                shown(min)
            ));
        }
        None
    }

    /// The guard in words: "output_diff at most 0".
    pub(crate) fn describe(&self) -> String {
        let shown = |number: f64| format_value(Some(number));
        let bounds = match (self.min, self.max) {
            (Some(min), Some(max)) => format!("between {} and {}", shown(min), shown(max)),
            (Some(min), None) => format!("at least {}", shown(min)),
            (None, Some(max)) => format!("at most {}", shown(max)),
            (None, None) => "unbounded".to_string(),
        };
        match &self.split {
            Some(split) => format!("{} ({split}) {bounds}", self.key),
            None => format!("{} {bounds}", self.key),
        }
    }
}

fn ensure_guards(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vlr_metric_guards (
             optimization_id TEXT NOT NULL,
             key             TEXT NOT NULL,
             split           TEXT,
             max             REAL,
             min             REAL,
             created_at      INTEGER NOT NULL,
             PRIMARY KEY (optimization_id, key)
         );",
    )?;
    Ok(())
}

/// An optimization's guards, in the order they were set.
pub(crate) fn guards(store: &Store, optimization_id: &str) -> Result<Vec<Guard>> {
    let conn = store.flavor_conn();
    ensure_guards(conn)?;
    let mut stmt = conn.prepare(
        "SELECT key, split, max, min FROM vlr_metric_guards
         WHERE optimization_id = ?1 ORDER BY created_at, key",
    )?;
    let rows = stmt.query_map([optimization_id], |row| {
        Ok(Guard {
            key: row.get(0)?,
            split: row.get(1)?,
            max: row.get(2)?,
            min: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Sets a guard, replacing one on the same key.
pub(crate) fn set_guard(store: &Store, optimization_id: &str, guard: &Guard) -> Result<()> {
    let key = guard.key.trim();
    if key.is_empty() {
        return Err(anyhow!("The guard's key can't be empty."));
    }
    let bounds = [guard.min, guard.max];
    if bounds.iter().all(Option::is_none) {
        return Err(anyhow!("Give the guard a bound: `--max`, `--min` or both."));
    }
    if bounds.iter().flatten().any(|bound| !bound.is_finite()) {
        return Err(anyhow!("A guard's bounds must be numbers."));
    }
    if let (Some(min), Some(max)) = (guard.min, guard.max) {
        if min > max {
            return Err(anyhow!("The guard's --min is above its --max."));
        }
    }
    if super::scope::get(store, optimization_id)?.is_none() {
        return Err(anyhow!("Optimization {optimization_id} was not found."));
    }
    let conn = store.flavor_conn();
    ensure_guards(conn)?;
    conn.execute(
        "INSERT INTO vlr_metric_guards (optimization_id, key, split, max, min, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(optimization_id, key) DO UPDATE SET split = excluded.split,
             max = excluded.max, min = excluded.min",
        params![
            optimization_id,
            key,
            guard
                .split
                .as_deref()
                .map(str::trim)
                .filter(|split| !split.is_empty()),
            guard.max,
            guard.min,
            crate::store::now_ms(),
        ],
    )?;
    Ok(())
}

/// Removes the guard on `key`; whether there was one.
pub(crate) fn remove_guard(store: &Store, optimization_id: &str, key: &str) -> Result<bool> {
    let conn = store.flavor_conn();
    ensure_guards(conn)?;
    Ok(conn.execute(
        "DELETE FROM vlr_metric_guards WHERE optimization_id = ?1 AND key = ?2",
        params![optimization_id, key.trim()],
    )? > 0)
}

/// The experiments that break a guard, each with why: an experiment's value
/// on a guard's key is the mean of its finished, counted runs.
pub(crate) fn guard_breaches<'a>(
    store: &Store,
    project_id: &str,
    guards: &[Guard],
    experiments: impl IntoIterator<Item = &'a str>,
) -> Result<HashMap<String, String>> {
    let experiments: Vec<&str> = experiments.into_iter().collect();
    let mut breaches = HashMap::new();
    for guard in guards {
        let runs = scored_runs(store, project_id, &guard.key, guard.split.as_deref())?;
        let scores = experiment_scores(&runs, &|_| false);
        for experiment in &experiments {
            if breaches.contains_key(*experiment) {
                continue;
            }
            let value = scores
                .iter()
                .find(|score| score.experiment_id == *experiment)
                .map(|score| score.score);
            if let Some(reason) = guard.breach(value) {
                breaches.insert(experiment.to_string(), reason);
            }
        }
    }
    Ok(breaches)
}

/// The splits a key has been reported under across the project's runs
/// (`None` for points without a split).
pub(crate) fn key_splits(
    store: &Store,
    project_id: &str,
    key: &str,
) -> Result<Vec<Option<String>>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT m.split FROM vlr_run_metrics m JOIN runs r ON r.id = m.run_id
         WHERE r.project_id = ?1 AND m.key = ?2 ORDER BY m.split",
    )?;
    let splits = stmt
        .query_map(params![project_id, key], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(splits)
}

/// What a run reported for one key.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeySummary {
    pub key: String,
    pub split: Option<String>,
    pub points: i64,
    pub last_step: Option<i64>,
    /// The last value reported; `None` when it was NaN or infinite.
    pub last: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// How many of the points were NaN or infinite.
    pub non_finite: i64,
}

/// Every key a run has reported, in the order each first appeared.
pub(crate) fn run_summary(store: &Store, run_id: &str) -> Result<Vec<KeySummary>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT key, split, COUNT(*), MIN(value), MAX(value),
                SUM(CASE WHEN value IS NULL THEN 1 ELSE 0 END), MIN(seq), MAX(seq)
         FROM vlr_run_metrics WHERE run_id = ?1
         GROUP BY key, split ORDER BY MIN(seq)",
    )?;
    let groups = stmt
        .query_map([run_id], |row| {
            Ok((
                KeySummary {
                    key: row.get(0)?,
                    split: row.get(1)?,
                    points: row.get(2)?,
                    last_step: None,
                    last: None,
                    min: row.get(3)?,
                    max: row.get(4)?,
                    non_finite: row.get(5)?,
                },
                row.get::<_, i64>(7)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut summaries = Vec::with_capacity(groups.len());
    for (mut summary, last_seq) in groups {
        let (step, value): (Option<i64>, Option<f64>) = conn.query_row(
            "SELECT step, value FROM vlr_run_metrics WHERE run_id = ?1 AND seq = ?2",
            params![run_id, last_seq],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        summary.last_step = step;
        summary.last = value;
        summaries.push(summary);
    }
    Ok(summaries)
}

/// A run's score on `key` (and `split`, when given): the last finite value it
/// reported. Code that keeps the best checkpoint should report that
/// checkpoint's score last.
pub(crate) fn final_value(
    store: &Store,
    run_id: &str,
    key: &str,
    split: Option<&str>,
) -> Result<Option<f64>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    Ok(conn
        .query_row(
            "SELECT value FROM vlr_run_metrics
             WHERE run_id = ?1 AND key = ?2 AND value IS NOT NULL
               AND (?3 IS NULL OR split = ?3)
             ORDER BY seq DESC LIMIT 1",
            params![run_id, key, split],
            |row| row.get(0),
        )
        .optional()?)
}

/// A run with its experiment and its score on the key being compared.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScoredRun {
    pub run_id: String,
    pub experiment_id: String,
    pub experiment: String,
    pub status: String,
    pub created_at: i64,
    pub ended_at: Option<i64>,
    pub score: Option<f64>,
    /// Launched with `VLR_PROBE=1`: a smoke test or baseline check, never a
    /// result.
    pub probe: bool,
}

impl ScoredRun {
    /// Whether the run can stand as a result: finished, and not a probe.
    pub(crate) fn counts(&self) -> bool {
        self.status == "done" && !self.probe
    }

    fn ended(&self) -> i64 {
        self.ended_at.unwrap_or(self.created_at)
    }
}

/// Every run in the project, newest first, scored on `key` (in `split`, when
/// given) after reading any new output.
pub(crate) fn scored_runs(
    store: &Store,
    project_id: &str,
    key: &str,
    split: Option<&str>,
) -> Result<Vec<ScoredRun>> {
    let names = experiment_names(store, project_id)?;
    let mut scored = Vec::new();
    for run in store.list_runs_by_project(project_id)? {
        ingest(store, &run)?;
        let score = final_value(store, &run.id, key, split)?;
        let probe = super::scope::is_probe(store, &run.id)?;
        scored.push(ScoredRun {
            experiment: names
                .get(&run.experiment_id)
                .cloned()
                .unwrap_or_else(|| run.experiment_id.clone()),
            run_id: run.id,
            experiment_id: run.experiment_id,
            status: run.status,
            created_at: run.created_at,
            ended_at: run.ended_at,
            score,
            probe,
        });
    }
    Ok(scored)
}

/// Keys reported by any run in the project, most used first.
pub(crate) fn project_keys(store: &Store, project_id: &str) -> Result<Vec<String>> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT m.key FROM vlr_run_metrics m JOIN runs r ON r.id = m.run_id
         WHERE r.project_id = ?1 GROUP BY m.key ORDER BY COUNT(DISTINCT m.run_id) DESC, m.key",
    )?;
    let keys = stmt
        .query_map([project_id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(keys)
}

fn experiment_names(store: &Store, project_id: &str) -> Result<HashMap<String, String>> {
    Ok(store
        .list_experiments_by_project(project_id)?
        .into_iter()
        .map(|experiment| {
            let name = display_name(&experiment);
            (experiment.id, name)
        })
        .collect())
}

fn display_name(experiment: &LocalExperiment) -> String {
    experiment
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(&experiment.slug)
        .to_owned()
}

/// What an experiment scores on the primary metric: the mean of its finished,
/// counted runs, so a rerun with a new seed (a replicate) moves it. A
/// baseline that only probe runs measured (its seed runs, say) is scored by
/// their mean.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExperimentScore {
    pub experiment_id: String,
    pub experiment: String,
    /// Its latest scored run.
    pub run_id: String,
    pub score: f64,
    /// How many runs the mean is over.
    pub runs: usize,
    /// Probe runs scored it: a baseline measured only with seed runs.
    pub probe: bool,
    /// When its first and its latest scored runs ended.
    pub first_ended_at: i64,
    pub ended_at: i64,
}

/// Every scored experiment among `runs`, in the order each was first scored.
/// `is_baseline` says which experiments may be scored by probe runs when no
/// counted run scores them: roots, whose seed runs measure the baseline.
pub(crate) fn experiment_scores<'a>(
    runs: impl IntoIterator<Item = &'a ScoredRun>,
    is_baseline: &dyn Fn(&str) -> bool,
) -> Vec<ExperimentScore> {
    let mut groups: Vec<(&str, Vec<&ScoredRun>, Vec<&ScoredRun>)> = Vec::new();
    for run in runs {
        if run.status != "done" || run.score.is_none() {
            continue;
        }
        let index = match groups
            .iter()
            .position(|(id, _, _)| *id == run.experiment_id)
        {
            Some(index) => index,
            None => {
                groups.push((run.experiment_id.as_str(), Vec::new(), Vec::new()));
                groups.len() - 1
            }
        };
        if run.probe {
            groups[index].2.push(run);
        } else {
            groups[index].1.push(run);
        }
    }
    let mut scores: Vec<ExperimentScore> = groups
        .into_iter()
        .filter_map(|(id, counted, probes)| {
            if !counted.is_empty() {
                mean_score(&counted, false)
            } else if is_baseline(id) {
                mean_score(&probes, true)
            } else {
                None
            }
        })
        .collect();
    scores.sort_by(|a, b| {
        (a.first_ended_at, &a.experiment_id).cmp(&(b.first_ended_at, &b.experiment_id))
    });
    scores
}

fn mean_score(runs: &[&ScoredRun], probe: bool) -> Option<ExperimentScore> {
    let order = |run: &&&ScoredRun| (run.ended(), run.created_at);
    let first = runs.iter().min_by_key(order)?;
    let last = runs.iter().max_by_key(order)?;
    let sum: f64 = runs.iter().filter_map(|run| run.score).sum();
    Some(ExperimentScore {
        experiment_id: first.experiment_id.clone(),
        experiment: first.experiment.clone(),
        run_id: last.run_id.clone(),
        score: sum / runs.len() as f64,
        runs: runs.len(),
        probe,
        first_ended_at: first.ended(),
        ended_at: last.ended(),
    })
}

/// The one rule for "best": walking experiments in the order they were first
/// scored, one takes over only when its score gains more than the noise floor
/// on the current best's.
pub(crate) fn best_of<'a>(
    scores: &'a [ExperimentScore],
    metric: &PrimaryMetric,
) -> Option<&'a ExperimentScore> {
    let floor = metric.noise_floor.unwrap_or(0.0);
    let mut best: Option<&ExperimentScore> = None;
    for score in scores {
        let takes_over = best
            .is_none_or(|incumbent| metric.direction.gain(score.score, incumbent.score) > floor);
        if takes_over {
            best = Some(score);
        }
    }
    best
}

/// What an experiment scores, for measuring a change against it: the mean of
/// its finished, counted runs; or, with `allow_probes` and no counted run,
/// the mean of its probe runs (a baseline measured only with seed runs, say).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Reference {
    pub experiment_id: String,
    pub experiment: String,
    /// Its latest counted run; `None` when probe runs score it.
    pub run_id: Option<String>,
    pub score: f64,
    /// How many counted runs the score averages.
    pub runs: usize,
    /// How many probe runs the score averages, when they score it.
    pub probe_runs: usize,
}

pub(crate) fn experiment_reference(
    runs: &[ScoredRun],
    experiment_id: &str,
    allow_probes: bool,
) -> Option<Reference> {
    let of_experiment = runs.iter().filter(|run| run.experiment_id == experiment_id);
    let score = experiment_scores(of_experiment, &|_| allow_probes)
        .into_iter()
        .next()?;
    Some(Reference {
        run_id: (!score.probe).then(|| score.run_id.clone()),
        runs: if score.probe { 0 } else { score.runs },
        probe_runs: if score.probe { score.runs } else { 0 },
        experiment_id: score.experiment_id,
        experiment: score.experiment,
        score: score.score,
    })
}

/// Where a project stands on its primary metric.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Standing {
    /// The best experiment's root, scored by the same rule: the baseline every
    /// gain is measured from.
    pub baseline: Option<Reference>,
    pub best: Option<ExperimentScore>,
    /// Finished, counted runs in the order they ended, with the best so far.
    pub history: Vec<HistoryPoint>,
    /// Experiments a guard sets aside, with why; none of them can be the best.
    pub breaches: Vec<Breach>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Breach {
    pub experiment_id: String,
    pub experiment: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryPoint {
    pub run_id: String,
    pub ended_at: i64,
    pub score: f64,
    /// The best experiment's score once this run had ended.
    pub best_so_far: f64,
    /// This run's experiment became the best: it beat the previous best by
    /// more than the noise floor.
    pub new_best: bool,
}

/// The running best over the experiments of `runs`. Baselines (`is_baseline`)
/// measured only by probe runs take part, so an attempt has to beat them;
/// experiments in `set_aside` (a guard's) can't be the best.
pub(crate) fn running_best(
    runs: &[ScoredRun],
    metric: &PrimaryMetric,
    is_baseline: &dyn Fn(&str) -> bool,
    set_aside: &std::collections::HashSet<String>,
) -> Standing {
    let eligible = |scores: Vec<ExperimentScore>| -> Vec<ExperimentScore> {
        scores
            .into_iter()
            .filter(|score| !set_aside.contains(&score.experiment_id))
            .collect()
    };
    let mut scored: Vec<&ScoredRun> = runs
        .iter()
        .filter(|run| run.status == "done" && run.score.is_some())
        .filter(|run| !run.probe || is_baseline(&run.experiment_id))
        .collect();
    scored.sort_by_key(|run| (run.ended(), run.created_at));
    let mut history = Vec::new();
    let mut previous: Option<String> = None;
    for (index, run) in scored.iter().enumerate() {
        let scores = eligible(experiment_scores(
            scored[..=index].iter().copied(),
            is_baseline,
        ));
        let best = best_of(&scores, metric);
        let best_id = best.map(|best| best.experiment_id.clone());
        if run.counts() {
            history.push(HistoryPoint {
                run_id: run.run_id.clone(),
                ended_at: run.ended(),
                score: run.score.unwrap_or_default(),
                best_so_far: best.map_or(0.0, |best| best.score),
                new_best: best_id.as_deref() == Some(run.experiment_id.as_str())
                    && previous.as_deref() != Some(run.experiment_id.as_str()),
            });
        }
        previous = best_id;
    }
    let scores = eligible(experiment_scores(scored.iter().copied(), is_baseline));
    Standing {
        baseline: None,
        best: best_of(&scores, metric).cloned(),
        history,
        breaches: Vec::new(),
    }
}

/// One experiment on the path from the baseline to the best run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PathStep {
    pub experiment_id: String,
    pub experiment: String,
    /// The title of the idea the experiment tested, when one is linked.
    pub idea: Option<String>,
    /// The latest counted run behind the step's score.
    pub run_id: Option<String>,
    pub score: Option<f64>,
    /// How many counted runs the score averages.
    pub runs: usize,
    /// How many probe runs the score averages, when they score it.
    pub probe_runs: usize,
    /// Change from the previous scored step, in the metric's raw units.
    pub delta: Option<f64>,
}

/// The lineage from the root of the best experiment down to it. The last
/// step is the best itself and the first is the baseline, so the deltas
/// between scored steps add up to the gain the headline reports.
fn lineage(
    experiments: &HashMap<String, LocalExperiment>,
    best: &ExperimentScore,
    runs: &[ScoredRun],
) -> Vec<PathStep> {
    let chain = ancestry(&best.experiment_id, |id| {
        experiments
            .get(id)
            .and_then(|experiment| experiment.parent_experiment_id.clone())
    });
    let name = |id: &str| {
        experiments
            .get(id)
            .map(display_name)
            .unwrap_or_else(|| id.to_string())
    };
    path_steps(&chain, name, best, runs)
}

/// An experiment and its ancestors, root first.
pub(crate) fn ancestry(
    experiment_id: &str,
    parent: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut chain = Vec::new();
    let mut cursor = Some(experiment_id.to_string());
    while let Some(id) = cursor {
        if chain.contains(&id) || chain.len() > 10_000 {
            break;
        }
        cursor = parent(&id);
        chain.push(id);
    }
    chain.reverse();
    chain
}

/// The scored steps along `chain` (root first), ending at the best.
pub(crate) fn path_steps(
    chain: &[String],
    name: impl Fn(&str) -> String,
    best: &ExperimentScore,
    runs: &[ScoredRun],
) -> Vec<PathStep> {
    let mut steps = Vec::with_capacity(chain.len());
    let mut previous: Option<f64> = None;
    for id in chain {
        // The best is scored as the headline scores it.
        let (run_id, score, counted, probe_runs) = if *id == best.experiment_id {
            if best.probe {
                (None, Some(best.score), 0, best.runs)
            } else {
                (Some(best.run_id.clone()), Some(best.score), best.runs, 0)
            }
        } else {
            match experiment_reference(runs, id, true) {
                Some(reference) => (
                    reference.run_id,
                    Some(reference.score),
                    reference.runs,
                    reference.probe_runs,
                ),
                None => (None, None, 0, 0),
            }
        };
        let delta = match (score, previous) {
            (Some(score), Some(previous)) => Some(score - previous),
            _ => None,
        };
        if score.is_some() {
            previous = score;
        }
        steps.push(PathStep {
            experiment: name(id),
            idea: None,
            run_id,
            score,
            runs: counted,
            probe_runs,
            experiment_id: id.clone(),
            delta,
        });
    }
    steps
}

/// An optimization's standing on its primary metric: the running best among
/// its own runs, the baseline it is measured against, and the path between
/// them. Returns the optimization's runs, scored; the baseline and earlier
/// steps on the path are scored from any of the project's runs.
pub(crate) fn standing(
    store: &Store,
    brief: &super::scope::Brief,
    metric: &PrimaryMetric,
) -> Result<(Standing, Vec<PathStep>, Vec<ScoredRun>)> {
    let members = super::scope::run_ids(store, brief)?;
    let guards = guards(store, &brief.id)?;
    standing_with(store, &brief.project_id, metric, Some(&members), &guards)
}

/// The same standing over the project's runs: all of them, or `members`,
/// with no guards.
pub(crate) fn standing_among(
    store: &Store,
    project_id: &str,
    metric: &PrimaryMetric,
    members: Option<&std::collections::HashSet<String>>,
) -> Result<(Standing, Vec<PathStep>, Vec<ScoredRun>)> {
    standing_with(store, project_id, metric, members, &[])
}

fn standing_with(
    store: &Store,
    project_id: &str,
    metric: &PrimaryMetric,
    members: Option<&std::collections::HashSet<String>>,
    guards: &[Guard],
) -> Result<(Standing, Vec<PathStep>, Vec<ScoredRun>)> {
    let all = scored_runs(store, project_id, &metric.key, metric.split.as_deref())?;
    let runs: Vec<ScoredRun> = all
        .iter()
        .filter(|run| members.is_none_or(|members| members.contains(&run.run_id)))
        .cloned()
        .collect();
    let experiments: HashMap<String, LocalExperiment> = store
        .list_experiments_by_project(project_id)?
        .into_iter()
        .map(|experiment| (experiment.id.clone(), experiment))
        .collect();
    let is_baseline = |id: &str| {
        experiments
            .get(id)
            .is_some_and(|experiment| experiment.parent_experiment_id.is_none())
    };
    // The baselines the runs build on take part too, scored by any of the
    // project's runs, as the path scores them: seed runs launched from a
    // terminal, say, before the optimization had runs of its own.
    let parent = |id: &str| {
        experiments
            .get(id)
            .and_then(|experiment| experiment.parent_experiment_id.clone())
    };
    let roots: std::collections::HashSet<String> = runs
        .iter()
        .filter_map(|run| ancestry(&run.experiment_id, parent).into_iter().next())
        .collect();
    let mut candidates = runs.clone();
    candidates.extend(
        all.iter()
            .filter(|run| roots.contains(&run.experiment_id))
            .filter(|run| members.is_some_and(|members| !members.contains(&run.run_id)))
            .cloned(),
    );
    // Guards hold attempts, not the baselines they are measured against.
    let mut attempts: Vec<&str> = candidates
        .iter()
        .map(|run| run.experiment_id.as_str())
        .filter(|id| !is_baseline(id))
        .collect();
    attempts.sort_unstable();
    attempts.dedup();
    let breaches = if guards.is_empty() {
        HashMap::new()
    } else {
        guard_breaches(store, project_id, guards, attempts)?
    };
    let set_aside = breaches.keys().cloned().collect();
    let mut standing = running_best(&candidates, metric, &is_baseline, &set_aside);
    let mut breaches: Vec<Breach> = breaches
        .into_iter()
        .map(|(experiment_id, reason)| Breach {
            experiment: experiments
                .get(&experiment_id)
                .map(display_name)
                .unwrap_or_else(|| experiment_id.clone()),
            experiment_id,
            reason,
        })
        .collect();
    breaches.sort_by(|a, b| a.experiment_id.cmp(&b.experiment_id));
    standing.breaches = breaches;
    let mut path = match &standing.best {
        Some(best) => lineage(&experiments, best, &all),
        None => Vec::new(),
    };
    let ideas = super::ideas::titles_by_experiment(store, project_id)?;
    for step in &mut path {
        step.idea = ideas.get(&step.experiment_id).cloned();
    }
    if let Some(root) = path.first() {
        standing.baseline = root.score.map(|score| Reference {
            experiment_id: root.experiment_id.clone(),
            experiment: root.experiment.clone(),
            run_id: root.run_id.clone(),
            score,
            runs: root.runs,
            probe_runs: root.probe_runs,
        });
    }
    Ok((standing, path, runs))
}

/// Compact display for a metric value: up to six significant digits.
pub(crate) fn format_value(value: Option<f64>) -> String {
    let Some(value) = value else {
        return "nan".to_string();
    };
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs();
    if !(1e-4..1e6).contains(&magnitude) {
        return format!("{value:.4e}");
    }
    let digits = (5 - magnitude.log10().floor() as i32).clamp(0, 9) as usize;
    let text = format!("{value:.digits$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoredRun;

    fn point(key: &str, value: Option<f64>, step: Option<i64>) -> MetricPoint {
        MetricPoint {
            key: key.into(),
            value,
            step,
            split: None,
        }
    }

    #[test]
    fn single_and_multi_metric_lines_parse() {
        assert_eq!(
            parse_line(r#"ORX_METRIC {"key": "val_ic", "value": 0.031, "step": 1200}"#),
            [point("val_ic", Some(0.031), Some(1200))]
        );
        assert_eq!(
            parse_line(
                r#"2026-10-15 12:00:01 INFO ORX_METRIC {"step": 3.0, "val_ic": 0.03, "train_loss": 0.4, "note": "x"}"#
            ),
            [
                point("val_ic", Some(0.03), Some(3)),
                point("train_loss", Some(0.4), Some(3)),
            ]
        );
        let split = parse_line(r#"ORX_METRIC: {"key": "ic", "value": 0.02, "split": "val"}"#);
        assert_eq!(split[0].split.as_deref(), Some("val"));
    }

    #[test]
    fn python_nan_and_infinity_are_kept_as_missing_values() {
        assert_eq!(
            parse_line(
                r#"ORX_METRIC {"loss": NaN, "grad_norm": Infinity, "lr": -Infinity, "tag": "NaN"}"#
            ),
            [
                point("loss", None, None),
                point("grad_norm", None, None),
                point("lr", None, None),
            ]
        );
    }

    #[test]
    fn printed_nan_and_inf_spellings_are_missing_values_too() {
        assert_eq!(
            parse_line(r#"ORX_METRIC {"loss": nan, "ic": -inf, "note": "inference", "acc": 0.5}"#),
            [
                point("loss", None, None),
                point("ic", None, None),
                point("acc", Some(0.5), None),
            ]
        );
        assert!(has_non_finite(
            r#"ORX_METRIC {"sharpe_ratio_annualized_net": NaN}"#
        ));
        assert!(!has_non_finite(r#"ORX_METRIC {"tag": "NaN", "info": 1}"#));
        assert!(!has_non_finite("loss nan, but no marker"));
    }

    #[test]
    fn other_lines_and_broken_records_are_ignored() {
        for line in [
            "epoch 3 loss 0.4",
            "ORX_METRIC",
            "ORX_METRIC {not json}",
            "ORX_METRIC [1, 2]",
            r#"ORX_METRIC {"key": "", "value": 1}"#,
            r#"ORX_METRIC {"key": "acc", "value": "high"}"#,
            r#"see ORX_METRIC_DOCS {"acc": 1}"#,
        ] {
            assert!(parse_line(line).is_empty(), "{line}");
        }
        // Two records on one line (a progress bar's carriage return in between).
        assert_eq!(
            parse_line("ORX_METRIC {\"a\": 1}\rORX_METRIC {\"b\": 2} trailing").len(),
            2
        );
    }

    fn run(id: &str, experiment: &str, status: &str, ended: Option<i64>) -> StoredRun {
        StoredRun {
            id: id.into(),
            experiment_id: experiment.into(),
            project_id: "proj_1".into(),
            status: status.into(),
            backend_json: "{}".into(),
            command: "python train.py".into(),
            created_at: ended.unwrap_or(1) - 1,
            updated_at: ended.unwrap_or(1),
            ended_at: ended,
            exit_code: None,
            commit_sha: None,
            result_markdown: None,
            cancel_requested: false,
            cancel_reason: None,
            chat_session_id: None,
        }
    }

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("vlr-metrics-{}", uuid::Uuid::new_v4()));
        (Store::open_at(dir.clone()).unwrap(), dir)
    }

    #[test]
    fn ingestion_reads_only_new_complete_lines() {
        let (store, dir) = temp_store();
        let live = run("run_live", "exp_1", "running", None);
        let log = super::super::wake::run_log_path(&store, "run_live");
        std::fs::write(
            &log,
            "setup\nORX_METRIC {\"step\": 1, \"val_ic\": 0.01}\nORX_METRIC {\"step\": 2, \"val",
        )
        .unwrap();
        assert_eq!(ingest(&store, &live).unwrap(), 1);
        assert_eq!(ingest(&store, &live).unwrap(), 0);

        let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        std::io::Write::write_all(
            &mut file,
            b"_ic\": 0.02}\nORX_METRIC {\"step\": 3, \"val_ic\": 0.025}",
        )
        .unwrap();
        assert_eq!(ingest(&store, &live).unwrap(), 1);
        // Once the run is over, its unterminated last line counts too.
        let finished = run("run_live", "exp_1", "done", Some(10));
        assert_eq!(ingest(&store, &finished).unwrap(), 1);

        let summary = run_summary(&store, "run_live").unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].points, 3);
        assert_eq!(summary[0].last, Some(0.025));
        assert_eq!(summary[0].last_step, Some(3));
        assert_eq!(
            final_value(&store, "run_live", "val_ic", None).unwrap(),
            Some(0.025)
        );

        // A huge redraw-only "line" is skipped, and the record after it read.
        let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        let bar = vec![b'#'; (2 * MAX_LINE_BYTES + 10) as usize];
        std::io::Write::write_all(&mut file, &bar).unwrap();
        std::io::Write::write_all(&mut file, b"\nORX_METRIC {\"val_ic\": 0.03}\n").unwrap();
        assert_eq!(ingest(&store, &finished).unwrap(), 1);
        assert_eq!(
            final_value(&store, "run_live", "val_ic", None).unwrap(),
            Some(0.03)
        );

        // A replaced log starts over rather than double counting.
        std::fs::write(&log, "ORX_METRIC {\"val_ic\": 0.5}\n").unwrap();
        assert_eq!(ingest(&store, &finished).unwrap(), 1);
        assert_eq!(run_summary(&store, "run_live").unwrap()[0].points, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scored(id: &str, experiment: &str, ended: i64, score: f64) -> ScoredRun {
        ScoredRun {
            run_id: id.into(),
            experiment_id: experiment.into(),
            experiment: experiment.into(),
            status: "done".into(),
            created_at: ended - 1,
            ended_at: Some(ended),
            score: Some(score),
            probe: false,
        }
    }

    #[test]
    fn the_running_best_respects_direction_and_the_noise_floor() {
        let runs = [
            scored("a", "base", 1, 0.020),
            scored("b", "e1", 2, 0.0205), // within the floor: not a new best
            scored("c", "e2", 3, 0.030),
            scored("d", "e3", 4, 0.010),
        ];
        let metric = PrimaryMetric {
            key: "val_ic".into(),
            direction: Direction::Max,
            noise_floor: Some(0.001),
            split: None,
        };
        let standing = running_best(&runs, &metric, &|_| false, &Default::default());
        assert_eq!(standing.best.unwrap().run_id, "c");
        let marks: Vec<_> = standing.history.iter().map(|h| h.new_best).collect();
        assert_eq!(marks, [true, false, true, false]);
        assert_eq!(standing.history[3].best_so_far, 0.030);

        let lower = PrimaryMetric {
            direction: Direction::Min,
            noise_floor: None,
            ..metric
        };
        assert_eq!(
            running_best(&runs, &lower, &|_| false, &Default::default())
                .best
                .unwrap()
                .run_id,
            "d"
        );
    }

    fn probe(id: &str, experiment: &str, ended: i64, score: f64) -> ScoredRun {
        ScoredRun {
            probe: true,
            ..scored(id, experiment, ended, score)
        }
    }

    #[test]
    fn an_attempt_has_to_beat_a_baseline_measured_with_seed_runs() {
        let metric = PrimaryMetric {
            key: "val_ic".into(),
            direction: Direction::Max,
            noise_floor: Some(0.002),
            split: None,
        };
        let is_baseline = |id: &str| id == "base";
        let mut runs = vec![
            probe("s1", "base", 1, 0.0200),
            probe("s2", "base", 2, 0.0210),
            probe("s3", "base", 3, 0.0190),
            // Worse than the baseline: it doesn't become the best.
            scored("a", "e1", 4, 0.0180),
        ];
        let standing = running_best(&runs, &metric, &is_baseline, &Default::default());
        let best = standing.best.unwrap();
        assert_eq!((best.experiment_id.as_str(), best.probe), ("base", true));
        assert!((best.score - 0.020).abs() < 1e-12);
        assert_eq!(standing.history.len(), 1, "probe runs aren't results");
        assert!(!standing.history[0].new_best);

        // A lucky run takes over; its replicate with a new seed pulls the
        // experiment's mean back under the floor, and the baseline is the
        // best again.
        runs.push(scored("b", "e2", 5, 0.0260));
        let lucky = running_best(&runs, &metric, &is_baseline, &Default::default());
        assert_eq!(lucky.best.unwrap().experiment_id, "e2");
        assert!(lucky.history.last().unwrap().new_best);
        runs.push(scored("b2", "e2", 6, 0.0180));
        let replicated = running_best(&runs, &metric, &is_baseline, &Default::default());
        let best = replicated.best.unwrap();
        assert_eq!(best.experiment_id, "base");
        let point = replicated.history.last().unwrap();
        assert!(!point.new_best);
        assert!((point.best_so_far - 0.020).abs() < 1e-12);
        let e2 = experiment_reference(&runs, "e2", false).unwrap();
        assert_eq!((e2.runs, e2.run_id.as_deref()), (2, Some("b2")));
        assert!((e2.score - 0.022).abs() < 1e-12);
    }

    #[test]
    fn the_lineage_path_splits_the_total_gain_into_steps() {
        let mut experiments = HashMap::new();
        for (id, parent) in [("base", None), ("e1", Some("base")), ("e2", Some("e1"))] {
            experiments.insert(
                id.to_string(),
                LocalExperiment {
                    id: id.into(),
                    project_id: "proj_1".into(),
                    parent_experiment_id: parent.map(Into::into),
                    slug: id.into(),
                    branch_name: format!("orx/{id}"),
                    title: Some(format!("Step {id}")),
                    description: None,
                    run_command: "python train.py".into(),
                    agent_status: "idle".into(),
                    created_at: 1,
                    updated_at: 1,
                    chat_session_id: None,
                    archived: false,
                },
            );
        }
        let runs = [
            scored("r0", "base", 1, 0.020),
            scored("r1", "e1", 2, 0.026),
            scored("r1b", "e1", 3, 0.024),
            scored("r2", "e2", 4, 0.031),
        ];
        let scores = experiment_scores(&runs, &|_| false);
        let best = scores.last().unwrap();
        assert_eq!(best.experiment_id, "e2");
        let path = lineage(&experiments, best, &runs);
        let ids: Vec<_> = path
            .iter()
            .map(|step| step.experiment_id.as_str())
            .collect();
        assert_eq!(ids, ["base", "e1", "e2"]);
        assert_eq!(path[0].experiment, "Step base");
        // A step run twice is scored by the mean of its runs.
        assert_eq!(path[1].run_id.as_deref(), Some("r1b"));
        assert_eq!(path[1].runs, 2);
        assert!((path[1].score.unwrap() - 0.025).abs() < 1e-12);
        let total: f64 = path.iter().filter_map(|step| step.delta).sum();
        assert!((total - (0.031 - 0.020)).abs() < 1e-12);
    }

    #[test]
    fn values_print_compactly() {
        assert_eq!(format_value(Some(0.031)), "0.031");
        assert_eq!(format_value(Some(1234.5678)), "1234.57");
        assert_eq!(format_value(Some(-0.5)), "-0.5");
        assert_eq!(format_value(Some(2.0)), "2");
        assert_eq!(format_value(Some(0.000012)), "1.2000e-5");
        assert_eq!(format_value(None), "nan");
    }

    #[test]
    fn each_optimization_measures_its_best_over_its_own_runs() {
        let (store, dir) = temp_store();
        let ic = crate::flavor::scope::create(&store, "p", Some("chat_ic")).unwrap();
        let speed = crate::flavor::scope::create(&store, "p", Some("chat_speed")).unwrap();
        for (run_id, experiment, value, chat) in [
            ("r_base", "base", 0.020, None),
            ("r_a1", "a1", 0.030, Some(&ic.id)),
            ("r_a2", "a2", 0.026, Some(&ic.id)),
            ("r_b1", "b1", 0.025, Some(&speed.id)),
        ] {
            store
                .create_local_experiment(&LocalExperiment {
                    id: experiment.into(),
                    project_id: "p".into(),
                    parent_experiment_id: (experiment != "base").then(|| "base".to_string()),
                    slug: experiment.into(),
                    branch_name: format!("orx/{experiment}"),
                    title: None,
                    description: None,
                    run_command: "python train.py".into(),
                    agent_status: "idle".into(),
                    created_at: 1,
                    updated_at: 1,
                    chat_session_id: None,
                    archived: false,
                })
                .unwrap();
            store
                .upsert_run(&StoredRun {
                    id: run_id.into(),
                    experiment_id: experiment.into(),
                    project_id: "p".into(),
                    status: "done".into(),
                    backend_json: "{}".into(),
                    command: "python train.py".into(),
                    created_at: crate::store::now_ms(),
                    updated_at: crate::store::now_ms(),
                    ended_at: Some(crate::store::now_ms()),
                    exit_code: Some(0),
                    commit_sha: None,
                    result_markdown: None,
                    cancel_requested: false,
                    cancel_reason: None,
                    chat_session_id: None,
                })
                .unwrap();
            let log = super::super::wake::run_log_path(&store, run_id);
            std::fs::write(log, format!("ORX_METRIC {{\"val_ic\": {value}}}\n")).unwrap();
            if let Some(optimization_id) = chat {
                crate::flavor::scope::after_launch(
                    &store,
                    run_id,
                    &crate::flavor::scope::Launch {
                        optimization_id: Some(optimization_id.clone()),
                        ..Default::default()
                    },
                );
            }
        }
        let metric = PrimaryMetric {
            key: "val_ic".into(),
            direction: Direction::Max,
            noise_floor: None,
            split: None,
        };
        for brief in [&ic, &speed] {
            set_primary(&store, &brief.id, &metric).unwrap();
        }
        let best = |brief| {
            let (standing, path, runs) = standing(&store, brief, &metric).unwrap();
            let best = standing.best.unwrap().run_id;
            // The baseline is scored from the project's runs either way.
            assert_eq!(path.first().unwrap().experiment_id, "base");
            assert_eq!(standing.baseline.unwrap().score, 0.020);
            (best, runs.len())
        };
        assert_eq!(best(&ic), ("r_a1".to_string(), 2));
        assert_eq!(best(&speed), ("r_b1".to_string(), 1));
        let (whole, _, _) = standing_among(&store, "p", &metric, None).unwrap();
        assert_eq!(whole.best.unwrap().run_id, "r_a1");
        // Each budget counts its own runs.
        let confirmed = |brief: &crate::flavor::scope::Brief| crate::flavor::scope::Brief {
            budget_since: Some(0),
            ..brief.clone()
        };
        assert_eq!(
            crate::flavor::scope::usage(&store, &confirmed(&ic))
                .unwrap()
                .runs,
            2
        );
        assert_eq!(
            crate::flavor::scope::usage(&store, &confirmed(&speed))
                .unwrap()
                .runs,
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guards_say_why_a_value_breaks_them() {
        let diff = Guard {
            key: "output_diff".into(),
            split: None,
            max: Some(0.0),
            min: None,
        };
        assert_eq!(diff.breach(Some(0.0)), None);
        assert_eq!(
            diff.breach(Some(0.03)).as_deref(),
            Some("output_diff is 0.03, above 0")
        );
        assert_eq!(
            diff.breach(None).as_deref(),
            Some("it didn't report output_diff")
        );
        assert_eq!(diff.describe(), "output_diff at most 0");
        let band = Guard {
            key: "turnover".into(),
            split: Some("val".into()),
            max: Some(2.0),
            min: Some(0.5),
        };
        assert_eq!(
            band.breach(Some(0.25)).as_deref(),
            Some("turnover is 0.25, below 0.5")
        );
        assert_eq!(band.describe(), "turnover (val) between 0.5 and 2");
    }

    #[test]
    fn a_guard_sets_aside_an_attempt_that_breaks_it_but_never_the_baseline() {
        let (store, dir) = temp_store();
        let opt = crate::flavor::scope::tests::named(&store, "opt_fast");
        for (id, parent) in [
            ("base", None),
            ("fast", Some("base")),
            ("faster", Some("base")),
        ] {
            store
                .create_local_experiment(&LocalExperiment {
                    id: id.into(),
                    project_id: "p".into(),
                    parent_experiment_id: parent.map(Into::into),
                    slug: id.into(),
                    branch_name: format!("orx/{id}"),
                    title: None,
                    description: None,
                    run_command: "python train.py".into(),
                    agent_status: "idle".into(),
                    created_at: 1,
                    updated_at: 1,
                    chat_session_id: None,
                    archived: false,
                })
                .unwrap();
        }
        // Lower seconds is better. "faster" is quickest, but its output moved;
        // the baseline reports no output_diff and is exempt.
        for (run, experiment, report, ended) in [
            ("r_base", "base", r#"{"seconds": 100}"#, 1),
            ("r_fast", "fast", r#"{"seconds": 80, "output_diff": 0}"#, 2),
            (
                "r_faster",
                "faster",
                r#"{"seconds": 50, "output_diff": 0.2}"#,
                3,
            ),
        ] {
            store
                .upsert_run(&StoredRun {
                    id: run.into(),
                    experiment_id: experiment.into(),
                    project_id: "p".into(),
                    status: "done".into(),
                    backend_json: "{}".into(),
                    command: "python train.py".into(),
                    created_at: ended - 1,
                    updated_at: ended,
                    ended_at: Some(ended),
                    exit_code: Some(0),
                    commit_sha: None,
                    result_markdown: None,
                    cancel_requested: false,
                    cancel_reason: None,
                    chat_session_id: None,
                })
                .unwrap();
            let log = super::super::wake::run_log_path(&store, run);
            std::fs::write(log, format!("ORX_METRIC {report}\n")).unwrap();
        }
        crate::flavor::scope::tests::adopt_runs(&store, "opt_fast");
        let metric = PrimaryMetric {
            key: "seconds".into(),
            direction: Direction::Min,
            noise_floor: None,
            split: None,
        };
        set_primary(&store, &opt.id, &metric).unwrap();
        let (ranked, _, _) = standing(&store, &opt, &metric).unwrap();
        assert_eq!(ranked.best.unwrap().experiment_id, "faster");
        assert!(ranked.breaches.is_empty());

        let guard = Guard {
            key: "output_diff".into(),
            split: None,
            max: Some(0.0),
            min: None,
        };
        set_guard(&store, &opt.id, &guard).unwrap();
        assert_eq!(
            guards(&store, &opt.id).unwrap(),
            std::slice::from_ref(&guard)
        );
        let (ranked, _, _) = standing(&store, &opt, &metric).unwrap();
        assert_eq!(ranked.best.unwrap().experiment_id, "fast");
        assert_eq!(ranked.breaches.len(), 1);
        assert_eq!(ranked.breaches[0].experiment_id, "faster");
        assert_eq!(ranked.breaches[0].reason, "output_diff is 0.2, above 0");

        // Bad guards are refused; removing one restores the plain ranking.
        let unbounded = Guard {
            max: None,
            ..guard.clone()
        };
        assert!(set_guard(&store, &opt.id, &unbounded).is_err());
        let crossed = Guard {
            max: Some(0.0),
            min: Some(1.0),
            ..guard.clone()
        };
        assert!(set_guard(&store, &opt.id, &crossed).is_err());
        assert!(set_guard(&store, "opt_missing", &guard).is_err());
        assert!(remove_guard(&store, &opt.id, "output_diff").unwrap());
        assert!(!remove_guard(&store, &opt.id, "output_diff").unwrap());
        let (ranked, _, _) = standing(&store, &opt, &metric).unwrap();
        assert_eq!(ranked.best.unwrap().experiment_id, "faster");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn primary_metrics_round_trip_and_reject_bad_floors() {
        let (store, dir) = temp_store();
        let opt = crate::flavor::scope::tests::named(&store, "opt_1");
        assert_eq!(primary(&store, &opt.id).unwrap(), None);
        let metric = PrimaryMetric {
            key: "val_ic".into(),
            direction: Direction::Max,
            noise_floor: Some(0.002),
            split: Some("val".into()),
        };
        set_primary(&store, &opt.id, &metric).unwrap();
        assert_eq!(primary(&store, &opt.id).unwrap(), Some(metric.clone()));
        let negative = PrimaryMetric {
            noise_floor: Some(-1.0),
            ..metric.clone()
        };
        assert!(set_primary(&store, &opt.id, &negative).is_err());
        // Each optimization keeps its own; a missing one has none to set.
        let other = crate::flavor::scope::tests::named(&store, "opt_2");
        assert_eq!(primary(&store, &other.id).unwrap(), None);
        assert!(set_primary(&store, "opt_missing", &metric).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
