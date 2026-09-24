//! One origin for the site. Requests for an alias host (www.grund.sh) get a
//! permanent redirect to the same path on the canonical origin, so there is a
//! single URL per page for links, caches and search engines.

use std::sync::Arc;

use axum::http::Uri;

use crate::{config::Config, state::State};

pub struct CanonicalHost {
    origin: String,
    aliases: Vec<String>,
}

impl CanonicalHost {
    pub fn from_config(config: &Config) -> Self {
        Self {
            origin: config.canonical_origin.clone(),
            aliases: config.redirect_hosts.clone(),
        }
    }

    /// The `Location` for a request, when its `Host` is an alias. Hosts that
    /// are neither canonical nor aliases are served as they are: probes and
    /// port-forwards arrive with a pod IP.
    pub fn redirect_for(&self, host: Option<&str>, uri: &Uri) -> Option<String> {
        let host = normalise_host(host?);
        if !self.aliases.contains(&host) {
            return None;
        }
        // The path is appended to an origin with no trailing slash, so even
        // `//evil.example` stays a path on the canonical host.
        let path_and_query = uri.path_and_query().map_or("/", |pq| pq.as_str());
        Some(format!("{}{}", self.origin, path_and_query))
    }
}

/// `WWW.Grund.Run.:443` -> `www.grund.sh`.
fn normalise_host(host: &str) -> String {
    let host = match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

pub trait CanonicalState {
    fn canonical(&self) -> Arc<CanonicalHost>;
}

impl CanonicalState for State {
    fn canonical(&self) -> Arc<CanonicalHost> {
        Arc::clone(&self.canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> CanonicalHost {
        CanonicalHost {
            origin: "https://grund.sh".into(),
            aliases: vec!["www.grund.sh".into()],
        }
    }

    #[test]
    fn an_alias_redirects_to_the_same_path_and_query_on_the_canonical_origin() {
        let uri: Uri = "/docs/start?ref=hn".parse().unwrap();
        assert_eq!(
            policy().redirect_for(Some("www.grund.sh"), &uri).as_deref(),
            Some("https://grund.sh/docs/start?ref=hn")
        );
    }

    #[test]
    fn alias_matching_ignores_case_port_and_a_trailing_dot() {
        let uri: Uri = "/".parse().unwrap();
        for host in ["WWW.grund.sh", "www.grund.sh:443", "www.grund.sh."] {
            assert!(policy().redirect_for(Some(host), &uri).is_some(), "{host}");
        }
    }

    #[test]
    fn the_canonical_host_and_unknown_hosts_are_served_not_redirected() {
        let uri: Uri = "/".parse().unwrap();
        for host in [Some("grund.sh"), Some("10.42.0.17:8080"), None] {
            assert_eq!(policy().redirect_for(host, &uri), None, "{host:?}");
        }
    }

    #[test]
    fn a_protocol_relative_path_cannot_redirect_off_the_canonical_host() {
        let uri: Uri = "//evil.example/x".parse().unwrap();
        let location = policy().redirect_for(Some("www.grund.sh"), &uri).unwrap();
        assert!(location.starts_with("https://grund.sh/"), "{location}");
    }
}
