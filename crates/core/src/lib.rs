//! # lacrymira
//!
//! The primary crate of the lacrymira data access layer: all core types
//! plus the [`DeriveEntity`] macro re-export. Drivers are separate crates
//! (`lacrymira-pg` / `lacrymira-redb` / `lacrymira-wasi`).
//!
//! The core types of the lacrymira data access layer: `Entity`, `Condition`,
//! `Command` / `Reply`, `Value`, `Error`, `Select`, `TypedCondition`,
//! `LockMode`, `InsertPolicy`, and `IndexBuilder`.
//!
//! This crate is **WASI-compatible**: zero tokio, compiles to
//! `wasm32-wasip2`. Drivers live in `lacrymira-pg` (host-side) and
//! `lacrymira-redb` (WASI-compatible).
//!
//! ## Design invariants
//!
//! - The `Command` / `Reply` msgpack wire format is frozen by contract tests.
//! - `Select<E>` is a single concrete type (not typestate) — dynamic
//!   composition (`let mut q = ...; if c { q = q.filter(..); }`) compiles.
//! - `TypedCondition<E>` constrains filters to E's own Column enum;
//!   cross-Entity conditions are a compile error.

pub mod command;
pub mod condition;
pub mod entity;
pub mod error;
pub mod field;
pub mod index;
pub mod insert_policy;
pub mod lock;
pub mod reply;
pub mod select;
pub mod typed_condition;
pub mod value;

pub use command::{AggExpr, Command, ConflictAction, Dialect, KvOp, OnConflict};
pub use condition::{ColumnRef, Condition};
pub use entity::{ColumnTrait, Entity, JsonColumn, PrimaryKey, VersionColumn};
pub use error::Error;
pub use field::Field;
pub use index::{IndexBuilder, IndexSpec};
pub use insert_policy::{CollisionPolicy, InsertPolicy, UuidStrategy};
pub use lock::LockMode;
pub use reply::Reply;
pub use select::{CursorBatch, Page, Select};
pub use typed_condition::{
    JsonPathTyped, TypedColumnName, TypedColumnOps, TypedCondition, TypedJsonColumnOps,
};
pub use value::Value;

// ── Derive macro（用户不再需要单独依赖 lacrymira-macros）──
pub use lacrymira_macros::Entity as DeriveEntity;

/// Everything a typical consumer needs in ONE import.
///
/// ```ignore
/// use lacrymira::prelude::*;
/// // Entity, Select, Condition, ColumnRef, Value, Command, Reply,
/// // TypedColumnOps (the .eq/.gt/... operators), LockMode, Page,
/// // CursorBatch, AggExpr, Field, InsertPolicy, IndexBuilder — all in.
/// ```
pub mod prelude {
    pub use crate::command::{AggExpr, Command, ConflictAction, Dialect, KvOp, OnConflict};
    pub use crate::condition::{ColumnRef, Condition, JsonPathBuilder};
    pub use crate::entity::{ColumnTrait, Entity, JsonColumn, PrimaryKey, VersionColumn};
    pub use crate::error::Error;
    pub use crate::field::Field;
    pub use crate::index::{IndexBuilder, IndexSpec};
    pub use crate::insert_policy::{CollisionPolicy, InsertPolicy, UuidStrategy};
    pub use crate::lock::LockMode;
    pub use crate::reply::Reply;
    pub use crate::select::{CursorBatch, Page, Select};
    pub use crate::typed_condition::{
        JsonPathTyped, TypedColumnName, TypedColumnOps, TypedCondition, TypedJsonColumnOps,
    };
    pub use crate::value::Value;
    pub use crate::DeriveEntity;
}
