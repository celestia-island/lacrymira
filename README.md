<div align="center">
  <img src="https://raw.githubusercontent.com/celestia-island/celestia-island.github.io/main/res/logos/lacrymira.webp" alt="lacrymira" width="180" />
</div>

<h1 align="center">Lacrymira</h1>

<div align="center">
  <strong>Typed multi-model data access layer — SQL and KV under one command surface, native and WASI</strong>
</div>

<br />

<div align="center">
  <a href="https://github.com/celestia-island/lacrymira/actions/workflows/ci.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/celestia-island/lacrymira/ci.yml?branch=dev&label=CI" alt="CI" />
  </a>
  <a href="https://github.com/celestia-island/lacrymira/actions/workflows/depspec-lint.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/celestia-island/lacrymira/depspec-lint.yml?branch=master&label=depspec" alt="Depspec" />
  </a>
  <a href="https://sysl.celestia.world">
    <img src="https://img.shields.io/badge/license-SySL%201.0-blue" alt="License: SySL" />
  </a>
  <a href="https://github.com/casey/just">
    <img src="https://img.shields.io/badge/built%20with-just-blue" alt="Built with just" />
  </a>
  <a href="https://github.com/celestia-island/lacrymira">
    <img src="https://img.shields.io/badge/github-celestia--island%2Flacrymira-blue" alt="GitHub" />
  </a>
</div>

<div align="center">

**English**

</div>

---

## Overview

Lacrymira is the Celestia family's data access layer: one typed command surface
(`Command` / `Reply`) over PostgreSQL and redb, designed to run both natively
on the host and inside `wasm32-wasip2` components.

Named after [Lacrymira](https://arcaea.fandom.com/wiki/Lacrymira_(Ascendant_-_8th_Seeker))
(Ascendant — 8th Seeker) from Arcaea Act II, the keeper of sculpted memories.

## Design pillars

- **Hygienic macros** — `#[derive(Entity)]` generates a named child module
  (`mod users_entity`); zero namespace pollution. Nothing exists unless you
  explicitly import it.
- **Type-safe columns** — `Column::UserId.eq(value)` returns
  `TypedCondition<E>`; a Column from another Entity will not compile.
  Zero hand-written string column names.
- **Compile-time JSONB restriction** — `json_get` / `json_exists` /
  `json_contains` are only available on entities that have
  `serde_json::Value` fields. No JSON columns → compile error.
- **Optimistic locking** — `#[lacrymira(version)]` marks a version column;
  `save()` generates `UPDATE SET version = version + 1 WHERE pk AND
  version = $expected`; 0 rows → `Error::VersionConflict`.
- **Pessimistic locking** — `LockMode::Update / UpdateNoWait / SkipLocked /
  Share` renders the `FOR UPDATE` family.
- **UUID collision retry** — `InsertPolicy` with v4 / v5 / v7 strategies;
  23505 triggers automatic retry up to `CollisionPolicy::Retry(n)`.
- **jiff first, chrono opt-in** — `jiff::Timestamp` is the default time type;
  chrono requires an explicit feature flag.
- **WASI-ready from day one** — core has zero tokio; redb runs natively inside
  components; drivers stay host-side behind a wit import.

## Quick start

```rust
use lacrymira::{DeriveEntity, Select, TypedColumnOps};
use lacrymira_pg::PgExecutor;

// Define an entity (hygienic: generates mod users_entity { Column, ActiveModel, ... })
#[derive(Debug, Clone, PartialEq, lacrymira::DeriveEntity)]
#[lacrymira(table = "users")]
pub struct Model {
    #[lacrymira(primary_key)]
    pub id: i64,
    pub name: String,
    pub metadata: serde_json::Value,
}

// Re-export Column from the generated module
use users_entity::Column;

#[tokio::main]
async fn main() -> Result<(), lacrymira::Error> {
    let executor = PgExecutor::connect("postgres://localhost/mydb").await?;

    // Typed select — zero string column names
    let rows = Select::<Model>::new()
        .filter(Column::Name.eq("Alice"))
        .filter(Column::Metadata.json_get("$.theme").eq("dark"))
        .lock(lacrymira::LockMode::SkipLocked)  // FOR UPDATE SKIP LOCKED
        .limit(10)
        .all(&executor)
        .await?;

    // Dynamic composition (if-branch filter — the diesel lesson)
    let age: Option<i32> = Some(18);
    let mut q = Select::<Model>::new().filter(Column::Name.like("%A%"));
    if let Some(a) = age {
        q = q.filter(Column::Id.gte(a as i64));  // ✅ compiles
    }

    Ok(())
}
```

## Crates

| Crate | Role |
|---|---|
| `lacrymira` | Entity / Condition / Command / Reply / Error / TypedCondition / LockMode / InsertPolicy / IndexBuilder |
| `lacrymira-macros` | `#[derive(Entity)]` — hygienic child module, Column enum, ActiveModel, TypedColumnOps |
| `lacrymira-pg` | PostgreSQL driver (sqlx 0.9, pgvector, jiff-sqlx) + migration runner + index drift check |
| `lacrymira-redb` | redb KV driver (runs natively and in WASI, zero tokio) |
| `lacrymira-wasi` | wit world `celestia:lacrymira@0.1.0` + msgpack component bindings |

## Wire contract

All `Command` / `Reply` types serialize via **msgpack** (rmp-serde). The wire
format is frozen by contract tests: encode → decode → re-encode must produce
byte-identical output. The wit world carries opaque `list<u8>` payloads.

## Capability matrix

| Capability | PostgreSQL | redb | TiKV (planned) |
|---|---|---|---|
| `find` + typed `filter` | ✅ | ⚠️ range only | ⚠️ range only |
| Optimistic locking | ✅ | ❌ | ⬜ |
| Pessimistic locking | ✅ `FOR UPDATE` family | ❌ | ❌ |
| JSONB path queries | ✅ | ❌ | ❌ |
| UUID auto-generate + retry | ✅ | ❌ | ❌ |
| Composite / partial / covering indexes | ✅ `IndexBuilder` | ❌ | ❌ |
| Vector search (pgvector) | ✅ | ❌ | ❌ |
| Transactions | ✅ ACID | ✅ single-node | ⬜ Percolator |
| Runs inside `wasm32-wasip2` | ❌ | ✅ | ❌ |

## Status

All core milestones (M0–M7) are complete: core types, condition AST, derive
macro, PG renderer + executor, WASI channel, redb KV driver, migration runner,
and the arcaea.celestia.world pilot ([PR #49](
https://github.com/celestia-island/arcaea.celestia.world/pull/49)).

Follow-up: full-family msgpack unification (tairitsu / hikari switching from
`serde_json` to `rmp-serde`).

## License

[SySL-1.0](https://sysl.celestia.world)
