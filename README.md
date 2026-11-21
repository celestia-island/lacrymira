# lacrymira

A multi-model data access layer for the Celestia family — SQL and KV under one
typed command surface, designed to run both natively and inside wasm components.

Named after [Lacrymira](https://arcaea.fandom.com/wiki/Lacrymira_(Ascendant_-_8th_Seeker))
(Ascendant - 8th Seeker) from Arcaea Act II — the keeper of sculpted memories.
The default transaction isolation level is named `Solitude` for a reason.

## Design pillars

- **jiff first, chrono opt-in**: `jiff::Timestamp` is the default time type;
  `chrono` support requires an explicit feature flag.
- **Postgres + pgvector first**: the primary SQL backend. MySQL / SQLite /
  TiDB are opt-in features.
- **KV native**: `redb` is the first-class KV backend (it compiles to
  `wasm32-wasip2`, making it the only backend that runs inside a component).
  Redis is opt-in.
- **One command surface**: all backends share a `Condition` tree and a
  `Command`/`Reply` protocol; capability differences are compile-time traits
  (`CanFilter`, `CanTransact`, `CanVectorSearch`), not runtime errors.
- **WASI-ready from day one**: the core has zero tokio dependency; drivers stay
  on the host side and are reached through a `wit` import.

## Crates

| Crate | Role |
|---|---|
| `lacrymira-core` | Entity / Column / Condition / Command / Reply / Error / DbTime |
| `lacrymira-macros` | `#[derive(Entity)]` — table metadata, Column enum, ActiveModel, FromRow |
| `lacrymira-pg` | PostgreSQL driver (sqlx 0.9, pgvector, jiff-sqlx 1.0) |
| `lacrymira-redb` | redb KV driver (runs natively and in WASI) |
| `lacrymira-wasi` | wit world + component-side bindings |

## Status

Pre-M0 (workspace skeleton). See `_reports/orm-strategy-20261121/PLAN.md` in
the workspace root for the engineering plan.
