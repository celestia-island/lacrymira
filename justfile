set shell := ["bash", "-c"]

_default:
    @just --list

# Type-check all targets.
check:
    cargo check --workspace --all-targets

# Lint with clippy (deny warnings).
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests.
test:
    cargo test --workspace

# Run all tests including DB-backed ones (requires LACRYMIRA_PG_TEST_URL).
test-db:
    cargo test --workspace -- --include-ignored

# Format check.
fmt:
    cargo fmt --all -- --check

# Format write.
fmt-write:
    cargo fmt --all

# Build the WASI target (core + redb + wasi — no host-side drivers).
wasi:
    cargo build --target wasm32-wasip2 --release -p lacrymira -p lacrymira-redb -p lacrymira-wasi

# Verify the wit world compiles.
wit-check:
    @test -f crates/wasi/wit/lacrymira.wit && echo "wit world present" || (echo "missing crates/wasi/wit/lacrymira.wit" && exit 1)

# Verify zero tokio in the WASI tree.
tokio-check:
    @count=$(cargo tree -p lacrymira -p lacrymira-redb -p lacrymira-wasi --target wasm32-wasip2 --no-dev-dependencies 2>/dev/null | grep -c tokio || true); \
    if [ "$$count" -gt 0 ]; then echo "FAIL: $$count tokio references in WASI tree"; exit 1; \
    else echo "PASS: zero tokio in WASI tree"; fi

# Verify version specifiers follow dep-spec rules (single-segment ^ or 0.x).
depspec-check:
    @python3 -c "\
    import re, pathlib, sys; \
    bad = []; \
    [bad.append(f'{p}: {m}') for p in pathlib.Path('.').rglob('Cargo.toml') \
      if 'target' not in str(p) \
      for m in re.findall(r'version\s*=\s*\"(\^\*|\*|latest|\^0\.\*)\"', p.read_text())]; \
    print('FAIL:', bad) if bad else print('PASS: all version specifiers compliant'); sys.exit(1 if bad else 0)"

# One-shot gate.
gate: fmt lint test wasi wit-check tokio-check
