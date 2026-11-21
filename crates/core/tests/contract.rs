//! The msgpack wire-contract test: `Command` and `Reply` must round-trip
//! byte-identically through msgpack (encode → decode → re-encode).
//!
//! This is the M0 frozen contract. Once merged, any change that breaks
//! these tests is a **wire-format breaking change** and requires a version
//! bump of the wit world. The test exists to make that impossible to do
//! by accident.

use lacrymira::*;

fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(v: &T) -> (T, bool) {
    let first = rmp_serde::to_vec(v).expect("first encode");
    let decoded: T = rmp_serde::from_slice(&first).expect("decode");
    let second = rmp_serde::to_vec(&decoded).expect("re-encode");
    let identical = first == second;
    (decoded, identical)
}

fn assert_round_trip<
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
>(
    label: &str,
    v: &T,
) {
    let (decoded, identical) = round_trip(v);
    assert!(identical, "{}: re-encode differs from first encode", label);
    assert_eq!(&decoded, v, "{}: decoded value differs", label);
}

// ── Command variants ──

#[test]
fn command_sql_round_trips() {
    assert_round_trip(
        "Sql",
        &Command::Sql {
            dialect: Dialect::Postgres,
            text: "SELECT * FROM users WHERE id = $1".into(),
            params: vec![Value::Uuid("01234567-89ab-cdef-0123-456789abcdef".into())],
        },
    );
}

#[test]
fn command_select_round_trips() {
    assert_round_trip(
        "Select",
        &Command::Select {
            table: "billing_ledger".into(),
            filter: Some(Condition::All(vec![
                Condition::Eq(
                    ColumnRef {
                        name: "user_id".into(),
                    },
                    Value::Uuid("abc".into()),
                ),
                Condition::In(
                    ColumnRef {
                        name: "entry_type".into(),
                    },
                    vec![Value::Text("reserve".into()), Value::Text("refund".into())],
                ),
            ])),
            order_by: vec![("week_start".into(), true)],
            limit: Some(10),
            offset: None,
            lock: None,
        },
    );
}

#[test]
fn command_insert_with_upsert_round_trips() {
    assert_round_trip(
        "Insert+OnConflict",
        &Command::Insert {
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
        },
    );
}

#[test]
fn command_update_round_trips() {
    assert_round_trip(
        "Update",
        &Command::Update {
            table: "users".into(),
            sets: vec![("degrees".into(), Value::I32(100))],
            filter: Some(Condition::Gt(
                ColumnRef {
                    name: "degrees".into(),
                },
                Value::I32(50),
            )),
        },
    );
}

#[test]
fn command_delete_round_trips() {
    assert_round_trip(
        "Delete",
        &Command::Delete {
            table: "sessions".into(),
            filter: Some(Condition::Lt(
                ColumnRef {
                    name: "expires_at".into(),
                },
                Value::Timestamp("2026-01-01T00:00:00Z".into()),
            )),
        },
    );
}

#[test]
fn command_kv_round_trips() {
    assert_round_trip(
        "Kv.Get",
        &Command::Kv {
            namespace: "workspace".into(),
            op: KvOp::Get {
                key: Value::Text("ws-123".into()),
            },
        },
    );
    assert_round_trip(
        "Kv.Scan",
        &Command::Kv {
            namespace: "index".into(),
            op: KvOp::Scan {
                start: Some(Value::Text("a".into())),
                end: Some(Value::Text("z".into())),
                limit: Some(100),
            },
        },
    );
}

// ── Condition edge cases ──

#[test]
fn condition_nested_round_trips() {
    let deep = Condition::All(vec![
        Condition::Any(vec![
            Condition::Eq(ColumnRef { name: "a".into() }, Value::I64(1)),
            Condition::Not(Box::new(Condition::IsNull(ColumnRef { name: "b".into() }))),
        ]),
        Condition::Raw("LOWER(name) = $1".into(), vec![Value::Text("x".into())]),
        Condition::NotIn(
            ColumnRef { name: "c".into() },
            vec![Value::I32(1), Value::I32(2)],
        ),
    ]);
    assert_round_trip("DeepCondition", &deep);
}

#[test]
fn condition_all_value_types_round_trip() {
    let all_types = Condition::Eq(
        ColumnRef { name: "x".into() },
        Value::Array(vec![
            Value::Null,
            Value::Bool(true),
            Value::I32(-42),
            Value::I64(i64::MIN),
            Value::F64(std::f64::consts::PI),
            Value::Text("hello 世界 🌍".into()),
            Value::Bytes(vec![0, 1, 2, 255]),
            Value::Uuid("01234567-89ab-cdef-0123-456789abcdef".into()),
            Value::Timestamp("2026-11-21T15:30:00Z".into()),
            Value::Json("{\"nested\":[1,2,3]}".into()),
        ]),
    );
    assert_round_trip("AllValueTypes", &all_types);
}

// ── Reply variants ──

#[test]
fn reply_rows_round_trips() {
    assert_round_trip(
        "Reply.Rows",
        &Reply::Rows {
            columns: vec!["id".into(), "name".into(), "score".into()],
            rows: vec![
                vec![
                    Value::Uuid("a".into()),
                    Value::Text("alice".into()),
                    Value::F64(1.5),
                ],
                vec![
                    Value::Uuid("b".into()),
                    Value::Text("bob".into()),
                    Value::F64(2.5),
                ],
            ],
        },
    );
}

#[test]
fn reply_affected_round_trips() {
    assert_round_trip("Reply.Affected", &Reply::Affected(42));
}

#[test]
fn reply_entries_round_trips() {
    assert_round_trip(
        "Reply.Entries",
        &Reply::Entries(vec![
            (Value::Text("k1".into()), Value::Json("{\"v\":1}".into())),
            (Value::Text("k2".into()), Value::Null),
        ]),
    );
}

#[test]
fn reply_unit_round_trips() {
    assert_round_trip("Reply.Unit", &Reply::Unit);
}

// ── Error round-trip (wit 层也要传) ──

#[test]
fn error_round_trips() {
    assert_round_trip(
        "Error.Db",
        &Error::Db {
            kind: "unique_violation".into(),
            message: "duplicate key".into(),
            sqlstate: Some("23505".into()),
        },
    );
    assert_round_trip("Error.NotFound", &Error::NotFound);
    assert_round_trip("Error.Decode", &Error::Decode("bad uuid".into()));
}

// ── 确定性：同一输入两次编码字节相同 ──

#[test]
fn msgpack_encoding_is_deterministic() {
    let cmd = Command::Select {
        table: "t".into(),
        filter: Some(Condition::Eq(ColumnRef { name: "a".into() }, Value::I32(1))),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
    };
    let a = rmp_serde::to_vec(&cmd).unwrap();
    let b = rmp_serde::to_vec(&cmd).unwrap();
    assert_eq!(a, b, "same input must encode identically");
}

#[test]
fn command_count_round_trips() {
    assert_round_trip(
        "Count",
        &Command::Count {
            table: "billing_ledger".into(),
            filter: Some(Condition::Eq(
                ColumnRef {
                    name: "user_id".into(),
                },
                Value::Text("u1".into()),
            )),
        },
    );
}

#[test]
fn command_count_no_filter_round_trips() {
    assert_round_trip(
        "Count (no filter)",
        &Command::Count {
            table: "users".into(),
            filter: None,
        },
    );
}
