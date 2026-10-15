//! The only place reqwest clients are built; `clippy.toml` bans building them elsewhere.

/// For servers on this machine. reqwest proxies 127.0.0.1 through `HTTP(S)_PROXY` too (OR-352).
#[allow(clippy::disallowed_methods)]
pub fn loopback_client() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .no_proxy()
        // VLResearch: refuse anything that is not this machine.
        .proxy(crate::flavor::egress::loopback_proxy())
}

/// For internet hosts; honors `HTTP(S)_PROXY` and `NO_PROXY`.
#[allow(clippy::disallowed_methods)]
pub fn remote_client() -> reqwest::ClientBuilder {
    // VLResearch: egress gate (loopback and VLR_EGRESS_ALLOW only), then the env proxies.
    crate::flavor::egress::remote_proxies()
        .into_iter()
        .fold(reqwest::Client::builder(), |builder, proxy| {
            builder.proxy(proxy)
        })
}
