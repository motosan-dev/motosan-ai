# Release Process

Python and Rust SDKs are versioned and released **independently**.

## Tag Convention

| SDK    | Tag format       | Registry   | Workflow              |
|--------|------------------|------------|-----------------------|
| Python | `python-vX.Y.Z`  | PyPI       | `publish-python.yml`  |
| Rust   | `rust-vX.Y.Z`    | crates.io  | `publish-rust.yml`    |

## Release Checklist

### 1. Version Bump

| SDK    | File                          | Field              |
|--------|-------------------------------|--------------------|
| Python | `sdks/python/pyproject.toml`  | `version = "X.Y.Z"` |
| Rust   | `sdks/rust/Cargo.toml`        | `version = "X.Y.Z"` |

### 2. Update CHANGELOG

Both use Keep a Changelog format:

```markdown
## [X.Y.Z] - YYYY-MM-DD

### Added
- ...

### Changed
- ...

### Fixed
- ...
```

- Python: `sdks/python/CHANGELOG.md`
- Rust: `sdks/rust/CHANGELOG.md`

### 3. Commit

```bash
# Python
git add sdks/python/pyproject.toml sdks/python/CHANGELOG.md
git commit -m "chore: release python-vX.Y.Z"

# Rust
git add sdks/rust/Cargo.toml sdks/rust/CHANGELOG.md
git commit -m "chore: release rust-vX.Y.Z"
```

### 4. Tag + Push

```bash
# Python → triggers publish-python.yml → PyPI
git tag -a python-vX.Y.Z -m "python-vX.Y.Z — summary of changes"
git push origin main python-vX.Y.Z

# Rust → triggers publish-rust.yml → crates.io
git tag -a rust-vX.Y.Z -m "rust-vX.Y.Z — summary of changes"
git push origin main rust-vX.Y.Z
```

## CI Publish Pipelines

### publish-python.yml

Trigger: `push tags: ["python-v*"]`

```
Steps:
1. Checkout
2. Setup uv
3. uv build --out-dir dist
4. pypa/gh-action-pypi-publish (secret: PYPI_API_TOKEN)
```

### publish-rust.yml

Trigger: `push tags: ["rust-v*"]` OR `workflow_dispatch`

```
Steps:
1. Checkout
2. Setup stable Rust
3. cargo fmt --all -- --check
4. cargo clippy --all-features --all-targets -- -D warnings
5. cargo test --all-features
6. cargo publish (secret: CARGO_REGISTRY_TOKEN)
```

Key: Rust workflow runs full validation (fmt + clippy + test) before publish.

## Pre-Push Local Validation

```bash
./scripts/pre-push-gate.sh
```

Runs 4 steps:
1. Python unit tests (`uv run pytest`)
2. Rust unit tests (`cargo test --all-features`)
3. Python live integration tests (requires `ANTHROPIC_API_KEY`, skipped if unavailable)
4. Rust live integration tests (requires `ANTHROPIC_API_KEY`, skipped if unavailable)

## Emergency Manual Publish

```bash
# Python
cd sdks/python && uv build --out-dir dist && uv publish dist/*

# Rust
cd sdks/rust && cargo publish
```

## GitHub Secrets

| Secret                | Used by              | Purpose                |
|-----------------------|----------------------|------------------------|
| `PYPI_API_TOKEN`      | publish-python.yml   | Authenticate to PyPI   |
| `CARGO_REGISTRY_TOKEN`| publish-rust.yml     | Authenticate to crates.io |

## CI Workflows (non-release)

| Workflow         | Trigger                     | Steps                              |
|------------------|-----------------------------|------------------------------------|
| `ci-python.yml`  | Push/PR to `sdks/python/**` | `uv sync` → `ruff check` → `pytest` |
| `ci-rust.yml`    | Push/PR to `sdks/rust/**`   | `fmt` → `clippy` → `test` (stable + MSRV 1.82) |

## Dual Release (both SDKs)

When releasing both at once, use separate tags:

```bash
git add sdks/python/pyproject.toml sdks/python/CHANGELOG.md sdks/rust/Cargo.toml sdks/rust/CHANGELOG.md
git commit -m "chore: release python-v0.5.0 + rust-v0.3.4"
git tag -a python-v0.5.0 -m "python-v0.5.0 — summary"
git tag -a rust-v0.3.4 -m "rust-v0.3.4 — summary"
git push origin main python-v0.5.0 rust-v0.3.4
```

Both publish workflows will run in parallel.
