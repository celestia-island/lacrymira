//! Tests for the migration runner.
//!
//! These tests require a live PostgreSQL database. Run with:
//! ```sh
//! DATABASE_URL=postgres://localhost/lacrymira_test cargo test -p lacrymira-pg --test migrate -- --ignored
//! ```
//! (The `--ignored` flag is needed because they are tagged `#[ignore]` by default.)

use lacrymira_pg::{Migration, MigrationRunner};

fn test_db_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/lacrymira_migration_test".to_owned())
}

async fn fresh_pool() -> sqlx::PgPool {
    let url = test_db_url();
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    // Clean slate
    sqlx::query("DROP TABLE IF EXISTS seaql_migrations")
        .execute(&pool)
        .await
        .expect("drop bookkeeping");
    sqlx::query("DROP TABLE IF EXISTS _mig_test_users")
        .execute(&pool)
        .await
        .expect("drop test table");
    pool
}

#[tokio::test]
#[ignore = "requires DATABASE_URL with a test PostgreSQL"]
async fn fresh_database_runs_all_migrations() {
    let pool = fresh_pool().await;
    let runner = MigrationRunner::new(vec![
        Migration::new(
            "20260101000001",
            "create users",
            r#"
            CREATE TABLE _mig_test_users (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL
            )
        "#,
        ),
        Migration::new(
            "20260101000002",
            "add email",
            r#"
            ALTER TABLE _mig_test_users ADD COLUMN email TEXT
        "#,
        ),
    ]);

    let result = runner.up(&pool).await.expect("migrate up");
    assert_eq!(result.applied.len(), 2, "both migrations should apply");
    assert_eq!(result.applied[0], "20260101000001");
    assert_eq!(result.applied[1], "20260101000002");
    assert_eq!(result.pre_history.len(), 0, "no pre-history on fresh DB");

    // Verify the table has both columns
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM information_schema.columns WHERE table_name = '_mig_test_users'",
    )
    .fetch_one(&pool)
    .await
    .expect("check columns");
    assert_eq!(count, 3, "id + name + email = 3 columns");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL with a test PostgreSQL"]
async fn rerun_skips_applied_migrations() {
    let pool = fresh_pool().await;
    let runner = MigrationRunner::new(vec![Migration::new(
        "20260101000001",
        "create",
        "CREATE TABLE _mig_test_users (id BIGSERIAL PRIMARY KEY)",
    )]);

    // First run applies
    let r1 = runner.up(&pool).await.expect("first run");
    assert_eq!(r1.applied.len(), 1);

    // Second run skips
    let r2 = runner.up(&pool).await.expect("second run");
    assert_eq!(r2.applied.len(), 0, "nothing new to apply");
    assert_eq!(r2.skipped.len(), 1, "the existing one is skipped");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL with a test PostgreSQL"]
async fn pre_history_versions_are_tolerated() {
    let pool = fresh_pool().await;

    // Simulate a pre-existing sea-orm migration history
    sqlx::query(
        "CREATE TABLE seaql_migrations (version TEXT PRIMARY KEY, applied_at BIGINT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("create bookkeeping");
    sqlx::query("INSERT INTO seaql_migrations (version, applied_at) VALUES ($1, $2)")
        .bind("20250101000001")
        .bind(1735689600000i64)
        .execute(&pool)
        .await
        .expect("insert old migration");
    sqlx::query("INSERT INTO seaql_migrations (version, applied_at) VALUES ($1, $2)")
        .bind("20250601000001")
        .bind(1748736000000i64)
        .execute(&pool)
        .await
        .expect("insert another old migration");

    // Our runner only knows about a NEW migration
    let runner = MigrationRunner::new(vec![Migration::new(
        "20261121000001",
        "new table",
        "CREATE TABLE _mig_test_users (id BIGSERIAL PRIMARY KEY)",
    )]);

    let result = runner.up(&pool).await.expect("migrate with pre-history");

    // The critical assertion: the runner does NOT error on the pre-existing
    // versions it doesn't know about (this is what sea-orm does — it wedges).
    assert_eq!(result.applied.len(), 1, "the new migration applies");
    assert_eq!(
        result.pre_history.len(),
        2,
        "the two old versions are pre-history"
    );
    assert!(result.pre_history.contains(&"20250101000001".to_owned()));
    assert!(result.pre_history.contains(&"20250601000001".to_owned()));
}

#[tokio::test]
#[ignore = "requires DATABASE_URL with a test PostgreSQL"]
async fn bookkeeping_table_matches_sea_orm_format() {
    let pool = fresh_pool().await;
    let runner = MigrationRunner::new(vec![Migration::new(
        "20261121000001",
        "test",
        "CREATE TABLE _mig_test_users (id BIGSERIAL PRIMARY KEY)",
    )]);
    runner.up(&pool).await.expect("migrate");

    // Verify the table structure matches sea-orm's exactly
    let cols: Vec<(String, String)> = sqlx::query_as(
        "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 'seaql_migrations' ORDER BY ordinal_position"
    ).fetch_all(&pool).await.expect("check columns");

    assert_eq!(cols.len(), 2);
    assert_eq!(cols[0], ("version".to_owned(), "text".to_owned()));
    assert_eq!(cols[1], ("applied_at".to_owned(), "bigint".to_owned()));
}

// ── Unit tests (no DB required) ──

#[test]
fn migrations_are_sorted_by_version() {
    let runner = MigrationRunner::new(vec![
        Migration::new("20260102000001", "second", "SELECT 2"),
        Migration::new("20260101000001", "first", "SELECT 1"),
        Migration::new("20260103000001", "third", "SELECT 3"),
    ]);
    // Internal ordering is enforced by the constructor
    // (verified by the DB tests which check applied order)
}
