//! Tests for index drift checking.

use lacrymira_core::IndexBuilder;
use lacrymira_pg::{check_drift, parse_pg_index};

// ── Drift detection: clean state ──

#[test]
fn no_drift_when_all_match() {
    let declared = vec![IndexBuilder::on("users")
        .name("idx_users_email")
        .columns(&["email"])
        .build()];
    let actual = vec![parse_pg_index(
        "idx_users_email",
        "users",
        false,
        "CREATE INDEX idx_users_email ON public.users USING btree (email)",
    )];
    let drift = check_drift(&declared, &actual);
    assert!(drift.is_clean(), "should be clean: {:?}", drift);
    assert_eq!(drift.matched.len(), 1);
}

// ── Drift detection: missing index ──

#[test]
fn missing_index_detected() {
    let declared = vec![
        IndexBuilder::on("users")
            .name("idx_users_email")
            .columns(&["email"])
            .build(),
        IndexBuilder::on("users")
            .name("idx_users_name")
            .columns(&["name"])
            .build(),
    ];
    // Only one exists in DB
    let actual = vec![parse_pg_index(
        "idx_users_email",
        "users",
        false,
        "CREATE INDEX idx_users_email ON public.users USING btree (email)",
    )];
    let drift = check_drift(&declared, &actual);
    assert!(!drift.is_clean());
    assert_eq!(drift.missing.len(), 1, "idx_users_name is missing");
    assert_eq!(drift.missing[0].name, Some("idx_users_name".to_string()));
}

// ── Drift detection: extra index ──

#[test]
fn extra_index_detected() {
    let declared = vec![IndexBuilder::on("users")
        .name("idx_users_email")
        .columns(&["email"])
        .build()];
    let actual = vec![
        parse_pg_index(
            "idx_users_email",
            "users",
            false,
            "CREATE INDEX idx_users_email ON public.users USING btree (email)",
        ),
        parse_pg_index(
            "idx_users_legacy",
            "users",
            false,
            "CREATE INDEX idx_users_legacy ON public.users USING btree (old_col)",
        ),
    ];
    let drift = check_drift(&declared, &actual);
    assert!(!drift.is_clean());
    assert_eq!(drift.extra.len(), 1);
    assert_eq!(drift.extra[0].name, "idx_users_legacy");
}

// ── Drift detection: uniqueness mismatch ──

#[test]
fn unique_mismatch_detected() {
    let declared = vec![IndexBuilder::on("users")
        .name("idx_users_email")
        .columns(&["email"])
        .unique() // declared as UNIQUE
        .build()];
    let actual = vec![parse_pg_index(
        "idx_users_email",
        "users",
        false, // but in DB it's NOT unique
        "CREATE INDEX idx_users_email ON public.users USING btree (email)",
    )];
    let drift = check_drift(&declared, &actual);
    assert!(!drift.is_clean());
    assert_eq!(drift.mismatched.len(), 1);
}

// ── Drift detection: column mismatch ──

#[test]
fn column_mismatch_detected() {
    let declared = vec![IndexBuilder::on("org_members")
        .name("idx_org_user")
        .columns(&["org_id", "user_id"])
        .build()];
    let actual = vec![parse_pg_index(
        "idx_org_user",
        "org_members",
        false,
        "CREATE INDEX idx_org_user ON public.org_members USING btree (org_id)",
        // ↑ missing user_id column
    )];
    let drift = check_drift(&declared, &actual);
    assert!(!drift.is_clean());
    assert_eq!(drift.mismatched.len(), 1, "user_id column missing in DB");
}

// ── Full scenario: multiple tables, mixed state ──

#[test]
fn full_scenario_mixed_drift() {
    let declared = vec![
        IndexBuilder::on("users")
            .name("idx_users_email")
            .columns(&["email"])
            .build(),
        IndexBuilder::on("orders")
            .name("idx_orders_user")
            .columns(&["user_id"])
            .build(),
        IndexBuilder::on("orders")
            .name("idx_orders_status")
            .columns(&["status"])
            .unique()
            .where_clause("status != 'draft'")
            .build(),
    ];
    let actual = vec![
        parse_pg_index(
            "idx_users_email",
            "users",
            false,
            "CREATE INDEX idx_users_email ON public.users USING btree (email)",
        ),
        // idx_orders_user missing
        parse_pg_index(
            "idx_orders_status",
            "orders",
            true,
            "CREATE UNIQUE INDEX idx_orders_status ON public.orders USING btree (status) WHERE status != 'draft'",
        ),
        parse_pg_index(
            "idx_extra",
            "orders",
            false,
            "CREATE INDEX idx_extra ON public.orders USING btree (extra)",
        ),
    ];
    let drift = check_drift(&declared, &actual);
    assert!(!drift.is_clean());
    assert_eq!(drift.missing.len(), 1, "idx_orders_user missing");
    assert_eq!(
        drift.matched.len(),
        2,
        "idx_users_email + idx_orders_status match"
    );
    assert_eq!(drift.extra.len(), 1, "idx_extra not declared");
    assert_eq!(drift.mismatched.len(), 0);
}
