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
