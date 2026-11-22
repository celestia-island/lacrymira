//! The typed query builder — single-type, runtime-composable.

use crate::condition::Condition;
use crate::entity::Entity;

/// A typed SELECT query on an entity.
///
/// **Deliberately a single concrete type** (not typestate): `Select<E>` can be
/// assigned, passed, stored, and conditionally extended. This is the diesel
/// lesson — see `tests/composition_pin.rs`.
pub struct Select<E: Entity> {
    table: String,
    filter: Option<Condition>,
    orders: Vec<(String, bool)>, // (column, is_desc)
    limit: Option<u64>,
    offset: Option<u64>,
    lock_mode: Option<crate::lock::LockMode>,
    distinct: bool,
    _marker: std::marker::PhantomData<fn() -> E>,
}

impl<E: Entity> Select<E> {
    /// Start a new select on this entity's table.
    pub fn new() -> Self {
        Select {
            table: E::TABLE.to_owned(),
            filter: None,
            orders: Vec::new(),
            limit: None,
            offset: None,
            lock_mode: None,
            distinct: false,
            _marker: std::marker::PhantomData,
        }
    }

    /// Add a filter condition (AND-combined with any existing filter).
    ///
    /// Consumes and returns `Self` for chaining, but the value is the **same
    /// type** — so `q = q.filter(...)` inside an `if` block compiles.
    /// Add a filter condition (AND-combined with any existing filter).
    ///
    /// Accepts `TypedCondition<E>` (from Column enum) or `Condition` (escape hatch).
    /// Column from a different Entity will not compile (type mismatch).
    pub fn filter(mut self, cond: impl Into<crate::typed_condition::TypedCondition<E>>) -> Self {
        let typed: crate::typed_condition::TypedCondition<E> = cond.into();
        let cond = typed.into_inner();
        self.filter = Some(match self.filter.take() {
            Some(existing) => {
                // Flatten All([...]) chains so .filter(a).filter(b).filter(c)
                // produces All([a, b, c]) rather than nested All(All(a,b), c).
                match (existing, cond) {
                    (Condition::All(mut left), Condition::All(right)) => {
                        left.extend(right);
                        Condition::All(left)
                    }
                    (Condition::All(mut left), other) => {
                        left.push(other);
                        Condition::All(left)
                    }
                    (other, Condition::All(mut right)) => {
                        right.insert(0, other);
                        Condition::All(right)
                    }
                    (a, b) => Condition::All(vec![a, b]),
                }
            }
            None => cond,
        });
        self
    }

    /// Add an ORDER BY column (ascending).
    pub fn order_by(mut self, column: impl Into<String>) -> Self {
        self.orders.push((column.into(), false));
        self
    }

    /// Add an ORDER BY column (descending).
    pub fn order_by_desc(mut self, column: impl Into<String>) -> Self {
        self.orders.push((column.into(), true));
        self
    }

    /// Set the LIMIT.
    pub fn limit(mut self, n: u64) -> Self {
        self.limit = Some(n);
        self
    }

    /// Set the OFFSET.
    pub fn offset(mut self, n: u64) -> Self {
        self.offset = Some(n);
        self
    }

    /// ORDER BY a typed column (the entity's own Column enum — rename-safe).
    pub fn order_by_typed<C: crate::typed_condition::TypedColumnName>(mut self, col: C) -> Self {
        self.orders.push((col.col_name().to_string(), false));
        self
    }

    /// ORDER BY a typed column, descending.
    pub fn order_by_typed_desc<C: crate::typed_condition::TypedColumnName>(
        mut self,
        col: C,
    ) -> Self {
        self.orders.push((col.col_name().to_string(), true));
        self
    }

    /// Mark this SELECT as DISTINCT.
    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    /// Set a pessimistic lock mode (FOR UPDATE family).
    pub fn lock(mut self, mode: crate::lock::LockMode) -> Self {
        self.lock_mode = Some(mode);
        self
    }

    /// Render to a `Command::Select`.
    pub fn to_command(&self) -> crate::command::Command {
        crate::command::Command::Select {
            table: self.table.clone(),
            filter: self.filter.clone(),
            order_by: self.orders.clone(),
            limit: self.limit,
            offset: self.offset,
            lock: self.lock_mode,
            distinct: self.distinct,
        }
    }

    /// A copy of this builder with only the filter carried over
    /// (pagination composes: fresh limit/offset on top).
    pub fn clone_limit(&self) -> Self {
        Self {
            table: self.table.clone(),
            filter: self.filter.clone(),
            orders: Vec::new(),
            limit: None,
            offset: None,
            lock_mode: None,
            distinct: false,
            _marker: std::marker::PhantomData,
        }
    }

    /// The accumulated filter (for COUNT companions).
    pub fn filter_clone(&self) -> Option<Condition> {
        self.filter.clone()
    }

    /// Convert this Select's filter/orders into an aggregating Command.
    ///
    /// The projection, GROUP BY and HAVING come from the caller; the WHERE
    /// and ORDER BY/LIMIT from this builder's accumulated state.
    pub fn to_aggregate(
        &self,
        select_expr: Vec<(crate::command::AggExpr, Option<String>)>,
        group_by: Vec<String>,
        having: Option<Condition>,
    ) -> crate::command::Command {
        crate::command::Command::Aggregate {
            table: self.table.clone(),
            select_expr,
            group_by,
            having,
            filter: self.filter.clone(),
            order_by: self.orders.clone(),
            limit: self.limit,
        }
    }
}

impl<E: Entity> Default for Select<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// One page of offset-paginated results.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    /// Rows on this page.
    pub rows: Vec<T>,
    /// Total matching rows (from COUNT).
    pub total: i64,
    /// 1-based page number.
    pub page: u64,
    /// Total page count (ceil).
    pub pages: u64,
}

/// One batch of keyset-paginated results.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorBatch<T> {
    /// Rows in this batch, ordered by the cursor key.
    pub rows: Vec<T>,
    /// The next cursor value (None = exhausted).
    pub next: Option<crate::value::Value>,
}
