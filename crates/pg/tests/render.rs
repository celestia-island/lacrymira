//! Tests for the PostgreSQL SQL renderer.

use lacrymira::*;
use lacrymira_pg::render;

#[test]
fn select_no_filter() {
    let cmd = Command::Select {
        table: "users".into(),
        filter: None,
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
    };
    let r = render(&cmd).unwrap();
    assert_eq!(r.sql, r#"SELECT * FROM "users""#);
    assert_eq!(r.params.len(), 0);
}

#[test]
fn select_with_filter_order_limit() {
    let cmd = Command::Select {
        table: "billing_ledger".into(),
        filter: Some(Condition::All(vec![
            Condition::Eq(ColumnRef::new("user_id"), Value::Uuid("abc".into())),
            Condition::In(
                ColumnRef::new("entry_type"),
                vec![Value::Text("reserve".into()), Value::Text("refund".into())],
            ),
        ])),
        order_by: vec![("week_start".into(), true)],
        limit: Some(10),
        offset: Some(20),
        lock: None,
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.contains(r#""user_id" = $1"#), "got: {}", r.sql);
    assert!(
        r.sql.contains(r#""entry_type" = ANY($2)"#),
        "IN must render as = ANY: {}",
        r.sql
    );
    assert!(r.sql.contains(r#"ORDER BY "week_start" DESC"#));
    assert!(r.sql.contains("LIMIT 10"));
    assert!(r.sql.contains("OFFSET 20"));
    assert_eq!(r.params.len(), 2);
}

#[test]
fn select_all_18_operators() {
    let col = |n: &str| ColumnRef::new(n);
    let conditions = vec![
        (col("a").eq(1), r#""a" = $1"#),
        (col("b").ne(2), r#""b" != $1"#),
        (col("c").lt(3), r#""c" < $1"#),
        (col("d").lte(4), r#""d" <= $1"#),
        (col("e").gt(5), r#""e" > $1"#),
        (col("f").gte(6), r#""f" >= $1"#),
        (col("g").like("%x%"), r#""g" LIKE $1"#),
        (col("h").ilike("%x%"), r#""h" ILIKE $1"#),
        (col("i").is_null(), r#""i" IS NULL"#),
        (col("j").is_not_null(), r#""j" IS NOT NULL"#),
        (col("k").in_(vec![1, 2]), r#""k" = ANY($1"#),
        (col("l").not_in(vec![1]), r#""l" != ANY($"#),
        (col("m").contains("x"), r#""m" LIKE $"#),
        (col("n").starts_with("x"), r#""n" LIKE $"#),
        (col("o").ends_with("x"), r#""o" LIKE $"#),
        // between → All(Gte, Lte)
        (col("p").between(1, 10), r#"("p" >= $"#),
        // raw
        (
            Condition::Raw("LOWER(q) = ${}".into(), vec![Value::Text("x".into())]),
            r#"LOWER(q) = $"#,
        ),
        // not
        (Condition::Not(Box::new(col("r").eq(1))), r#"NOT ("r" = $"#),
    ];

    for (i, (cond, expected_fragment)) in conditions.iter().enumerate() {
        let cmd = Command::Select {
            table: "t".into(),
            filter: Some(cond.clone()),
            order_by: vec![],
            limit: None,
            offset: None,
            lock: None,
        };
        let r = render(&cmd).unwrap();
        assert!(
            r.sql.contains(expected_fragment),
            "operator #{}: expected {} in {}",
            i,
            expected_fragment,
            r.sql
        );
    }
}

#[test]
fn insert_with_upsert() {
    let cmd = Command::Insert {
        table: "kv_store".into(),
        columns: vec!["key".into(), "value".into()],
        rows: vec![vec![
            Value::Text("k1".into()),
            Value::Json("{\"a\":1}".into()),
        ]],
        on_conflict: Some(OnConflict {
            target: vec!["key".into()],
            action: ConflictAction::DoUpdate {
                set: vec![("value".into(), Value::Text("EXCLUDED.value".into()))],
            },
        }),
    };
    let r = render(&cmd).unwrap();
    assert!(
        r.sql.contains(r#"INSERT INTO "kv_store" ("key", "value")"#),
        "got: {}",
        r.sql
    );
    assert!(r.sql.contains("VALUES ($1, $2)"));
    assert!(r.sql.contains(r#"ON CONFLICT ("key") DO UPDATE SET"#));
    assert!(r.sql.contains(r#""value" = EXCLUDED.value"#));
    assert!(r.sql.contains("RETURNING *"));
}

#[test]
fn insert_do_nothing() {
    let cmd = Command::Insert {
        table: "t".into(),
        columns: vec!["id".into()],
        rows: vec![vec![Value::I64(1)]],
        on_conflict: Some(OnConflict {
            target: vec!["id".into()],
            action: ConflictAction::DoNothing,
        }),
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.contains(r#"ON CONFLICT ("id") DO NOTHING"#));
}

#[test]
fn update_with_where() {
    let cmd = Command::Update {
        table: "users".into(),
        sets: vec![("degrees".into(), Value::I32(100))],
        filter: Some(Condition::Gt(ColumnRef::new("degrees"), Value::I32(50))),
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.contains(r#"UPDATE "users" SET "degrees" = $1"#));
    assert!(r.sql.contains(r#""degrees" > $2"#));
    assert!(r.sql.contains("RETURNING *"));
}

#[test]
fn delete_with_where() {
    let cmd = Command::Delete {
        table: "sessions".into(),
        filter: Some(Condition::Lt(
            ColumnRef::new("expires_at"),
            Value::Timestamp("2026-01-01T00:00:00Z".into()),
        )),
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.contains(r#"DELETE FROM "sessions""#));
    assert!(r.sql.contains(r#""expires_at" < $1"#));
}

#[test]
fn raw_sql_passthrough() {
    let cmd = Command::Sql {
        dialect: Dialect::Postgres,
        text: "SELECT count(*) FROM $1::regclass".into(),
        params: vec![Value::Text("users".into())],
    };
    let r = render(&cmd).unwrap();
    assert_eq!(r.sql, "SELECT count(*) FROM $1::regclass");
    assert_eq!(r.params.len(), 1);
}

#[test]
fn nested_or_and() {
    let cond = Condition::Any(vec![
        Condition::Eq(ColumnRef::new("a"), Value::I32(1)),
        Condition::All(vec![
            Condition::Gt(ColumnRef::new("b"), Value::I32(2)),
            Condition::Lt(ColumnRef::new("c"), Value::I32(3)),
        ]),
    ]);
    let cmd = Command::Select {
        table: "t".into(),
        filter: Some(cond),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
    };
    let r = render(&cmd).unwrap();
    assert!(
        r.sql.contains(r#"("a" = $1 OR ("b" > $2 AND "c" < $3))"#),
        "got: {}",
        r.sql
    );
}

#[test]
fn null_equality_becomes_is_null() {
    let cmd = Command::Select {
        table: "t".into(),
        filter: Some(Condition::Eq(ColumnRef::new("x"), Value::Null)),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.contains(r#""x" IS NULL"#), "got: {}", r.sql);
}

#[test]
fn identifier_quoting_escapes_embedded_quotes() {
    let cmd = Command::Select {
        table: "weird\"table".into(),
        filter: None,
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.contains(r#""weird""table""#), "got: {}", r.sql);
}
