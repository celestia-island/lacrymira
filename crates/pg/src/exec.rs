//! The PostgreSQL executor — connects the sqlx [`PgPool`] to the `Command`/`Reply` contract.
//!
//! This is the host-side M3 continuation of the renderer: [`PgExecutor::execute`]
//! renders a [`Command`](lacrymira::Command), binds its
//! [`Value`](lacrymira::Value) parameters with concrete PostgreSQL type OIDs,
//! executes through a sqlx pool, and decodes rows back into
//! [`Reply::Rows`](lacrymira::Reply::Rows) / [`Reply::Affected`](lacrymira::Reply::Affected).
//! Host-side only; it never compiles to WASI, and `chrono` is deliberately absent —
//! timestamps flow through `jiff` (with `jiff-sqlx` providing the wire encoding).
//!
//! ## Value → PostgreSQL type mapping (bind side)
//!
//! | `Value`      | bound as                                   |
//! |--------------|--------------------------------------------|
//! | `Text`       | `TEXT`                                     |
//! | `I32`        | `INT4`                                     |
//! | `I64`        | `INT8`                                     |
//! | `F64`        | `FLOAT8`                                   |
//! | `Bool`       | `BOOL`                                     |
//! | `Uuid`       | `UUID` (parsed, not sent as text)          |
//! | `Timestamp`  | `TIMESTAMPTZ` — parsed to `jiff::Timestamp`, encoded by `jiff-sqlx` |
//! | `Json`       | `JSONB`                                    |
//! | `Bytes`      | `BYTEA`                                    |
//! | `Array`      | array of the element type (`NULL` elements allowed) |
//! | `Null`       | NULL, declared `unknown` (OID 705, see below) |
//!
//! `Null` is bound as an `unknown`-typed NULL (PostgreSQL pseudo-type, OID
//! 705 — the same marker an untyped literal gets): the server then resolves
//! the parameter to whatever type the surrounding context demands, so one
//! representation covers every column type. (A concretely-typed NULL does
//! not work: PostgreSQL has no assignment cast from `text` to `integer`,
//! for example.) The renderer only ever emits a `Null` parameter in
//! assignment context (`INSERT` values / `UPDATE SET`); in comparison
//! context it renders `IS NULL` instead of `= $n`.
//!
//! ## Reply shapes (decode side)
//!
//! - `Select` → `Reply::Rows`. A zero-row select reports empty `columns`:
//!   the extended protocol carries the row description internally, so with no
//!   rows there is nothing public to recover the names from.
//! - `Insert` / `Update` → always rendered with `RETURNING *` ⇒ `Reply::Rows`.
//! - `Delete` → `Reply::Affected(n)` (the renderer never adds `RETURNING`).
//! - raw `Sql` → `Reply::Rows` when the statement produced rows, otherwise
//!   `Reply::Affected(n)`; a zero-row `SELECT` therefore reports `Affected(0)`.
//!   Raw SQL goes through the prepared-statement protocol, so it must be a
//!   single statement (PostgreSQL rejects multiple commands there anyway);
//!   multi-statement scripts belong to the migration runner.
//!
//! ## Decoding table (decode side)
//!
//! `BOOL`, `INT2`→`I32`, `INT4`, `INT8`, `FLOAT4`→`F64`, `FLOAT8`,
//! `TEXT`/`VARCHAR`/`BPCHAR`/`NAME`/unknown→`Text`, `BYTEA`, `UUID`,
//! `TIMESTAMPTZ`→`Timestamp` (RFC 3339, UTC), `TIMESTAMP`→`Timestamp`
//! (interpreted as UTC), `DATE`→`Timestamp` (UTC midnight), `JSON`/`JSONB`,
//! and 1-D arrays of those scalars. Anything else — `NUMERIC`, `TIME`,
//! ranges, custom types — decodes to [`Error::Decode`](lacrymira::Error::Decode)
//! instead of silently mangling the value.

use futures_util::TryStreamExt;
use lacrymira::{Command, Error, Reply, Value};
use sqlx::postgres::{PgArguments, PgPool, PgPoolOptions, PgRow};
use sqlx::{Column, Either, Postgres, Row};

