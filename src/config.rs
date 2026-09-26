use std::{net::SocketAddr, time::Duration};

use clap::Parser;

/// Every knob is a flag and an environment variable; `--help` is the reference.
#[derive(Clone, Debug, Parser)]
#[command(
    name = "grund-website",
    version,
    about = "Serves grund.sh: a static site embedded at build time"
)]
pub struct Config {
    /// Where the HTTP server binds. The image sets 0.0.0.0:8080.
    #[arg(long, env = "GRUND_WEBSITE_LISTEN", default_value = "127.0.0.1:8080")]
    pub listen: SocketAddr,

    /// The one origin the site is served from, e.g. https://grund.sh. Alias
    /// hosts redirect here. An https origin with no path or trailing slash;
    /// plain http is accepted only for loopback, for local runs.
    #[arg(
        long,
        env = "GRUND_WEBSITE_CANONICAL_ORIGIN",
        default_value = "https://grund.sh"
    )]
    pub canonical_origin: String,

    /// Hosts that answer every request with a 308 to the canonical origin,
    /// keeping path and query (e.g. www.grund.sh). Comma-separated. Hosts not
    /// listed here are served normally, so kubelet probes and port-forwards,
    /// which send a pod IP as Host, keep working.
    #[arg(
        long,
        env = "GRUND_WEBSITE_REDIRECT_HOSTS",
        value_delimiter = ',',
        default_value = ""
    )]
    pub redirect_hosts: Vec<String>,

    /// Send `X-Robots-Tag: noindex` on every response. Set on dev, so a
    /// pre-production host never lands in a search index.
    #[arg(long, env = "GRUND_WEBSITE_NOINDEX", default_value_t = false, action = clap::ArgAction::Set)]
    pub noindex: bool,

    /// Serve blog drafts (posts with `draft: true`) and list them on /blog/,
    /// each marked as a draft and `noindex`. Set on dev only: without it, a
    /// draft is a 404, and with no published post there is no blog.
    #[arg(long, env = "GRUND_WEBSITE_BLOG_DRAFTS", default_value_t = false, action = clap::ArgAction::Set)]
    pub blog_drafts: bool,

    /// Show the newsletter sign-up (the pages' `<!-- newsletter -->` blocks)
    /// and serve its routes, which forward to grund insights. Needs
    /// GRUND_WEBSITE_INSIGHTS_URL. Off: the form is not in the pages and the
    /// routes are 404.
    #[arg(long, env = "GRUND_WEBSITE_NEWSLETTER", default_value_t = false, action = clap::ArgAction::Set)]
    pub newsletter: bool,

    /// Strict-Transport-Security max-age in seconds; 0 sends no header. Enable
    /// only once HTTPS on every host of this origin is proven: browsers keep
    /// the promise for the whole max-age even if TLS later breaks.
    #[arg(long, env = "GRUND_WEBSITE_HSTS_MAX_AGE", default_value_t = 0)]
    pub hsts_max_age: u64,

    /// Upper bound on one request. Every response is an in-memory copy, so
    /// anything near this is a slow client, not slow work.
    #[arg(long, env = "GRUND_WEBSITE_REQUEST_TIMEOUT", value_parser = secs, default_value = "10")]
    pub request_timeout: Duration,

    /// How long in-flight requests get to finish after SIGTERM. Must stay
    /// inside the kubelet's terminationGracePeriodSeconds (30 s by default).
    #[arg(long, env = "GRUND_WEBSITE_SHUTDOWN_GRACE", value_parser = secs, default_value = "10")]
    pub shutdown_grace: Duration,

    /// The origin of grund's dashboard, e.g. https://app.grund.sh. /sign-in
    /// sends visitors to its login page. Unset, /sign-in is an ordinary 404,
    /// for an environment whose dashboard is not deployed yet. Same form as
    /// GRUND_WEBSITE_CANONICAL_ORIGIN.
    #[arg(long, env = "GRUND_WEBSITE_APP_URL")]
    pub app_url: Option<String>,

    /// Where page views are reported: the base URL of grund insights' ingest
    /// listener, plain http inside the cluster (e.g. http://grund-insights:8081).
    /// Unset, nothing is reported. Comes from forest config, never from code.
    #[arg(long, env = "GRUND_WEBSITE_INSIGHTS_URL")]
    pub insights_url: Option<String>,

    /// Bearer token for the ingest listener, when insights requires one.
    #[arg(long, env = "GRUND_WEBSITE_INSIGHTS_TOKEN", hide_env_values = true)]
    pub insights_token: Option<String>,

    /// The request header that carries the client address as the edge saw it
    /// (Traefik sets X-Real-Ip). Unset, no address is reported, and insights
    /// counts views but not visitors.
    #[arg(long, env = "GRUND_WEBSITE_INSIGHTS_CLIENT_IP_HEADER")]
    pub insights_client_ip_header: Option<String>,

    /// The site name page views are reported under. Defaults to the host of
    /// GRUND_WEBSITE_CANONICAL_ORIGIN.
    #[arg(long, env = "GRUND_WEBSITE_INSIGHTS_SITE")]
    pub insights_site: Option<String>,

    #[arg(long, env = "GRUND_WEBSITE_LOG_FORMAT", value_parser = ["compact", "json"], default_value = "compact")]
    pub log_format: String,

    #[arg(
        long,
        env = "RUST_LOG",
        default_value = "grund_website=info,notmad=info,info"
    )]
    pub log: String,
}

