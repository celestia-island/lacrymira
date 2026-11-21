//! The entity trait and column marker traits.

/// Marker trait: this column holds JSON data (`serde_json::Value`).
/// Only columns with this marker have `json_get`/`json_exists` etc.
pub trait JsonColumn {
    #[doc(hidden)]
    fn _json_marker() {}
}

/// Marker trait: this column is a version counter for optimistic locking.
pub trait VersionColumn {
    #[doc(hidden)]
    fn _version_marker() {}
}

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

    /// The index into `COLUMNS` of the `#[lacrymira(version)]` column, if any.
    ///
    /// Drives optimistic locking: [`save`](crate::Entity) uses this to build
    /// the `WHERE version = $expected` guard without guessing column names.
    const VERSION_INDEX: Option<usize> = None;

    /// The read model type (one row).
    type Model: Clone + Send + Sync + 'static;

    /// The partial-update type.
    type ActiveModel: Clone + Send + Sync + 'static;

    /// The primary-key type.
    type PrimaryKey: PrimaryKey;

    /// Convert a Model instance into a Vec of column values (declaration order).
    fn to_values(model: &Self::Model) -> Vec<crate::value::Value>;

    /// Reconstruct a Model from a Vec of column values (declaration order).
    /// Returns None if the values don't match the expected shape.
    fn from_values(values: &[crate::value::Value]) -> Option<Self::Model>;
}
