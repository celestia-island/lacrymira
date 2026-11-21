//! The condition tree shared by all backends.

use serde::{Deserialize, Serialize};

use crate::value::Value;

/// A column reference in a condition (table-qualified).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnRef {
    /// Column name as it appears in SQL.
    pub name: String,
}

/// A condition that can be attached to a query.
///
/// This is a runtime tree, not a typestate chain — conditions can be
/// composed, stored, and branched on. The diesel lesson: a filter that
/// changes type on every `.filter()` call makes `let mut q = ...; if c {
/// q = q.filter(..); }` unrepresentable. We make it work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    /// `col = value`
    Eq(ColumnRef, Value),
    /// `col != value`
    Ne(ColumnRef, Value),
    /// `col < value`
    Lt(ColumnRef, Value),
    /// `col <= value`
    Lte(ColumnRef, Value),
    /// `col > value`
    Gt(ColumnRef, Value),
    /// `col >= value`
    Gte(ColumnRef, Value),
    /// `col LIKE pattern`
    Like(ColumnRef, Value),
    /// `col ILIKE pattern` (PostgreSQL case-insensitive)
    Ilike(ColumnRef, Value),
    /// `col IS NULL`
    IsNull(ColumnRef),
    /// `col IS NOT NULL`
    IsNotNull(ColumnRef),
    /// `col IN (values)` — rendered as `= ANY($n)` on PostgreSQL
    In(ColumnRef, Vec<Value>),
    /// `col NOT IN (values)`
    NotIn(ColumnRef, Vec<Value>),
    /// Raw SQL with bound parameters (escape hatch).
    Raw(String, Vec<Value>),
    /// `col->path = value` (JSONB path extraction equality)
    JsonGetEq(ColumnRef, String, Value),
    /// `col ? path` (JSONB path/key existence)
    JsonExists(ColumnRef, String),
    /// `col @> value` (JSONB containment)
    JsonContains(ColumnRef, Value),
    /// `jsonb_path_match(col, path, vars)` (PostgreSQL 12+ JSONB path query)
    JsonbPathMatch(ColumnRef, String, Value),
    /// All sub-conditions must hold (AND).
    All(Vec<Condition>),
    /// At least one sub-condition must hold (OR).
    Any(Vec<Condition>),
    /// The negation of a condition.
    Not(Box<Condition>),
}

impl Condition {
    /// Combine two conditions with AND.
    pub fn and(self, other: Condition) -> Condition {
        Condition::All(vec![self, other])
    }

    /// Combine two conditions with OR.
    pub fn or(self, other: Condition) -> Condition {
        Condition::Any(vec![self, other])
    }
}

// ── Builder methods: the 18 comparison operators ──
//
// These live on `ColumnRef` so that `ColumnRef { name: "user_id".into() }.eq(v)`
// reads naturally. They return `Condition` (a runtime value), not a typestate
// chain — the diesel lesson.

impl ColumnRef {
    /// Create a column reference from a name.
    pub fn new(name: impl Into<String>) -> Self {
        ColumnRef { name: name.into() }
    }

