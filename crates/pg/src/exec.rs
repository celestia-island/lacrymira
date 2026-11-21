//! The PostgreSQL executor — connects sqlx PgPool to lacrymira Command.
//!
//! M3 continuation: the actual sqlx integration.

// This module requires the `postgres` feature.
#[cfg(feature = "postgres")]
pub mod imp {
    // sqlx executor implementation goes here (M3 continuation).
}
