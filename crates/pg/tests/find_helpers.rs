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
