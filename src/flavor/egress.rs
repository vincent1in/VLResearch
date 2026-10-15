//! Egress policy. orx never calls an LLM API itself (the coding agent does),
//! so in VLResearch orx may reach only loopback hosts and the hosts listed in
//! `VLR_EGRESS_ALLOW`. Everything else orx would send on its own (update
//! checks, accounts and managed compute, literature APIs, GitHub, Overleaf,
//! agent installers) is refused here.
//!
//! What this module cannot police is traffic from programs the agent runs in
//! its own shell. For that, restrict the machine itself: allow only your LLM
//! provider and your SSH hosts at the firewall. See `VLRESEARCH.md`.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{Mutex, OnceLock};

/// Comma-separated hosts orx itself may reach, beyond loopback. Entries are an
/// exact host (`export.arxiv.org`), a subdomain wildcard (`*.example.com` or
/// `.example.com`), or `*` to switch the gate off.
pub(crate) const ALLOW_ENV: &str = "VLR_EGRESS_ALLOW";
/// Turns GitHub integration (the `gh` CLI, repo creation, branch pushes) back on.
pub(crate) const GITHUB_ENV: &str = "VLR_ALLOW_GITHUB";
/// Lets the dashboard install or update coding agents from their vendors.
pub(crate) const INSTALLERS_ENV: &str = "VLR_ALLOW_INSTALLERS";
/// Value for `GIT_ALLOW_PROTOCOL` in every git process orx starts; default `file`.
pub(crate) const GIT_PROTOCOLS_ENV: &str = "VLR_GIT_PROTOCOLS";
/// Set to `0` to stop orx adding the no-telemetry defaults to agent processes.
pub(crate) const AGENT_DEFAULTS_ENV: &str = "VLR_AGENT_ENV_DEFAULTS";

pub(crate) const GITHUB_DISABLED: &str =
    "GitHub integration is off in VLResearch. Set VLR_ALLOW_GITHUB=1 to turn it on.";
pub(crate) const INSTALLERS_DISABLED: &str =
    "VLResearch does not download coding agents. Install and sign in to your agent yourself, \
     or set VLR_ALLOW_INSTALLERS=1.";
pub(crate) const UPDATES_DISABLED: &str =
    "VLResearch is built from source and never installs alphaXiv's releases over itself. \
     Update by pulling your VLResearch checkout and rebuilding.";

/// Blocked requests are routed through a proxy at the discard port on
/// loopback, which refuses the connection at once.
const BLACKHOLE: &str = "http://127.0.0.1:9";

/// Settings that stop coding agents' own non-essential traffic. Each is set
/// only when the variable has no value yet, so an explicit choice wins.
const AGENT_ENV_DEFAULTS: &[(&str, &str)] = &[
    // Claude Code: auto-updates, telemetry, error reporting, /feedback and
    // feature-flag fetches.
    ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
    // OpenCode: self-updates and on-demand language-server downloads.
    ("OPENCODE_DISABLE_AUTOUPDATE", "1"),
    ("OPENCODE_DISABLE_LSP_DOWNLOAD", "1"),
    // The cross-tool "Console Do Not Track" convention.
    ("DO_NOT_TRACK", "1"),
];

/// Whether `host` is this machine: `localhost`, `*.localhost`, or a loopback IP.
pub(crate) fn is_loopback_host(host: &str) -> bool {
    let host = bare_host(host);
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Whether `list` (the `VLR_EGRESS_ALLOW` syntax) admits `host`.
pub(crate) fn list_allows(list: &str, host: &str) -> bool {
    let host = bare_host(host);
    if host.is_empty() {
        return false;
    }
    list.split([',', ';', ' '])
        .map(|entry| entry.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }
            if let Some(domain) = entry.strip_prefix("*.").or_else(|| entry.strip_prefix('.')) {
                return host == domain || host.ends_with(&format!(".{domain}"));
            }
            host == entry
        })
}

/// The gate for internet-facing clients: loopback plus `VLR_EGRESS_ALLOW`.
pub(crate) fn remote_host_allowed(host: &str) -> bool {
    is_loopback_host(host) || std::env::var(ALLOW_ENV).is_ok_and(|list| list_allows(&list, host))
}

