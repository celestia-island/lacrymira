//! Tests for IndexBuilder and InsertPolicy.

use lacrymira::*;

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

// ── Uuid / Option conversions (0.1.1 gap found by the arcaea pilot) ──

#[test]
fn uuid_ref_round_trip() {
    let u = uuid::Uuid::new_v4();
    let v = lacrymira::Value::from(&u);
    assert!(matches!(v, lacrymira::Value::Uuid(_)));
    let back: uuid::Uuid = v.try_into().unwrap();
    assert_eq!(back, u);
}

#[test]
fn option_string_round_trip() {
    let some = Some("x".to_string());
    let v = lacrymira::Value::from(&some);
    assert!(matches!(v, lacrymira::Value::Text(_)));
    let back: Option<String> = v.try_into().unwrap();
    assert_eq!(back, some);

    let none: Option<String> = None;
    let v = lacrymira::Value::from(&none);
    assert!(matches!(v, lacrymira::Value::Null));
    let back: Option<String> = v.try_into().unwrap();
    assert_eq!(back, None);
}

#[test]
fn option_numeric_and_bool_round_trip() {
    let v = lacrymira::Value::from(&Some(42i64));
    assert_eq!(v, lacrymira::Value::I64(42));
    let back: Option<i64> = v.try_into().unwrap();
    assert_eq!(back, Some(42));

    let v = lacrymira::Value::from(&Some(true));
    let back: Option<bool> = v.try_into().unwrap();
    assert_eq!(back, Some(true));

    let none_i: Option<i32> = None;
    let back: Option<i32> = lacrymira::Value::from(&none_i).try_into().unwrap();
    assert_eq!(back, None);
}

#[test]
fn option_uuid_round_trip() {
    let u = uuid::Uuid::new_v4();
    let v = lacrymira::Value::from(&Some(u));
    let back: Option<uuid::Uuid> = v.try_into().unwrap();
    assert_eq!(back, Some(u));
    let back_none: Option<uuid::Uuid> = lacrymira::Value::Null.try_into().unwrap();
    assert_eq!(back_none, None);
}

#[test]
fn bytes_ref_round_trip() {
    let b = vec![1u8, 2, 3, 255];
    let v = lacrymira::Value::from(&b);
    assert_eq!(v, lacrymira::Value::Bytes(b.clone()));
    let back: Vec<u8> = v.try_into().unwrap();
    assert_eq!(back, b);
}

#[test]
fn option_bytes_round_trip() {
    // A nullable bytea column: Some binds the blob, None binds SQL NULL (the
    // two are NOT interchangeable for "does this row carry a credential").
    let blob = vec![0u8, 9, 8, 255];
    let some = lacrymira::Value::from(&Some(blob.clone()));
    assert_eq!(some, lacrymira::Value::Bytes(blob.clone()));
    let back: Option<Vec<u8>> = some.try_into().unwrap();
    assert_eq!(back, Some(blob));

    let none: Option<Vec<u8>> = None;
    let v = lacrymira::Value::from(&none);
    assert_eq!(v, lacrymira::Value::Null);
    let back: Option<Vec<u8>> = v.try_into().unwrap();
    assert_eq!(back, None);
}

#[test]
fn option_string_array_round_trip() {
    // A nullable TEXT[] column: None is SQL NULL, `Some(vec![])` is an empty
    // array — the distinction a tag column depends on.
    let tags = vec!["kg".to_string(), "w2".to_string()];
    let some = lacrymira::Value::from(&Some(tags.clone()));
    assert_eq!(
        some,
        lacrymira::Value::Array(tags.iter().cloned().map(lacrymira::Value::Text).collect())
    );
    let back: Option<Vec<String>> = some.try_into().unwrap();
    assert_eq!(back, Some(tags));

    let empty: Option<Vec<String>> = Some(Vec::new());
    let v = lacrymira::Value::from(&empty);
    assert_eq!(v, lacrymira::Value::Array(Vec::new()));
    let back: Option<Vec<String>> = v.try_into().unwrap();
    assert_eq!(back, Some(Vec::new()));

    let none: Option<Vec<String>> = None;
    let v = lacrymira::Value::from(&none);
    assert_eq!(v, lacrymira::Value::Null);
    let back: Option<Vec<String>> = v.try_into().unwrap();
    assert_eq!(back, None);
}

#[cfg(feature = "jiff")]
mod jiff_ts {
    use lacrymira::Value;

    #[test]
    fn timestamp_round_trip() {
        let ts = jiff::Timestamp::now();
        let v = Value::from(&ts);
        assert!(matches!(v, Value::Timestamp(_)));
        let back: jiff::Timestamp = v.try_into().unwrap();
        assert_eq!(back, ts);
    }

    #[test]
    fn timestamp_from_text_rfc3339() {
        let raw = "2026-11-22T00:00:00Z";
        let v = Value::Text(raw.into());
        let back: jiff::Timestamp = v.try_into().unwrap();
        assert_eq!(back, raw.parse::<jiff::Timestamp>().unwrap());
    }

    #[test]
    fn timestamp_wrong_variant_fails() {
        let v = Value::I64(1);
        assert!(<jiff::Timestamp as TryFrom<Value>>::try_from(v).is_err());
    }
}

#[cfg(feature = "chrono")]
mod chrono_ts {
    use lacrymira::Value;

    #[test]
    fn chrono_datetime_round_trip() {
        let dt = chrono::Utc::now();
        let v = Value::from(&dt);
        assert!(matches!(v, Value::Timestamp(_)));
        let back: chrono::DateTime<chrono::Utc> = v.try_into().unwrap();
        assert_eq!(back, dt);
    }

