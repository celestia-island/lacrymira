//! Type-safe condition wrapper: `TypedCondition<E>` constrains filters to E's columns.
//!
//! Users write `Column::UserId.eq(value)` which returns `TypedCondition<E>`.
//! `Select<E>::filter()` only accepts `TypedCondition<E>` — a Column from
//! another Entity will not compile. Internally converts to untyped `Condition`
//! for serialization (msgpack) and rendering.

use std::marker::PhantomData;

use crate::condition::{ColumnRef, Condition};
use crate::entity::Entity;
use crate::value::Value;

/// A condition bound to a specific Entity's columns.
///
/// Wraps an untyped `Condition` with a phantom `E` marker so that
/// `Select<E>` can only accept conditions built from E's Column enum.
pub struct TypedCondition<E: Entity> {
    pub(crate) inner: Condition,
    _marker: PhantomData<fn() -> E>,
}

impl<E: Entity> TypedCondition<E> {
    /// Create from an untyped Condition (internal use).
    pub(crate) fn from_inner(inner: Condition) -> Self {
        TypedCondition {
            inner,
            _marker: PhantomData,
        }
    }

    /// Extract the untyped Condition (for serialization / rendering).
    pub fn into_inner(self) -> Condition {
        self.inner
    }

    /// Combine with AND.
    pub fn and(self, other: TypedCondition<E>) -> TypedCondition<E> {
        TypedCondition::from_inner(self.inner.and(other.inner))
    }

    /// Combine with OR.
    pub fn or(self, other: TypedCondition<E>) -> TypedCondition<E> {
        TypedCondition::from_inner(self.inner.or(other.inner))
    }

    /// Negate.
    pub fn not(self) -> TypedCondition<E> {
        TypedCondition::from_inner(Condition::Not(Box::new(self.inner)))
    }
}

impl<E: Entity> From<Condition> for TypedCondition<E> {
    fn from(inner: Condition) -> Self {
        TypedCondition::from_inner(inner)
    }
}

impl<E: Entity> From<TypedCondition<E>> for Condition {
    fn from(tc: TypedCondition<E>) -> Self {
        tc.into_inner()
    }
}

// ── Builder methods that produce TypedCondition ──
// These live on a trait so the derive macro can implement them for each
// entity's Column enum without the macro needing to generate 18 methods.

/// Column builder that produces `TypedCondition<E>`.
///
/// The derive macro generates `impl TypedColumnOps for Column` where
/// `type Entity = MyEntity`, giving all 18 operators type safety.
pub trait TypedColumnOps: Sized {
    /// The Entity this column belongs to.
    type Entity: Entity;

    /// The SQL column name for this variant.
    fn column_name(&self) -> &'static str;

    /// Convert to ColumnRef.
    fn to_ref(&self) -> ColumnRef {
        ColumnRef::new(self.column_name())
    }

    // ── 18 comparison operators ──

    fn eq<V: Into<Value>>(self, v: V) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().eq(v))
    }
    fn ne<V: Into<Value>>(self, v: V) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().ne(v))
    }
    fn lt<V: Into<Value>>(self, v: V) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().lt(v))
    }
    fn lte<V: Into<Value>>(self, v: V) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().lte(v))
    }
    fn gt<V: Into<Value>>(self, v: V) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().gt(v))
    }
    fn gte<V: Into<Value>>(self, v: V) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().gte(v))
    }
    fn is_null(self) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().is_null())
    }
    fn is_not_null(self) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().is_not_null())
    }
    fn in_<V: Into<Value>>(self, values: Vec<V>) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().in_(values))
    }
    fn not_in<V: Into<Value>>(self, values: Vec<V>) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().not_in(values))
    }
    fn like(self, pattern: impl Into<String>) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().like(pattern))
    }
    fn ilike(self, pattern: impl Into<String>) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().ilike(pattern))
    }
    fn contains(self, substr: impl Into<String>) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().contains(substr))
    }
    fn starts_with(self, prefix: impl Into<String>) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().starts_with(prefix))
    }
    fn ends_with(self, suffix: impl Into<String>) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().ends_with(suffix))
    }
    fn between<V: Into<Value>>(self, low: V, high: V) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().between(low, high))
    }
    fn not_between<V: Into<Value>>(self, low: V, high: V) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().not_between(low, high))
    }
}

// ── JSONB compile-time restriction ──
// TypedJsonColumnOps is only implemented for entities that HAVE JSON columns.
// Entities without any serde_json::Value field → json methods unavailable at compile time.

/// JSONB path operations, only available on entities with JSON columns.
///
/// The derive macro generates `impl TypedJsonColumnOps for Column` **only when
/// the entity has at least one `serde_json::Value` field**. On entities without
/// JSON columns, calling `.json_get()` is a compile error (trait not in scope).
pub trait TypedJsonColumnOps: TypedColumnOps {
    /// `col->path = value` (JSONB path extraction equality)
    fn json_get(self, path: impl Into<String>) -> JsonPathTyped<Self::Entity> {
        JsonPathTyped {
            column: self.to_ref(),
            path: path.into(),
            _marker: PhantomData,
        }
    }

    /// `col ? path` (JSONB key/path existence)
    fn json_exists(self, path: impl Into<String>) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().json_exists(path))
    }

    /// `col @> value` (JSONB containment)
    fn json_contains(self, v: serde_json::Value) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().json_contains(v))
    }

    /// `jsonb_path_match(col, path, vars)` (PostgreSQL 12+)
    fn jsonb_path_match(
        self,
        path: impl Into<String>,
        vars: serde_json::Value,
    ) -> TypedCondition<Self::Entity> {
        TypedCondition::from_inner(self.to_ref().jsonb_path_match(path, vars))
    }
}

/// Builder for `col->path` comparisons (returned by `json_get`).
pub struct JsonPathTyped<E: Entity> {
    column: ColumnRef,
    path: String,
    _marker: PhantomData<fn() -> E>,
}

impl<E: Entity> JsonPathTyped<E> {
    /// `col->path = value`
    pub fn eq<V: Into<Value>>(self, v: V) -> TypedCondition<E> {
        TypedCondition::from_inner(crate::condition::Condition::JsonGetEq(
            self.column,
            self.path,
            v.into(),
        ))
    }

    /// `col->path != value`
    pub fn ne<V: Into<Value>>(self, v: V) -> TypedCondition<E> {
        TypedCondition::from_inner(crate::condition::Condition::Not(Box::new(
            crate::condition::Condition::JsonGetEq(self.column, self.path, v.into()),
        )))
    }
}