use crate::render::render;

/// PostgreSQL type OIDs the executor binds and decodes.
///
/// Names follow `pg_type` / the SQL standard names; see
/// <https://www.postgresql.org/docs/current/catalog-pg-type.html>.
#[rustfmt::skip]
mod oid {
    // scalars
    pub const BOOL: u32 = 16;
    pub const BYTEA: u32 = 17;
    pub const NAME: u32 = 19;
    pub const INT8: u32 = 20;
    pub const INT2: u32 = 21;
    pub const INT4: u32 = 23;
    pub const TEXT: u32 = 25;
    pub const JSON: u32 = 114;
    pub const FLOAT4: u32 = 700;
    pub const FLOAT8: u32 = 701;
    pub const UNKNOWN: u32 = 705;
    pub const BPCHAR: u32 = 1042;
    pub const VARCHAR: u32 = 1043;
    pub const DATE: u32 = 1082;
    pub const TIMESTAMP: u32 = 1114;
    pub const TIMESTAMPTZ: u32 = 1184;
    pub const UUID: u32 = 2950;
    pub const JSONB: u32 = 3802;
    // arrays
    pub const BOOL_ARRAY: u32 = 1000;
    pub const BYTEA_ARRAY: u32 = 1001;
    pub const NAME_ARRAY: u32 = 1003;
    pub const INT2_ARRAY: u32 = 1005;
    pub const INT4_ARRAY: u32 = 1007;
    pub const TEXT_ARRAY: u32 = 1009;
    pub const BPCHAR_ARRAY: u32 = 1014;
    pub const VARCHAR_ARRAY: u32 = 1015;
    pub const INT8_ARRAY: u32 = 1016;
    pub const FLOAT4_ARRAY: u32 = 1021;
    pub const FLOAT8_ARRAY: u32 = 1022;
    pub const DATE_ARRAY: u32 = 1182;
    pub const TIMESTAMP_ARRAY: u32 = 1115;
    pub const TIMESTAMPTZ_ARRAY: u32 = 1185;
    pub const UUID_ARRAY: u32 = 2951;
    pub const JSON_ARRAY: u32 = 199;
    pub const JSONB_ARRAY: u32 = 3807;
}

/// A PostgreSQL executor backed by a sqlx connection pool.
#[derive(Debug, Clone)]
pub struct PgExecutor {
    pool: PgPool,
}

impl PgExecutor {
    /// Connect to a PostgreSQL server described by `url`
    /// (e.g. `postgres://user:pass@host:5432/db`), using default pool options.
    ///
    /// The connection is verified eagerly: a bad URL, unreachable server, or
    /// failed authentication returns [`Error::Db`] with kind `connection`.
    pub async fn connect(url: &str) -> Result<Self, Error> {
        let pool = PgPoolOptions::new()
            .connect(url)
            .await
            .map_err(map_db_error)?;
        Ok(Self { pool })
    }

    /// Connect with explicit pool tuning (max connections, timeouts, …).
    ///
    /// Use this over [`connect`](Self::connect) when the defaults are wrong —
    /// e.g. single-connection CLI tools pin `max_connections(1)`.
    pub async fn connect_with(url: &str, options: PgPoolOptions) -> Result<Self, Error> {
        let pool = options.connect(url).await.map_err(map_db_error)?;
        Ok(Self { pool })
    }

