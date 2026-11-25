//! Forge pull-request webhooks to Linear (COD-102).
//!
//! Verified webhook bodies from Forgejo, Gitea, GitLab and git.dabney.moe are
//! queued durably, parsed into typed [`domain::PrSnapshot`]s and applied to
//! Linear as idempotent attachments. Only a repository configured as
//! [`domain::Authority::Canonical`] may move an issue, and only into Done,
//! once every canonical PR on the issue has merged or closed.

pub mod config;
pub mod domain;
pub mod event;
pub mod http;
pub mod linear;
#[cfg(feature = "mock-linear")]
pub mod mock;
pub mod queue;
pub mod sync;
pub mod verify;
pub mod worker;

use std::sync::Arc;

use tokio::sync::Notify;

/// A ready-to-run bridge: the webhook router plus its queue worker.
pub struct Bridge {
    pub router: axum::Router,
    pub worker: worker::Worker,
}

impl Bridge {
    pub fn new(config: config::SyncConfig, store: queue::EventStore) -> Self {
        let config = Arc::new(config);
        let store = Arc::new(store);
        let notify = Arc::new(Notify::new());
        let router = http::router(http::HookState {
            config: config.clone(),
            store: store.clone(),
            notify: notify.clone(),
        });
        Bridge {
            router,
            worker: worker::Worker::new(store, config, notify),
        }
    }
}
