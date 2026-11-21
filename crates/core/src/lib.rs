//! lacrymira-core: the backend-neutral data access core.
//!
//! This crate has **zero tokio dependency** and compiles to `wasm32-wasip2`.
//! All I/O lives in the driver crates (`lacrymira-pg`, `lacrymira-redb`, …);
//! the core only constructs and renders commands.
//!
//! The `Command`/`Reply` pair is the **frozen wire contract** — see
//! `tests/contract.rs` for the msgpack round-trip pin.

pub mod command;
pub mod condition;
pub mod entity;
pub mod error;
pub mod field;
pub mod reply;
pub mod select;
pub mod value;

pub use command::{Command, ConflictAction, Dialect, KvOp, OnConflict};
pub use condition::{ColumnRef, Condition};
pub use entity::{ColumnTrait, Entity, PrimaryKey};
pub use error::Error;
pub use field::Field;
pub use reply::Reply;
pub use select::Select;
pub use value::Value;
