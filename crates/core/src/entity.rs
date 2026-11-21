//! The entity trait: table metadata for typed CRUD.

/// A column reference within an entity.
pub trait ColumnTrait {
    /// The entity this column belongs to.
    type Entity: Entity;
}

/// The primary-key shape for an entity.
pub trait PrimaryKey {
    /// The Rust type of the PK (e.g. `i64`, `Uuid`, `(String, i64)`).
    type Value: Clone + PartialEq + std::fmt::Debug;
}

/// The entity trait: maps a Rust `Model` struct to a database table.
pub trait Entity: Sized + Send + Sync + 'static {
    /// The table name (without schema prefix).
    const TABLE: &'static str;

    /// The schema name (default "public").
    const SCHEMA: &'static str = "public";

    /// The number of columns.
    const COLUMN_COUNT: usize;

    /// All column names in declaration order.
    const COLUMNS: &'static [&'static str];

    /// The primary-key column indices into `COLUMNS` (empty = no PK).
    const PK_INDICES: &'static [usize];

    /// The read model type (one row).
    type Model: Clone + Send + Sync + 'static;

    /// The partial-update type.
    type ActiveModel: Clone + Send + Sync + 'static;

    /// The primary-key type.
    type PrimaryKey: PrimaryKey;
}
