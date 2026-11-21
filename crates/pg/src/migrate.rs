//! The migration runner — compatible with the existing `seaql_migrations` table.
//!
//! ## Why this is the highest-risk piece
//!
//! sea-orm's `Migrator::up` refuses to run when a recorded version has no
//! registered migration file. This family has production databases with
//! recorded histories that go back years, and has already been permanently
//! wedged once by this behavior (chest `legacy_history.rs`). Our runner:
//!
//! 1. Reads the **same** `seaql_migrations(version TEXT PK, applied_at BIGINT)` table.
//! 2. Treats recorded versions as "already applied" — **even if we don't have
//!    a file for them** (they are pre-history / baseline).
//! 3. Only executes migrations whose version is NOT already recorded.
//! 4. Records newly applied migrations in the same table, same format.
//!
//! This means it can be dropped into a database that sea-orm has been
//! migrating, and it will pick up exactly where sea-orm left off.

use lacrymira_core::Error;

/// A single migration.
#[derive(Debug, Clone)]
pub struct Migration {
    /// The version string (must be unique, e.g. "20261121000001").
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// The SQL to execute (multiple statements separated by `;`).
    pub sql: String,
}

impl Migration {
    /// Create a new migration from version, description, and SQL.
    pub fn new(
        version: impl Into<String>,
        description: impl Into<String>,
        sql: impl Into<String>,
    ) -> Self {
        Migration {
            version: version.into(),
            description: description.into(),
            sql: sql.into(),
        }
    }
}

/// The migration runner. Compatible with `seaql_migrations` bookkeeping.
pub struct MigrationRunner {
    migrations: Vec<Migration>,
}

/// The result of running migrations.
#[derive(Debug, Clone, PartialEq)]
pub struct MigrationResult {
    /// Versions that were applied in this run.
    pub applied: Vec<String>,
    /// Versions that were already recorded (skipped).
    pub skipped: Vec<String>,
    /// Recorded versions that have no matching migration file (pre-history).
    pub pre_history: Vec<String>,
}

impl MigrationRunner {
    /// Create a runner with the given migrations (will be sorted by version).
    pub fn new(mut migrations: Vec<Migration>) -> Self {
        migrations.sort_by(|a, b| a.version.cmp(&b.version));
        MigrationRunner { migrations }
    }

    /// Create a runner from a single baseline SQL dump.
    ///
    /// Use this when adopting an existing database: `pg_dump --schema-only`
    /// produces the baseline; all previously-applied migrations become
    /// pre-history (recorded in the table but with no file here).
    pub fn baseline(version: impl Into<String>, sql: impl Into<String>) -> Self {
        MigrationRunner::new(vec![Migration::new(version, "baseline schema", sql)])
    }

    /// Run all pending migrations against the given pool.
    pub async fn up(&self, pool: &sqlx::PgPool) -> Result<MigrationResult, Error> {
        // 1. Ensure the bookkeeping table exists (same shape as sea-orm)
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS seaql_migrations (
                version TEXT PRIMARY KEY,
                applied_at BIGINT NOT NULL
            )",
        )
        .execute(pool)
        .await
        .map_err(map_sqlx)?;

        // 2. Read all recorded versions
        let recorded: Vec<(String,)> =
            sqlx::query_as("SELECT version FROM seaql_migrations ORDER BY version")
                .fetch_all(pool)
                .await
                .map_err(map_sqlx)?;

        let recorded_set: std::collections::HashSet<String> =
            recorded.into_iter().map(|(v,)| v).collect();

        // 3. Identify pre-history (recorded but no file)
        let our_versions: std::collections::HashSet<&str> =
            self.migrations.iter().map(|m| m.version.as_str()).collect();
        let pre_history: Vec<String> = recorded_set
            .iter()
            .filter(|v| !our_versions.contains(v.as_str()))
            .cloned()
            .collect();

        // 4. Execute pending migrations
        let mut applied = Vec::new();
        let mut skipped = Vec::new();

        for migration in &self.migrations {
            if recorded_set.contains(&migration.version) {
                skipped.push(migration.version.clone());
                continue;
            }

            // Execute the migration SQL in a transaction
            let mut tx = pool.begin().await.map_err(map_sqlx)?;
            sqlx::raw_sql(sqlx::AssertSqlSafe(migration.sql.as_str()))
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;

            // Record it
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            sqlx::query("INSERT INTO seaql_migrations (version, applied_at) VALUES ($1, $2)")
                .bind(&migration.version)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;

            tx.commit().await.map_err(map_sqlx)?;
            applied.push(migration.version.clone());
        }

        Ok(MigrationResult {
            applied,
            skipped,
            pre_history,
        })
    }

    /// List migration status without executing anything.
    pub async fn status(&self, pool: &sqlx::PgPool) -> Result<MigrationResult, Error> {
        let recorded: Vec<(String,)> =
            sqlx::query_as("SELECT version FROM seaql_migrations ORDER BY version")
                .fetch_all(pool)
                .await
                .map_err(map_sqlx)?;

        let recorded_set: std::collections::HashSet<String> =
            recorded.into_iter().map(|(v,)| v).collect();

        let our_versions: std::collections::HashSet<&str> =
            self.migrations.iter().map(|m| m.version.as_str()).collect();

        let pre_history: Vec<String> = recorded_set
            .iter()
            .filter(|v| !our_versions.contains(v.as_str()))
            .cloned()
            .collect();

        let applied: Vec<String> = self
            .migrations
            .iter()
            .filter(|m| recorded_set.contains(&m.version))
            .map(|m| m.version.clone())
            .collect();

        let skipped = applied.clone(); // same set for status

        // pending = our migrations not in recorded set
        // (reported as "applied" for interface compat; caller checks pre_history)
        Ok(MigrationResult {
            applied,
            skipped,
            pre_history,
        })
    }
}

fn map_sqlx(e: sqlx::Error) -> Error {
    let sqlstate = match &e {
        sqlx::Error::Database(db_err) => db_err.code().map(|c| c.to_string()),
        _ => None,
    };
    Error::Db {
        kind: "postgres".into(),
        message: e.to_string(),
        sqlstate,
    }
}
