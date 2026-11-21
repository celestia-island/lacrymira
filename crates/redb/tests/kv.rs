//! Tests for the redb KV driver.

use lacrymira_core::*;
use lacrymira_redb::RedbStore;

fn store() -> RedbStore {
    RedbStore::open_in_memory().expect("open in-memory store")
}

#[test]
fn set_get_round_trip() {
    let s = store();
    let set = Command::Kv {
        namespace: "test".into(),
        op: KvOp::Set {
            key: Value::Text("hello".into()),
            value: Value::Text("world".into()),
        },
    };
    s.execute(&set).unwrap();

    let get = Command::Kv {
        namespace: "test".into(),
        op: KvOp::Get {
            key: Value::Text("hello".into()),
        },
    };
    match s.execute(&get).unwrap() {
        Reply::Value(Some(Value::Text(v))) => assert_eq!(v, "world"),
        other => panic!("expected Value(Some(Text)), got {:?}", other),
    }
}

#[test]
fn get_missing_returns_none() {
    let s = store();
    let cmd = Command::Kv {
        namespace: "test".into(),
        op: KvOp::Get {
            key: Value::Text("nope".into()),
        },
    };
    assert_eq!(s.execute(&cmd).unwrap(), Reply::Value(None));
}

#[test]
fn delete_removes_entry() {
    let s = store();
    let key = Value::Text("k".into());
    s.execute(&Command::Kv {
        namespace: "ns".into(),
        op: KvOp::Set {
            key: key.clone(),
            value: Value::I64(42),
        },
    })
    .unwrap();

    let del = Command::Kv {
        namespace: "ns".into(),
        op: KvOp::Delete { key: key.clone() },
    };
    assert_eq!(s.execute(&del).unwrap(), Reply::Affected(1));
    // Second delete affects 0
    assert_eq!(s.execute(&del).unwrap(), Reply::Affected(0));
}

#[test]
fn namespaces_are_isolated() {
    let s = store();
    let key = Value::Text("shared-key".into());
    s.execute(&Command::Kv {
        namespace: "ns1".into(),
        op: KvOp::Set {
            key: key.clone(),
            value: Value::Text("from-ns1".into()),
        },
    })
    .unwrap();
    s.execute(&Command::Kv {
        namespace: "ns2".into(),
        op: KvOp::Set {
            key: key.clone(),
            value: Value::Text("from-ns2".into()),
        },
    })
    .unwrap();

    let r1 = s
        .execute(&Command::Kv {
            namespace: "ns1".into(),
            op: KvOp::Get { key: key.clone() },
        })
        .unwrap();
    let r2 = s
        .execute(&Command::Kv {
            namespace: "ns2".into(),
            op: KvOp::Get { key },
        })
        .unwrap();
    assert_eq!(r1, Reply::Value(Some(Value::Text("from-ns1".into()))));
    assert_eq!(r2, Reply::Value(Some(Value::Text("from-ns2".into()))));
}

#[test]
fn scan_returns_range() {
    let s = store();
    for i in 0..5 {
        s.execute(&Command::Kv {
            namespace: "scan".into(),
            op: KvOp::Set {
                key: Value::Text(format!("key-{}", i)),
                value: Value::I64(i as i64),
            },
        })
        .unwrap();
    }
    let scan = Command::Kv {
        namespace: "scan".into(),
        op: KvOp::Scan {
            start: None,
            end: None,
            limit: Some(3),
        },
    };
    match s.execute(&scan).unwrap() {
        Reply::Entries(entries) => assert_eq!(entries.len(), 3),
        other => panic!("expected Entries, got {:?}", other),
    }
}

#[test]
fn all_value_types_round_trip() {
    let s = store();
    let values = vec![
        Value::Null,
        Value::Bool(true),
        Value::I32(-42),
        Value::I64(i64::MAX),
        Value::F64(3.15),
        Value::Text("unicode: 你好 🌍".into()),
        Value::Bytes(vec![0, 255, 128]),
        Value::Uuid("01234567-89ab-cdef-0123-456789abcdef".into()),
        Value::Timestamp("2026-11-21T15:30:00Z".into()),
        Value::Json("{\"a\":[1,2]}".into()),
    ];
    for (i, v) in values.iter().enumerate() {
        let key = Value::Text(format!("val-{}", i));
        s.execute(&Command::Kv {
            namespace: "types".into(),
            op: KvOp::Set {
                key,
                value: v.clone(),
            },
        })
        .unwrap();
    }
    for (i, expected) in values.iter().enumerate() {
        let get = Command::Kv {
            namespace: "types".into(),
            op: KvOp::Get {
                key: Value::Text(format!("val-{}", i)),
            },
        };
        match s.execute(&get).unwrap() {
            Reply::Value(Some(actual)) => assert_eq!(&actual, expected, "type #{}", i),
            other => panic!("type #{}: got {:?}", i, other),
        }
    }
}

#[test]
fn non_kv_command_rejected() {
    let s = store();
    let cmd = Command::Select {
        table: "t".into(),
        filter: None,
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
    };
    assert!(s.execute(&cmd).is_err());
}
