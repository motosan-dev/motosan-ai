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
4. pypa/gh-action-pypi-publish (Trusted Publishing via OIDC — no token)
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
6. rust-lang/crates-io-auth-action → cargo publish (Trusted Publishing via OIDC — no token)
```

Key: Rust workflow runs full validation (fmt + clippy + test) before publish.

## Pre-Push Local Validation

The hook is installed by `./scripts/setup-hooks.sh` and runs
`scripts/pre-push-gate.sh`, which is **path-scoped**: it reads the pushed range
and runs only the suites whose SDK was touched, so a docs-only push skips them.

1. Version metadata (`scripts/check-versions.py`) — always
2. Python unit tests — when `sdks/python/**`, `pyproject.toml`, or `uv.lock` changed
3. Rust unit tests — when `sdks/rust/**` or the workspace `Cargo.toml` changed
4. TypeScript build + tests — when `sdks/typescript/**` changed

Live provider tests never run in CI and are opt-in locally: `RUN_LIVE=1 git push`.
Unit runs have provider credentials stripped from the environment, so the
env-gated live tests inside those suites cannot fire.

## Emergency Manual Publish

```bash
# Python
cd sdks/python && uv build --out-dir dist && uv publish dist/*

# Rust
cd sdks/rust && cargo publish
```

## Registry Authentication

All six publish workflows use **Trusted Publishing (OIDC)** — no registry
secrets are stored. Each job declares `permissions: id-token: write` and the
registry verifies the workflow's GitHub identity:

| Registry  | Mechanism                                                      |
|-----------|----------------------------------------------------------------|
| crates.io | `rust-lang/crates-io-auth-action@v1` mints a short-lived token  |
| PyPI      | `pypa/gh-action-pypi-publish` with no `password:`               |
| npm       | `npm publish` with npm >= 11.5.1 (provenance attached by default) |

Each registry must have a trusted publisher registered for the specific
repository *and* workflow file, otherwise publishing fails with an auth error.

## CI Workflows (non-release)

| Workflow         | Trigger                     | Steps                              |
|------------------|-----------------------------|------------------------------------|
| `ci-python.yml`  | Push/PR to `sdks/python/**` | `uv sync` → `ruff check` → `pytest` |
| `ci-rust.yml`    | Push/PR to `sdks/rust/**`   | `fmt` → `clippy` → `test` (stable + MSRV 1.82) |

## Dual Release (both SDKs)

When releasing both at once, use separate tags:

```bash
git add sdks/python/pyproject.toml sdks/python/CHANGELOG.md sdks/rust/Cargo.toml sdks/rust/CHANGELOG.md
git commit -m "chore: release python-vX.Y.Z + rust-vA.B.C"
git tag -a python-vX.Y.Z -m "python-vX.Y.Z — summary"
git tag -a rust-vA.B.C -m "rust-vA.B.C — summary"
git push origin main python-vX.Y.Z rust-vA.B.C
```

Both publish workflows will run in parallel.
