//! VLResearch additions to OpenResearch.
//!
//! Everything specific to this distribution lives under `src/flavor/`.
//! Upstream files call into it from a few one-line hooks (each marked
//! `VLResearch:`), so merging new alphaXiv/OpenResearch releases stays a
//! matter of keeping those hooks in place. See `VLRESEARCH.md`.

pub(crate) mod egress;
pub(crate) mod harness;
pub(crate) mod wake;

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
