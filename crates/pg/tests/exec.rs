//! Integration tests for the PostgreSQL executor (`PgExecutor`).
//!
//! These tests run against a **real PostgreSQL** — the executor is the piece
//! that wires the sqlx `PgPool` to real wire-format encoding/decoding, so a
//! real server is the only meaningful fixture. No credentials belong in the
//! tree (workspace red line), so the connection URL is taken from the
//! environment and every database-backed test is `#[ignore]`d by default.
//!
//! ## How to run
//!
//! Spin up a throwaway local server (image already present on the dev hosts)
//! and point the tests at it:
//!
//! ```sh
//! docker run --rm -d --name lacrymira-pg-test \
//!   -e POSTGRES_PASSWORD=postgres -p 127.0.0.1:54399:5432 postgres:16-alpine
//!
//! LACRYMIRA_PG_TEST_URL='postgres://postgres:postgres@127.0.0.1:54399/postgres' \
//!   cargo test -p lacrymira-pg --test exec -- --ignored --nocapture
//!
//! docker rm -f lacrymira-pg-test
//! ```
//!
//! (`cargo test -p lacrymira-pg` still runs the DB-less tests and the
//! renderer suite; only the `--ignored` ones need the server.)
//!
//! Tests isolate themselves with per-process table names (`DROP TABLE IF
//! EXISTS` + `CREATE`), so a shared scratch database is safe to reuse.

use lacrymira_core::{
    ColumnRef, Command, Condition, ConflictAction, Dialect, OnConflict, Reply, Value,
};
use lacrymira_pg::PgExecutor;

/// Unique table name per test + process, so parallel and repeated runs collide.
fn table(tag: &str) -> String {
    format!("exec_test_{tag}_{}", std::process::id())
}

/// Connect via `LACRYMIRA_PG_TEST_URL`.
async fn executor() -> PgExecutor {
    let url = std::env::var("LACRYMIRA_PG_TEST_URL")
        .expect("LACRYMIRA_PG_TEST_URL must point at a scratch PostgreSQL (see module docs)");
    PgExecutor::connect(&url)
        .await
        .expect("connect to test PostgreSQL")
}

/// Run a single raw statement (DDL etc.).
async fn sql(x: &PgExecutor, text: &str) -> Reply {
    x.execute(&Command::Sql {
        dialect: Dialect::Postgres,
        text: text.into(),
        params: vec![],
    })
    .await
    .expect("raw SQL")
}

/// Unpack a `Reply::Rows`, panicking on anything else.
fn unpack(reply: Reply) -> (Vec<String>, Vec<Vec<Value>>) {
    match reply {
        Reply::Rows { columns, rows } => (columns, rows),
        other => panic!("expected Reply::Rows, got {other:?}"),
    }
}

/// Fetch a cell by column name (column order is the table's, not the test's).
fn cell<'a>(columns: &[String], rows: &'a [Vec<Value>], r: usize, name: &str) -> &'a Value {
    let c = columns
        .iter()
        .position(|c| c == name)
        .unwrap_or_else(|| panic!("column {name:?} not in {columns:?}"));
    &rows[r][c]
}

fn select(table: &str) -> Command {
    Command::Select {
        table: table.into(),
        filter: None,
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
    }
}