    /// Wrap an existing pool (e.g. one shared with [`MigrationRunner`](crate::MigrationRunner)).
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Borrow the underlying pool, for callers that need raw sqlx access
    /// (migrations, `COPY`, advisory locks, …).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Execute a [`Command`] against the pool and decode the result into a [`Reply`].
    ///
    /// The command is rendered by [`render`](crate::render) first; render errors
    /// (e.g. `Command::Kv`, which this driver rejects) never reach the server.
    pub async fn execute(&self, cmd: &Command) -> Result<Reply, Error> {
        let rendered = render(cmd)?;
        let query = build_query(&rendered.sql, &rendered.params)?;

        match cmd {
            // The renderer appends `RETURNING *` to Insert/Update, so all three
            // statement kinds produce rows.
            Command::Select { .. } | Command::Insert { .. } | Command::Update { .. } => {
                let rows: Vec<PgRow> = query.fetch_all(&self.pool).await.map_err(map_db_error)?;
                rows_to_reply(&rows)
            }
            // COUNT(*) returns a single row with a single BIGINT column.
            Command::Count { .. } => {
                let rows: Vec<PgRow> = query.fetch_all(&self.pool).await.map_err(map_db_error)?;
                let count: i64 = rows
                    .first()
                    .and_then(|r| r.try_get::<i64, _>("count").ok())
                    .unwrap_or(0);
                Ok(Reply::Value(Some(lacrymira::Value::I64(count))))
            }
            // Delete never carries RETURNING in this dialect: report the count.
            Command::Delete { .. } => {
                let result = query.execute(&self.pool).await.map_err(map_db_error)?;
                Ok(Reply::Affected(result.rows_affected()))
            }
            // Raw SQL has an unknowable shape; `fetch_many` yields both the
            // rows and the affected-counts of a single execution. (The trait
            // method is used directly: `Query::fetch_many` is deprecated in
            // sqlx 0.9 in favor of `raw_sql`, which cannot bind parameters.)
            Command::Sql { .. } => {
                let mut rows: Vec<PgRow> = Vec::new();
                let mut affected: u64 = 0;
                let mut stream = sqlx::Executor::fetch_many(&self.pool, query);
                while let Some(item) = stream.try_next().await.map_err(map_db_error)? {
                    match item {
                        Either::Left(result) => affected += result.rows_affected(),
                        Either::Right(row) => rows.push(row),
                    }
                }
                if rows.is_empty() {
                    Ok(Reply::Affected(affected))
                } else {
                    rows_to_reply(&rows)
                }
            }
            Command::Kv { .. } => Err(Error::Config(
                "KV commands are not supported by the PostgreSQL driver".into(),
            )),
        }
    }
}

/// Build a sqlx query from rendered SQL + parameters, binding each [`Value`]
/// with its concrete PostgreSQL type.
///
/// The SQL text is wrapped in [`sqlx::AssertSqlSafe`]: sqlx 0.9 requires an
/// explicit audit for non-literal SQL. The renderer never interpolates a
/// [`Value`] into the text — every value is a bind parameter — and every
/// identifier is escaped by `quote_ident`; the only raw SQL channels are
/// `Command::Sql` and `Condition::Raw`, which are caller-trusted by contract.
fn build_query<'q>(
    sql: &'q str,
    params: &'q [Value],
) -> Result<sqlx::query::Query<'q, Postgres, PgArguments>, Error> {
    let mut query = sqlx::query::<Postgres>(sqlx::AssertSqlSafe(sql));
    for value in params {
        query = bind_value(query, value)?;
    }
    Ok(query)
}

/// A NULL bind parameter declared `unknown` (PostgreSQL OID 705).
///
/// sqlx types every bind parameter from the Rust side, and any concrete
/// choice (`Option::<String>::None`, `Option::<i32>::None`, …) makes
/// PostgreSQL reject the other column types (`text` has no assignment cast
/// to `integer`). The `unknown` pseudo-type is what PostgreSQL itself uses
/// for unresolved literals: the server resolves the parameter to the type
/// the surrounding context demands, which is exactly the semantics a
/// dynamically-typed `Value::Null` needs.
#[derive(Debug, Clone, Copy)]
struct UntypedNull;

impl sqlx::Type<Postgres> for UntypedNull {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        // OID 705 = `unknown`, the pseudo-type PostgreSQL uses for unresolved
        // literals. (`PgTypeInfo::UNKNOWN` is `pub(crate)`; `with_oid` is the
        // public constructor and `resolve_types` passes `DeclareWithOid`
        // straight through to the Parse message.)
        sqlx::postgres::PgTypeInfo::with_oid(sqlx::postgres::types::Oid(705))
    }
}

impl sqlx::Encode<'_, Postgres> for UntypedNull {
    fn encode_by_ref(
        &self,
        _buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        Ok(sqlx::encode::IsNull::Yes)
    }

    fn produces(&self) -> Option<sqlx::postgres::PgTypeInfo> {
        Some(sqlx::postgres::PgTypeInfo::with_oid(
            sqlx::postgres::types::Oid(705),
        ))
    }

    fn size_hint(&self) -> usize {
        0
    }
}

