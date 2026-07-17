# Rust Feature Architecture — Umbrella Features & Module Strata

**Status:** Accepted (2026-07-17). Scheduled as the FIRST task of milestone M4.
**Scope:** `sdks/rust` only. Non-breaking — no public feature name changes, no API changes.
**Baseline evidence:** origin/main @ `acf5d7f` (post-M2); re-verify counts at execution time.

## Problem

The per-provider feature partitioning is directionally right (CLI-only users avoid `reqwest`
entirely; routing aliases like `minimax = ["anthropic"]` are already expressed as feature
dependencies). The defect is **hand-enumerated cfg lists for shared HTTP code**:

- `feature = "gemini-code-assist"` appears **34×** in `src/` — i.e. ~30 hand-maintained
  `#[cfg(any(anthropic, openai, minimax, ollama_native, gemini, gemini-code-assist,
  chatgpt-codex))]` attributes across `client.rs`, `stream.rs`, `lib.rs`, `providers/mod.rs`.
- The same 5-dep block (`dep:reqwest, dep:chrono, dep:eventsource-stream, dep:tokio-stream,
  dep:tokio`) is repeated verbatim **5×** in `[features]`.
- Every new provider must touch every list; a missed entry only surfaces under exotic feature
  combos. This already billed us: M2's `send_with_retry` shipped without its cfg attribute and
  was caught only by review (the `cargo check --no-default-features` gate exists because of it),
  and every M3 Rust task instructs executors to copy the 7-feature attribute by hand.
- Root cause: `providers/mod.rs` mixes always-compiled items (traits, CLI helpers) with
  HTTP-only helpers, forcing item-level gating instead of module-level.

## Goals

1. New provider = one line in `[features]`; zero edits to existing cfg lists.
2. Shared HTTP code carries ONE module-level gate, not ~30 item-level enumerations.
3. Core API (`types`, `error`, `retry`, `stream::collect_stream`/`BoxStream`) compiles with
   `--no-default-features` — downstream crates (motosan-agent-loop) stop feature-juggling to
   name types.
4. CI makes a missed gate impossible to ship, not merely unlikely.

## Non-goals (considered and rejected)

- **Workspace split** (`motosan-ai-core` + per-provider crates, aws-sdk style): overkill for six
  providers sharing one wire-helper layer; adds version-matrix and release-chain cost. Revisit
  only at ~15+ providers or heavyweight provider-exclusive dependencies.
- **Default provider** (`default = ["anthropic"]`): would force `reqwest` onto CLI-only users and
  semi-break lean consumers. `default = []` + explicit README snippets + `full` stays.
- **Merging `ollama`/`ollama_native`**: runtime routing semantics are documented compat surface;
  naming gets an alias only (below), semantics untouched.

## Target design

### 1. `[features]` (Cargo.toml)

```toml
[features]
default = []

# -- internal aggregation layers ------------------------------------------
# Underscore prefix = private convention (sqlx `_rt-tokio` precedent).
# Documented as internal-only, NOT covered by semver. Never enable directly.
_http = ["dep:reqwest", "dep:chrono", "dep:eventsource-stream", "dep:tokio-stream", "dep:tokio"]
_cli  = ["dep:tokio", "dep:tokio-stream", "dep:async-stream"]

# -- public provider features ---------------------------------------------
anthropic          = ["_http"]
openai             = ["_http"]
gemini             = ["_http"]
chatgpt-codex      = ["_http"]
minimax            = ["anthropic"]        # routing alias (unchanged)
ollama             = ["openai", "dep:bytes"]
ollama_native      = ["ollama"]           # legacy spelling, kept forever
ollama-native      = ["ollama_native"]    # NEW canonical alias; docs teach this one
gemini-code-assist = ["gemini"]           # drop its duplicated dep list
claude-code        = ["_cli"]
codex-cli          = ["_cli"]
gemini-cli         = ["_cli"]
full = [ /* every public feature above */ ]
```

Note: after this change `tokio-stream` moves OUT of `_http`/`_cli` if step 3 promotes it to an
unconditional dependency (preferred) — see below; the sketch above shows the conservative form.

### 2. Module strata (one gate per stratum)