/// The full-type matrix: every `Value` variant this driver supports, in one row.
async fn typed_table(x: &PgExecutor, tag: &str) -> String {
    let t = table(tag);
    // One statement per Command::Sql: the executor's raw path uses the
    // prepared protocol, which PostgreSQL limits to single statements.
    sql(x, &format!(r#"DROP TABLE IF EXISTS "{t}""#)).await;
    sql(
        x,
        &format!(
            r#"CREATE TABLE "{t}" (
                   id UUID PRIMARY KEY,
                   label TEXT NOT NULL,
                   n_i32 INT,
                   n_i64 BIGINT,
                   n_f64 DOUBLE PRECISION,
                   flag BOOL,
                   ts TIMESTAMPTZ,
                   meta JSONB,
                   blob BYTEA,
                   tags TEXT[]
               )"#
        ),
    )
    .await;
    t
}

/// Create → insert → select → update → delete across the full type matrix,
/// including NULL parameters and NULL/empty array elements.
#[tokio::test]
#[ignore = "needs LACRYMIRA_PG_TEST_URL pointing at a scratch PostgreSQL"]
async fn crud_round_trip() {
    let x = executor().await;
    let t = typed_table(&x, "crud").await;

    let id_a = "01234567-89ab-cdef-0123-456789abcdef";
    let id_c = "fedcba98-7654-3210-fedc-ba9876543210";

    // --- Insert (3 rows, one statement; RETURNING * comes back as Rows).
    let reply = x
        .execute(&Command::Insert {
            table: t.clone(),
            columns: vec![
                "id".into(),
                "label".into(),
                "n_i32".into(),
                "n_i64".into(),
                "n_f64".into(),
                "flag".into(),
                "ts".into(),
                "meta".into(),
                "blob".into(),
                "tags".into(),
            ],
            rows: vec![
                // Row A: every type populated.
                vec![
                    Value::Uuid(id_a.into()),
                    Value::Text("alpha".into()),
                    Value::I32(7),
                    Value::I64(4_611_686_018_427_387_904), // 2^62
                    Value::F64(2.5),
                    Value::Bool(true),
                    Value::Timestamp("2026-11-21T12:34:56Z".into()),
                    Value::Json(r#"{"a":1,"b":["x","y"]}"#.into()),
                    Value::Bytes(vec![0, 1, 2, 3, 255]),
                    Value::Array(vec![
                        Value::Text("x".into()),
                        Value::Null, // NULL array element must survive
                        Value::Text("z".into()),
                    ]),
                ],
                // Row B: NULLs across typed columns (text-typed NULL params +
                // assignment casts), plus an empty array.
                vec![
                    Value::Uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into()),
                    Value::Text("beta".into()),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Bool(false),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Array(vec![]),
                ],
                // Row C: microsecond timestamp precision.
                vec![
                    Value::Uuid(id_c.into()),
                    Value::Text("gamma".into()),
                    Value::I32(42),
                    Value::I64(-1),
                    Value::F64(-0.125),
                    Value::Bool(true),
                    Value::Timestamp("2026-11-21T12:34:56.123456Z".into()),
                    Value::Json(r#"{"ok":true}"#.into()),
                    Value::Bytes(vec![]),
                    Value::Array(vec![Value::Text("only".into())]),
                ],
            ],
            on_conflict: None,
        })
        .await
        .expect("insert");
    let (columns, rows) = unpack(reply);
    assert_eq!(rows.len(), 3, "RETURNING * yields one row per input");
    assert_eq!(columns.len(), 10);

    // --- Select all, ordered.
    let mut sel = select(&t);
    if let Command::Select { order_by, .. } = &mut sel {
        order_by.push(("label".into(), false));
    }
    let (columns, rows) = unpack(x.execute(&sel).await.expect("select all"));
    assert_eq!(
        columns,
        vec!["id", "label", "n_i32", "n_i64", "n_f64", "flag", "ts", "meta", "blob", "tags"]
    );
    assert_eq!(rows.len(), 3);

    // Row A decodes back exactly as inserted (full type round-trip).
    assert_eq!(cell(&columns, &rows, 0, "id"), &Value::Uuid(id_a.into()));
    assert_eq!(
        cell(&columns, &rows, 0, "label"),
        &Value::Text("alpha".into())
    );
    assert_eq!(cell(&columns, &rows, 0, "n_i32"), &Value::I32(7));
    assert_eq!(
        cell(&columns, &rows, 0, "n_i64"),
        &Value::I64(4_611_686_018_427_387_904)
    );
    assert_eq!(cell(&columns, &rows, 0, "n_f64"), &Value::F64(2.5));
    assert_eq!(cell(&columns, &rows, 0, "flag"), &Value::Bool(true));
    assert_eq!(
        cell(&columns, &rows, 0, "ts"),
        &Value::Timestamp("2026-11-21T12:34:56Z".into())
    );
    // serde_json re-serializes compactly; fixture keys are already sorted.
    assert_eq!(
        cell(&columns, &rows, 0, "meta"),
        &Value::Json(r#"{"a":1,"b":["x","y"]}"#.into())
    );
    assert_eq!(
        cell(&columns, &rows, 0, "blob"),
        &Value::Bytes(vec![0, 1, 2, 3, 255])
    );
    assert_eq!(
        cell(&columns, &rows, 0, "tags"),
        &Value::Array(vec![
            Value::Text("x".into()),
            Value::Null,
            Value::Text("z".into())
        ])
    );

    // Row B: NULLs stay NULL, empty array stays empty.
    assert_eq!(cell(&columns, &rows, 1, "n_i32"), &Value::Null);
    assert_eq!(cell(&columns, &rows, 1, "ts"), &Value::Null);
    assert_eq!(cell(&columns, &rows, 1, "meta"), &Value::Null);
    assert_eq!(cell(&columns, &rows, 1, "blob"), &Value::Null);
    assert_eq!(cell(&columns, &rows, 1, "tags"), &Value::Array(vec![]));

    // Row C: microsecond timestamps survive the round trip.
    assert_eq!(
        cell(&columns, &rows, 2, "ts"),
        &Value::Timestamp("2026-11-21T12:34:56.123456Z".into())
    );
    assert_eq!(cell(&columns, &rows, 2, "blob"), &Value::Bytes(vec![]));

    // --- Filtered selects: Eq / In (ANY-bound array) / Like / Gt / IsNull.
    let filter = |cond| Command::Select {
        table: t.clone(),
        filter: Some(cond),
        order_by: vec![("label".into(), false)],
        limit: None,
        offset: None,
        lock: None,
    };

    let (_, rows) = unpack(
        x.execute(&filter(Condition::Eq(
            ColumnRef::new("label"),
            Value::Text("beta".into()),
        )))
        .await
        .expect("eq select"),
    );
    assert_eq!(rows.len(), 1);

    let (_, rows) = unpack(
        x.execute(&filter(Condition::In(
            ColumnRef::new("label"),
            vec![Value::Text("alpha".into()), Value::Text("gamma".into())],
        )))
        .await
        .expect("in select"),
    );
    assert_eq!(rows.len(), 2, "In renders as = ANY($n)");

    let (_, rows) = unpack(
        x.execute(&filter(Condition::Like(
            ColumnRef::new("label"),
            Value::Text("gam%".into()),
        )))
        .await
        .expect("like select"),
    );
    assert_eq!(rows.len(), 1);

    let (_, rows) = unpack(
        x.execute(&filter(Condition::Gt(
            ColumnRef::new("n_i32"),
            Value::I32(10),
        )))
        .await
        .expect("gt select"),
    );
    assert_eq!(rows.len(), 1, "NULL n_i32 does not match Gt");

    let (_, rows) = unpack(
        x.execute(&filter(Condition::IsNull(ColumnRef::new("meta"))))
            .await
            .expect("is-null select"),
    );
    assert_eq!(rows.len(), 1);

    // LIMIT / OFFSET.
    let limited = Command::Select {
        table: t.clone(),
        filter: None,
        order_by: vec![("label".into(), false)],
        limit: Some(1),
        offset: Some(1),
        lock: None,
    };
    let (_, rows) = unpack(x.execute(&limited).await.expect("limit select"));
    assert_eq!(rows.len(), 1);

    // --- Update: RETURNING * as Rows.
    let reply = x
        .execute(&Command::Update {
            table: t.clone(),
            sets: vec![
                ("label".into(), Value::Text("alpha2".into())),
                ("flag".into(), Value::Bool(false)),
            ],
            filter: Some(Condition::Eq(
                ColumnRef::new("id"),
                Value::Uuid(id_a.into()),
            )),
        })
        .await
        .expect("update");
    let (columns, rows) = unpack(reply);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        cell(&columns, &rows, 0, "label"),
        &Value::Text("alpha2".into())
    );
    assert_eq!(cell(&columns, &rows, 0, "flag"), &Value::Bool(false));

    // --- Delete: Affected(n), no RETURNING on this path.
    let affected = x
        .execute(&Command::Delete {
            table: t.clone(),
            filter: Some(Condition::Eq(ColumnRef::new("flag"), Value::Bool(false))),
        })
        .await
        .expect("delete");
    assert_eq!(
        affected,
        Reply::Affected(2),
        "alpha2 and beta had flag=false"
    );

    let (_, rows) = unpack(x.execute(&select(&t)).await.expect("select after delete"));
    assert_eq!(rows.len(), 1);

    sql(&x, &format!(r#"DROP TABLE IF EXISTS "{t}""#)).await;
}

/// `ON CONFLICT DO UPDATE` (bound values + `EXCLUDED.` passthrough) and
/// `DO NOTHING`, including a unique-violation error round-trip.
#[tokio::test]
#[ignore = "needs LACRYMIRA_PG_TEST_URL pointing at a scratch PostgreSQL"]
async fn upsert_on_conflict() {
    let x = executor().await;
    let t = table("upsert");
    sql(&x, &format!(r#"DROP TABLE IF EXISTS "{t}""#)).await;
    sql(
        &x,
        &format!(
            r#"CREATE TABLE "{t}" (id INT PRIMARY KEY, label TEXT NOT NULL, counter INT NOT NULL)"#
        ),
    )
    .await;

    let insert = |rows: Vec<Vec<Value>>, on_conflict| Command::Insert {
        table: t.clone(),
        columns: vec!["id".into(), "label".into(), "counter".into()],
        rows,
        on_conflict,
    };
    let row = |id: i32, label: &str, counter: i32| {
        vec![
            Value::I32(id),
            Value::Text(label.into()),
            Value::I32(counter),
        ]
    };
    let all = || {
        let mut c = select(&t);
        if let Command::Select { order_by, .. } = &mut c {
            order_by.push(("id".into(), false));
        }
        c
    };

    // Plain insert of the base row.
    unpack(
        x.execute(&insert(vec![row(1, "one", 1)], None))
            .await
            .expect("seed insert"),
    );

    // DO UPDATE: one assignment from EXCLUDED, one from a bound parameter.
    let reply = x
        .execute(&insert(
            vec![row(1, "ignored-label", 2)],
            Some(OnConflict {
                target: vec!["id".into()],
                action: ConflictAction::DoUpdate {
                    set: vec![
                        // EXCLUDED.col is passed through as SQL by the renderer.
                        ("counter".into(), Value::Text("EXCLUDED.counter".into())),
                        ("label".into(), Value::Text("renamed".into())),
                    ],
                },
            }),
        ))
        .await
        .expect("upsert do update");
    let (columns, rows) = unpack(reply);
    assert_eq!(rows.len(), 1, "updated row returns via RETURNING *");
    assert_eq!(
        cell(&columns, &rows, 0, "label"),
        &Value::Text("renamed".into())
    );
    assert_eq!(cell(&columns, &rows, 0, "counter"), &Value::I32(2));

    // DO NOTHING: conflicting row is skipped, nothing returned.
    let reply = x
        .execute(&insert(
            vec![row(1, "other", 99)],
            Some(OnConflict {
                target: vec!["id".into()],
                action: ConflictAction::DoNothing,
            }),
        ))
        .await
        .expect("upsert do nothing");
    assert_eq!(reply.row_count(), Some(0));
    let (_, rows) = unpack(x.execute(&all()).await.expect("select after do nothing"));
    assert_eq!(rows.len(), 1);

    // Multi-row upsert: one conflict + one fresh row in a single statement.
    unpack(
        x.execute(&insert(
            vec![row(1, "x", 3), row(2, "two", 1)],
            Some(OnConflict {
                target: vec!["id".into()],
                action: ConflictAction::DoUpdate {
                    set: vec![("counter".into(), Value::Text("EXCLUDED.counter".into()))],
                },
            }),
        ))
        .await
        .expect("multi-row upsert"),
    );
    let (columns, rows) = unpack(x.execute(&all()).await.expect("select after multi upsert"));
    assert_eq!(rows.len(), 2);
    assert_eq!(cell(&columns, &rows, 0, "counter"), &Value::I32(3));
    assert_eq!(cell(&columns, &rows, 1, "counter"), &Value::I32(1));

    // Duplicate key without ON CONFLICT surfaces as Error::Db with the
    // SQLSTATE preserved (retry logic keys off it).
    let err = x
        .execute(&insert(vec![row(1, "dup", 0)], None))
        .await
        .expect_err("duplicate key must fail");
    assert!(
        err.is_unique_violation(),
        "expected unique violation (23505), got {err:?}"
    );

    sql(&x, &format!(r#"DROP TABLE IF EXISTS "{t}""#)).await;
}

/// Raw `Command::Sql`: row-producing statements reply `Rows`, others `Affected`.
#[tokio::test]
#[ignore = "needs LACRYMIRA_PG_TEST_URL pointing at a scratch PostgreSQL"]
async fn raw_sql_shapes() {
    let x = executor().await;
    let t = table("raw");

    // DDL through the raw path.
    assert_eq!(
        sql(
            &x,
            &format!(r#"CREATE TABLE "{t}" (id INT PRIMARY KEY, v TEXT)"#)
        )
        .await,
        Reply::Affected(0),
        "DDL completes with zero affected rows"
    );

    // Parameterized insert (prepared protocol).
    let reply = x
        .execute(&Command::Sql {
            dialect: Dialect::Postgres,
            text: format!(r#"INSERT INTO "{t}" (id, v) VALUES ($1, $2)"#),
            params: vec![Value::I32(1), Value::Text("hello".into())],
        })
        .await
        .expect("raw insert");
    assert_eq!(reply, Reply::Affected(1));

    // Row-producing raw SQL.
    let reply = x
        .execute(&Command::Sql {
            dialect: Dialect::Postgres,
            text: format!(r#"SELECT id, v FROM "{t}" WHERE id = $1"#),
            params: vec![Value::I32(1)],
        })
        .await
        .expect("raw select");
    let (columns, rows) = unpack(reply);
    assert_eq!(columns, vec!["id", "v"]);
    assert_eq!(rows, vec![vec![Value::I32(1), Value::Text("hello".into())]]);

    // Update without RETURNING → affected count.
    let reply = x
        .execute(&Command::Sql {
            dialect: Dialect::Postgres,
            text: format!(r#"UPDATE "{t}" SET v = $1 WHERE id = $2"#),
            params: vec![Value::Text("world".into()), Value::I32(1)],
        })
        .await
        .expect("raw update");
    assert_eq!(reply, Reply::Affected(1));

    // Zero-row SELECT reports Affected(0) — documented shape contract.
    let reply = x
        .execute(&Command::Sql {
            dialect: Dialect::Postgres,
            text: format!(r#"SELECT id FROM "{t}" WHERE id = $1"#),
            params: vec![Value::I32(404)],
        })
        .await
        .expect("raw empty select");
    assert_eq!(reply, Reply::Affected(0));

    sql(&x, &format!(r#"DROP TABLE IF EXISTS "{t}""#)).await;
}

/// `NULL` parameters bind as text-typed NULLs and rely on assignment casts —
/// pin that this holds for every column type the driver supports.
#[tokio::test]
#[ignore = "needs LACRYMIRA_PG_TEST_URL pointing at a scratch PostgreSQL"]
async fn null_params_across_column_types() {
    let x = executor().await;
    let t = table("nulls");
    sql(&x, &format!(r#"DROP TABLE IF EXISTS "{t}""#)).await;
    sql(
        &x,
        &format!(
            r#"CREATE TABLE "{t}" (
                   a INT, b BIGINT, c DOUBLE PRECISION, d BOOL, e TEXT,
                   f UUID, g TIMESTAMPTZ, h JSONB, i BYTEA, j TEXT[]
               )"#
        ),
    )
    .await;

    let columns = vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]
        .into_iter()
        .map(String::from)
        .collect();
    let reply = x
        .execute(&Command::Insert {
            table: t.clone(),
            columns,
            rows: vec![std::iter::repeat_n(Value::Null, 10).collect()],
            on_conflict: None,
        })
        .await
        .expect("all-NULL insert");
    let (_cols, rows) = unpack(reply);
    assert_eq!(rows, vec![vec![Value::Null; 10]], "NULLs stay NULL");

    sql(&x, &format!(r#"DROP TABLE IF EXISTS "{t}""#)).await;
}

/// `Command::Kv` is rejected before any server round trip — no fixture needed.
#[tokio::test]
async fn kv_command_rejected_without_db() {
    // Lazy pool: nothing connects until a statement runs, and the Kv arm
    // never issues one.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://lacrymira:no-round-trip@127.0.0.1:1/scratch")
        .expect("lazy pool does not connect");
    let x = PgExecutor::new(pool);

    let err = x
        .execute(&Command::Kv {
            namespace: "anything".into(),
            op: lacrymira_core::KvOp::Get {
                key: Value::Text("k".into()),
            },
        })
        .await
        .expect_err("KV must be rejected by the PG driver");
    assert!(matches!(err, lacrymira_core::Error::Config(_)), "{err:?}");
}
