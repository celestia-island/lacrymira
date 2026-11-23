//! Round-trip tests: `Command` → msgpack → execute → `Reply`, driven through
//! a mock host executor on the native target.
//!
//! The native target never touches the real wit import (its non-wasm body is
//! an `unreachable!()` stub), so these tests inject transports — either a raw
//! closure for byte-level assertions or [`HostTransport`] for the full
//! host-side path ([`host_serve_bytes`] included). The wire bytes are decoded
//! with `rmp-serde` **inside the mock**, independently of the crate's own
//! codec, so a pass here proves the bytes crossing the boundary really are
//! frozen-contract msgpack.

use std::cell::RefCell;

use lacrymira::{
    ColumnRef, Command, Condition, ConflictAction, Dialect, Error, KvOp, OnConflict, Reply, Value,
};
use lacrymira_wasi::{HostTransport, WasiExecutor};

#[test]
fn wire_bytes_are_msgpack_command_and_reply() {
    let cmd = Command::Kv {
        namespace: "workspace".into(),
        op: KvOp::Get {
            key: Value::Text("ws-123".into()),
        },
    };
    let expected = Reply::Value(Some(Value::Text("payload".into())));

    let executor = WasiExecutor::with_transport(|bytes: &[u8]| {
        // What crossed the wire must decode — independently — as the exact
        // command we sent, i.e. genuine msgpack of the frozen contract.
        let decoded: Command = rmp_serde::from_slice(bytes).expect("wire bytes are msgpack");
        assert_eq!(decoded, cmd);
        // And the reply travels back as msgpack bytes too.
        Ok(rmp_serde::to_vec(&expected).expect("reply encodes"))
    });

    let got = executor.execute(&cmd).expect("round trip succeeds");
    assert_eq!(got, expected);
}

#[test]
fn mock_host_executor_sees_identical_command_and_replies() {
    let cmd = Command::Select {
        table: "billing_ledger".into(),
        filter: Some(Condition::All(vec![
            Condition::Eq(
                ColumnRef {
                    name: "user_id".into(),
                },
                Value::Uuid("01234567-89ab-cdef-0123-456789abcdef".into()),
            ),
            Condition::Gt(
                ColumnRef {
                    name: "degrees".into(),
                },
                Value::I32(50),
            ),
        ])),
        order_by: vec![("week_start".into(), true)],
        limit: Some(10),
        offset: None,
        lock: None,
        distinct: false,
    };
    let expected = Reply::Rows {
        columns: vec!["week_start".into(), "total".into()],
        rows: vec![vec![
            Value::Timestamp("2026-11-16T00:00:00Z".into()),
            Value::F64(12.5),
        ]],
    };

    let seen = RefCell::new(Vec::new());
    let executor = WasiExecutor::with_transport(HostTransport::new(|c: &Command| {
        seen.borrow_mut().push(c.clone());
        Ok(expected.clone())
    }));

    let got = executor.execute(&cmd).expect("round trip succeeds");
    assert_eq!(got, expected, "reply must survive the wire both ways");
    assert_eq!(
        seen.into_inner(),
        vec![cmd],
        "the mock host must receive the identical decoded command"
    );
}

/// One pair per `Command` variant family and `Reply` shape.
fn command_reply_pairs() -> Vec<(Command, Reply)> {
    vec![
        (
            Command::Sql {
                dialect: Dialect::Postgres,
                text: "SELECT * FROM users WHERE id = $1".into(),
                params: vec![Value::Uuid("01234567-89ab-cdef-0123-456789abcdef".into())],
            },
            Reply::Rows {
                columns: vec!["id".into(), "name".into()],
                rows: vec![vec![
                    Value::Uuid("01234567-89ab-cdef-0123-456789abcdef".into()),
                    Value::Text("hikari".into()),
                ]],
            },
        ),
        (
            Command::Insert {
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
            Reply::Affected(1),
        ),
        (
            Command::Update {
                table: "users".into(),
                sets: vec![("degrees".into(), Value::I32(100))],
                filter: Some(Condition::Gt(
                    ColumnRef {
                        name: "degrees".into(),
                    },
                    Value::I32(50),
                )),
                returning: true,
            },
            Reply::Affected(7),
        ),
        (
            Command::Delete {
                table: "sessions".into(),
                filter: None,
            },
            Reply::Unit,
        ),
        (
            Command::Kv {
                namespace: "index".into(),
                op: KvOp::Scan {
                    start: Some(Value::Text("a".into())),
                    end: Some(Value::Text("z".into())),
                    limit: Some(100),
                },
            },
            Reply::Entries(vec![
                (Value::Text("k1".into()), Value::Json("{\"v\":1}".into())),
                (Value::Text("k2".into()), Value::Null),
            ]),
        ),
        (
            Command::Kv {
                namespace: "workspace".into(),
                op: KvOp::Get {
                    key: Value::Text("missing".into()),
                },
            },
            Reply::Value(None),
        ),
    ]
}

#[test]
fn every_command_and_reply_kind_round_trips() {
    for (cmd, expected) in command_reply_pairs() {
        let executor = WasiExecutor::with_transport(HostTransport::new(|c: &Command| {
            // The mock host decodes successfully — a wrong wire encoding of
            // any Command variant would fail inside HostTransport first.
            assert_eq!(c, &cmd);
            Ok(expected.clone())
        }));
        let got = executor.execute(&cmd).unwrap_or_else(|e| {
            panic!("round trip failed for {cmd:?}: {e:?}");
        });
        assert_eq!(got, expected, "reply must round-trip for {cmd:?}");
    }
}

#[test]
fn mock_host_errors_never_reach_the_reply_decoder() {
    // A host executor that refuses everything: the error must travel the wit
    // error channel and never be mistaken for reply bytes.
    let executor =
        WasiExecutor::with_transport(HostTransport::new(|_c: &Command| Err(Error::NotFound)));
    let cmd = Command::Kv {
        namespace: "workspace".into(),
        op: KvOp::Get {
            key: Value::Text("k".into()),
        },
    };
    assert_eq!(executor.execute(&cmd), Err(Error::NotFound));
}
