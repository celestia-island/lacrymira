//! Three-state field for `ActiveModel` partial updates.

use serde::{Deserialize, Serialize};

/// A field in an `ActiveModel`: either untouched, set to a value, or
/// explicitly set to NULL (which is different from untouched).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Field<T> {
    /// Not set — the column is omitted from INSERT/UPDATE.
    Unset,
    /// Set to a value.
    Value(T),
    /// Explicitly set to SQL NULL (UPDATE ... SET col = NULL).
    Null,
}

impl<T> Field<T> {
    /// Returns `true` if this field participates in a write.
    pub fn is_set(&self) -> bool {
        !matches!(self, Field::Unset)
    }

    /// Returns the value if `Field::Value`, otherwise `None`.
    pub fn value(&self) -> Option<&T> {
        match self {
            Field::Value(v) => Some(v),
            _ => None,
        }
    }
}

impl<T> From<T> for Field<T> {
    fn from(v: T) -> Self {
        Field::Value(v)
    }
}

impl<T> Default for Field<T> {
    fn default() -> Self {
        Field::Unset
    }
}
