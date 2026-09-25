use std::sync::Arc;

use crate::{canonical::CanonicalHost, config::Config, insights::Insights, site::Sites};

/// What the process opens once. Cheap to clone: every field is an `Arc` or a
/// `Copy` handle onto the embedded tables.
#[derive(Clone)]
pub struct State {
    pub config: Arc<Config>,
    /// Every variant of the site; each request is served the current one.
    pub(crate) sites: Sites,
    pub(crate) canonical: Arc<CanonicalHost>,
    /// Present only when GRUND_WEBSITE_INSIGHTS_URL is set.
    pub(crate) insights: Option<Insights>,
}

impl State {
    pub fn new(config: Config, sites: Sites, insights: Option<Insights>) -> Self {
        let canonical = Arc::new(CanonicalHost::from_config(&config));
        Self {
            config: Arc::new(config),
            sites,
            canonical,
            insights,
        }
    }
}