```
Stratum 0 — always compiled, ZERO cfg:
  types.rs, error.rs, retry.rs, stream.rs (incl. collect_stream/BoxStream),
  Provider enum, ProviderImpl trait, message factories.

Stratum 1 — #[cfg(feature = "_http")] ONCE at the module declaration:
  src/transport/http.rs  ← moved from providers/mod.rs: send_with_retry,
  observe_and_sleep, parse_retry_after, extract_request_id, is_retryable_status,
  is_retryable_network_error, map_http_error, RETRY_AFTER_CAP, TimeoutConfig,
  the HTTP half of ChatResponseBuilder helpers, and the in-crate
  providers::retry_conformance test mod (moves with its subjects).

Stratum 1b — #[cfg(feature = "_cli")] once:
  shared CLI helpers currently cfg-listed per backend (cli_terminal_stop_reason etc.).

Stratum 2 — per-provider files gated by their own feature (unchanged).
```

### 3. Promote `tokio-stream` to unconditional

`collect_stream` is core API but is currently cfg-gated solely because `tokio-stream` is
optional. `tokio-stream` is a small pure-Rust crate and `futures-core` is already unconditional.
Making it unconditional un-gates `stream.rs` entirely (goal 3). If rejected at execution time
(measured dep-cost objection), fallback: keep `stream.rs` gated on
`any(feature = "_http", feature = "_cli")` — still 1 gate, not 30.

### 4. CI guard

- Add **`cargo hack check --each-feature --no-dev-deps`** (and ideally `--feature-powerset
  --depth 2`) to the Rust CI job. This converts "missed gate breaks some combo someday" into
  "missed gate fails every PR".
- Keep the existing `cargo check --no-default-features` gate.

### 5. Documentation rules (add to sdks/rust/README + AGENTS.md)

1. Underscore features are internal implementation detail, not semver-covered.
2. New providers MUST route through `_http` or `_cli`; adding a new per-provider cfg
   enumeration in shared code is a review-blocking offense.
3. Docs teach `ollama-native`; `ollama_native` remains as a permanent alias.

## Migration plan (one PR, mechanical)

1. Rewrite `[features]` per §1 (pure dedup; `cargo metadata` diff must show identical resolved
   deps per feature before/after).
2. Create `src/transport/http.rs`; move Stratum-1 items from `providers/mod.rs`; re-export
   `pub(crate)` from the old paths temporarily if it shrinks the diff (optional).
3. Replace every `#[cfg(any(<7 features>))]` with the module-level `_http` gate (delete the
   item-level attributes); same for `_cli` lists.
4. §3 tokio-stream promotion + un-gate `stream.rs`.
5. Add cargo-hack to CI; run the full matrix locally once
   (`cargo hack check --each-feature`).
6. Gate: `cargo fmt && cargo clippy --all-features --all-targets -- -D warnings &&
   cargo test --all-features && cargo check --no-default-features && cargo hack check
   --each-feature`. All existing tests pass unchanged (this refactor moves code, it does not
   change behavior).

## Acceptance criteria

- `grep -rn 'feature = "gemini-code-assist"' src/` returns only the provider's own file and the
  `full`/docs references — no shared-code enumerations (34 → ≤3).
- `[features]` contains exactly one copy of the HTTP dep set and one of the CLI dep set.
- `cargo check --no-default-features` compiles Stratum 0 including `collect_stream`.
- `cargo hack check --each-feature` green in CI.
- Public feature set unchanged (plus the `ollama-native` alias); `cargo metadata` resolved-deps
  diff is empty for every pre-existing feature.

## Amendments (2026-07-17, planning-time verification at origin/main `b9bcc3e`)

Two corrections surfaced while instantiating the migration into the M4 implementation plan
(`docs/superpowers/plans/2026-07-17-stream-retry-m4-implementation.md`, Task 1):

1. **`TimeoutConfig` placement.** It does not live in `providers/mod.rs`; it lives at
   `client.rs:18` and backs the UNGATED public accessors
   `Client::connect_timeout/read_idle_timeout/total_timeout`, which exist under
   `--no-default-features` today. Placing it in `_http`-gated `transport/http.rs` would delete
   public API from CLI-only builds (breaking). It moves to the ungated `transport/mod.rs` root
   instead; everything else in Stratum 1 moves as written.
2. **Acceptance criterion №1 refined.** The baseline count grew 34 → **45** (M3 additions), and
   the literal post-migration `grep -rn 'feature = "gemini-code-assist"' src/` count is **16**,
   not ≤3: 15 are single-feature Stratum-2 gates in `client.rs` per-provider wiring that §2
   explicitly leaves unchanged, plus the provider's module decl. The enforceable form of the
   original intent: shared-code files contain exactly **1** mention (the module decl) and
   **zero `any(...)` enumerations anywhere** name the feature.