impl Config {
    pub fn validated() -> anyhow::Result<Self> {
        let mut config = Self::parse();
        config.validate()?;
        Ok(config)
    }

    /// Normalises the host list and refuses a configuration that would serve
    /// the wrong origin or loop.
    pub fn validate(&mut self) -> anyhow::Result<()> {
        let canonical_host = origin_host(&self.canonical_origin).ok_or_else(|| {
            anyhow::anyhow!(
                "GRUND_WEBSITE_CANONICAL_ORIGIN must be an https origin with no path or trailing \
                 slash (plain http only for loopback), got {:?}",
                self.canonical_origin
            )
        })?;

        self.redirect_hosts = self
            .redirect_hosts
            .iter()
            .map(|host| host.trim().to_ascii_lowercase())
            .filter(|host| !host.is_empty())
            .collect();
        for host in &self.redirect_hosts {
            anyhow::ensure!(
                is_hostname(host),
                "GRUND_WEBSITE_REDIRECT_HOSTS entries must be bare host names, got {host:?}"
            );
            anyhow::ensure!(
                *host != canonical_host,
                "GRUND_WEBSITE_REDIRECT_HOSTS contains the canonical host {host:?}; that would \
                 redirect every request to itself"
            );
        }
        anyhow::ensure!(
            self.shutdown_grace <= Duration::from_secs(30),
            "GRUND_WEBSITE_SHUTDOWN_GRACE must be at most 30 s, the kubelet's default grace period"
        );
        anyhow::ensure!(
            !self.request_timeout.is_zero(),
            "GRUND_WEBSITE_REQUEST_TIMEOUT must be positive"
        );
        self.app_url = self.app_url.take().filter(|url| !url.trim().is_empty());
        if let Some(url) = &self.app_url {
            anyhow::ensure!(
                origin_host(url).is_some(),
                "GRUND_WEBSITE_APP_URL must be an https origin with no path or trailing slash \
                 (plain http only for loopback), got {url:?}"
            );
        }
        self.insights_url = self
            .insights_url
            .take()
            .filter(|url| !url.trim().is_empty());
        if let Some(url) = &self.insights_url {
            let authority = url.trim_end_matches('/').strip_prefix("http://");
            anyhow::ensure!(
                authority.is_some_and(|a| !a.is_empty() && !a.contains('/')),
                "GRUND_WEBSITE_INSIGHTS_URL must be a plain http base URL with no path, e.g. \
                 http://grund-insights:8081 (in-cluster; there is no TLS client), got {url:?}"
            );
            let site = self
                .insights_site
                .clone()
                .unwrap_or_else(|| canonical_host.clone());
            anyhow::ensure!(
                is_hostname(&site),
                "GRUND_WEBSITE_INSIGHTS_SITE must be a bare host name, got {site:?}"
            );
            self.insights_site = Some(site);
        }
        if let Some(name) = &self.insights_client_ip_header {
            anyhow::ensure!(
                axum::http::HeaderName::from_bytes(name.as_bytes()).is_ok(),
                "GRUND_WEBSITE_INSIGHTS_CLIENT_IP_HEADER must be a header name, got {name:?}"
            );
        }
        anyhow::ensure!(
            !self.newsletter || self.insights_url.is_some(),
            "GRUND_WEBSITE_NEWSLETTER needs GRUND_WEBSITE_INSIGHTS_URL: grund insights keeps the sign-ups"
        );
        Ok(())
    }

    #[cfg(test)]
    pub fn canonical_host(&self) -> String {
        origin_host(&self.canonical_origin).unwrap_or_default()
    }
}

