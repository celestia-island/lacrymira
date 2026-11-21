//! The dynamic value type that crosses the `Condition`/`Command` boundary.

use serde::{Deserialize, Serialize};

/// A dynamically-typed database value.
///
/// This is what `Condition` comparisons carry and what `Command` parameters
/// serialize to. It is deliberately closed (no `Any`, no trait objects) so
/// that the msgpack encoding is total and deterministic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// SQL NULL.
    Null,
    /// Boolean.
    Bool(bool),
    /// 32-bit integer.
    I32(i32),
    /// 64-bit integer.
    I64(i64),
    /// 64-bit float.
    F64(f64),
    /// Text.
    Text(String),
    /// Bytes (for `bytea`, vector embeddings, etc).
    Bytes(Vec<u8>),
    /// UUID as canonical hyphenated text.
    Uuid(String),
    /// RFC 3339 timestamp (always UTC, `Z` suffix).
    Timestamp(String),
    /// JSON value (stored as its serialization).
    Json(String),
    /// Array of values (for `Vec<T>` columns).
    Array(Vec<Value>),
}