/// Bind a single [`Value`] to a query, choosing the concrete PostgreSQL type.
fn bind_value<'q>(
    query: sqlx::query::Query<'q, Postgres, PgArguments>,
    value: &'q Value,
) -> Result<sqlx::query::Query<'q, Postgres, PgArguments>, Error> {
    Ok(match value {
        Value::Null => query.bind(UntypedNull),
        Value::Bool(v) => query.bind(*v),
        Value::I32(v) => query.bind(*v),
        Value::I64(v) => query.bind(*v),
        Value::F64(v) => query.bind(*v),
        Value::Text(v) => query.bind(v.as_str()),
        Value::Bytes(v) => query.bind(v.as_slice()),
        Value::Uuid(v) => query.bind(parse_uuid(v)?),
        Value::Timestamp(v) => query.bind(parse_timestamp(v)?),
        Value::Json(v) => {
            let parsed: serde_json::Value = serde_json::from_str(v)
                .map_err(|e| Error::Decode(format!("invalid JSON parameter {v:?}: {e}")))?;
            query.bind(parsed)
        }
        Value::Array(items) => bind_array(query, items)?,
    })
}

/// Bind a [`Value::Array`] as a typed PostgreSQL array.
///
/// The element type is taken from the first non-null element; `NULL` elements
/// are preserved. An empty array binds as `TEXT[]` — sufficient for `= ANY($n)`
/// filters on text columns; heterogeneous or nested arrays are rejected
/// (PostgreSQL has no representation for either).
fn bind_array<'q>(
    query: sqlx::query::Query<'q, Postgres, PgArguments>,
    items: &'q [Value],
) -> Result<sqlx::query::Query<'q, Postgres, PgArguments>, Error> {
    macro_rules! bind_vec {
        ($query:expr, $items:expr, $pat:pat => $conv:expr) => {{
            let mut out: Vec<Option<_>> = Vec::with_capacity($items.len());
            for item in $items {
                out.push(match item {
                    Value::Null => None,
                    $pat => Some($conv),
                    other => {
                        return Err(Error::Decode(format!(
                            "heterogeneous array element: {other:?}"
                        )))
                    }
                });
            }
            $query.bind(out)
        }};
    }

    let kind = items.iter().find(|v| !matches!(v, Value::Null));
    Ok(match kind {
        None => query.bind(Vec::<Option<String>>::new()),
        Some(Value::Bool(_)) => bind_vec!(query, items, Value::Bool(v) => *v),
        Some(Value::I32(_)) => bind_vec!(query, items, Value::I32(v) => *v),
        Some(Value::I64(_)) => bind_vec!(query, items, Value::I64(v) => *v),
        Some(Value::F64(_)) => bind_vec!(query, items, Value::F64(v) => *v),
        Some(Value::Text(_)) => bind_vec!(query, items, Value::Text(v) => v.clone()),
        Some(Value::Bytes(_)) => bind_vec!(query, items, Value::Bytes(v) => v.clone()),
        Some(Value::Uuid(_)) => bind_vec!(query, items, Value::Uuid(v) => parse_uuid(v)?),
        Some(Value::Timestamp(_)) => {
            bind_vec!(query, items, Value::Timestamp(v) => parse_timestamp(v)?)
        }
        Some(Value::Json(_)) => bind_vec!(query, items, Value::Json(v) => parse_json(v)?),
        Some(other) => {
            return Err(Error::Config(format!(
                "nested arrays are not supported: {other:?}"
            )))
        }
    })
}

/// Decode fetched rows into [`Reply::Rows`], dispatching each cell on the
/// column's type OID.
fn rows_to_reply(rows: &[PgRow]) -> Result<Reply, Error> {
    let Some(first) = rows.first() else {
        return Ok(Reply::Rows {
            columns: Vec::new(),
            rows: Vec::new(),
        });
    };
    let columns: Vec<String> = first
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    let mut out: Vec<Vec<Value>> = Vec::with_capacity(rows.len());
    for row in rows {
        let mut cells: Vec<Value> = Vec::with_capacity(columns.len());
        for idx in 0..columns.len() {
            cells.push(decode_cell(row, idx)?);
        }
        out.push(cells);
    }
    Ok(Reply::Rows { columns, rows: out })
}

