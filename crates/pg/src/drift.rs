//! Index drift check: compare declared IndexSpec against pg_indexes.

use lacrymira::IndexSpec;

/// An index as it exists in the database (parsed from pg_indexes).
#[derive(Debug, Clone, PartialEq)]
pub struct DbIndex {
    /// Index name as created in the database.
    pub name: String,
    /// Table the index belongs to.
    pub table: String,
    /// Whether the index is unique.
    pub unique: bool,
    /// Full `CREATE INDEX` definition from `pg_indexes`.
    pub definition: String,
}

/// Drift between declared and actual indexes.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexDrift {
    /// Indexes declared but missing from the database.
    pub missing: Vec<IndexSpec>,
    /// Indexes in the database but not declared.
    pub extra: Vec<DbIndex>,
    /// Indexes with matching names but different definitions.
    pub mismatched: Vec<(IndexSpec, DbIndex)>,
    /// Indexes that match (name and definition compatible).
    pub matched: Vec<String>,
}

impl IndexDrift {
    /// Returns true if there is no drift.
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty() && self.extra.is_empty() && self.mismatched.is_empty()
    }
}

/// Parse a pg_indexes row into a DbIndex.
pub fn parse_pg_index(name: &str, table: &str, unique: bool, definition: &str) -> DbIndex {
    DbIndex {
        name: name.to_string(),
        table: table.to_string(),
        unique,
        definition: definition.to_string(),
    }
}

/// Compare declared IndexSpec list against actual database indexes.
pub fn check_drift(declared: &[IndexSpec], actual: &[DbIndex]) -> IndexDrift {
    let mut drift = IndexDrift {
        missing: Vec::new(),
        extra: Vec::new(),
        mismatched: Vec::new(),
        matched: Vec::new(),
    };

    let declared_names: std::collections::HashMap<&str, &IndexSpec> = declared
        .iter()
        .map(|spec| {
            let name = spec.name.as_deref().unwrap_or("");
            (name, spec)
        })
        .collect();

    let actual_names: std::collections::HashMap<&str, &DbIndex> =
        actual.iter().map(|idx| (idx.name.as_str(), idx)).collect();

    // Check declared → actual
    for spec in declared {
        let name = spec.name.as_deref().unwrap_or("");
        match actual_names.get(name) {
            Some(db_idx) => {
                // Name matches — check definition compatibility
                if definitions_match(spec, db_idx) {
                    drift.matched.push(name.to_string());
                } else {
                    drift.mismatched.push((spec.clone(), (*db_idx).clone()));
                }
            }
            None => {
                drift.missing.push(spec.clone());
            }
        }
    }

    // Check actual → declared (for extras)
    for idx in actual {
        let name = idx.name.as_str();
        if !declared_names.contains_key(name) {
            drift.extra.push(idx.clone());
        }
    }

    drift
}

/// Check if a declared IndexSpec is compatible with a database index.
fn definitions_match(spec: &IndexSpec, db: &DbIndex) -> bool {
    // Check uniqueness
    if spec.unique != db.unique {
        return false;
    }

    // Check that all declared columns appear in the definition
    let def_lower = db.definition.to_lowercase();
    for col in &spec.columns {
        if !def_lower.contains(&col.to_lowercase()) {
            return false;
        }
    }

    // Check WHERE clause if declared
    if let Some(ref wc) = spec.where_clause {
        if !def_lower.contains(&wc.to_lowercase()) {
            return false;
        }
    }

    true
}

/// SQL to query pg_indexes for a table.
pub fn pg_indexes_query(table: &str) -> String {
    format!(
        "SELECT indexname, tablename, indexdef FROM pg_indexes WHERE tablename = '{}'",
        table
    )
}
