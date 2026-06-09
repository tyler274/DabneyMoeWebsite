//! Structured logging, Loki push, HTTP tracing, and consent-gated client error intake.

use axum::{
    extract::Json,
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,tower_http=debug".into());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(false);

    if loki_configured() {
        match build_loki_layer() {
            Ok((loki_layer, task)) => {
                tokio::spawn(task);
                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt_layer)
                    .with(loki_layer)
                    .init();
                return;
            }
            Err(err) => {
                eprintln!("loki tracing layer disabled: {err}");
            }
        }
    }

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();
}

fn loki_configured() -> bool {
    ["LOKI_URL", "LOKI_USER", "LOKI_TOKEN"]
        .into_iter()
        .all(|key| std::env::var(key).is_ok_and(|v| !v.is_empty()))
}

fn build_loki_layer() -> Result<
    (tracing_loki::Layer, tracing_loki::BackgroundTask),
    Box<dyn std::error::Error + Send + Sync>,
> {
    use base64::Engine;
    use tracing_loki::url::Url;

    let url = std::env::var("LOKI_URL")?;
    let user = std::env::var("LOKI_USER")?;
    let token = std::env::var("LOKI_TOKEN")?;

    let auth = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{token}"));
    let (layer, task) = tracing_loki::builder()
        .label("service", "dabney-web")?
        .http_header("Authorization", format!("Basic {auth}"))?
        .build_url(Url::parse(&url)?)?;

    Ok((layer, task))
}

pub fn scrub_path(path: &str) -> String {
    path.split('?').next().unwrap_or(path).to_string()
}

#[derive(Debug, Deserialize)]
pub(crate) struct ClientErrorReport {
    message: String,
    filename: String,
    lineno: u32,
}

pub async fn report_client_error(
    headers: HeaderMap,
    Json(body): Json<ClientErrorReport>,
) -> StatusCode {
    if headers.get("x-consent").and_then(|v| v.to_str().ok()) != Some("error-reporting") {
        return StatusCode::FORBIDDEN;
    }

    tracing::error!(
        service = "dabney-web",
        message = %scrub_client_text(&body.message),
        filename = %scrub_client_text(&body.filename),
        lineno = body.lineno,
        "client error report"
    );

    StatusCode::NO_CONTENT
}

fn scrub_client_text(input: &str) -> String {
    let mut out = input.to_string();
    if let Some(q) = out.find('?') {
        out.truncate(q);
    }
    let lower = out.to_lowercase();
    for pattern in ["@", "token=", "password=", "secret="] {
        if lower.contains(pattern) {
            return "[redacted]".into();
        }
    }
    if out.len() > 500 {
        out.truncate(500);
        out.push('…');
    }
    out
}
