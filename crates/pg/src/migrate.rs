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

use std::pin::Pin;

use lacrymira::Error;

/// A boxed async migration body: runs inside the migration transaction.
///
/// Receives the transaction so data migrations do their work in the same
/// atomic scope as the bookkeeping row (a failure rolls both back).
pub type MigrationFuture<'a> =
    Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>>;

/// A Rust data-migration step (the "code lane").
///
/// Some migrations cannot be expressed as static SQL — they read rows and
/// write derived ones, or branch on what they find. This is the door for
/// those: a closure that receives the open transaction.
pub type MigrationFn =
    std::sync::Arc<dyn for<'a> Fn(&'a mut sqlx::PgConnection) -> MigrationFuture<'a> + Send + Sync>;

/// A single migration.
#[derive(Clone)]
pub struct Migration {
    /// The version string (must be unique, e.g. "20261121000001").
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// The SQL to execute (multiple statements separated by `;`).
    pub sql: String,
    /// The roll-back SQL. Executed by [`MigrationRunner::down`]; empty
    /// means "no down lane" and `down` skips it.
    pub down_sql: String,
    /// Optional Rust body (the code lane). Runs BEFORE `sql` inside the
    /// same transaction.
    pub code: Option<MigrationFn>,
}

impl std::fmt::Debug for Migration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Migration")
            .field("version", &self.version)
            .field("description", &self.description)
            .field("sql_len", &self.sql.len())
            .field("down_sql_len", &self.down_sql.len())
            .field("has_code", &self.code.is_some())
            .finish()
    }
}

impl Migration {
    /// Attach a Rust data-migration body (the code lane).
    ///
    /// The closure runs inside the migration's transaction, before `sql`.
    pub fn with_code<F>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a mut sqlx::PgConnection) -> MigrationFuture<'a> + Send + Sync + 'static,
    {
        self.code = Some(std::sync::Arc::new(f));
        self
    }

    /// Create a new migration from version, description, and SQL (no down).
    pub fn new(
        version: impl Into<String>,
        description: impl Into<String>,
        sql: impl Into<String>,
    ) -> Self {
        Migration {
            version: version.into(),
            description: description.into(),
            sql: sql.into(),
            down_sql: String::new(),
            code: None,
        }
    }

    /// Attach a roll-back SQL to this migration.
    pub fn with_down(mut self, down_sql: impl Into<String>) -> Self {
        self.down_sql = down_sql.into();
        self
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
    /// Build a runner that applies migrations **in the given order**.
    ///
    /// The runner does NOT sort — declaration order is execution order,
    /// matching sea-orm's `MigratorTrait::migrations()` semantics. This is
    /// deliberate: version strings are arbitrary (date-padded like
    /// `m20260511_init` sorts lexicographically, but `m2_compliance` and
    /// `m10_role_member` do NOT — `"m10"` sorts before `"m2"`), and history
    /// is order-sensitive. Order your `Vec` the way the migrations must run.
    pub fn new(migrations: Vec<Migration>) -> Self {
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

    /// Roll back the most recent `steps` applied migrations.
    ///
    /// Walks the registered set in REVERSE declaration order; for each
    /// recorded version whose `down_sql` is non-empty, executes it in a
    /// transaction and deletes the bookkeeping row. Recorded versions
    /// without a down lane (empty `down_sql`) are skipped without error —
    /// the caller sees them in the result's `skipped` list.
    pub async fn down(&self, pool: &sqlx::PgPool, steps: u32) -> Result<MigrationResult, Error> {
        let recorded: Vec<(String,)> = sqlx::query_as("SELECT version FROM seaql_migrations")
            .fetch_all(pool)
            .await
            .map_err(map_sqlx)?;
        let recorded_set: std::collections::HashSet<String> =
            recorded.into_iter().map(|(v,)| v).collect();

        let mut rolled_back = Vec::new();
        let mut skipped = Vec::new();

        for migration in self.migrations.iter().rev() {
            if rolled_back.len() as u32 >= steps {
                break;
            }
            if !recorded_set.contains(&migration.version) {
                continue;
            }
            if migration.down_sql.is_empty() {
                skipped.push(migration.version.clone());
                continue;
            }
            let mut tx = pool.begin().await.map_err(map_sqlx)?;
            sqlx::raw_sql(sqlx::AssertSqlSafe(migration.down_sql.as_str()))
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
            sqlx::query("DELETE FROM seaql_migrations WHERE version = $1")
                .bind(&migration.version)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
            tx.commit().await.map_err(map_sqlx)?;
            rolled_back.push(migration.version.clone());
        }

        Ok(MigrationResult {
            applied: rolled_back,
            skipped,
            pre_history: Vec::new(),
        })
    }

    /// Run all pending migrations against the given pool.
    pub async fn up(&self, pool: &sqlx::PgPool) -> Result<MigrationResult, Error> {
        // 1. Ensure the bookkeeping table exists (same shape as sea-orm)
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS seaql_migrations (
                version VARCHAR(255) PRIMARY KEY,
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

            // Execute the migration in a transaction: the Rust body first
            // (when present), then the literal SQL.
            let mut tx = pool.begin().await.map_err(map_sqlx)?;
            if let Some(code) = &migration.code {
                code(&mut tx).await?;
            }
            if !migration.sql.is_empty() {
                sqlx::raw_sql(sqlx::AssertSqlSafe(migration.sql.as_str()))
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx)?;
            }

            // Record it
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                // sea-orm records applied_at in SECONDS — keep the unit
                // identical so a mixed-era table stays comparable.
                .as_secs() as i64;
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