/// The lowercased host of an origin like `https://grund.sh` or
/// `http://127.0.0.1:8080`, or `None` when it is not such an origin.
fn origin_host(origin: &str) -> Option<String> {
    let (scheme, authority) = origin.split_once("://")?;
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, port)| {
            if port.bytes().all(|b| b.is_ascii_digit()) && !port.is_empty() {
                host
            } else {
                authority
            }
        });
    let host = host.to_ascii_lowercase();
    if !is_hostname(&host) {
        return None;
    }
    let loopback = host == "localhost" || host == "127.0.0.1";
    match scheme {
        "https" => Some(host),
        "http" if loopback => Some(host),
        _ => None,
    }
}

fn is_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn secs(s: &str) -> Result<Duration, String> {
    s.parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> anyhow::Result<Config> {
        let mut config =
            Config::try_parse_from(std::iter::once("grund-website").chain(args.iter().copied()))?;
        config.validate()?;
        Ok(config)
    }

    #[test]
    fn the_command_definition_is_internally_consistent() {
        Config::command().debug_assert();
    }

    #[test]
    fn defaults_are_a_valid_configuration_for_grund_run() {
        let config = parse(&[]).unwrap();
        assert_eq!(config.canonical_host(), "grund.sh");
        assert!(config.redirect_hosts.is_empty());
        assert_eq!(config.hsts_max_age, 0);
    }

    #[test]
    fn redirect_hosts_are_split_trimmed_and_lowercased() {
        let config = parse(&["--redirect-hosts", "WWW.grund.sh, grund.dev"]).unwrap();
        assert_eq!(config.redirect_hosts, ["www.grund.sh", "grund.dev"]);
    }

    #[test]
    fn an_app_url_must_be_an_origin_and_an_empty_one_is_unset() {
        for url in [
            "https://app.grund.sh/",
            "https://app.grund.sh/login",
            "http://app.grund.sh",
        ] {
            let error = parse(&["--app-url", url]).unwrap_err().to_string();
            assert!(error.contains("GRUND_WEBSITE_APP_URL"), "{url}: {error}");
        }
        assert_eq!(parse(&["--app-url", ""]).unwrap().app_url, None);
        let config = parse(&["--app-url", "https://dev.app.grund.sh"]).unwrap();
        assert_eq!(config.app_url.as_deref(), Some("https://dev.app.grund.sh"));
    }

    #[test]
    fn a_canonical_origin_with_a_path_or_trailing_slash_is_refused() {
        for origin in [
            "https://grund.sh/",
            "https://grund.sh/home",
            "grund.sh",
            "ftp://grund.sh",
        ] {
            let error = parse(&["--canonical-origin", origin])
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("GRUND_WEBSITE_CANONICAL_ORIGIN"),
                "{origin}: {error}"
            );
        }
    }

    #[test]
    fn plain_http_is_accepted_only_for_loopback() {
        assert!(parse(&["--canonical-origin", "http://127.0.0.1:8080"]).is_ok());
        assert!(parse(&["--canonical-origin", "http://grund.sh"]).is_err());
    }

    #[test]
    fn redirecting_the_canonical_host_to_itself_is_refused() {
        let error = parse(&["--redirect-hosts", "grund.sh"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("GRUND_WEBSITE_REDIRECT_HOSTS"), "{error}");
    }

    #[test]
    fn a_redirect_host_with_a_scheme_or_path_is_refused() {
        assert!(parse(&["--redirect-hosts", "https://www.grund.sh"]).is_err());
        assert!(parse(&["--redirect-hosts", "www.grund.sh/x"]).is_err());
    }

    #[test]
    fn a_grace_period_longer_than_the_kubelets_is_refused() {
        assert!(parse(&["--shutdown-grace", "31"]).is_err());
    }

    #[test]
    fn insights_is_off_by_default_and_reports_under_the_canonical_host() {
        assert!(parse(&[]).unwrap().insights_url.is_none());
        let config = parse(&["--insights-url", "http://grund-insights:8081"]).unwrap();
        assert_eq!(config.insights_site.as_deref(), Some("grund.sh"));
    }

    #[test]
    fn an_insights_url_with_a_path_or_tls_is_refused() {
        for url in [
            "https://insights.example",
            "http://grund-insights:8081/v1",
            "grund-insights:8081",
        ] {
            let error = parse(&["--insights-url", url]).unwrap_err().to_string();
            assert!(
                error.contains("GRUND_WEBSITE_INSIGHTS_URL"),
                "{url}: {error}"
            );
        }
    }

    #[test]
    fn an_unknown_flag_is_an_error_not_a_server() {
        assert!(parse(&["--canonical-orign", "https://grund.sh"]).is_err());
    }
}
