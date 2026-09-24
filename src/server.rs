//! The HTTP listener, as a notmad component.
//!
//! notmad installs the signal handlers, cancels every component when one
//! arrives, and gives them the configured grace to finish. A pod that dropped
//! in-flight requests on SIGTERM would show up as a few 502s on every rollout.

use notmad::{Component, ComponentInfo, MadError};
use tokio_util::sync::CancellationToken;

use crate::state::State;

pub struct Http {
    state: State,
}

impl Http {
    pub fn new(state: State) -> Self {
        Self { state }
    }
}

impl Component for Http {
    fn info(&self) -> ComponentInfo {
        "grund-website/http".into()
    }

    async fn run(&self, cancellation: CancellationToken) -> Result<(), MadError> {
        let address = self.state.config.listen;
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(anyhow::Error::from)?;
        tracing::info!(%address, "grund-website listening");

        axum::serve(listener, crate::api::router(self.state.clone()))
            .with_graceful_shutdown(async move { cancellation.cancelled().await })
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }
}