/// Proxies for an internet-facing client: the policy gate first, then the
/// `HTTP(S)_PROXY` / `ALL_PROXY` settings reqwest would otherwise have read on
/// its own (adding any proxy turns reqwest's automatic system proxy off).
pub(crate) fn remote_proxies() -> Vec<reqwest::Proxy> {
    let mut proxies = vec![gate(remote_host_allowed)];
    proxies.extend(env_proxies());
    proxies
}

/// The gate for loopback-only clients, which upstream builds with no proxy
/// and never pointed anywhere but 127.0.0.1; this keeps it that way.
pub(crate) fn loopback_proxy() -> reqwest::Proxy {
    gate(is_loopback_host)
}

/// Whether GitHub integration may run (off unless `VLR_ALLOW_GITHUB` is on).
pub(crate) fn github_allowed() -> bool {
    super::env_flag(GITHUB_ENV)
}

/// Whether the dashboard may download coding agents (off unless
/// `VLR_ALLOW_INSTALLERS` is on).
pub(crate) fn installers_allowed() -> bool {
    super::env_flag(INSTALLERS_ENV)
}

/// Source builds never check alphaXiv's release feed or install its builds:
/// an upstream release would replace this distribution with plain OpenResearch.
pub(crate) fn updates_blocked() -> bool {
    crate::telemetry::build_channel() != "production"
}

/// Pins the protocols every git process may use: `file` (local repositories
/// only) unless `VLR_GIT_PROTOCOLS` says otherwise (`file:ssh` for an internal
/// git server, `*` for no limit). A `GIT_ALLOW_PROTOCOL` you exported yourself
/// is left alone when `VLR_GIT_PROTOCOLS` is unset.
pub(crate) fn restrict_git(command: &mut std::process::Command) {
    match std::env::var(GIT_PROTOCOLS_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(protocols) if protocols == "*" => {
            command.env_remove("GIT_ALLOW_PROTOCOL");
        }
        Some(protocols) => {
            command.env("GIT_ALLOW_PROTOCOL", protocols);
        }
        None if std::env::var_os("GIT_ALLOW_PROTOCOL").is_some() => {}
        None => {
            command.env("GIT_ALLOW_PROTOCOL", "file");
        }
    }
}

/// Adds [`AGENT_ENV_DEFAULTS`] to a coding-agent process, skipping any
/// variable already set in orx's environment, the user's shell, or on the
/// command itself.
pub(crate) fn apply_agent_env_defaults(cmd: &mut tokio::process::Command) {
    if std::env::var(AGENT_DEFAULTS_ENV).is_ok_and(|value| value.trim() == "0") {
        return;
    }
    super::set_env_defaults(cmd, AGENT_ENV_DEFAULTS);
}

/// Lowercased host without IPv6 brackets or a trailing root dot.
fn bare_host(host: &str) -> String {
    host.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn gate(allowed: fn(&str) -> bool) -> reqwest::Proxy {
    reqwest::Proxy::custom(move |url| {
        let host = url.host_str().unwrap_or_default();
        if allowed(host) {
            None
        } else {
            note_blocked(host);
            Some(BLACKHOLE)
        }
    })
}

fn env_proxies() -> Vec<reqwest::Proxy> {
    let first_set = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| std::env::var(name).ok())
            .filter(|value| !value.trim().is_empty())
    };
    let mut proxies = Vec::new();
    if let Some(url) = first_set(&["HTTPS_PROXY", "https_proxy"]) {
        if let Ok(proxy) = reqwest::Proxy::https(url.as_str()) {
            proxies.push(proxy.no_proxy(reqwest::NoProxy::from_env()));
        }
    }
    // Like reqwest: never trust HTTP_PROXY under CGI, where a client sets it.
    if std::env::var_os("REQUEST_METHOD").is_none() {
        if let Some(url) = first_set(&["HTTP_PROXY", "http_proxy"]) {
            if let Ok(proxy) = reqwest::Proxy::http(url.as_str()) {
                proxies.push(proxy.no_proxy(reqwest::NoProxy::from_env()));
            }
        }
    }
    if let Some(url) = first_set(&["ALL_PROXY", "all_proxy"]) {
        if let Ok(proxy) = reqwest::Proxy::all(url.as_str()) {
            proxies.push(proxy.no_proxy(reqwest::NoProxy::from_env()));
        }
    }
    proxies
}

