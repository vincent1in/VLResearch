//! How the research loop behaves on this machine, set on the dashboard's
//! Settings page (its Research section) instead of in environment variables.
//!
//! Each setting keeps its variable's name and syntax. A variable set in the
//! environment orx starts in, or on the Environment page (which hands it to
//! agents and compute), still wins, so a launch script can pin a setting; the
//! page shows which ones are pinned that way. Otherwise the saved value
//! applies, and with neither, the default.
//!
//! The network policy (`egress`) is shown on the page but changes only in
//! the environment orx starts in: letting orx reach more of the internet
//! should take more than a click, or a request from a coding agent.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

use super::{compact, egress, harness, ideas, scope, stall, wake};
use crate::error::{anyhow, Result};
use crate::store::Store;

/// Where the value in effect comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Source {
    Environment,
    Saved,
    Default,
}

/// A setting the page can change.
struct Setting {
    key: &'static str,
    /// The value as the setting's reader takes it (`None`: unset), in the
    /// form the page shows and saves.
    reads_as: fn(Option<&str>) -> String,
    /// A value from the page, checked and put in that form.
    accepts: fn(&str) -> Result<String>,
}

const SETTINGS: &[Setting] = &[
    Setting {
        key: compact::AUTO_COMPACT_ENV,
        reads_as: compaction,
        accepts: accept_compaction,
    },
    Setting {
        key: ideas::AUTO_ABLATE_ENV,
        reads_as: switch,
        accepts: accept_switch,
    },
    Setting {
        key: stall::STALL_MINUTES_ENV,
        reads_as: stall_window,
        accepts: accept_minutes,
    },
    Setting {
        key: wake::AUTO_WAKE_ENV,
        reads_as: switch,
        accepts: accept_switch,
    },
    Setting {
        key: scope::GATE_ENV,
        reads_as: gate,
        accepts: accept_gate,
    },
    Setting {
        key: harness::CHECKLIST_ENV,
        reads_as: switch,
        accepts: accept_switch,
    },
];

/// The variables of the network policy, shown on the page but not changed
/// there.
const NETWORK: &[&str] = &[
    egress::ALLOW_ENV,
    egress::GITHUB_ENV,
    egress::INSTALLERS_ENV,
    egress::GIT_PROTOCOLS_ENV,
    egress::AGENT_DEFAULTS_ENV,
];

/// The share of the context window the page lets autocompaction wait for.
const COMPACT_PERCENT: std::ops::RangeInclusive<f64> = 20.0..=95.0;
/// The longest stall window the page takes, a day.
const MAX_STALL_MINUTES: i64 = 24 * 60;

fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vlr_settings (
             key        TEXT PRIMARY KEY,
             value      TEXT NOT NULL,
             updated_at INTEGER NOT NULL
         );",
    )?;
    Ok(())
}

/// The value in effect for `key`: the environment's, else the one saved on
/// the Settings page. `None` leaves the reader's default.
pub(crate) fn value(store: &Store, key: &str) -> Option<String> {
    environment(key).or_else(|| saved(store.flavor_conn(), key))
}

/// [`value`] for code with no store at hand. It reads the saved value
/// without creating anything, so where there is no store yet the default
/// applies.
pub(crate) fn current(key: &str) -> Option<String> {
    environment(key).or_else(|| {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn =
            Connection::open_with_flags(crate::store::data_dir().join("orx.db"), flags).ok()?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).ok()?;
        conn.query_row(
            "SELECT value FROM vlr_settings WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
    })
}

/// Whether a switch is on: it is unless set to `0`, `off`, `false` or `no`.
pub(crate) fn switch_on(value: Option<&str>) -> bool {
    !value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        )
    })
}

/// A value given in orx's environment, or on the Environment page, whose
/// file reaches agents' commands as environment too.
fn environment(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| crate::config::synced_env_var(key))
}

