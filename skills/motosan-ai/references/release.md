# Release Process

The Rust, Python, and TypeScript SDKs are versioned and released
**independently**, as are the three OAuth helper crates.

## Tag Convention

| Package          | Tag format                | Registry  | Workflow                        |
|------------------|---------------------------|-----------|---------------------------------|
| Python           | `python-vX.Y.Z`           | PyPI      | `publish-python.yml`            |
| Rust             | `rust-vX.Y.Z`             | crates.io | `publish-rust.yml`              |
| TypeScript       | `ts-vX.Y.Z`               | npm       | `publish-typescript.yml`        |
| motosan-ai-oauth | `motosan-ai-oauth-vX.Y.Z` | crates.io | `publish-motosan-ai-oauth.yml`  |
| codex-oauth      | `codex-oauth-vX.Y.Z`      | crates.io | `publish-codex-oauth.yml`       |
| anthropic-oauth  | `anthropic-oauth-vX.Y.Z`  | crates.io | `publish-anthropic-oauth.yml`   |

## Release Checklist

### 1. Bump (scripted)

```bash
python3 scripts/bump-version.py --rust 0.28.0 --python 0.20.0   # --dry-run previews
```

Writes the manifests, both lockfiles, the CHANGELOG headings (renaming whatever
sits under `[Unreleased]`), and the doc version banners, then runs
`scripts/check-versions.py`. Several SDKs can go in one run; re-running is a
no-op. Do not hand-edit those locations — `ci-metadata` and the pre-push hook
verify them. The OAuth helper crates are not covered; bump those by hand.

### 2. Write the prose

- root `CHANGELOG.md` — a combined entry naming only the SDKs that moved
- `AGENTS.md` — a release paragraph

### 3. Commit and open a PR

```bash
git commit -m "chore(release): rust-v0.28.0 and python-v0.20.0 (#<issue>)"
```

`chore(release):` is the one exception to the `fix:` / `feat:` / `refactor:`
commit-type rule: a release ships no change of its own, so labelling it a
feature or a fix would be inaccurate. Releases go through review like anything
else.

### 4. Tag the merge commit

```bash
git tag -a rust-v0.28.0 -m "rust-v0.28.0 — summary of changes"
git push origin rust-v0.28.0
```

Create tags from a checkout of `origin/main` after the PR merges — not from a
stale branch, whose pre-push hook may be an older version.

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

Re-run the failed publish workflow first. Its steps are gated on digests, so a
re-run either finds its own artifact already published and succeeds, or uploads
what is missing.

Publishing from a laptop is a last resort: Trusted Publishing leaves no ambient
credential, so it requires temporarily minting a registry token, and it skips
every guard — the tag-vs-manifest check, the SDK gates, and the digest
verification that proves the registry serves what was built.

## Registry Authentication

All six publish workflows use **Trusted Publishing (OIDC)** — no registry
secrets are stored. Each job declares `permissions: id-token: write` and the
registry verifies the workflow's GitHub identity:

| Registry  | Mechanism                                                      |
|-----------|----------------------------------------------------------------|
| crates.io | `rust-lang/crates-io-auth-action@v1` mints a short-lived token  |
| PyPI      | `pypa/gh-action-pypi-publish` with no `password:`               |
| npm       | `npm publish` with npm >= 11.5.1 (provenance attached by default) |

## Publish Verification

crates.io, PyPI, and npm publishes are all gated on what the registry actually
serves, via `scripts/verify-published.py`: before publishing, a matching
version already on the registry short-circuits the upload, and a *differing*
one fails the run; after publishing, the workflow polls until the registry
serves the artifact and compares digests. Re-running a half-finished release
is therefore safe.

The comparison is by construction rather than by reproducible build: PyPI
receives the exact `dist/*` files that were hashed, and npm receives the exact
tarball `npm pack` produced and reported an integrity for. (`npm pack` is *not*
byte-reproducible across npm versions, so re-packing at verification time would
compare two different tarballs.)

Each registry must have a trusted publisher registered for the specific
repository *and* workflow file, otherwise publishing fails with an auth error.

## CI Workflows (non-release)

| Workflow         | Trigger                     | Steps                              |
|------------------|-----------------------------|------------------------------------|
| `ci-metadata.yml`   | Every push/PR (no path filter) | `scripts/check-versions.py` |
| `ci-python.yml`     | Push/PR to `sdks/python/**` | `uv sync` → `ruff check` → `mypy` → `pytest` |
| `ci-rust.yml`       | Push/PR to `sdks/rust/**`   | `fmt` → `clippy` → `test` → `cargo hack --each-feature` (stable + MSRV 1.82) |
| `ci-typescript.yml` | Push/PR to `sdks/typescript/**` | `npm ci` → build → typecheck → test → pack smoke |

## Releasing Several SDKs Together

This is the normal case, not a special one: `bump-version.py` takes every SDK in
a single run because they share the doc banners, the root `CHANGELOG.md` entry
names only the SDKs that moved, and one PR carries all of it. After it merges,
push one tag per SDK:

```bash
git tag -a python-vX.Y.Z -m "python-vX.Y.Z — summary"
git tag -a rust-vA.B.C -m "rust-vA.B.C — summary"
git push origin python-vX.Y.Z rust-vA.B.C
```

The publish workflows run in parallel, each verifying its own tag against its
own manifest.