    /// `col = value`
    pub fn eq<V: Into<Value>>(self, v: V) -> Condition {
        Condition::Eq(self, v.into())
    }
    /// `col != value`
    pub fn ne<V: Into<Value>>(self, v: V) -> Condition {
        Condition::Ne(self, v.into())
    }
    /// `col < value`
    pub fn lt<V: Into<Value>>(self, v: V) -> Condition {
        Condition::Lt(self, v.into())
    }
    /// `col <= value`
    pub fn lte<V: Into<Value>>(self, v: V) -> Condition {
        Condition::Lte(self, v.into())
    }
    /// `col > value`
    pub fn gt<V: Into<Value>>(self, v: V) -> Condition {
        Condition::Gt(self, v.into())
    }
    /// `col >= value`
    pub fn gte<V: Into<Value>>(self, v: V) -> Condition {
        Condition::Gte(self, v.into())
    }
    /// `col LIKE pattern`
    pub fn like(self, pattern: impl Into<String>) -> Condition {
        Condition::Like(self, Value::Text(pattern.into()))
    }
    /// `col ILIKE pattern` (PostgreSQL)
    pub fn ilike(self, pattern: impl Into<String>) -> Condition {
        Condition::Ilike(self, Value::Text(pattern.into()))
    }
    /// `col IS NULL`
    pub fn is_null(self) -> Condition {
        Condition::IsNull(self)
    }
    /// `col IS NOT NULL`
    pub fn is_not_null(self) -> Condition {
        Condition::IsNotNull(self)
    }
    /// `col IN (values)` — rendered as `col = ANY($n)` on PostgreSQL
    pub fn in_<V: Into<Value>>(self, values: Vec<V>) -> Condition {
        Condition::In(self, values.into_iter().map(Into::into).collect())
    }
    /// `col NOT IN (values)`
    pub fn not_in<V: Into<Value>>(self, values: Vec<V>) -> Condition {
        Condition::NotIn(self, values.into_iter().map(Into::into).collect())
    }
    /// `col LIKE '%substr%'` (contains)
    pub fn contains(self, substr: impl Into<String>) -> Condition {
        Condition::Like(self, Value::Text(format!("%{}%", substr.into())))
    }
    /// `col LIKE 'prefix%'` (starts with)
    pub fn starts_with(self, prefix: impl Into<String>) -> Condition {
        Condition::Like(self, Value::Text(format!("{}%", prefix.into())))
    }
    /// `col LIKE '%suffix'` (ends with)
    pub fn ends_with(self, suffix: impl Into<String>) -> Condition {
        Condition::Like(self, Value::Text(format!("%{}", suffix.into())))
    }
    /// `col BETWEEN low AND high`
    pub fn between<V: Into<Value>>(self, low: V, high: V) -> Condition {
        Condition::All(vec![
            Condition::Gte(self.clone(), low.into()),
            Condition::Lte(self, high.into()),
        ])
    }
    /// `col NOT BETWEEN low AND high`
    pub fn not_between<V: Into<Value>>(self, low: V, high: V) -> Condition {
        Condition::Not(Box::new(self.between(low, high)))
    }
    /// `col && array` (PostgreSQL array overlap)
    pub fn overlaps(self, values: Vec<Value>) -> Condition {
        Condition::Raw(
            format!("{} && ${{}}", self.name),
            vec![Value::Array(values)],
        )
    }

    // ── JSONB path operators ──

    /// `col->path = value` (extract JSONB path and compare)
    pub fn json_get(self, path: impl Into<String>) -> JsonPathBuilder {
        JsonPathBuilder {
            column: self,
            path: path.into(),
        }
    }

    /// `col ? path` (check if JSONB key/path exists)
    pub fn json_exists(self, path: impl Into<String>) -> Condition {
        Condition::JsonExists(self, path.into())
    }

    /// `col @> value` (JSONB containment: does col contain this subset?)
    pub fn json_contains(self, v: serde_json::Value) -> Condition {
        Condition::JsonContains(self, Value::Json(v.to_string()))
    }

    /// `jsonb_path_match(col, path, vars)` (PostgreSQL 12+ JSONB path query)
    pub fn jsonb_path_match(self, path: impl Into<String>, vars: serde_json::Value) -> Condition {
        Condition::JsonbPathMatch(self, path.into(), Value::Json(vars.to_string()))
    }
}

/// Builder for `col->path` comparisons (returned by `json_get`).
pub struct JsonPathBuilder {
    column: ColumnRef,
    path: String,
}

impl JsonPathBuilder {
    /// `col->path = value`
    pub fn eq<V: Into<Value>>(self, v: V) -> Condition {
        Condition::JsonGetEq(self.column, self.path, v.into())
    }
    /// `col->path != value`
    pub fn ne<V: Into<Value>>(self, v: V) -> Condition {
        Condition::Not(Box::new(Condition::JsonGetEq(
            self.column,
            self.path,
            v.into(),
        )))
    }
}

// ── Into<Value> impls for the common Rust types ──

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::I32(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::I64(v)
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::F64(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.to_owned())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}
impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(v)
    }
}
impl From<uuid::Uuid> for Value {
    fn from(v: uuid::Uuid) -> Self {
        Value::Uuid(v.to_string())
    }
}

impl From<&String> for Value {
    fn from(v: &String) -> Self {
        Value::Text(v.clone())
    }
}
impl From<&i64> for Value {
    fn from(v: &i64) -> Self {
        Value::I64(*v)
    }
}
impl From<&i32> for Value {
    fn from(v: &i32) -> Self {
        Value::I32(*v)
    }
}
impl From<&bool> for Value {
    fn from(v: &bool) -> Self {
        Value::Bool(*v)
    }
}
impl From<&f64> for Value {
    fn from(v: &f64) -> Self {
        Value::F64(*v)
    }
}
