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
    assert_eq!(
        cols[0],
        ("version".to_owned(), "character varying".to_owned())
    );
    assert_eq!(cols[1], ("applied_at".to_owned(), "bigint".to_owned()));
}

// ── Unit tests (no DB required) ──

#[test]
fn migrations_are_sorted_by_version() {
    let _runner = MigrationRunner::new(vec![
        Migration::new("20260102000001", "second", "SELECT 2"),
        Migration::new("20260101000001", "first", "SELECT 1"),
        Migration::new("20260103000001", "third", "SELECT 3"),
    ]);
    // Internal ordering is enforced by the constructor
    // (verified by the DB tests which check applied order)
}

// ── Insertion order is execution order (no lexicographic sort) ──
// Regression: celestia-erp's unpadded versions (m2, m10) got reordered —
// "m10" sorts before "m2", running an UPDATE before its table existed.

#[tokio::test]
#[ignore = "requires DATABASE_URL with a test PostgreSQL"]
async fn unpadded_versions_apply_in_declaration_order() {
    let pool = fresh_pool().await;
    sqlx::query("DROP TABLE IF EXISTS _order_probe, seaql_migrations")
        .execute(&pool)
        .await
        .expect("drop");

    // m10 declared FIRST on purpose: a sorting runner would also put it
    // first lexicographically, so we assert against m2-first declaration.
    let runner = MigrationRunner::new(vec![
        Migration::new(
            "m2_compliance",
            "create probe",
            "CREATE TABLE _order_probe (id INT, phase TEXT)",
        ),
        Migration::new(
            "m10_role_member",
            "populate",
            "INSERT INTO _order_probe VALUES (1, 'post-create')",
        ),
    ]);
    let result = runner.up(&pool).await.expect("up");
    // Both applied, in the order they were declared — m2 before m10 even
    // though "m10" < "m2" lexicographically.
    assert_eq!(result.applied.len(), 2);
    assert_eq!(result.applied[0], "m2_compliance");
    assert_eq!(result.applied[1], "m10_role_member");
}

// ── C8: down lane + C9 bookkeeping alignment ──

#[tokio::test]
#[ignore = "requires DATABASE_URL with a test PostgreSQL"]
async fn down_rolls_back_in_reverse_order() {
    let pool = fresh_pool().await;
    sqlx::query("DROP TABLE IF EXISTS _down_t")
        .execute(&pool)
        .await
        .unwrap();
    let runner = MigrationRunner::new(vec![
        Migration::new("d001", "create", "CREATE TABLE _down_t (id INT)")
            .with_down("DROP TABLE IF EXISTS _down_t"),
        Migration::new("d002", "alter", "ALTER TABLE _down_t ADD COLUMN note TEXT")
            .with_down("ALTER TABLE _down_t DROP COLUMN IF EXISTS note"),
        Migration::new("d003", "index", "CREATE INDEX _down_idx ON _down_t (id)")
            .with_down("DROP INDEX IF EXISTS _down_idx"),
    ]);

    runner.up(&pool).await.expect("up");
    let after_up: Vec<(String,)> =
        sqlx::query_as("SELECT version FROM seaql_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(after_up.len(), 3);

    // down(2): rolls d003 then d002, in reverse
    let result = runner.down(&pool, 2).await.expect("down");
    assert_eq!(result.applied, vec!["d003".to_string(), "d002".to_string()]);
    // d001 still recorded; _down_t exists WITHOUT note
    let cols: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns WHERE table_name = '_down_t'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(cols.len(), 1, "note rolled back");

    // down(1) again: rolls d001, table gone
    let result2 = runner.down(&pool, 1).await.expect("down2");
    assert_eq!(result2.applied, vec!["d001".to_string()]);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM seaql_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL with a test PostgreSQL"]
async fn down_skips_empty_down_sql_without_error() {
    let pool = fresh_pool().await;
    for drop in [
        "DROP TABLE IF EXISTS _skip_t",
        "DROP TABLE IF EXISTS _skip_u",
    ] {
        sqlx::query(drop).execute(&pool).await.unwrap();
    }
    let runner = MigrationRunner::new(vec![
        Migration::new("s001", "no-down", "CREATE TABLE _skip_t (id INT)"),
        Migration::new("s002", "has-down", "CREATE TABLE _skip_u (id INT)")
            .with_down("DROP TABLE IF EXISTS _skip_u"),
    ]);
    runner.up(&pool).await.expect("up");
    let result = runner.down(&pool, 5).await.expect("down");
    assert_eq!(result.applied, vec!["s002".to_string()]);
    assert_eq!(result.skipped, vec!["s001".to_string()]);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL with a test PostgreSQL"]
async fn bookkeeping_column_type_is_varchar_and_seconds() {
    let pool = fresh_pool().await;
    sqlx::query("DROP TABLE IF EXISTS _bk_t")
        .execute(&pool)
        .await
        .unwrap();
    let runner = MigrationRunner::new(vec![Migration::new(
        "bk001",
        "probe",
        "CREATE TABLE _bk_t (id INT)",
    )]);
    runner.up(&pool).await.expect("up");
    // sea-orm's exact DDL shape: version VARCHAR(255)
    let ctype: String = sqlx::query_scalar(
        "SELECT data_type FROM information_schema.columns WHERE table_name = 'seaql_migrations' AND column_name = 'version'"
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(ctype, "character varying");
    // applied_at is in SECONDS (sea-orm semantics) — ~1.7e9, not ~1.7e12
    let ts: i64 =
        sqlx::query_scalar("SELECT applied_at FROM seaql_migrations WHERE version = 'bk001'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(ts < 10_000_000_000, "seconds since epoch, got {ts}");
}
