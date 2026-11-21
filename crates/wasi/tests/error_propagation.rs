//! Error propagation tests: `Error` → wit `error-info` → `Error`.
//!
//! The wit surface has one error shape — `error-info { kind, message,
//! sqlstate }` — which mirrors the core `Error::Db` variant one-to-one. The
//! other core variants travel under reserved `err:`-prefixed kinds and are
//! reconstructed losslessly on the guest side; every other `kind` is carried
//! verbatim as a database error, so host-side driver classifications can
//! never collide with the reserved names.

use lacrymira_core::{Command, Error, KvOp, Reply, Value};
use lacrymira_wasi::{host_serve_bytes, ErrorInfo, HostTransport, WasiExecutor, KIND_DECODE};

fn kv_get() -> Command {
    Command::Kv {
        namespace: "workspace".into(),
        op: KvOp::Get {
            key: Value::Text("ws-123".into()),
        },
    }
}

#[test]
fn error_to_error_info_to_error_is_lossless() {
    let errors = vec![
        Error::Db {
            kind: "unique_violation".into(),
            message: "duplicate key value violates unique constraint".into(),
            sqlstate: Some("23505".into()),
        },
        // A Db kind that *looks like* a reserved word must stay a Db error —
        // only `err:`-prefixed kinds are reserved.
        Error::Db {
            kind: "decode".into(),
            message: "column decode failed".into(),
            sqlstate: None,
        },
        Error::NotFound,
        Error::Decode("bad uuid".into()),
        Error::Config("pool not initialized".into()),
    ];
    for err in errors {
        let info = ErrorInfo::from(err.clone());
        let back = Error::from(info);
        assert_eq!(back, err, "round trip must preserve {err:?}");
    }
}

#[test]
fn db_error_crosses_the_wire_with_sqlstate() {
    let err = Error::Db {
        kind: "unique_violation".into(),
        message: "duplicate key value violates unique constraint \"users_pkey\"".into(),
        sqlstate: Some("23505".into()),
    };
    let host_err = err.clone();
    let executor =
        WasiExecutor::with_transport(HostTransport::new(
            move |_c: &Command| Err(host_err.clone()),
        ));

    let got = executor
        .execute(&kv_get())
        .expect_err("host error must propagate");
    assert_eq!(got, err);
    assert!(got.is_unique_violation(), "sqlstate 23505 must survive");
    assert_eq!(got.sqlstate(), Some("23505"));
}

#[test]
fn non_db_variants_survive_the_wit_error_shape() {
    for err in [
        Error::NotFound,
        Error::Decode("bad uuid".into()),
        Error::Config("no pool".into()),
    ] {
        let host_err = err.clone();
        let executor = WasiExecutor::with_transport(HostTransport::new(move |_c: &Command| {
            Err(host_err.clone())
        }));
        assert_eq!(
            executor.execute(&kv_get()),
            Err(err),
            "variant must survive the wit error format"
        );
    }
}

#[test]
fn unknown_host_kinds_map_to_db_verbatim() {
    // A host may classify errors however it likes; unknown kinds (including
    // future reserved ones) must degrade to a Db error, not vanish.
    let info = ErrorInfo {
        kind: "connection".into(),
        message: "pool timed out".into(),
        sqlstate: Some("57014".into()),
    };
    let executor = WasiExecutor::with_transport(move |_bytes: &[u8]| Err(info.clone()));

    match executor.execute(&kv_get()) {
        Err(Error::Db {
            kind,
            message,
            sqlstate,
        }) => {
            assert_eq!(kind, "connection");
            assert_eq!(message, "pool timed out");
            assert_eq!(sqlstate.as_deref(), Some("57014"));
        }
        other => panic!("expected Error::Db, got {other:?}"),
    }
}

#[test]
fn host_rejects_malformed_command_bytes_before_the_executor() {
    let never = |_c: &Command| -> Result<Reply, Error> {
        panic!("undecodable command bytes must not reach the host executor")
    };
    // 0xC1 is the one byte the msgpack spec reserves as never-valid.
    let out = host_serve_bytes(&never, &[0xC1, 0xFF, 0xFF]);

    match out {
        Err(info) => {
            assert_eq!(info.kind, KIND_DECODE);
            let err = Error::from(info);
            assert!(
                matches!(err, Error::Decode(ref m) if m.contains("Command")),
                "expected a Command decode error, got {err:?}"
            );
        }
        Ok(bytes) => panic!("garbage must be rejected, got {} bytes back", bytes.len()),
    }
}

#[test]
fn guest_rejects_malformed_reply_bytes() {
    let executor = WasiExecutor::with_transport(|_bytes: &[u8]| Ok(vec![0xC1, 0xC1, 0xC1]));
    match executor.execute(&kv_get()) {
        Err(err @ Error::Decode(_)) => {
            assert!(
                matches!(&err, Error::Decode(m) if m.contains("Reply")),
                "expected a Reply decode error, got {err:?}"
            );
        }
        other => panic!("expected Error::Decode, got {other:?}"),
    }
}
