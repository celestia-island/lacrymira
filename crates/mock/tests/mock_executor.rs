//! Mock executor behaviour: shape matching, the statement log, and the
//! unmatched-expectation guard.

use lacrymira::{AggExpr, Command, Reply, Value};
use lacrymira_mock::MockExecutor;

#[tokio::test]
async fn replies_match_by_command_shape() {
    let mock = MockExecutor::new();
    mock.push_reply(
        "count users",
        |c| matches!(c, Command::Count { table, .. } if table == "users"),
        Reply::Value(Some(Value::I64(7))),
    );
    mock.push_reply(
        "raw probe",
        |c| matches!(c, Command::Sql { text, .. } if text.contains("SELECT 1")),
        Reply::Affected(0),
    );

    let count = mock
        .execute(&Command::Count {
            table: "users".into(),
            filter: None,
        })
        .await
        .unwrap();
    assert!(matches!(count, Reply::Value(Some(Value::I64(7)))));

    // A raw Command with unrelated text must NOT consume the "raw probe"
    // expectation — shape predicates are per-item, and queue order does not
    // leak across shapes.
    let other = mock
        .execute(&Command::Sql {
            dialect: lacrymira::Dialect::Postgres,
            text: "SELECT 2".into(),
            params: vec![],
        })
        .await
        .unwrap();
    assert!(matches!(other, Reply::Unit));
    assert_eq!(mock.unmatched(), vec!["raw probe".to_string()]);

    // The still-queued expectation matches its own shape later.
    let raw = mock
        .execute(&Command::Sql {
            dialect: lacrymira::Dialect::Postgres,
            text: "SELECT 1".into(),
            params: vec![],
        })
        .await
        .unwrap();
    assert!(matches!(raw, Reply::Affected(0)));
    mock.assert_all_consumed();
}

#[tokio::test]
async fn select_and_count_never_cross_consume() {
    let mock = MockExecutor::new();
    mock.push_reply(
        "select t",
        |c| matches!(c, Command::Select { table, .. } if table == "t"),
        Reply::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::I64(1)]],
        },
    );
    mock.push_reply(
        "count t",
        |c| matches!(c, Command::Count { table, .. } if table == "t"),
        Reply::Value(Some(Value::I64(1))),
    );

    let _ = mock
        .execute(&Command::Count {
            table: "t".into(),
            filter: None,
        })
        .await
        .unwrap();
    let rows = mock
        .execute(&Command::Select {
            table: "t".into(),
            filter: None,
            order_by: vec![],
            limit: None,
            offset: None,
            lock: None,
            distinct: false,
        })
        .await
        .unwrap();
    assert!(matches!(rows, Reply::Rows { ref rows, .. } if rows.len() == 1));
    mock.assert_all_consumed();
}

#[tokio::test]
async fn aggregate_and_duplicate_commands_are_distinguishable() {
    let mock = MockExecutor::new();
    mock.push_reply_for(
        Command::Aggregate {
            table: "t".into(),
            select_expr: vec![(AggExpr::Count, None)],
            group_by: vec![],
            having: None,
            filter: None,
            order_by: vec![],
            limit: None,
        },
        Reply::Rows {
            columns: vec!["count".into()],
            rows: vec![vec![Value::I64(3)]],
        },
    );
    let out = mock
        .execute(&Command::Aggregate {
            table: "t".into(),
            select_expr: vec![(AggExpr::Count, None)],
            group_by: vec![],
            having: None,
            filter: None,
            order_by: vec![],
            limit: None,
        })
        .await
        .unwrap();
    assert!(matches!(out, Reply::Rows { .. }));
    mock.assert_all_consumed();
}

#[tokio::test]
async fn statement_log_records_every_command_in_order() {
    let mock = MockExecutor::new();
    let _ = mock
        .execute(&Command::Count {
            table: "a".into(),
            filter: None,
        })
        .await
        .unwrap();
    let _ = mock
        .execute(&Command::Count {
            table: "b".into(),
            filter: None,
        })
        .await
        .unwrap();
    let log = mock.statements();
    assert_eq!(log.len(), 2);
    assert!(matches!(&log[0], Command::Count { table, .. } if table == "a"));
    assert!(matches!(&log[1], Command::Count { table, .. } if table == "b"));
    assert_eq!(mock.statement_count(), 2);
}

#[tokio::test]
#[should_panic(expected = "unconsumed expectation")]
async fn assert_all_consumed_catches_a_leftover() {
    let mock = MockExecutor::new();
    mock.push_reply("never matches", |_| false, Reply::Unit);
    mock.assert_all_consumed();
}
