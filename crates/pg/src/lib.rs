//! lacrymira-pg: PostgreSQL driver for lacrymira.
//!
//! Host-side only (does not compile to WASI). Renders `Command` to PostgreSQL
//! SQL and executes via sqlx.

pub mod exec;
pub mod render;

pub use render::{render, Rendered};