    #[test]
    fn chrono_from_text_rfc3339() {
        let raw = "2026-11-22T12:34:56.789+00:00";
        let v = Value::Text(raw.into());
        let back: chrono::DateTime<chrono::Utc> = v.try_into().unwrap();
        assert_eq!(
            back.to_rfc3339(),
            chrono::DateTime::parse_from_rfc3339(raw)
                .unwrap()
                .with_timezone(&chrono::Utc)
                .to_rfc3339()
        );
    }

    #[test]
    fn chrono_option_null_is_none() {
        let back: Option<chrono::DateTime<chrono::Utc>> = Value::Null.try_into().unwrap();
        assert!(back.is_none());
    }

    #[test]
    fn chrono_wrong_variant_fails() {
        let v = Value::I64(1);
        assert!(<chrono::DateTime<chrono::Utc> as TryFrom<Value>>::try_from(v).is_err());
    }
}

#[cfg(feature = "chrono")]
mod chrono_opt_encode {
    use lacrymira::Value;

    #[test]
    fn option_chrono_encode_some_and_none() {
        let dt = chrono::Utc::now();
        let v = Value::from(&Some(dt));
        assert!(matches!(v, Value::Timestamp(_)));
        let back: chrono::DateTime<chrono::Utc> = v.try_into().unwrap();
        assert_eq!(back, dt);

        let none: Option<chrono::DateTime<chrono::Utc>> = None;
        assert!(matches!(Value::from(&none), Value::Null));
    }

    #[test]
    fn option_jiff_encode_some_and_none() {
        let ts = jiff::Timestamp::now();
        let v = Value::from(&Some(ts));
        assert!(matches!(v, Value::Timestamp(_)));
        let back: jiff::Timestamp = v.try_into().unwrap();
        assert_eq!(back, ts);

        let none: Option<jiff::Timestamp> = None;
        assert!(matches!(Value::from(&none), Value::Null));
    }
}

#[cfg(feature = "chrono")]
mod chrono_naive {
    use lacrymira::Value;

    #[test]
    fn naive_date_round_trip() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let v = Value::from(&d);
        assert!(matches!(v, Value::Text(_)));
        let back: chrono::NaiveDate = v.try_into().unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn naive_datetime_round_trip_both_forms() {
        let dt = chrono::NaiveDate::from_ymd_opt(2026, 1, 2)
            .unwrap()
            .and_hms_micro_opt(3, 4, 5, 123456)
            .unwrap();
        let v = Value::from(&dt);
        let back: chrono::NaiveDateTime = v.try_into().unwrap();
        assert_eq!(back, dt);
        // bare form parses too
        let bare = Value::Text("2026-01-02 03:04:05.123456".into());
        let back2: chrono::NaiveDateTime = bare.try_into().unwrap();
        assert_eq!(back2, dt);
    }

    #[test]
    fn fixed_offset_round_trip() {
        let dt = chrono::DateTime::parse_from_rfc3339("2026-11-22T08:00:00+08:00").unwrap();
        let v = Value::from(&dt);
        let back: chrono::DateTime<chrono::FixedOffset> = v.try_into().unwrap();
        assert_eq!(back, dt);
    }
}

// ── Decimal / Vector round-trips (feature-gated) ──

#[cfg(feature = "decimal")]
mod decimal_tests {
    use lacrymira::Value;

    #[test]
    fn decimal_round_trip() {
        use std::str::FromStr as _;
        let d = rust_decimal::Decimal::from_str("123.4567").unwrap();
        let v = Value::from(&d);
        assert!(matches!(v, Value::Decimal(_)));
        let back: rust_decimal::Decimal = v.try_into().unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn decimal_option_round_trip() {
        let some = Some(rust_decimal::Decimal::new(99, 2));
        let v = Value::from(&some);
        let back: Option<rust_decimal::Decimal> = v.try_into().unwrap();
        assert_eq!(back, some);
        let none: Option<rust_decimal::Decimal> = None;
        assert!(matches!(Value::from(&none), Value::Null));
        let back: Option<rust_decimal::Decimal> = Value::Null.try_into().unwrap();
        assert!(back.is_none());
    }

    #[test]
    fn decimal_from_text_accepts_numeric_strings() {
        let v = Value::Text("42.5".into());
        let back: rust_decimal::Decimal = v.try_into().unwrap();
        assert_eq!(back, rust_decimal::Decimal::new(425, 1));
    }
}

#[cfg(feature = "pgvector")]
mod vector_tests {
    use lacrymira::Value;

    #[test]
    fn vector_option_round_trip() {
        let vec = vec![0.1f32, 0.2, 0.3];
        let v = Value::from(&Some(vec.clone()));
        assert!(matches!(v, Value::Vector(_)));
        let back: Option<Vec<f32>> = v.try_into().unwrap();
        let back = back.unwrap();
        assert_eq!(back.len(), 3);
        assert!((back[0] - 0.1).abs() < 1e-6);
        let none: Option<Vec<f32>> = None;
        assert!(matches!(Value::from(&none), Value::Null));
    }

    #[test]
    fn vector_text_form_parses() {
        let v = Value::Text("[1.0,2.0,3.0]".into());
        let back: Option<Vec<f32>> = v.try_into().unwrap();
        assert_eq!(back.unwrap(), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn vector_empty_and_null() {
        let v = Value::Vector("[]".into());
        let back: Option<Vec<f32>> = v.try_into().unwrap();
        assert!(back.unwrap().is_empty());
        let back: Option<Vec<f32>> = Value::Null.try_into().unwrap();
        assert!(back.is_none());
    }
}
