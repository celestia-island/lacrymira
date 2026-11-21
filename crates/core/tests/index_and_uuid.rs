//! Tests for IndexBuilder and InsertPolicy.

use lacrymira_core::*;

// ── IndexBuilder ──

#[test]
fn simple_index() {
    let sql = IndexBuilder::on("users")
        .columns(&["email"])
        .build()
        .to_sql();
    assert!(
        sql.contains(r#"CREATE INDEX idx_users_email ON "users" ("email")"#),
        "got: {}",
        sql
    );
}

#[test]
fn composite_unique_index() {
    let sql = IndexBuilder::on("org_members")
        .columns(&["org_id", "user_id"])
        .unique()
        .build()
        .to_sql();
    assert!(sql.contains("CREATE UNIQUE INDEX"), "got: {}", sql);
    assert!(sql.contains(r#"("org_id", "user_id")"#), "got: {}", sql);
}

#[test]
fn covering_index_with_include() {
    let sql = IndexBuilder::on("org_members")
        .columns(&["org_id", "user_id"])
        .include(&["role"])
        .build()
        .to_sql();
    assert!(sql.contains(r#"INCLUDE ("role")"#), "got: {}", sql);
}

#[test]
fn partial_index_with_where() {
    let sql = IndexBuilder::on("tasks")
        .columns(&["status", "created_at"])
        .where_clause("status != 'deleted'")
        .build()
        .to_sql();
    assert!(sql.contains(r#"WHERE status != 'deleted'"#), "got: {}", sql);
}

#[test]
fn full_index_all_features() {
    let sql = IndexBuilder::on("billing_ledger")
        .name("idx_billing_composite")
        .columns(&["user_id", "week_start"])
        .include(&["degrees"])
        .unique()
        .where_clause("entry_type != 'void'")
        .build()
        .to_sql();
    assert!(
        sql.contains("CREATE UNIQUE INDEX idx_billing_composite"),
        "got: {}",
        sql
    );
    assert!(sql.contains(r#"INCLUDE ("degrees")"#), "got: {}", sql);
    assert!(
        sql.contains(r#"WHERE entry_type != 'void'"#),
        "got: {}",
        sql
    );
}

// ── InsertPolicy UUID generation ──

#[test]
fn uuid_v4_generates_valid() {
    let policy = InsertPolicy::v4();
    let uuid = policy.generate_uuid(None).unwrap();
    assert_eq!(uuid.get_version_num(), 4);
}

#[test]
fn uuid_v7_generates_valid() {
    let policy = InsertPolicy::v7();
    let uuid = policy.generate_uuid(None).unwrap();
    // v7 sets version nibble to 7
    assert_eq!(uuid.get_version_num(), 7);
}

#[test]
fn uuid_v5_deterministic() {
    let policy = InsertPolicy::v5("6ba7b810-9dad-11d1-80b4-00c04fd430c8", 3);
    let a = policy.generate_uuid(Some("test-name")).unwrap();
    let b = policy.generate_uuid(Some("test-name")).unwrap();
    assert_eq!(a, b, "v5 is deterministic");
    assert_eq!(a.get_version_num(), 5);
}

#[test]
fn uuid_v5_different_names_differ() {
    let policy = InsertPolicy::v5("6ba7b810-9dad-11d1-80b4-00c04fd430c8", 3);
    let a = policy.generate_uuid(Some("name-a")).unwrap();
    let b = policy.generate_uuid(Some("name-b")).unwrap();
    assert_ne!(a, b);
}

#[test]
fn uuid_v5_requires_name() {
    let policy = InsertPolicy::v5("6ba7b810-9dad-11d1-80b4-00c04fd430c8", 3);
    assert!(policy.generate_uuid(None).is_err());
}

#[test]
fn user_provided_does_not_generate() {
    let policy = InsertPolicy {
        uuid: UuidStrategy::UserProvided,
        collision: CollisionPolicy::Fail,
    };
    assert!(policy.generate_uuid(None).is_err());
}

// ── Msgpack round-trip for new types ──

#[test]
fn insert_policy_msgpack_round_trip() {
    let policy = InsertPolicy::v5("6ba7b810-9dad-11d1-80b4-00c04fd430c8", 3);
    let bytes = rmp_serde::to_vec(&policy).unwrap();
    let back: InsertPolicy = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(back, policy);
    let re = rmp_serde::to_vec(&back).unwrap();
    assert_eq!(bytes, re, "byte-identical");
}

#[test]
fn index_spec_msgpack_round_trip() {
    let spec = IndexBuilder::on("t").columns(&["a", "b"]).unique().build();
    let bytes = rmp_serde::to_vec(&spec).unwrap();
    let back: IndexSpec = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(back, spec);
}
