//! The dynamic-composition compile pin.
//!
//! This test is a **verbatim copy** of the pattern used 60 times across the
//! family (chest `billing_reconcile.rs:49` is the canonical example). If this
//! file fails to compile, the design is wrong — see the diesel lesson in the
//! plan (§3.2 of _reports/orm-strategy-20261121/PLAN.md).

use lacrymira_core::*;

// A minimal entity for the test (M2 will generate this from a derive).
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerModel {
    pub id: i64,
    pub user_id: String,
    pub entry_type: String,
    pub week_start: String,
    pub degrees: i32,
}

pub struct LedgerEntity;
pub struct LedgerPk;

impl PrimaryKey for LedgerPk {
    type Value = i64;
}

impl Entity for LedgerEntity {
    const TABLE: &'static str = "billing_ledger";
    const SCHEMA: &'static str = "public";
    const COLUMN_COUNT: usize = 5;
    const COLUMNS: &'static [&'static str] =
        &["id", "user_id", "entry_type", "week_start", "degrees"];
    const PK_INDICES: &'static [usize] = &[0];
    type Model = LedgerModel;
    type ActiveModel = LedgerModel; // simplified for the pin
    type PrimaryKey = LedgerPk;

    fn to_values(model: &Self::Model) -> Vec<Value> {
        vec![
            Value::I64(model.id),
            Value::Text(model.user_id.clone()),
            Value::Text(model.entry_type.clone()),
            Value::Text(model.week_start.clone()),
            Value::I32(model.degrees),
        ]
    }

    fn from_values(values: &[Value]) -> Option<Self::Model> {
        if values.len() != 5 {
            return None;
        }
        Some(LedgerModel {
            id: values[0].clone().try_into().ok()?,
            user_id: values[1].clone().try_into().ok()?,
            entry_type: values[2].clone().try_into().ok()?,
            week_start: values[3].clone().try_into().ok()?,
            degrees: values[4].clone().try_into().ok()?,
        })
    }
}

/// **THE PIN**: `let mut query = ...; if cond { query = query.filter(...); }`
/// must compile. This is the exact shape used in chest/arona/erp/entelecheia.
#[test]
fn dynamic_composition_pin_compiles() {
    let user_id: Option<String> = Some("user-123".to_owned());

    // Verbatim from billing_reconcile.rs:49
    let mut query = Select::<LedgerEntity>::new()
        .filter(ColumnRef::new("entry_type").in_(vec!["reserve", "refund"]));
    if let Some(uid) = user_id {
        query = query.filter(ColumnRef::new("user_id").eq(uid));
    }

    let cmd = query.to_command();
    let filter = match cmd {
        Command::Select {
            filter: Some(f), ..
        } => f,
        _ => panic!("expected Select with filter"),
    };
    assert!(filter.is_composite(), "both filters must be present");
}

/// Store a query in a variable, pass it to a function, extend it there.
#[test]
fn query_can_be_passed_and_extended() {
    fn add_date_filter(q: Select<LedgerEntity>) -> Select<LedgerEntity> {
        q.filter(ColumnRef::new("week_start").gte("2026-01-01"))
    }

    let q = Select::<LedgerEntity>::new().filter(ColumnRef::new("degrees").gt(50));
    let q2 = add_date_filter(q);
    let cmd = q2.to_command();
    assert!(matches!(
        cmd,
        Command::Select {
            filter: Some(_),
            ..
        }
    ));
}

/// Multiple conditions in sequence.
#[test]
fn chained_filters_accumulate() {
    let q = Select::<LedgerEntity>::new()
        .filter(ColumnRef::new("a").eq(1))
        .filter(ColumnRef::new("b").eq(2))
        .filter(ColumnRef::new("c").eq(3));

    let cmd = q.to_command();
    if let Command::Select {
        filter: Some(Condition::All(conds)),
        ..
    } = cmd
    {
        assert_eq!(
            conds.len(),
            3,
            "three filters should AND into 3 sub-conditions"
        );
    } else {
        panic!("expected All(3)");
    }
}

/// Condition operators cover the full set (18 comparison operators).
#[test]
fn all_18_operators_available() {
    let col = ColumnRef::new("test_col");
    let conditions = vec![
        col.clone().eq(1),
        col.clone().ne(1),
        col.clone().lt(1),
        col.clone().lte(1),
        col.clone().gt(1),
        col.clone().gte(1),
        col.clone().like("%x%"),
        col.clone().ilike("%x%"),
        col.clone().is_null(),
        col.clone().is_not_null(),
        col.clone().in_(vec![1, 2, 3]),
        col.clone().not_in(vec![1, 2, 3]),
        col.clone().contains("x"),
        col.clone().starts_with("x"),
        col.clone().ends_with("x"),
        col.clone().between(1, 10),
        col.clone().not_between(1, 10),
        col.clone().overlaps(vec![Value::I32(1)]),
    ];
    assert_eq!(conditions.len(), 18, "exactly 18 operators");
}

// Helper for the Condition check
trait ConditionExt {
    fn is_composite(&self) -> bool;
}

impl ConditionExt for Condition {
    fn is_composite(&self) -> bool {
        matches!(self, Condition::All(conds) if conds.len() >= 2)
    }
}
