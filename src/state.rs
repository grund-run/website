use std::sync::Arc;

use crate::{canonical::CanonicalHost, config::Config, site::Site};

/// What the process opens once. Cheap to clone: every field is an `Arc` or a
/// `Copy` handle onto the embedded table.
#[derive(Clone)]
pub struct State {
    pub config: Arc<Config>,
    pub(crate) site: Site,
    pub(crate) canonical: Arc<CanonicalHost>,
}

impl State {
    pub fn new(config: Config, site: Site) -> Self {
        let canonical = Arc::new(CanonicalHost::from_config(&config));
        Self {
            config: Arc::new(config),
            site,
            canonical,
        }
    }
}
