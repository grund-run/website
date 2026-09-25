//! The server behind grund.sh.
//!
//! ```text
//!   request ─► trace ─► security headers ─► panic guard ─► timeout
//!           ─► canonical host (308 alias -> origin)
//!           ─► /health/live, /health/ready
//!           └► static site, embedded by build.rs from site/ and blog/
//! ```
//!
//! No database, no outbound calls, no filesystem at runtime: every response
//! comes from a table compiled into the binary.

mod api;
#[cfg(test)]
mod blog;
mod canonical;
mod config;
mod insights;
mod server;
mod site;
mod state;

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{config::Config, site::Site, state::State};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::validated()?;
    init_tracing(&config);

    let site = Site::embedded(config.blog_drafts);
    tracing::info!(
        revision = site::REVISION,
        site_digest = site.digest(),
        files = site.len(),
        blog_drafts = config.blog_drafts,
        canonical_origin = %config.canonical_origin,
        "serving embedded site"
    );

    let grace = config.shutdown_grace;
    // Page views for grund insights, only when configured (insights.rs).
    let (insights, sender) = match &config.insights_url {
        Some(url) => {
            let (insights, receiver) =
                insights::Insights::channel(config.insights_client_ip_header.clone());
            let site = config.insights_site.clone().unwrap_or_default();
            tracing::info!(%site, client_ip = config.insights_client_ip_header.is_some(), "reporting page views to insights");
            let sender = insights::Sender::new(
                url,
                site,
                config.insights_token.clone(),
                &insights,
                receiver,
            )?;
            (Some(insights), Some(sender))
        }
        None => (None, None),
    };
    let state = State::new(config, site, insights);

    // The listener first, so it stops taking requests before the sender
    // makes its last, bounded send.
    let mut mad = notmad::Mad::builder();
    mad.add(server::Http::new(state));
    if let Some(sender) = sender {
        mad.add(sender);
    }
    mad.cancellation(Some(grace)).run().await?;
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
