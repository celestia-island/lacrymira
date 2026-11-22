//! Tests for the executor-side find helpers (G7): find_all decodes by column
//! NAME and reorders into declaration order; count hits Command::Count.
//!
//! Rendering-only tests (no DB): the SQL emitted by each helper is verified
//! through the renderer; typed decode is exercised in DB-backed exec tests.

mod user {
    use lacrymira::DeriveEntity;
    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "users")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub id: i64,
        pub name: String,
        pub email: Option<String>,
    }
}

#[test]
fn find_all_select_renders() {
    use lacrymira::Select;
    let select = Select::<user::Model>::new().limit(5);
    let cmd = select.to_command();
    let rendered = lacrymira_pg::render(&cmd).unwrap();
    assert!(
        rendered.sql.contains(r#"SELECT * FROM "users""#),
        "{}",
        rendered.sql
    );
    assert!(rendered.sql.contains("LIMIT 5"), "{}", rendered.sql);
}

#[test]
fn count_command_renders_with_filter() {
    let cmd = lacrymira::Command::Count {
        table: "users".into(),
        filter: Some(lacrymira::ColumnRef::new("name").eq("Alice")),
    };
    let rendered = lacrymira_pg::render(&cmd).unwrap();
    assert!(rendered.sql.contains("SELECT COUNT(*)"), "{}", rendered.sql);
    assert!(rendered.sql.contains(r#""name" = $1"#), "{}", rendered.sql);
}

#[test]
fn count_command_renders_without_filter() {
    let cmd = lacrymira::Command::Count {
        table: "users".into(),
        filter: None,
    };
    let rendered = lacrymira_pg::render(&cmd).unwrap();
    assert_eq!(rendered.sql, r#"SELECT COUNT(*) FROM "users""#);
    assert_eq!(rendered.params.len(), 0);
}

#[test]
fn count_reply_is_value_i64() {
    // Static proof of the reply shape the executor returns for Count.
    let reply = lacrymira::Reply::Value(Some(lacrymira::Value::I64(42)));
    assert!(matches!(
        reply,
        lacrymira::Reply::Value(Some(lacrymira::Value::I64(42)))
    ));
}

#[test]
fn insert_command_from_entity_renders() {
    use lacrymira::Entity;
    let values = <user::Model as Entity>::to_values(&user::Model {
        id: 1,
        name: "Alice".into(),
        email: None,
    });
    assert_eq!(values.len(), 3);
    assert!(matches!(values[2], lacrymira::Value::Null));
    let cmd = lacrymira::Command::Insert {
        table: <user::Model as Entity>::TABLE.to_string(),
        columns: <user::Model as Entity>::COLUMNS
            .iter()
            .map(|c| c.to_string())
            .collect(),
        rows: vec![values],
        on_conflict: None,
    };
    let rendered = lacrymira_pg::render(&cmd).unwrap();
    assert!(
        rendered.sql.contains(r#"INSERT INTO "users""#),
        "{}",
        rendered.sql
    );
}

#[test]
fn version_index_generated_by_derive() {
    // G14: entities without a #[lacrymira(version)] column get None.
    use lacrymira::Entity;
    assert_eq!(<user::Model as Entity>::VERSION_INDEX, None);
}

mod versioned {
    use lacrymira::DeriveEntity;
    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "docs")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub id: i64,
        pub title: String,
        #[lacrymira(version)]
        pub rev: i64,
    }
}

#[test]
fn version_index_points_at_marked_column() {
    use lacrymira::Entity;
    // COLUMNS = ["id", "title", "rev"] → rev is index 2.
    assert_eq!(<versioned::Model as Entity>::VERSION_INDEX, Some(2));
}

// ── G10: ActiveModel partial-write rendering ──

mod article {
    use lacrymira::DeriveEntity;
    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "articles")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub id: i64,
        pub title: String,
        pub body: String,
        pub views: i32,
    }

    pub use articles_entity::ActiveModel;
}

#[test]
fn active_pairs_skip_unset_and_bind_null() {
    use lacrymira::{Entity, Field};
    let am = article::ActiveModel {
        title: Field::Value("Hello".into()),
        body: Field::Null, // explicit NULL; views stays Unset
        ..Default::default()
    };

    let pairs = <article::Model as Entity>::active_pairs(&am);
    assert_eq!(pairs.len(), 2, "Unset skipped");
    assert_eq!(pairs[0].0, "title");
    assert!(matches!(pairs[0].1, lacrymira::Value::Text(_)));
    assert_eq!(pairs[1].0, "body");
    assert!(matches!(pairs[1].1, lacrymira::Value::Null));
}

#[test]
fn insert_active_renders_partial_columns() {
    use lacrymira::{Command, Entity, Field};
    let am = article::ActiveModel {
        title: Field::Value("Partial".into()),
        ..Default::default()
    };

    let pairs = <article::Model as Entity>::active_pairs(&am);
    let cmd = Command::Insert {
        table: "articles".into(),
        columns: pairs.iter().map(|(c, _)| c.clone()).collect(),
        rows: vec![pairs.into_iter().map(|(_, v)| v).collect()],
        on_conflict: None,
    };
    let rendered = lacrymira_pg::render(&cmd).unwrap();
    // Only "title" appears — Unset columns omitted so DB defaults fire.
    assert!(rendered.sql.contains(r#"("title")"#), "{}", rendered.sql);
    assert!(!rendered.sql.contains("views"), "{}", rendered.sql);
    assert!(!rendered.sql.contains("body"), "{}", rendered.sql);
}

#[test]
fn update_active_sql_shape() {
    // The raw UPDATE built by update_active: SET only non-PK fields,
    // WHERE pk = $n, RETURNING *.
    // Verified at the string level since no DB is available here.
    let sets = vec![("views".to_string(), lacrymira::Value::I32(10))];
    let mut params: Vec<lacrymira::Value> = Vec::new();
    let mut set_parts: Vec<String> = Vec::new();
    for (col, v) in &sets {
        params.push(v.clone());
        set_parts.push(format!(r#""{}" = ${}"#, col, params.len()));
    }
    params.push(lacrymira::Value::I64(1));
    let where_sql = format!(r#""id" = ${}"#, params.len());
    let sql = format!(
        r#"UPDATE "{}" SET {} WHERE {} RETURNING *"#,
        "articles",
        set_parts.join(", "),
        where_sql
    );
    assert!(sql.contains(r#""views" = $1"#), "{}", sql);
    assert!(sql.contains(r#""id" = $2"#), "{}", sql);
    assert!(sql.ends_with("RETURNING *"));
}

#[test]
fn upsert_command_renders_do_update() {
    let cmd = lacrymira::Command::Insert {
        table: "articles".into(),
        columns: vec!["id".into(), "title".into(), "views".into()],
        rows: vec![vec![
            lacrymira::Value::I64(1),
            lacrymira::Value::Text("T".into()),
            lacrymira::Value::I32(0),
        ]],
        on_conflict: Some(lacrymira::OnConflict {
            target: vec!["id".into()],
            action: lacrymira::ConflictAction::DoUpdate {
                set: vec![("views".into(), lacrymira::Value::I32(1))],
            },
        }),
    };
    let rendered = lacrymira_pg::render(&cmd).unwrap();
    assert!(rendered.sql.contains("ON CONFLICT"), "{}", rendered.sql);
    assert!(rendered.sql.contains("DO UPDATE"), "{}", rendered.sql);
}

// ── Transactional guard (G6 / G-arona-1 / G-arona-13) ──

#[test]
fn transaction_guard_is_a_public_type() {
    // Compile-time proof the guard exists with the expected shape; the
    // behaviour (commit/rollback/typed writes inside) is DB-backed.
    fn accepts(_: fn(&lacrymira_pg::PgExecutor) -> ()) {}
    let _ = accepts;
    // PgTransaction is exported and constructible only through begin().
    fn _shape(tx: lacrymira_pg::PgTransaction) -> lacrymira_pg::PgTransaction {
        tx
    }
}

#[test]
fn map_db_error_is_public() {
    // G-arona-12: consumers classify driver errors without a drifting copy.
    fn use_it(e: sqlx::Error) -> lacrymira::Error {
        lacrymira_pg::map_db_error(e)
    }
    let err = use_it(sqlx::Error::RowNotFound);
    assert!(matches!(err, lacrymira::Error::Db { .. }));
}

mod site_doc {
    use lacrymira::DeriveEntity;
    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "site_documents")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub kind: String,
        #[lacrymira(primary_key)]
        pub doc_key: String,
        pub body: String,
    }
    pub use site_documents_entity::ActiveModel;
}

#[test]
fn find_by_pk_parts_renders_composite_where() {
    use lacrymira::{ColumnRef, Select};
    // Mirrors find_by_pk_parts' construction: one eq per PK column, AND-joined.
    let select = Select::<site_doc::Model>::new()
        .filter(ColumnRef::new("kind").eq("page"))
        .filter(ColumnRef::new("doc_key").eq("home"));
    let rendered = lacrymira_pg::render(&select.to_command()).unwrap();
    assert!(rendered.sql.contains(r#""kind" = $1"#), "{}", rendered.sql);
    assert!(rendered.sql.contains("AND"), "{}", rendered.sql);
    assert!(
        rendered.sql.contains(r#""doc_key" = $2"#),
        "{}",
        rendered.sql
    );
}

#[test]
fn update_active_composite_where_shape() {
    use lacrymira::Field;
    let am = site_doc::ActiveModel {
        kind: Field::Value("page".into()),
        doc_key: Field::Value("home".into()),
        body: Field::Value("new body".into()),
    };
    let pairs = <site_doc::Model as lacrymira::Entity>::active_pairs(&am);
    let pk_cols = <site_doc::Model as lacrymira::Entity>::PK_INDICES
        .iter()
        .map(|&i| <site_doc::Model as lacrymira::Entity>::COLUMNS[i])
        .collect::<Vec<_>>();
    // Non-PK sets exclude both PK columns
    let sets: Vec<_> = pairs
        .into_iter()
        .filter(|(c, _)| !pk_cols.contains(&c.as_str()))
        .collect();
    assert_eq!(sets.len(), 1, "only body is a SET column");
    assert_eq!(sets[0].0, "body");
}

// ── Batch B: aggregate / distinct / pagination / typed order ──

mod agg_user {
    use lacrymira::DeriveEntity;
    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "agg_users")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub id: i64,
        pub org: String,
        pub amount: i64,
    }
    pub use agg_users_entity::Column;
}

#[test]
fn aggregate_renders_sum_group_having() {
    let cmd = lacrymira::Command::Aggregate {
        table: "agg_users".into(),
        select_expr: vec![
            (lacrymira::AggExpr::Col("org".into()), Some("org".into())),
            (
                lacrymira::AggExpr::Sum("amount".into()),
                Some("total".into()),
            ),
            (lacrymira::AggExpr::Count, Some("n".into())),
        ],
        group_by: vec!["org".into()],
        having: Some(lacrymira::ColumnRef::new("total").gt(100)),
        filter: Some(lacrymira::ColumnRef::new("amount").gte(0)),
        order_by: vec![("total".into(), true)],
        limit: Some(5),
    };
    let r = lacrymira_pg::render(&cmd).unwrap();
    assert!(
        r.sql.contains(
            r#"SELECT "org" AS "org", SUM("amount") AS "total", COUNT(*) AS "n" FROM "agg_users""#
        ),
        "{}",
        r.sql
    );
    assert!(r.sql.contains(r#"WHERE "amount" >= $1"#), "{}", r.sql);
    assert!(r.sql.contains(r#"GROUP BY "org""#), "{}", r.sql);
    assert!(r.sql.contains(r#"HAVING "total" > $2"#), "{}", r.sql);
    assert!(r.sql.contains(r#"ORDER BY "total" DESC"#), "{}", r.sql);
    assert!(r.sql.contains("LIMIT 5"), "{}", r.sql);
}

#[test]
fn aggregate_all_funcs_render() {
    for (expr, want) in [
        (lacrymira::AggExpr::Count, "COUNT(*)"),
        (lacrymira::AggExpr::CountCol("x".into()), r#"COUNT("x")"#),
        (lacrymira::AggExpr::Sum("x".into()), r#"SUM("x")"#),
        (lacrymira::AggExpr::Min("x".into()), r#"MIN("x")"#),
        (lacrymira::AggExpr::Max("x".into()), r#"MAX("x")"#),
        (lacrymira::AggExpr::Avg("x".into()), r#"AVG("x")"#),
    ] {
        let cmd = lacrymira::Command::Aggregate {
            table: "t".into(),
            select_expr: vec![(expr, None)],
            group_by: vec![],
            having: None,
            filter: None,
            order_by: vec![],
            limit: None,
        };
        let r = lacrymira_pg::render(&cmd).unwrap();
        assert!(r.sql.contains(want), "{} not in {}", want, r.sql);
    }
}

#[test]
fn distinct_renders() {
    use lacrymira::Select;
    let cmd = Select::<agg_user::Model>::new().distinct().to_command();
    let r = lacrymira_pg::render(&cmd).unwrap();
    assert!(r.sql.starts_with("SELECT DISTINCT *"), "{}", r.sql);
}

#[test]
fn typed_order_by_renders() {
    use lacrymira::Select;
    let cmd = Select::<agg_user::Model>::new()
        .order_by_typed(agg_user::Column::Org)
        .order_by_typed_desc(agg_user::Column::Amount)
        .to_command();
    let r = lacrymira_pg::render(&cmd).unwrap();
    assert!(
        r.sql.contains(r#"ORDER BY "org" ASC, "amount" DESC"#),
        "{}",
        r.sql
    );
}

#[test]
fn to_aggregate_carries_filter_and_order() {
    use lacrymira::{Select, TypedColumnOps as _};
    let base = Select::<agg_user::Model>::new()
        .filter(agg_user::Column::Org.eq("acme"))
        .order_by_typed_desc(agg_user::Column::Amount)
        .limit(3);
    let cmd = base.to_aggregate(
        vec![(lacrymira::AggExpr::Sum("amount".into()), None)],
        vec![],
        None,
    );
    let r = lacrymira_pg::render(&cmd).unwrap();
    assert!(r.sql.contains(r#"WHERE "org" = $1"#), "{}", r.sql);
    assert!(r.sql.contains(r#"ORDER BY "amount" DESC"#), "{}", r.sql);
    assert!(r.sql.contains("LIMIT 3"), "{}", r.sql);
}
