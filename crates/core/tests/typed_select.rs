//! Type-safe Select: Column enum → TypedCondition → Select<E>.

mod user {
    use lacrymira_macros::Entity as DeriveEntity;

    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "users")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub id: i64,
        pub name: String,
        pub email: String,
        pub age: i32,
    }

    // Re-export Column so tests outside this module can use it
    pub use users_entity::Column;
}

mod order {
    use lacrymira_macros::Entity as DeriveEntity;

    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "orders")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub id: i64,
        pub user_id: i64,
        pub total: f64,
    }
}

// ── Zero hand-written string column names ──

#[test]
fn typed_column_eq() {
    use lacrymira_core::TypedColumnOps;
    let cond = user::Column::Name.eq("Alice");
    let inner = cond.into_inner();
    assert!(matches!(
        inner,
        lacrymira_core::Condition::Eq(ref col, _) if col.name == "name"
    ));
}

#[test]
fn typed_composition() {
    use lacrymira_core::TypedColumnOps;
    let cond = user::Column::Age.gt(25).and(user::Column::Age.lt(65));
    let inner = cond.into_inner();
    assert!(matches!(inner, lacrymira_core::Condition::All(_)));
}

// ── Dynamic composition pin (diesel lesson) ──

#[test]
fn typed_dynamic_composition_pin() {
    use lacrymira_core::{Select, TypedColumnOps};

    let age_filter: Option<i32> = Some(30);
    let mut q = Select::<user::Model>::new().filter(user::Column::Name.like("%Alice%"));
    if let Some(age) = age_filter {
        q = q.filter(user::Column::Age.gte(age));
    }
    let cmd = q.to_command();
    assert!(matches!(cmd, lacrymira_core::Command::Select { .. }));
}

// ── Full typed query ──

#[test]
fn typed_select_full_query() {
    use lacrymira_core::{Select, TypedColumnOps};

    let q = Select::<user::Model>::new()
        .filter(user::Column::Age.between(18, 65))
        .filter(user::Column::Email.is_not_null())
        .order_by_desc("name")
        .limit(10);

    let cmd = q.to_command();
    if let lacrymira_core::Command::Select { limit, .. } = cmd {
        assert_eq!(limit, Some(10));
    } else {
        panic!("expected Select");
    }
}

// ── All operators through typed path ──

#[test]
fn all_operators_typed() {
    use lacrymira_core::TypedColumnOps;

    let _c1 = user::Column::Id.eq(1);
    let _c2 = user::Column::Id.ne(2);
    let _c3 = user::Column::Id.lt(3);
    let _c4 = user::Column::Id.lte(4);
    let _c5 = user::Column::Id.gt(5);
    let _c6 = user::Column::Id.gte(6);
    let _c7 = user::Column::Id.is_null();
    let _c8 = user::Column::Id.is_not_null();
    let _c9 = user::Column::Id.in_(vec![1, 2]);
    let _c10 = user::Column::Id.not_in(vec![3]);
    let _s1 = user::Column::Name.like("%x%");
    let _s2 = user::Column::Name.ilike("%y%");
    let _s3 = user::Column::Name.contains("z");
    let _s4 = user::Column::Name.starts_with("a");
    let _s5 = user::Column::Name.ends_with("b");
    let _r1 = user::Column::Age.between(18, 65);
    let _r2 = user::Column::Age.not_between(0, 17);
    // 17 operators verified (18th is overlaps, needs Vec<Value>)
}

// ── Same entity works; cross-entity would be compile error ──

#[test]
fn same_entity_column_works() {
    use lacrymira_core::{Select, TypedColumnOps};
    let q = Select::<user::Model>::new().filter(user::Column::Id.eq(42));
    assert!(matches!(
        q.to_command(),
        lacrymira_core::Command::Select { .. }
    ));
}

// ── Escape hatch: raw Condition accepted ──

#[test]
fn raw_condition_escape_hatch() {
    use lacrymira_core::{ColumnRef, Select};
    let q = Select::<user::Model>::new().filter(ColumnRef::new("any_column").eq("any_value"));
    assert!(matches!(
        q.to_command(),
        lacrymira_core::Command::Select { .. }
    ));
}

// ── TypedCondition composition ──

#[test]
fn typed_and_or_not() {
    use lacrymira_core::{Select, TypedColumnOps};
    let cond = user::Column::Age
        .gt(18)
        .and(user::Column::Age.lt(65))
        .or(user::Column::Name.eq("Admin"))
        .negate();
    let q = Select::<user::Model>::new().filter(cond);
    assert!(matches!(
        q.to_command(),
        lacrymira_core::Command::Select { .. }
    ));
}