/// The value saved on the page. A store that can't be read leaves the
/// default, which is never looser than a saved value would be.
fn saved(conn: &Connection, key: &str) -> Option<String> {
    let read = ensure_tables(conn).and_then(|()| {
        conn.query_row(
            "SELECT value FROM vlr_settings WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
    });
    read.unwrap_or_else(|error| {
        eprintln!("vlresearch: could not read the saved setting {key}: {error}");
        None
    })
}

/// Saves values from the page; `None` returns a setting to its default. All
/// or none: one value it can't take refuses the lot.
pub(crate) fn save(store: &Store, changes: &BTreeMap<String, Option<String>>) -> Result<()> {
    let mut checked = Vec::with_capacity(changes.len());
    for (key, value) in changes {
        let Some(setting) = SETTINGS.iter().find(|setting| setting.key == key) else {
            return Err(if NETWORK.contains(&key.as_str()) {
                anyhow!(
                    "{key} is part of the network policy, which changes only in the environment \
                     orx starts in."
                )
            } else {
                anyhow!("There is no setting called {key}.")
            });
        };
        let value = value.as_deref().map(setting.accepts).transpose()?;
        checked.push((setting.key, value));
    }
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let tx = conn.unchecked_transaction()?;
    let now = crate::store::now_ms();
    for (key, value) in checked {
        match value {
            Some(value) => tx.execute(
                "INSERT INTO vlr_settings (key, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                                updated_at = excluded.updated_at",
                params![key, value, now],
            )?,
            None => tx.execute("DELETE FROM vlr_settings WHERE key = ?1", [key])?,
        };
    }
    tx.commit()?;
    Ok(())
}

/// One setting as the page shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub key: &'static str,
    /// The value in effect.
    pub value: String,
    pub source: Source,
    /// The value given in the environment, as given.
    pub environment: Option<String>,
    /// The value saved on the page, which the environment may override.
    pub saved: Option<String>,
    pub default: String,
}

/// One part of the network policy, which the page shows but can't change.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Policy {
    pub key: &'static str,
    /// What orx goes by, in the variable's terms.
    pub value: String,
    /// Whether the environment sets it, rather than the default.
    pub set: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Report {
    pub settings: Vec<Entry>,
    pub network: Vec<Policy>,
}

/// Every setting with its value in effect and where that comes from, and
/// the network policy.
pub(crate) fn report(store: &Store) -> Result<Report> {
    let conn = store.flavor_conn();
    ensure_tables(conn)?;
    let mut stmt = conn.prepare("SELECT key, value FROM vlr_settings")?;
    let saved = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    let settings = SETTINGS
        .iter()
        .map(|setting| {
            let environment = environment(setting.key);
            let saved = saved.get(setting.key).cloned();
            let (value, source) = match (&environment, &saved) {
                (Some(value), _) => (Some(value), Source::Environment),
                (None, Some(value)) => (Some(value), Source::Saved),
                (None, None) => (None, Source::Default),
            };
            Entry {
                key: setting.key,
                value: (setting.reads_as)(value.map(String::as_str)),
                source,
                environment,
                saved,
                default: (setting.reads_as)(None),
            }
        })
        .collect();
    Ok(Report {
        settings,
        network: network(),
    })
}

/// The network policy as `egress` applies it.
fn network() -> Vec<Policy> {
    let env = |key: &str| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let on = |on: bool| if on { "on" } else { "off" }.to_string();
    let allow = env(egress::ALLOW_ENV);
    let protocols = env(egress::GIT_PROTOCOLS_ENV);
    let git_default = env("GIT_ALLOW_PROTOCOL");
    let agent_defaults = env(egress::AGENT_DEFAULTS_ENV);
    vec![
        Policy {
            key: egress::ALLOW_ENV,
            value: allow.clone().unwrap_or_default(),
            set: allow.is_some(),
        },
        Policy {
            key: egress::GITHUB_ENV,
            value: on(egress::github_allowed()),
            set: env(egress::GITHUB_ENV).is_some(),
        },
        Policy {
            key: egress::INSTALLERS_ENV,
            value: on(egress::installers_allowed()),
            set: env(egress::INSTALLERS_ENV).is_some(),
        },
        Policy {
            key: egress::GIT_PROTOCOLS_ENV,
            set: protocols.is_some() || git_default.is_some(),
            value: protocols
                .or(git_default)
                .unwrap_or_else(|| "file".to_string()),
        },
        Policy {
            key: egress::AGENT_DEFAULTS_ENV,
            value: on(agent_defaults.as_deref() != Some("0")),
            set: agent_defaults.is_some(),
        },
    ]
}

