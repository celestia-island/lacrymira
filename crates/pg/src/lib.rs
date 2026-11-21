//! lacrymira-pg: PostgreSQL driver for lacrymira.
//!
//! Host-side only (does not compile to WASI). Renders `Command` to PostgreSQL
//! SQL and executes via sqlx.

pub mod drift;
pub mod exec;
pub mod migrate;
pub mod render;

pub use drift::{check_drift, parse_pg_index, DbIndex, IndexDrift};
pub use exec::PgExecutor;
pub use migrate::{Migration, MigrationResult, MigrationRunner};
pub use render::{render, Rendered};
