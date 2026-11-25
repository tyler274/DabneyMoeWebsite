//! The forge's error type and how each case reaches an HTTP client.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::git::pack::PackError;
use crate::git::pkt::PktError;

#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    #[error("Git object database error")]
    Git(#[source] gix::Error),
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error("Git protocol error")]
    Protocol(#[from] PktError),
    #[error("database error")]
    Db(#[from] rusqlite::Error),
    #[error("I/O error")]
    Io(#[source] std::io::Error),
    #[error("invalid name {0:?}")]
    InvalidName(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("authentication required")]
    Unauthenticated,
    #[error("not allowed: {0}")]
    Forbidden(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unprocessable: {0}")]
    Unprocessable(String),
    #[error("background task failed")]
    Join(#[from] tokio::task::JoinError),
}

impl ForgeError {
    pub fn status(&self) -> StatusCode {
        match self {
            ForgeError::InvalidName(_) | ForgeError::BadRequest(_) | ForgeError::Protocol(_) => {
                StatusCode::BAD_REQUEST
            }
            ForgeError::Unprocessable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ForgeError::NotFound(_) => StatusCode::NOT_FOUND,
            ForgeError::Unauthenticated => StatusCode::UNAUTHORIZED,
            ForgeError::Forbidden(_) => StatusCode::FORBIDDEN,
            ForgeError::Conflict(_) => StatusCode::CONFLICT,
            ForgeError::Git(_)
            | ForgeError::Pack(_)
            | ForgeError::Db(_)
            | ForgeError::Io(_)
            | ForgeError::Join(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The message with its whole source chain, for logs and 5xx bodies.
    pub fn chain(&self) -> String {
        let mut text = self.to_string();
        let mut source = std::error::Error::source(self);
        while let Some(s) = source {
            text.push_str(": ");
            text.push_str(&s.to_string());
            source = s.source();
        }
        text
    }
}

impl IntoResponse for ForgeError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(error = %self.chain(), "request failed");
        }
        let body = if status.is_server_error() {
            "internal error".to_owned()
        } else {
            self.to_string()
        };
        let mut response = (status, body).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                header::HeaderValue::from_static("Basic realm=\"dabney-forge\", charset=\"UTF-8\""),
            );
        }
        response
    }
}