fn compaction(value: Option<&str>) -> String {
    match compact::parse_threshold(value) {
        Some(fraction) => format!("{}%", (fraction * 100.0).round()),
        None => "off".to_string(),
    }
}

fn accept_compaction(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_lowercase();
    if matches!(value.as_str(), "off" | "0" | "0%") {
        return Ok("off".to_string());
    }
    value
        .trim_end_matches('%')
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|percent| percent.fract() == 0.0 && COMPACT_PERCENT.contains(percent))
        .map(|percent| format!("{percent}%"))
        .ok_or_else(|| {
            anyhow!(
                "Compact chats at a whole percentage of the context window from 20% to 95%, or \
                 turn autocompaction off."
            )
        })
}

fn switch(value: Option<&str>) -> String {
    if switch_on(value) { "on" } else { "off" }.to_string()
}

fn accept_switch(value: &str) -> Result<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on" | "1" | "true" | "yes" => Ok("on".to_string()),
        "off" | "0" | "false" | "no" => Ok("off".to_string()),
        _ => Err(anyhow!("A switch is `on` or `off`.")),
    }
}

fn stall_window(value: Option<&str>) -> String {
    stall::parse_minutes(value).to_string()
}

fn accept_minutes(value: &str) -> Result<String> {
    value
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|minutes| (0..=MAX_STALL_MINUTES).contains(minutes))
        .map(|minutes| minutes.to_string())
        .ok_or_else(|| {
            anyhow!(
                "Give the stall window in whole minutes, up to {MAX_STALL_MINUTES}, or 0 to turn \
                 stall alerts off."
            )
        })
}

fn gate(value: Option<&str>) -> String {
    scope::parse_gate(value).as_str().to_string()
}

