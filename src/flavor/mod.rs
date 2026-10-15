//! VLResearch additions to OpenResearch.
//!
//! Everything specific to this distribution lives under `src/flavor/`.
//! Upstream files call into it from a few one-line hooks (each marked
//! `VLResearch:`), so merging new alphaXiv/OpenResearch releases stays a
//! matter of keeping those hooks in place. See `VLRESEARCH.md`.

pub(crate) mod activity;
pub(crate) mod api;
pub(crate) mod brief_cli;
pub(crate) mod cli;
pub(crate) mod egress;
pub(crate) mod finding_cli;
pub(crate) mod findings;
pub(crate) mod harness;
pub(crate) mod idea_cli;
pub(crate) mod ideas;
pub(crate) mod metrics;
pub(crate) mod scope;
pub(crate) mod skills;
pub(crate) mod stall;
pub(crate) mod wake;

/// Environment VLResearch adds to every coding-agent process: the
/// no-telemetry settings and the checklist tool.
pub(crate) fn prepare_agent_env(cmd: &mut tokio::process::Command) {
    egress::apply_agent_env_defaults(cmd);
    harness::apply_agent_env_defaults(cmd);
}

/// Sets each variable on `cmd` unless it already has a value in orx's
/// environment, the user's shell, or on the command itself.
pub(crate) fn set_env_defaults(cmd: &mut tokio::process::Command, defaults: &[(&str, &str)]) {
    for (key, value) in defaults {
        let on_command = cmd
            .as_std()
            .get_envs()
            .any(|(name, _)| name == std::ffi::OsStr::new(key));
        if !on_command && crate::local::shell_env::var(key).is_none() {
            cmd.env(key, value);
        }
    }
}

/// Reads a boolean switch from the environment: `1`, `true`, `yes` or `on`
/// (any case) turn it on; anything else, or unset, leaves it off.
pub(crate) fn env_flag(key: &str) -> bool {
    std::env::var(key).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::env_flag;

    #[test]
    fn env_flag_accepts_common_truthy_spellings_only() {
        let key = "VLR_TEST_ENV_FLAG_PARSING";
        for (value, expected) in [
            ("1", true),
            ("true", true),
            ("YES", true),
            (" on ", true),
            ("0", false),
            ("false", false),
            ("", false),
            ("enabled", false),
        ] {
            std::env::set_var(key, value);
            assert_eq!(env_flag(key), expected, "value {value:?}");
        }
        std::env::remove_var(key);
        assert!(!env_flag(key));
    }
}
