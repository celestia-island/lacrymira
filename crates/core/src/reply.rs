//! The reply type — what a command produces.

use serde::{Deserialize, Serialize};

use crate::value::Value;

/// The result of executing a `Command`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Reply {
    /// Rows returned (each row is a list of column values).
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
    /// Number of affected rows.
    Affected(u64),
    /// A single KV value.
    Value(Option<Value>),
    /// A range of KV entries.
    Entries(Vec<(Value, Value)>),
    /// No output.
    Unit,
}

impl Reply {
    /// Returns the number of rows if this is a `Rows` reply.
    pub fn row_count(&self) -> Option<usize> {
        match self {
            Reply::Rows { rows, .. } => Some(rows.len()),
            _ => None,
        }
    }
}
