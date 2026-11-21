//! The serializable command surface — the contract between the core and all drivers.

use serde::{Deserialize, Serialize};

use crate::condition::Condition;
use crate::value::Value;

/// The SQL dialect for `Command::Sql`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dialect {
    /// PostgreSQL (including TiDB via the MySQL protocol is a separate variant).
    Postgres,
    /// MySQL / TiDB (they share the wire protocol).
    MySql,
    /// SQLite.
    Sqlite,
}

/// The KV operation for `Command::Kv`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KvOp {
    /// Get a single value by key.
    Get {
        /// The key to read.
        key: Value,
    },
    /// Set a value, overwriting any existing value.
    Set {
        /// The key to write.
        key: Value,
        /// The value to store.
        value: Value,
    },
    /// Delete a key.
    Delete {
        /// The key to remove.
        key: Value,
    },
    /// Scan a range of keys.
    Scan {
        /// Scan start key (inclusive); `None` = from the beginning.
        start: Option<Value>,
        /// Scan end key (exclusive); `None` = to the end.
        end: Option<Value>,
        /// Maximum number of entries to return.
        limit: Option<u64>,
    },
    /// Count entries in a range.
    Count {
        /// Count start key (inclusive); `None` = from the beginning.
        start: Option<Value>,
        /// Count end key (exclusive); `None` = to the end.
        end: Option<Value>,
    },
}

/// A single database command, serializable to msgpack.
///
/// This is the **frozen contract** (M0): once published, adding a variant
/// is a breaking change to the wire format. The msgpack encoding is pinned
/// by the round-trip contract test in `tests/contract.rs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Execute raw SQL with bound parameters.
    Sql {
        /// The SQL dialect.
        dialect: Dialect,
        /// The SQL text (with $1, $2, ... placeholders for PostgreSQL).
        text: String,
        /// Bound parameter values.
        params: Vec<Value>,
    },
    /// Execute a typed SELECT on an entity.
    Select {
        /// The table name.
        table: String,
        /// Optional filter condition.
        filter: Option<Condition>,
        /// ORDER BY column + direction pairs.
        order_by: Vec<(String, bool)>, // (column, is_desc)
        /// LIMIT.
        limit: Option<u64>,
        /// OFFSET.
        offset: Option<u64>,
        /// Optional pessimistic lock (FOR UPDATE family).
        lock: Option<crate::lock::LockMode>,
    },
    /// Count matching rows.
    Count {
        /// The table name.
        table: String,
        /// Optional filter condition.
        filter: Option<Condition>,
    },
    /// Insert rows into a table.
    Insert {
        /// The table name.
        table: String,
        /// Column names in order.
        columns: Vec<String>,
        /// Rows of values (each row has the same length as `columns`).
        rows: Vec<Vec<Value>>,
        /// Optional ON CONFLICT clause.
        on_conflict: Option<OnConflict>,
    },
    /// Update rows matching a condition.
    Update {
        /// The table name.
        table: String,
        /// SET assignments.
        sets: Vec<(String, Value)>,
        /// Optional filter condition.
        filter: Option<Condition>,
    },
    /// Delete rows matching a condition.
    Delete {
        /// The table name.
        table: String,
        /// Optional filter condition.
        filter: Option<Condition>,
    },
    /// KV operation.
    Kv {
        /// The KV namespace/table.
        namespace: String,
        /// The operation.
        op: KvOp,
    },
}

/// The ON CONFLICT clause for upsert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OnConflict {
    /// The conflict-target columns.
    pub target: Vec<String>,
    /// What to do on conflict.
    pub action: ConflictAction,
}

/// What to do when a conflict is detected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConflictAction {
    /// Do nothing (skip the row).
    DoNothing,
    /// Update the listed columns from EXCLUDED.
    /// Update the conflicting row with the given column-value pairs.
    DoUpdate {
        /// The column-value pairs to set on conflict.
        set: Vec<(String, Value)>,
    },
}
