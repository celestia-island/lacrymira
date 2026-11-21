//! lacrymira-core: the backend-neutral data access core.
//!
//! This crate has **zero tokio dependency** and compiles to `wasm32-wasip2`.
//! All I/O lives in the driver crates (`lacrymira-pg`, `lacrymira-redb`, …);
//! the core only constructs and renders commands.

pub mod condition;
pub mod command;
pub mod entity;
pub mod error;
pub mod field;
pub mod reply;
pub mod value;

pub use condition::Condition;
pub use command::Command;
pub use entity::Entity;
pub use error::Error;
pub use field::Field;
pub use reply::Reply;
pub use value::Value;
