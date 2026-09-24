//! The server behind grund.sh.
//!
//! ```text
//!   request ─► trace ─► security headers ─► panic guard ─► timeout
//!           ─► canonical host (308 alias -> origin)
//!           ─► /health/live, /health/ready
//!           └► static site, embedded by build.rs from site/
//! ```
//!
//! No database, no outbound calls, no filesystem at runtime: every response
//! comes from a table compiled into the binary.

mod api;
mod canonical;
mod config;
mod server;
mod site;
mod state;

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{config::Config, site::Site, state::State};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::validated()?;
    init_tracing(&config);

    let site = Site::embedded();
    tracing::info!(
        revision = site::REVISION,
        site_digest = site.digest(),
        files = site.len(),
        canonical_origin = %config.canonical_origin,
        "serving embedded site"
    );

    let grace = config.shutdown_grace;
    let state = State::new(config, site);

    notmad::Mad::builder()
        .add(server::Http::new(state))
        .cancellation(Some(grace))
        .run()
        .await?;
    Ok(())
}

fn init_tracing(config: &Config) {
    let filter = EnvFilter::try_new(&config.log)
        .unwrap_or_else(|_| EnvFilter::new("grund_website=info,notmad=info,info"));
    let registry = tracing_subscriber::registry().with(filter);
    if config.log_format == "json" {
        registry.with(fmt::layer().json()).init();
    } else {
        registry.with(fmt::layer().compact()).init();
    }
}
