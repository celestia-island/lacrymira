//! Tests for the new database features: JSONB, locks, version conflict.

use lacrymira::*;
use lacrymira_pg::render;

// ── JSONB rendering ──

#[test]
fn json_get_eq_renders_arrow_path() {
    let cmd = Command::Select {
        table: "configs".into(),
        filter: Some(
            ColumnRef::new("metadata")
                .json_get("$.user.name")
                .eq("John"),
        ),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(
        r.sql.contains(r#""metadata"->>'user'->'name' = $1"#),
        "got: {}",
        r.sql
    );
}

#[test]
fn json_exists_renders_question_mark() {
    let cmd = Command::Select {
        table: "configs".into(),
        filter: Some(ColumnRef::new("settings").json_exists("$.notifications.email")),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(
        r.sql.contains(r#""settings"->'notifications' ? 'email'"#),
        "got: {}",
        r.sql
    );
}

#[test]
fn json_contains_renders_at_gt() {
    let cmd = Command::Select {
        table: "configs".into(),
        filter: Some(ColumnRef::new("data").json_contains(serde_json::json!({"role": "admin"}))),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.contains(r#""data" @> $1::jsonb"#), "got: {}", r.sql);
    assert_eq!(r.params.len(), 1);
}

// ── Lock rendering ──

#[test]
fn lock_update_renders_for_update() {
    let cmd = Command::Select {
        table: "accounts".into(),
        filter: Some(ColumnRef::new("id").eq(1)),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: Some(LockMode::Update),
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.ends_with("FOR UPDATE"), "got: {}", r.sql);
}

#[test]
fn lock_skip_locked_renders() {
    let cmd = Command::Select {
        table: "jobs".into(),
        filter: Some(ColumnRef::new("status").eq("pending")),
        order_by: vec![("created_at".into(), true)],
        limit: Some(10),
        offset: None,
        lock: Some(LockMode::SkipLocked),
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.ends_with("FOR UPDATE SKIP LOCKED"), "got: {}", r.sql);
}

#[test]
fn lock_nowait_renders() {
    let cmd = Command::Select {
        table: "inventory".into(),
        filter: None,
        order_by: vec![],
        limit: None,
        offset: None,
        lock: Some(LockMode::UpdateNoWait),
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.ends_with("FOR UPDATE NOWAIT"), "got: {}", r.sql);
}

#[test]
fn lock_share_renders() {
    let cmd = Command::Select {
        table: "refs".into(),
        filter: None,
        order_by: vec![],
        limit: None,
        offset: None,
        lock: Some(LockMode::Share),
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.ends_with("FOR SHARE"), "got: {}", r.sql);
}

#[test]
fn no_lock_by_default() {
    let cmd = Command::Select {
        table: "t".into(),
        filter: None,
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(
        !r.sql.contains("FOR UPDATE"),
        "should not have lock: {}",
        r.sql
    );
    assert!(
        !r.sql.contains("FOR SHARE"),
        "should not have lock: {}",
        r.sql
    );
}

// ── Error variant round-trip (msgpack) ──

#[test]
fn version_conflict_msgpack_round_trip() {
    let e = Error::VersionConflict {
        expected: 3,
        actual: 7,
    };
    let bytes = rmp_serde::to_vec(&e).unwrap();
    let back: Error = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(back, e);
}

#[test]
fn id_collision_msgpack_round_trip() {
    let e = Error::IdCollision {
        attempts: 3,
        id: "test-uuid".into(),
    };
    let bytes = rmp_serde::to_vec(&e).unwrap();
    let back: Error = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(back, e);
}

// ── LockMode serialization ──

#[test]
fn lock_mode_msgpack_round_trip() {
    for mode in [
        LockMode::Update,
        LockMode::UpdateNoWait,
        LockMode::SkipLocked,
        LockMode::Share,
    ] {
        let bytes = rmp_serde::to_vec(&mode).unwrap();
        let back: LockMode = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(back, mode);
    }
}

// ── JSONB in Command round-trip (msgpack contract) ──

#[test]
fn jsonb_condition_msgpack_round_trip() {
    let cmd = Command::Select {
        table: "t".into(),
        filter: Some(ColumnRef::new("meta").json_get("$.x.y").eq("val")),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: Some(LockMode::SkipLocked),
        distinct: false,
    };
    let bytes = rmp_serde::to_vec(&cmd).unwrap();
    let back: Command = rmp_serde::from_slice(&bytes).unwrap();
    let re = rmp_serde::to_vec(&back).unwrap();
    assert_eq!(bytes, re, "round-trip must be byte-identical");
    assert_eq!(back, cmd);
}

#[test]
fn empty_in_renders_false_without_binding() {
    let cmd = Command::Select {
        table: "roles".into(),
        filter: Some(ColumnRef::new("id").in_(Vec::<i64>::new())),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(
        r.sql.contains(r#"WHERE FALSE"#),
        "empty IN must render FALSE: {}",
        r.sql
    );
    assert_eq!(r.params.len(), 0, "no array bind for empty IN");
}

#[test]
fn empty_not_in_renders_true_without_binding() {
    let cmd = Command::Select {
        table: "roles".into(),
        filter: Some(ColumnRef::new("id").not_in(Vec::<i64>::new())),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(
        r.sql.contains(r#"WHERE TRUE"#),
        "empty NOT IN must render TRUE: {}",
        r.sql
    );
    assert_eq!(r.params.len(), 0, "no array bind for empty NOT IN");
}

#[test]
fn nonempty_in_still_binds_any() {
    let cmd = Command::Select {
        table: "roles".into(),
        filter: Some(ColumnRef::new("id").in_(vec![1i64, 2])),
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
        distinct: false,
    };
    let r = render(&cmd).unwrap();
    assert!(r.sql.contains("ANY($1)"), "{}", r.sql);
    assert_eq!(r.params.len(), 1);
}
