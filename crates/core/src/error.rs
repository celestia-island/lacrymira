//! The error type crossing all lacrymira boundaries.

use serde::{Deserialize, Serialize};

/// The single error type for all lacrymira operations.
///
/// `sqlx::Error` (or any driver error) is never exposed directly; the
/// SQLSTATE is preserved for callers that need to classify retries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
pub enum Error {
    /// A database driver error. The `sqlstate` is preserved for retry logic.
    #[error("database error ({kind}): {message}")]
    Db {
        /// Error classification (e.g. "connection", "unique_violation").
        kind: String,
        /// Human-readable message.
        message: String,
        /// PostgreSQL SQLSTATE if available (e.g. "23505" for unique_violation).
        #[serde(skip_serializing_if = "Option::is_none")]
        sqlstate: Option<String>,
    },
    /// The requested row does not exist.
    #[error("record not found")]
    NotFound,
    /// A value failed to decode into the expected type.
    #[error("decode error: {0}")]
    Decode(String),
    /// A configuration or usage error.
    #[error("configuration error: {0}")]
    Config(String),
}

impl Error {
    /// Returns the PostgreSQL SQLSTATE if this is a database error.
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Error::Db { sqlstate, .. } => sqlstate.as_deref(),
            _ => None,
        }
    }

    /// Returns `true` if this error is a unique-constraint violation (SQLSTATE 23505).
    pub fn is_unique_violation(&self) -> bool {
        self.sqlstate() == Some("23505")
    }
}
