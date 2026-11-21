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

// ── TryFrom<Value> for common types (used by from_values) ──

impl TryFrom<Value> for i64 {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::I64(x) => Ok(x),
            Value::I32(x) => Ok(x as i64),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for i32 {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::I32(x) => Ok(x),
            Value::I64(x) => Ok(x as i32),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for String {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Text(s) => Ok(s),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for bool {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Bool(b) => Ok(b),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for f64 {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::F64(x) => Ok(x),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Vec<u8> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Bytes(b) => Ok(b),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for serde_json::Value {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Json(s) => serde_json::from_str(&s).map_err(|_| ()),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for uuid::Uuid {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Uuid(s) => uuid::Uuid::parse_str(&s).map_err(|_| ()),
            Value::Text(s) => uuid::Uuid::parse_str(&s).map_err(|_| ()),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Option<String> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Text(s) => Ok(Some(s)),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Option<i64> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::I64(x) => Ok(Some(x)),
            Value::I32(x) => Ok(Some(x as i64)),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Option<i32> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::I32(x) => Ok(Some(x)),
            Value::I64(x) => Ok(Some(x as i32)),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Option<bool> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Bool(b) => Ok(Some(b)),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Option<f64> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::F64(x) => Ok(Some(x)),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Option<uuid::Uuid> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Uuid(s) => uuid::Uuid::parse_str(&s).map(Some).map_err(|_| ()),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}

/// Conversions between `jiff::Timestamp` and [`Value`] (RFC3339 wire form),
/// gated on the default-on `jiff` feature.
#[cfg(feature = "jiff")]
impl From<&jiff::Timestamp> for Value {
    fn from(v: &jiff::Timestamp) -> Self {
        Value::Timestamp(v.to_string())
    }
}

#[cfg(feature = "jiff")]
impl TryFrom<Value> for jiff::Timestamp {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        use std::str::FromStr;
        match v {
            Value::Timestamp(s) | Value::Text(s) => jiff::Timestamp::from_str(&s).map_err(|_| ()),
            _ => Err(()),
        }
    }
}

// ── G1: Option<serde_json::Value> round-trip (nullable JSONB) ──

impl TryFrom<Value> for Option<serde_json::Value> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Json(s) => serde_json::from_str(&s).map(Some).map_err(|_| ()),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}

// ── G3: array round-trips (TEXT[] / FLOAT4[]) ──

impl TryFrom<Value> for Vec<String> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    Value::Text(s) => Ok(s),
                    _ => Err(()),
                })
                .collect(),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Vec<f32> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    Value::F64(x) => Ok(x as f32),
                    Value::I64(x) => Ok(x as f32),
                    Value::I32(x) => Ok(x as f32),
                    _ => Err(()),
                })
                .collect(),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Vec<i64> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    Value::I64(x) => Ok(x),
                    Value::I32(x) => Ok(x as i64),
                    _ => Err(()),
                })
                .collect(),
            _ => Err(()),
        }
    }
}

// ── G5: f32 / i16 conversions (FLOAT4 decodes to F64, INT2 to I32) ──

impl TryFrom<Value> for f32 {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::F64(x) => Ok(x as f32),
            Value::I32(x) => Ok(x as f32),
            Value::I64(x) => Ok(x as f32),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for i16 {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::I32(x) => Ok(x as i16),
            Value::I64(x) => Ok(x as i16),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Option<f32> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::F64(x) => Ok(Some(x as f32)),
            Value::I32(x) => Ok(Some(x as f32)),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}

impl TryFrom<Value> for Option<i16> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::I32(x) => Ok(Some(x as i16)),
            Value::I64(x) => Ok(Some(x as i16)),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}

/// Conversions between `chrono::DateTime<Utc>` and [`Value`] (RFC3339 wire
/// form), gated on the opt-in `chrono` feature for callers still on chrono.
#[cfg(feature = "chrono")]
impl From<&chrono::DateTime<chrono::Utc>> for Value {
    fn from(v: &chrono::DateTime<chrono::Utc>) -> Self {
        Value::Timestamp(v.to_rfc3339())
    }
}

#[cfg(feature = "chrono")]
impl TryFrom<Value> for chrono::DateTime<chrono::Utc> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Timestamp(s) | Value::Text(s) => chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|_| ()),
            _ => Err(()),
        }
    }
}

#[cfg(feature = "chrono")]
impl TryFrom<Value> for Option<chrono::DateTime<chrono::Utc>> {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Timestamp(s) | Value::Text(s) => chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| Some(dt.with_timezone(&chrono::Utc)))
                .map_err(|_| ()),
            Value::Null => Ok(None),
            _ => Err(()),
        }
    }
}
