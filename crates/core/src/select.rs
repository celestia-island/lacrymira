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
            _marker: std::marker::PhantomData,
        }
    }

    /// Add a filter condition (AND-combined with any existing filter).
    ///
    /// Consumes and returns `Self` for chaining, but the value is the **same
    /// type** — so `q = q.filter(...)` inside an `if` block compiles.
    pub fn filter(mut self, cond: Condition) -> Self {
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
        }
    }
}

impl<E: Entity> Default for Select<E> {
    fn default() -> Self {
        Self::new()
    }
}