/// Says once per host and process why a request failed, so a refused
/// connection is not mistaken for a network fault.
fn note_blocked(host: &str) {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let shown = if host.is_empty() { "(no host)" } else { host };
    let first = SEEN
        .get_or_init(Default::default)
        .lock()
        .map(|mut seen| seen.insert(shown.to_string()))
        .unwrap_or(false);
    if first {
        eprintln!(
            "vlresearch: blocked an outbound request to {shown} (egress policy). \
             To allow it, add the host to {ALLOW_ENV}."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts_are_recognized() {
        for host in [
            "localhost",
            "LOCALHOST",
            "app.localhost",
            "127.0.0.1",
            "127.4.5.6",
            "::1",
            "[::1]",
            "localhost.",
        ] {
            assert!(is_loopback_host(host), "{host} should be loopback");
        }
        for host in [
            "",
            "example.com",
            "localhost.example.com",
            "10.0.0.1",
            "0.0.0.0",
            "[2001:db8::1]",
            "api.openresearch.sh",
        ] {
            assert!(!is_loopback_host(host), "{host} should not be loopback");
        }
    }

    #[test]
    fn allow_list_matches_exact_hosts_and_wildcards() {
        let list = "export.arxiv.org, *.example.com;.internal.lab  LLM.Provider.test.";
        assert!(list_allows(list, "export.arxiv.org"));
        assert!(list_allows(list, "EXPORT.ARXIV.ORG"));
        assert!(!list_allows(list, "arxiv.org"));
        assert!(list_allows(list, "api.example.com"));
        assert!(list_allows(list, "example.com"));
        assert!(!list_allows(list, "badexample.com"));
        assert!(list_allows(list, "gpu.internal.lab"));
        assert!(list_allows(list, "llm.provider.test"));
        assert!(!list_allows(list, "provider.test"));
        assert!(!list_allows("", "example.com"));
        assert!(list_allows("*", "anything.at.all"));
        assert!(!list_allows("*", ""));
    }

    #[test]
    fn git_protocols_default_to_local_only() {
        // Asserts on the command's own env overrides; reads the process env,
        // so it only checks the default when no override is exported.
        if std::env::var_os(GIT_PROTOCOLS_ENV).is_some()
            || std::env::var_os("GIT_ALLOW_PROTOCOL").is_some()
        {
            return;
        }
        let mut command = std::process::Command::new("git");
        restrict_git(&mut command);
        let value = command
            .get_envs()
            .find(|(name, _)| *name == "GIT_ALLOW_PROTOCOL")
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned());
        assert_eq!(value.as_deref(), Some("file"));
    }

    #[test]
    fn agent_defaults_never_override_a_value_on_the_command() {
        let mut cmd = tokio::process::Command::new("true");
        cmd.env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "custom");
        apply_agent_env_defaults(&mut cmd);
        let value = cmd
            .as_std()
            .get_envs()
            .find(|(name, _)| *name == "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC")
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned());
        assert_eq!(value.as_deref(), Some("custom"));
    }

    #[tokio::test]
    async fn blocked_hosts_fail_without_leaving_the_machine() {
        // `.invalid` never resolves; a refused proxy connection proves the
        // request went to the blackhole rather than to DNS or the network.
        let client = crate::net::remote_client()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let error = client
            .get("http://vlresearch-egress-test.invalid/")
            .send()
            .await
            .unwrap_err();
        assert!(error.is_connect(), "expected a refused connection: {error}");
    }

    #[tokio::test]
    async fn loopback_requests_still_work() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let client = crate::net::loopback_client()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let body = client
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "ok");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn loopback_client_refuses_other_hosts() {
        let client = crate::net::loopback_client()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let error = client
            .get("http://vlresearch-egress-test.invalid/")
            .send()
            .await
            .unwrap_err();
        assert!(error.is_connect(), "expected a refused connection: {error}");
    }
}