fn accept_gate(value: &str) -> Result<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "scoped" => Ok("scoped".to_string()),
        "strict" => Ok("strict".to_string()),
        "off" | "0" => Ok("off".to_string()),
        _ => Err(anyhow!("The brief gate is `scoped`, `strict` or `off`.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (std::path::PathBuf, Store) {
        let (store, dir) = scope::tests::temp_store();
        (dir, store)
    }

    fn change(pairs: &[(&str, Option<&str>)]) -> BTreeMap<String, Option<String>> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.map(str::to_string)))
            .collect()
    }

    fn entry(store: &Store, key: &str) -> Entry {
        report(store)
            .unwrap()
            .settings
            .into_iter()
            .find(|entry| entry.key == key)
            .unwrap()
    }

    #[test]
    fn a_saved_setting_applies_until_it_is_reset() {
        let (_dir, store) = temp_store();
        let key = stall::STALL_MINUTES_ENV;
        assert_eq!(value(&store, key), None);
        assert_eq!(stall::stall_minutes(&store), 30);
        let fresh = entry(&store, key);
        assert_eq!(
            (fresh.value.as_str(), fresh.source),
            ("30", Source::Default)
        );

        save(&store, &change(&[(key, Some(" 45 "))])).unwrap();
        assert_eq!(value(&store, key).as_deref(), Some("45"));
        assert_eq!(stall::stall_minutes(&store), 45);
        let saved = entry(&store, key);
        assert_eq!((saved.value.as_str(), saved.source), ("45", Source::Saved));
        assert_eq!(saved.default, "30");

        save(&store, &change(&[(key, None)])).unwrap();
        assert_eq!(value(&store, key), None);
        assert_eq!(entry(&store, key).source, Source::Default);
    }

    #[test]
    fn the_page_saves_each_setting_in_the_form_its_reader_takes() {
        let (_dir, store) = temp_store();
        save(
            &store,
            &change(&[
                (compact::AUTO_COMPACT_ENV, Some("70")),
                (ideas::AUTO_ABLATE_ENV, Some("off")),
                (wake::AUTO_WAKE_ENV, Some("0")),
                (scope::GATE_ENV, Some("Strict")),
                (harness::CHECKLIST_ENV, Some("on")),
            ]),
        )
        .unwrap();
        assert_eq!(compact::threshold(&store), Some(0.7));
        assert!(!switch_on(value(&store, ideas::AUTO_ABLATE_ENV).as_deref()));
        assert!(!switch_on(value(&store, wake::AUTO_WAKE_ENV).as_deref()));
        assert!(switch_on(value(&store, harness::CHECKLIST_ENV).as_deref()));
        assert_eq!(entry(&store, scope::GATE_ENV).value, "strict");
        assert_eq!(entry(&store, compact::AUTO_COMPACT_ENV).value, "70%");

        save(&store, &change(&[(compact::AUTO_COMPACT_ENV, Some("off"))])).unwrap();
        assert_eq!(compact::threshold(&store), None);
    }

    #[test]
    fn values_the_page_cannot_take_change_nothing() {
        let (_dir, store) = temp_store();
        for (key, bad) in [
            (compact::AUTO_COMPACT_ENV, "100%"),
            (compact::AUTO_COMPACT_ENV, "12.5"),
            (compact::AUTO_COMPACT_ENV, "on"),
            (stall::STALL_MINUTES_ENV, "-5"),
            (stall::STALL_MINUTES_ENV, "2000"),
            (wake::AUTO_WAKE_ENV, "maybe"),
            (scope::GATE_ENV, "loose"),
        ] {
            let error = save(
                &store,
                &change(&[(ideas::AUTO_ABLATE_ENV, Some("off")), (key, Some(bad))]),
            )
            .unwrap_err();
            assert!(!error.to_string().is_empty(), "{key}={bad}");
        }
        // The good value sent with each bad one was not saved either.
        assert_eq!(value(&store, ideas::AUTO_ABLATE_ENV), None);

        let network = save(&store, &change(&[(egress::GITHUB_ENV, Some("1"))])).unwrap_err();
        assert!(network.to_string().contains("network policy"));
        let unknown = save(&store, &change(&[("VLR_NOT_A_SETTING", Some("1"))])).unwrap_err();
        assert!(unknown.to_string().contains("no setting"));
    }

    #[test]
    fn the_environment_overrides_the_page() {
        let (_dir, store) = temp_store();
        // Only this test uses this setting's variable.
        let key = harness::CHECKLIST_ENV;
        save(&store, &change(&[(key, Some("on"))])).unwrap();
        std::env::set_var(key, "0");
        let pinned = entry(&store, key);
        let in_effect = value(&store, key);
        std::env::remove_var(key);
        assert_eq!(in_effect.as_deref(), Some("0"));
        assert_eq!(
            (pinned.value.as_str(), pinned.source),
            ("off", Source::Environment)
        );
        assert_eq!(pinned.environment.as_deref(), Some("0"));
        assert_eq!(pinned.saved.as_deref(), Some("on"));
        assert_eq!(value(&store, key).as_deref(), Some("on"));
    }

    #[test]
    fn values_read_the_way_their_variables_always_have() {
        assert_eq!(compaction(None), "80%");
        assert_eq!(compaction(Some("0.65")), "65%");
        assert_eq!(compaction(Some("0")), "off");
        assert_eq!(switch(None), "on");
        assert_eq!(switch(Some("0")), "off");
        assert_eq!(switch(Some("yes")), "on");
        assert_eq!(stall_window(Some("junk")), "30");
        assert_eq!(stall_window(Some("0")), "0");
        assert_eq!(gate(None), "scoped");
        assert_eq!(gate(Some("0")), "off");
        assert_eq!(gate(Some("STRICT")), "strict");
        for (raw, on) in [
            (None, true),
            (Some("1"), true),
            (Some(" off "), false),
            (Some("FALSE"), false),
            (Some("no"), false),
            (Some("0"), false),
        ] {
            assert_eq!(switch_on(raw), on, "{raw:?}");
        }
    }

    #[test]
    fn the_report_lists_every_setting_and_the_network_policy() {
        let (_dir, store) = temp_store();
        let report = report(&store).unwrap();
        let keys: Vec<_> = report.settings.iter().map(|entry| entry.key).collect();
        assert_eq!(
            keys,
            SETTINGS
                .iter()
                .map(|setting| setting.key)
                .collect::<Vec<_>>()
        );
        let network: Vec<_> = report.network.iter().map(|policy| policy.key).collect();
        assert_eq!(network, NETWORK);
    }
}
