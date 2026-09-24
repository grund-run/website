//! The site under test: either the real binary, spawned per test, or an
//! already-running target named by `GRUND_WEBSITE_ACCEPT_URL`.
//!
//! One suite, three audiences. `cargo test` spawns the binary cargo just
//! built. CI's images workflow points it at the release binary it is about to
//! package, `check.sh` at the read-only scratch container, and a person at a
//! live origin:
//!
//! ```text
//! GRUND_WEBSITE_ACCEPT_URL=https://dev.grund.sh \
//! GRUND_WEBSITE_ACCEPT_CANONICAL_ORIGIN=https://dev.grund.sh \
//! GRUND_WEBSITE_ACCEPT_NOINDEX=true cargo test --test tests
//!
//! GRUND_WEBSITE_ACCEPT_URL=https://grund.sh \
//! GRUND_WEBSITE_ACCEPT_REDIRECT_HOST=www.grund.sh cargo test --test tests
//! ```
//!
//! Against an https target the redirect host is requested by name, which also
//! proves its DNS, gateway route and certificate. Against http it is sent as
//! the Host header to the same address.

use std::{
    fs::File,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::Context;

use super::client::{self, Origin};

/// What the running site is configured to do, so a test knows what to expect.
#[derive(Clone, Debug)]
pub struct Expectations {
    pub canonical_origin: String,
    pub redirect_host: Option<String>,
    pub noindex: bool,
}

pub struct Fixture {
    pub origin: Origin,
    pub expect: Expectations,
    /// Spawned binaries only; killed when the test ends.
    child: Option<Child>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The configuration a spawned binary gets unless a test adds to it: the prod
/// shape, with www.grund.sh as an alias of https://grund.sh.
const SPAWN_DEFAULTS: &[(&str, &str)] = &[
    ("GRUND_WEBSITE_CANONICAL_ORIGIN", "https://grund.sh"),
    ("GRUND_WEBSITE_REDIRECT_HOSTS", "www.grund.sh"),
    ("GRUND_WEBSITE_NOINDEX", "false"),
];

pub fn external_target() -> Option<String> {
    std::env::var("GRUND_WEBSITE_ACCEPT_URL")
        .ok()
        .filter(|url| !url.is_empty())
}

impl Fixture {
    pub async fn start() -> anyhow::Result<Self> {
        match external_target() {
            Some(url) => Self::attach(&url).await,
            None => Self::spawn(&[]).await,
        }
    }

    async fn attach(url: &str) -> anyhow::Result<Self> {
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let fixture = Self {
            origin: Origin::parse(url)?,
            expect: Expectations {
                canonical_origin: env("GRUND_WEBSITE_ACCEPT_CANONICAL_ORIGIN")
                    .unwrap_or_else(|| "https://grund.sh".into()),
                redirect_host: env("GRUND_WEBSITE_ACCEPT_REDIRECT_HOST"),
                noindex: env("GRUND_WEBSITE_ACCEPT_NOINDEX").is_some_and(|v| v == "true"),
            },
            child: None,
        };
        fixture.wait_until_live(None).await?;
        Ok(fixture)
    }

    /// Starts the binary cargo built for this test run on a free port, with
    /// `extra` layered over `SPAWN_DEFAULTS`.
    pub async fn spawn(extra: &[(&str, &str)]) -> anyhow::Result<Self> {
        let port = std::net::TcpListener::bind("127.0.0.1:0")?
            .local_addr()?
            .port();
        let log_path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("grund-website-{port}.log"));
        let log = File::create(&log_path)?;

        let mut command = Command::new(env!("CARGO_BIN_EXE_grund-website"));
        command
            .env_clear()
            .env("GRUND_WEBSITE_LISTEN", format!("127.0.0.1:{port}"))
            .env("RUST_LOG", "grund_website=info,warn")
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        for (name, value) in SPAWN_DEFAULTS.iter().chain(extra) {
            command.env(name, value);
        }
        let setting = |name: &str| {
            extra
                .iter()
                .chain(SPAWN_DEFAULTS)
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        };

        let fixture = Self {
            origin: Origin::parse(&format!("http://127.0.0.1:{port}"))?,
            expect: Expectations {
                canonical_origin: setting("GRUND_WEBSITE_CANONICAL_ORIGIN").unwrap_or_default(),
                redirect_host: setting("GRUND_WEBSITE_REDIRECT_HOSTS").filter(|h| !h.is_empty()),
                noindex: setting("GRUND_WEBSITE_NOINDEX").is_some_and(|v| v == "true"),
            },
            child: Some(command.spawn().context("spawn grund-website")?),
        };
        fixture.wait_until_live(Some(&log_path)).await?;
        Ok(fixture)
    }

    /// Fails with the cause: the last error, and the server's own log when we
    /// started it.
    async fn wait_until_live(&self, log: Option<&std::path::Path>) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let error = match client::send(&self.origin, "GET", "/health/live", None, &[]).await {
                Ok(response) if response.status == 200 => return Ok(()),
                Ok(response) => anyhow::anyhow!("status {}", response.status),
                Err(error) => error,
            };
            if Instant::now() > deadline {
                let log = log
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .unwrap_or_default();
                anyhow::bail!(
                    "{}/health/live never answered 200: {error:#}\n--- server log\n{log}",
                    self.origin.authority()
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
