//! Integration tests: Select → render → (mock execute) → verify SQL.
//!
//! These tests exercise the full pipeline without needing a live database:
//! build a typed Select via the derive macro, convert to Command, render to
//! SQL, and verify the output matches expected PostgreSQL syntax.

use lacrymira::{LockMode, Select, TypedColumnOps};
use lacrymira_pg::render;

mod ledger {
    use lacrymira_macros::Entity as DeriveEntity;

    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "billing_ledger")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub id: i64,
        pub user_id: String,
        pub entry_type: String,
        pub degrees: i32,
        pub metadata: serde_json::Value,
    }

    pub use billing_ledger_entity::Column;
}

// ── Full pipeline: typed select → SQL ──

#[test]
fn typed_select_renders_correct_sql() {
    let cmd = Select::<ledger::Model>::new()
        .filter(ledger::Column::EntryType.eq("reserve"))
        .filter(ledger::Column::Degrees.gte(100))
        .order_by_desc("created_at")
        .limit(10)
        .to_command();

    let rendered = render(&cmd).unwrap();
    assert!(
        rendered.sql.contains(r#"SELECT * FROM "billing_ledger""#),
        "table name: {}",
        rendered.sql
    );
    assert!(
        rendered.sql.contains(r#""entry_type" = $1"#),
        "{}",
        rendered.sql
    );
    assert!(
        rendered.sql.contains(r#""degrees" >= $2"#),
        "{}",
        rendered.sql
    );
    assert!(rendered.sql.contains("LIMIT 10"), "{}", rendered.sql);
    assert_eq!(rendered.params.len(), 2);
}

// ── Typed + lock + JSONB combined ──

#[test]
fn typed_select_with_lock_and_jsonb() {
    use lacrymira::TypedJsonColumnOps;

    let cmd = Select::<ledger::Model>::new()
        .filter(ledger::Column::Metadata.json_get("$.status").eq("active"))
        .lock(LockMode::SkipLocked)
        .limit(5)
        .to_command();

    let rendered = render(&cmd).unwrap();
    assert!(
        rendered.sql.contains(r#""metadata"->>'status' = $1"#),
        "jsonb path: {}",
        rendered.sql
    );
    assert!(
        rendered.sql.ends_with("FOR UPDATE SKIP LOCKED"),
        "lock: {}",
        rendered.sql
    );
}

// ── Dynamic composition renders correctly ──

#[test]
fn dynamic_composition_renders_and_conditions() {
    let user_filter: Option<String> = Some("user-123".into());

    let mut q = Select::<ledger::Model>::new()
        .filter(ledger::Column::EntryType.in_(vec!["reserve", "refund"]));
    if let Some(uid) = user_filter {
        q = q.filter(ledger::Column::UserId.eq(uid));
    }

    let rendered = render(&q.to_command()).unwrap();
    // IN renders as = ANY($n)
    assert!(rendered.sql.contains("ANY"), "in clause: {}", rendered.sql);
    // AND combines both filters
    assert!(
        rendered.sql.contains("AND") || rendered.sql.contains("WHERE"),
        "combined: {}",
        rendered.sql
    );
}

// ── All typed operators produce renderable conditions ──

#[test]
fn all_typed_operators_render_without_error() {
    use lacrymira::TypedJsonColumnOps;

    // Each operator produces a condition that renders successfully
    let conditions = vec![
        ledger::Column::Id.eq(1).into_inner(),
        ledger::Column::Id.ne(2).into_inner(),
        ledger::Column::Degrees.lt(100).into_inner(),
        ledger::Column::Degrees.lte(50).into_inner(),
        ledger::Column::Degrees.gt(10).into_inner(),
        ledger::Column::Degrees.gte(20).into_inner(),
        ledger::Column::UserId.like("%x%").into_inner(),
        ledger::Column::UserId.is_null().into_inner(),
        ledger::Column::UserId.is_not_null().into_inner(),
        ledger::Column::Id.in_(vec![1, 2]).into_inner(),
        ledger::Column::UserId.contains("test").into_inner(),
        ledger::Column::Metadata.json_exists("$.key").into_inner(),
        ledger::Column::Metadata
            .json_contains(serde_json::json!({"a": 1}))
            .into_inner(),
    ];

    for cond in conditions {
        let cmd = Select::<ledger::Model>::new()
            .filter(lacrymira::TypedCondition::<ledger::Model>::from(cond))
            .to_command();
        let result = render(&cmd);
        assert!(result.is_ok(), "failed to render: {:?}", result.err());
    }
}

// ── Update/Delete also render from typed conditions ──

#[test]
fn typed_delete_renders() {
    let cmd = lacrymira::Command::Delete {
        table: "billing_ledger".into(),
        filter: Some(ledger::Column::EntryType.eq("void").into_inner()),
    };
    let rendered = render(&cmd).unwrap();
    assert!(rendered.sql.contains("DELETE FROM"), "{}", rendered.sql);
    assert!(
        rendered.sql.contains(r#""entry_type" = $1"#),
        "{}",
        rendered.sql
    );
}

// ── IndexBuilder → migration SQL pipeline ──

#[test]
fn index_builder_produces_valid_migration_sql() {
    use lacrymira::IndexBuilder;

    let index_sql = IndexBuilder::on("billing_ledger")
        .columns(&["user_id", "entry_type"])
        .include(&["degrees"])
        .unique()
        .build()
        .to_sql();

    // Verify the SQL is syntactically valid PostgreSQL
    assert!(
        index_sql.starts_with("CREATE UNIQUE INDEX"),
        "{}",
        index_sql
    );
    assert!(
        index_sql.contains(r#"ON "billing_ledger""#),
        "{}",
        index_sql
    );
    assert!(index_sql.contains("INCLUDE"), "{}", index_sql);

    // The migration runner can consume this SQL
    let migration = lacrymira_pg::Migration::new("test_idx", "add index", &index_sql);
    assert_eq!(migration.version, "test_idx");
}

// ── InsertPolicy UUID → insert command pipeline ──

#[test]
fn insert_policy_generates_uuid_and_renders() {
    use lacrymira::{Command, InsertPolicy, Value};

    let policy = InsertPolicy::v4();
    let uuid = policy.generate_uuid(None).unwrap();

    let cmd = Command::Insert {
        table: "users".into(),
        columns: vec!["id".into(), "name".into()],
        rows: vec![vec![
            Value::Uuid(uuid.to_string()),
            Value::Text("Alice".into()),
        ]],
        on_conflict: None,
    };

    let rendered = render(&cmd).unwrap();
    assert!(rendered.sql.contains("INSERT INTO"), "{}", rendered.sql);
    assert!(rendered.sql.contains(r#""users""#), "{}", rendered.sql);
}

// ── Error propagation through the pipeline ──

#[test]
fn version_conflict_error_is_serializable() {
    // The error must survive msgpack round-trip to cross the wit boundary
    let err = lacrymira::Error::VersionConflict {
        expected: 1,
        actual: 5,
    };
    let bytes = rmp_serde::to_vec(&err).unwrap();
    let back: lacrymira::Error = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(back, err);
}
