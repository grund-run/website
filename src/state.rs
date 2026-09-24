use std::sync::Arc;

use crate::{canonical::CanonicalHost, config::Config, insights::Insights, site::Site};

/// What the process opens once. Cheap to clone: every field is an `Arc` or a
/// `Copy` handle onto the embedded table.
#[derive(Clone)]
pub struct State {
    pub config: Arc<Config>,
    pub(crate) site: Site,
    pub(crate) canonical: Arc<CanonicalHost>,
    /// Present only when GRUND_WEBSITE_INSIGHTS_URL is set.
    pub(crate) insights: Option<Insights>,
}

impl State {
    pub fn new(config: Config, site: Site, insights: Option<Insights>) -> Self {
        let canonical = Arc::new(CanonicalHost::from_config(&config));
        Self {
            config: Arc::new(config),
            site,
            canonical,
            insights,
        }
    }
}
