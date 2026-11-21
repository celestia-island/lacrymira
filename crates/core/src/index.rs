//! Fluent index builder for CREATE INDEX DDL.

use serde::{Deserialize, Serialize};

/// An index being built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexSpec {
    /// Index name (auto-generated if not specified).
    pub name: Option<String>,
    /// Table to index.
    pub table: String,
    /// Columns to index (in order).
    pub columns: Vec<String>,
    /// Additional columns to INCLUDE (covering index, PostgreSQL 11+).
    pub include: Vec<String>,
    /// Whether this is a UNIQUE index.
    pub unique: bool,
    /// Partial index WHERE clause (e.g. "status != 'deleted'").
    pub where_clause: Option<String>,
}

/// Builder for index specifications.
pub struct IndexBuilder {
    spec: IndexSpec,
}

impl IndexBuilder {
    /// Start building an index on a table.
    pub fn on(table: impl Into<String>) -> Self {
        IndexBuilder {
            spec: IndexSpec {
                name: None,
                table: table.into(),
                columns: Vec::new(),
                include: Vec::new(),
                unique: false,
                where_clause: None,
            },
        }
    }

    /// Set the index name explicitly.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.spec.name = Some(name.into());
        self
    }

    /// Add indexed columns (can be called once with multiple, or chained).
    pub fn columns(mut self, cols: &[&str]) -> Self {
        self.spec.columns.extend(cols.iter().map(|c| c.to_string()));
        self
    }

    /// Add a single indexed column.
    pub fn column(mut self, col: impl Into<String>) -> Self {
        self.spec.columns.push(col.into());
        self
    }

    /// Add INCLUDE columns (covering index).
    pub fn include(mut self, cols: &[&str]) -> Self {
        self.spec.include.extend(cols.iter().map(|c| c.to_string()));
        self
    }

    /// Make this a UNIQUE index.
    pub fn unique(mut self) -> Self {
        self.spec.unique = true;
        self
    }

    /// Add a partial index WHERE clause.
    pub fn where_clause(mut self, clause: impl Into<String>) -> Self {
        self.spec.where_clause = Some(clause.into());
        self
    }

    /// Build the IndexSpec.
    pub fn build(self) -> IndexSpec {
        self.spec
    }
}

impl IndexSpec {
    /// Render to CREATE INDEX SQL (PostgreSQL dialect).
    pub fn to_sql(&self) -> String {
        let name = self.name.clone().unwrap_or_else(|| {
            format!(
                "idx_{}_{}{}",
                self.table,
                self.columns.join("_"),
                if self.unique { "_uniq" } else { "" }
            )
        });

        let unique_str = if self.unique { "UNIQUE " } else { "" };
        let mut sql = format!(
            "CREATE {}INDEX {} ON \"{}\" ({})",
            unique_str,
            name,
            self.table,
            self.columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ")
        );

        if !self.include.is_empty() {
            sql.push_str(&format!(
                " INCLUDE ({})",
                self.include
                    .iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if let Some(ref wc) = self.where_clause {
            sql.push_str(&format!(" WHERE {}", wc));
        }

        sql
    }
}
