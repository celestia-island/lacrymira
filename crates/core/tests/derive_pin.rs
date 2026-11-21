//! Tests for the #[derive(Entity)] macro with hygienic child modules.

mod ledger {
    use lacrymira_macros::Entity as DeriveEntity;

    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "billing_ledger")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub id: i64,
        pub user_id: String,
        #[lacrymira(column = "entry_type")]
        pub entry_type: String,
        pub degrees: i32,
    }

    pub fn check_metadata() {
        use lacrymira::Entity;
        assert_eq!(<Model as Entity>::TABLE, "billing_ledger");
        assert_eq!(<Model as Entity>::SCHEMA, "public");
        assert_eq!(<Model as Entity>::COLUMN_COUNT, 4);
        assert_eq!(
            <Model as Entity>::COLUMNS,
            &["id", "user_id", "entry_type", "degrees"]
        );
        assert_eq!(<Model as Entity>::PK_INDICES, &[0]);
    }

    pub fn check_column_rename() {
        assert_eq!(
            billing_ledger_entity::Column::EntryType.name(),
            "entry_type"
        );
        assert_eq!(billing_ledger_entity::Column::Id.name(), "id");
        assert_eq!(billing_ledger_entity::Column::ALL.len(), 4);
    }

    pub fn check_active_model() {
        let m = Model {
            id: 1,
            user_id: "u".into(),
            entry_type: "grant".into(),
            degrees: 100,
        };
        let a = billing_ledger_entity::ActiveModel::from(m);
        assert_eq!(a.id.value(), Some(&1));
        assert_eq!(a.degrees.value(), Some(&100));

        let d = billing_ledger_entity::ActiveModel::default();
        assert!(!d.id.is_set());
        assert!(!d.entry_type.is_set());
    }

    pub fn check_select() {
        use lacrymira::{ColumnRef, Command, Select};
        let q = Select::<Model>::new()
            .filter(ColumnRef::new("user_id").eq("test"))
            .limit(10);
        let cmd = q.to_command();
        assert!(
            matches!(cmd, Command::Select { table, limit: Some(10), .. } if table == "billing_ledger")
        );
    }
}

mod org_member {
    use lacrymira_macros::Entity as DeriveEntity;

    #[derive(Debug, Clone, PartialEq, DeriveEntity)]
    #[lacrymira(table = "org_members", schema = "billing")]
    pub struct Model {
        #[lacrymira(primary_key)]
        pub org_id: String,
        #[lacrymira(primary_key)]
        pub user_id: String,
        pub role: String,
    }

    pub fn check_composite_pk() {
        use lacrymira::Entity;
        assert_eq!(<Model as Entity>::TABLE, "org_members");
        assert_eq!(<Model as Entity>::SCHEMA, "billing");
        assert_eq!(<Model as Entity>::PK_INDICES, &[0, 1]);
    }
}

#[test]
fn single_pk_entity_metadata() {
    ledger::check_metadata();
}

#[test]
fn column_rename_works() {
    ledger::check_column_rename();
}

#[test]
fn active_model_from_model() {
    ledger::check_active_model();
}

#[test]
fn derive_generates_selectable_entity() {
    ledger::check_select();
}

#[test]
fn composite_pk_entity_metadata() {
    org_member::check_composite_pk();
}