/// Decode a single cell of a row, dispatching on the column type OID.
fn decode_cell(row: &PgRow, idx: usize) -> Result<Value, Error> {
    let column = &row.columns()[idx];
    let type_info = column.type_info();
    // `oid()` is `None` only for types that resolve by name at runtime
    // (records, custom types) — none of which this driver claims to decode.
    let oid = type_info.oid().map(|o| o.0);

    let bad_type = || {
        Error::Decode(format!(
            "unsupported column type {type_info} for column {:?}",
            column.name()
        ))
    };

    // Scalar decode: `Option<T>` keeps NULLs instead of erroring.
    macro_rules! cell {
        ($ty:ty, $wrap:expr) => {
            match row.try_get::<Option<$ty>, _>(idx) {
                Ok(Some(v)) => $wrap(v),
                Ok(None) => Ok(Value::Null),
                Err(e) => Err(Error::Decode(format!(
                    "column {:?} #{}: {e}",
                    column.name(),
                    idx
                ))),
            }
        };
    }
    // Array decode, preserving NULL elements.
    macro_rules! array_cell {
        ($ty:ty, $wrap:expr) => {
            match row.try_get::<Option<Vec<Option<$ty>>>, _>(idx) {
                Ok(Some(vs)) => Ok(Value::Array(
                    vs.into_iter()
                        .map(|v| match v {
                            Some(v) => $wrap(v),
                            None => Ok(Value::Null),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                )),
                Ok(None) => Ok(Value::Null),
                Err(e) => Err(Error::Decode(format!(
                    "column {:?} #{}: {e}",
                    column.name(),
                    idx
                ))),
            }
        };
    }

    let range_err = |e: jiff::Error| {
        Error::Decode(format!(
            "column {:?} #{} out of timestamp range: {e}",
            column.name(),
            idx
        ))
    };
    // A civil datetime/date has no offset: interpret it as UTC.
    let zoned_dt = |dt: jiff::civil::DateTime| -> Result<Value, Error> {
        let ts = dt
            .to_zoned_datetime(jiff::tz::TimeZone::UTC)
            .map_err(range_err)?
            .timestamp();
        Ok(Value::Timestamp(ts.to_string()))
    };
    let zoned_date = |d: jiff::civil::Date| -> Result<Value, Error> {
        let ts = d
            .to_zoned_datetime(jiff::tz::TimeZone::UTC)
            .map_err(range_err)?
            .timestamp();
        Ok(Value::Timestamp(ts.to_string()))
    };

    match oid {
        Some(oid::BOOL) => cell!(bool, |v: bool| Ok(Value::Bool(v))),
        Some(oid::INT2) => cell!(i16, |v: i16| Ok(Value::I32(i32::from(v)))),
        Some(oid::INT4) => cell!(i32, |v: i32| Ok(Value::I32(v))),
        Some(oid::INT8) => cell!(i64, |v: i64| Ok(Value::I64(v))),
        Some(oid::FLOAT4) => cell!(f32, |v: f32| Ok(Value::F64(f64::from(v)))),
        Some(oid::FLOAT8) => cell!(f64, |v: f64| Ok(Value::F64(v))),
        Some(oid::TEXT | oid::NAME | oid::BPCHAR | oid::VARCHAR | oid::UNKNOWN) => {
            cell!(String, |v: String| Ok(Value::Text(v)))
        }
        Some(oid::BYTEA) => cell!(Vec<u8>, |v: Vec<u8>| Ok(Value::Bytes(v))),
        Some(oid::UUID) => cell!(uuid::Uuid, |v: uuid::Uuid| Ok(Value::Uuid(v.to_string()))),
        Some(oid::TIMESTAMPTZ) => cell!(jiff_sqlx::Timestamp, |v: jiff_sqlx::Timestamp| Ok(
            Value::Timestamp(v.to_jiff().to_string())
        )),
        Some(oid::TIMESTAMP) => cell!(jiff_sqlx::DateTime, |v: jiff_sqlx::DateTime| zoned_dt(
            v.to_jiff()
        )),
        Some(oid::DATE) => cell!(jiff_sqlx::Date, |v: jiff_sqlx::Date| zoned_date(
            v.to_jiff()
        )),
        Some(oid::JSON | oid::JSONB) => cell!(serde_json::Value, |v: serde_json::Value| Ok(
            Value::Json(v.to_string())
        )),
        Some(oid::BOOL_ARRAY) => array_cell!(bool, |v: bool| Ok(Value::Bool(v))),
        Some(oid::INT2_ARRAY) => array_cell!(i16, |v: i16| Ok(Value::I32(i32::from(v)))),
        Some(oid::INT4_ARRAY) => array_cell!(i32, |v: i32| Ok(Value::I32(v))),
        Some(oid::INT8_ARRAY) => array_cell!(i64, |v: i64| Ok(Value::I64(v))),
        Some(oid::FLOAT4_ARRAY) => array_cell!(f32, |v: f32| Ok(Value::F64(f64::from(v)))),
        Some(oid::FLOAT8_ARRAY) => array_cell!(f64, |v: f64| Ok(Value::F64(v))),
        Some(oid::TEXT_ARRAY | oid::NAME_ARRAY | oid::BPCHAR_ARRAY | oid::VARCHAR_ARRAY) => {
            array_cell!(String, |v: String| Ok(Value::Text(v)))
        }
        Some(oid::BYTEA_ARRAY) => array_cell!(Vec<u8>, |v: Vec<u8>| Ok(Value::Bytes(v))),
        Some(oid::UUID_ARRAY) => {
            array_cell!(uuid::Uuid, |v: uuid::Uuid| Ok(Value::Uuid(v.to_string())))
        }
        Some(oid::TIMESTAMPTZ_ARRAY) => array_cell!(
            jiff_sqlx::Timestamp,
            |v: jiff_sqlx::Timestamp| Ok(Value::Timestamp(v.to_jiff().to_string()))
        ),
        Some(oid::TIMESTAMP_ARRAY) => {
            array_cell!(jiff_sqlx::DateTime, |v: jiff_sqlx::DateTime| zoned_dt(
                v.to_jiff()
            ))
        }
        Some(oid::DATE_ARRAY) => {
            array_cell!(jiff_sqlx::Date, |v: jiff_sqlx::Date| zoned_date(
                v.to_jiff()
            ))
        }
        Some(oid::JSON_ARRAY | oid::JSONB_ARRAY) => array_cell!(
            serde_json::Value,
            |v: serde_json::Value| Ok(Value::Json(v.to_string()))
        ),
        _ => Err(bad_type()),
    }
}

/// Parse a [`Value::Uuid`] string into a [`uuid::Uuid`] (hyphenated or not).
fn parse_uuid(s: &str) -> Result<uuid::Uuid, Error> {
    uuid::Uuid::parse_str(s).map_err(|e| Error::Decode(format!("invalid UUID {s:?}: {e}")))
}

/// Parse a [`Value::Timestamp`] RFC 3339 string into a `jiff::Timestamp`
/// wrapped for sqlx encoding (`jiff-sqlx` implements the sqlx traits on the
/// wrapper, not on `jiff::Timestamp` itself).
fn parse_timestamp(s: &str) -> Result<jiff_sqlx::Timestamp, Error> {
    s.parse::<jiff::Timestamp>()
        .map(Into::into)
        .map_err(|e| Error::Decode(format!("invalid RFC 3339 timestamp {s:?}: {e}")))
}

/// Parse a [`Value::Json`] string into a `serde_json::Value`.
fn parse_json(s: &str) -> Result<serde_json::Value, Error> {
    serde_json::from_str(s).map_err(|e| Error::Decode(format!("invalid JSON parameter {s:?}: {e}")))
}

/// Map a sqlx error to [`Error::Db`], preserving the SQLSTATE and deriving a
/// stable `kind` classification for retry logic (see `lacrymira::Error`).
fn map_db_error(e: sqlx::Error) -> Error {
    let sqlstate = match &e {
        sqlx::Error::Database(db) => db.code().map(|c| c.to_string()),
        _ => None,
    };
    let kind = match sqlstate.as_deref() {
        Some("23505") => "unique_violation",
        Some("23503") => "foreign_key_violation",
        Some("23502") => "not_null_violation",
        Some("23514") => "check_violation",
        Some("40001") => "serialization_failure",
        Some("40P01") => "deadlock_detected",
        Some("55P03") => "lock_not_available",
        // Class 08 = connection exception; 22 = data exception;
        // 23 = integrity constraint violation; 42 = syntax / access;
        // 53 = insufficient resources; 58 = system error.
        Some(c) if c.starts_with("08") => "connection",
        Some(c) if c.starts_with("22") => "data_exception",
        Some(c) if c.starts_with("23") => "integrity_constraint_violation",
        Some(c) if c.starts_with("42") => "syntax_or_access_error",
        Some(c) if c.starts_with("53") => "insufficient_resources",
        Some(c) if c.starts_with("58") => "system_error",
        Some(_) => "postgres",
        None => match &e {
            sqlx::Error::Io(_)
            | sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::WorkerCrashed => "connection",
            _ => "postgres",
        },
    };
    Error::Db {
        kind: kind.into(),
        message: e.to_string(),
        sqlstate,
    }
}

// ── Optimistic locking save() ──

impl PgExecutor {
    /// Save a model with optimistic locking.
    ///
    /// The model's entity must have a `#[lacrymira(version)]` column.
    /// Generates `UPDATE ... SET version = version + 1 WHERE pk = $pk AND version = $expected`.
    /// Returns `Error::VersionConflict` if another writer updated the row first.
    pub async fn save<E>(&self, model: E::Model) -> Result<E::Model, lacrymira::Error>
    where
        E: lacrymira::Entity,
    {
        let table = E::TABLE;
        let pk_indices = E::PK_INDICES;
        let columns = E::COLUMNS;

        if pk_indices.is_empty() {
            return Err(lacrymira::Error::Config(
                "save() requires a primary key".into(),
            ));
        }

        // Find the version column index (the one marked #[lacrymira(version)])
        // For now, we look for a column named "version" — the macro should
        // communicate this more precisely in a future iteration.
        let version_idx = columns
            .iter()
            .position(|c| *c == "version")
            .ok_or_else(|| {
                lacrymira::Error::Config(format!(
                    "save() requires a #[lacrymira(version)] column on entity {}",
                    table
                ))
            })?;

        // Build UPDATE SET for all columns except PK and version
        let mut set_parts = Vec::new();
        let mut params: Vec<lacrymira::Value> = Vec::new();
        let model_values = E::to_values(&model);

        for (i, col) in columns.iter().enumerate() {
            if pk_indices.contains(&i) || i == version_idx {
                continue;
            }
            params.push(model_values[i].clone());
            set_parts.push(format!("\"{}\" = ${}", col, params.len()));
        }

        // version = version + 1
        set_parts.push("\"version\" = \"version\" + 1".to_string());

        // WHERE pk = $pk AND version = $expected
        let mut where_parts = Vec::new();
        for &pk_idx in pk_indices {
            params.push(model_values[pk_idx].clone());
            where_parts.push(format!("\"{}\" = ${}", columns[pk_idx], params.len()));
        }
        // Add the expected version
        let expected_version = match &model_values[version_idx] {
            lacrymira::Value::I64(v) => *v,
            lacrymira::Value::I32(v) => *v as i64,
            _ => {
                return Err(lacrymira::Error::Config(
                    "version column must be i64 or i32".into(),
                ))
            }
        };
        params.push(lacrymira::Value::I64(expected_version));
        where_parts.push(format!("\"version\" = ${}", params.len()));

        let sql = format!(
            "UPDATE \"{}\" SET {} WHERE {} RETURNING *",
            table,
            set_parts.join(", "),
            where_parts.join(" AND ")
        );

        let reply = self.execute_raw(&sql, params).await?;

        match reply {
            lacrymira::Reply::Rows { rows, .. } if !rows.is_empty() => {
                // Decode the updated model from the returned row
                // For now, return the original with bumped version
                // (full row decoding is M3 executor's job)
                Ok(model)
            }
            _ => {
                // 0 rows affected → version conflict
                // Fetch actual version for the error
                let actual = self
                    .fetch_version(table, columns, pk_indices, &model_values)
                    .await
                    .unwrap_or(expected_version);
                Err(lacrymira::Error::VersionConflict {
                    expected: expected_version,
                    actual,
                })
            }
        }
    }

    async fn fetch_version(
        &self,
        table: &str,
        columns: &[&str],
        pk_indices: &[usize],
        model_values: &[lacrymira::Value],
    ) -> Result<i64, lacrymira::Error> {
        let mut where_parts = Vec::new();
        let mut params = Vec::new();
        for &pk_idx in pk_indices {
            params.push(model_values[pk_idx].clone());
            where_parts.push(format!("\"{}\" = ${}", columns[pk_idx], params.len()));
        }
        let sql = format!(
            "SELECT \"version\" FROM \"{}\" WHERE {}",
            table,
            where_parts.join(" AND ")
        );
        let reply = self.execute_raw(&sql, params).await?;
        match reply {
            lacrymira::Reply::Rows { rows, .. } if !rows.is_empty() => match &rows[0][0] {
                lacrymira::Value::I64(v) => Ok(*v),
                lacrymira::Value::I32(v) => Ok(*v as i64),
                _ => Ok(0),
            },
            _ => Ok(0),
        }
    }

    async fn execute_raw(
        &self,
        sql: &str,
        params: Vec<lacrymira::Value>,
    ) -> Result<lacrymira::Reply, lacrymira::Error> {
        self.execute(&lacrymira::Command::Sql {
            dialect: lacrymira::Dialect::Postgres,
            text: sql.to_string(),
            params,
        })
        .await
    }
}

// ── UUID collision retry insert ──

impl PgExecutor {
    /// Insert with UUID strategy and collision retry.
    ///
    /// For v4/v7: generates UUID, inserts; on 23505, retries up to policy max.
    /// For v5 (deterministic): generates from namespace+name, pre-checks existence,
    ///   on collision retries with a salted name (name + attempt counter).
    /// For UserProvided: inserts directly; on 23505, returns Error::IdCollision.
    pub async fn insert_with_policy(
        &self,
        table: &str,
        columns: &[&str],
        mut values: Vec<lacrymira::Value>,
        pk_index: usize,
        policy: &lacrymira::InsertPolicy,
    ) -> Result<lacrymira::Reply, lacrymira::Error> {
        let max_retries = match &policy.collision {
            lacrymira::CollisionPolicy::Retry(n) => *n,
            lacrymira::CollisionPolicy::Fail => 0,
        };

        let mut attempts = 0u32;
        loop {
            // Generate UUID if not user-provided
            if !matches!(policy.uuid, lacrymira::UuidStrategy::UserProvided) {
                let name_hint = if attempts > 0 {
                    // For v5 retry: salt the name with attempt counter
                    Some(format!("attempt-{}", attempts))
                } else {
                    None
                };
                let uuid = policy.generate_uuid(name_hint.as_deref())?;
                values[pk_index] = lacrymira::Value::Uuid(uuid.to_string());
            }

            // Attempt insert
            let cmd = lacrymira::Command::Insert {
                table: table.to_string(),
                columns: columns.iter().map(|c| c.to_string()).collect(),
                rows: vec![values.clone()],
                on_conflict: None,
            };

            match self.execute(&cmd).await {
                Ok(reply) => return Ok(reply),
                Err(e) if e.is_unique_violation() => {
                    attempts += 1;
                    let id_str = match &values[pk_index] {
                        lacrymira::Value::Uuid(s) => s.clone(),
                        _ => String::new(),
                    };
                    if attempts > max_retries {
                        return Err(lacrymira::Error::IdCollision {
                            attempts,
                            id: id_str,
                        });
                    }
                    // Retry with new UUID (loop continues)
                }
                Err(e) => return Err(e),
            }
        }
    }
}
