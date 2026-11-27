//! `POST /hooks/forge/{forge}`: verify, classify, enqueue durably, answer.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use bytes::Bytes;
use tokio::sync::Notify;

use crate::config::SyncConfig;
use crate::domain::Forge;
use crate::queue::{Enqueued, EventStore};
use crate::verify::{verify, EventKind};

#[derive(Clone)]
pub struct HookState {
    pub config: Arc<SyncConfig>,
    pub store: Arc<EventStore>,
    pub notify: Arc<Notify>,
}

/// Router with the webhook route, ready to merge into any Axum app.
pub fn router(state: HookState) -> Router {
    Router::new()
        .route("/hooks/forge/{forge}", post(receive))
        .with_state(state)
}

async fn receive(
    State(state): State<HookState>,
    Path(forge): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, &'static str) {
    let Ok(forge) = forge.parse::<Forge>() else {
        return (StatusCode::NOT_FOUND, "unknown forge");
    };
    // Authenticate the raw bytes before anything reads them.
    let payload = match verify(forge, &headers, body, state.config.candidates(forge)) {
        Ok(payload) => payload,
        Err(e) => {
            tracing::warn!(%forge, error = %e, "rejected webhook");
            return (StatusCode::UNAUTHORIZED, "invalid signature");
        }
    };
    if EventKind::from_headers(forge, &headers) == EventKind::Unhandled {
        return (StatusCode::NO_CONTENT, "");
    }
    match state.store.enqueue(&payload) {
        Ok(Enqueued::New(id)) => {
            tracing::info!(%forge, id, delivery = payload.delivery_id(), "queued webhook");
            state.notify.notify_one();
            (StatusCode::ACCEPTED, "queued")
        }
        Ok(Enqueued::Duplicate) => (StatusCode::OK, "duplicate delivery"),
        Err(e) => {
            tracing::error!(%forge, error = %e, "could not queue webhook");
            (StatusCode::INTERNAL_SERVER_ERROR, "could not queue")
        }
    }
}
