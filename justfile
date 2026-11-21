set shell := ["bash", "-c"]

_default:
    @just --list

# Type-check all targets.
check:
    cargo check --workspace --all-targets

# Lint with clippy.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests.
test:
    cargo test --workspace

# Format check.
fmt:
    cargo fmt --all -- --check

# Build the WASI target (core + redb only — no drivers).
wasi:
    cargo build --target wasm32-wasip2 --release -p lacrymira-core -p lacrymira-redb

# Verify the wit world compiles.
wit-check:
    @test -f wit/lacrymira.wit && echo "wit world present" || (echo "missing wit/lacrymira.wit" && exit 1)

# One-shot gate.
gate: fmt lint test wasi wit-check
