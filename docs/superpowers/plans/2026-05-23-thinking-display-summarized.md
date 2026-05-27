# motosan-ai 0.15.5 — opt thinking config into `display: "summarized"`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Patch `providers/anthropic.rs` so the request body's `thinking` config explicitly sets `display: "summarized"`. Without it, the OAuth product surface (`sk-ant-oat01-*` tokens via Claude Code subscriptions) defaults to `display: "omitted"`, suppressing all `thinking_delta` SSE events. With it, OAuth callers receive proper streaming thinking. Direct API key callers (`sk-ant-api03-*`) are unaffected — they already default to `"summarized"` for thinking-capable models.

**Architecture:** Two trivial body-builder edits. Bumps motosan-ai 0.15.4 → 0.15.5 (patch — additive default that matches Anthropic's documented "summarized" intent for older Claude 4 models and pi's behavior).

**Verification baseline (already done by the human):** A standalone Rust binary calling `motosan_ai::Client::stream_with(req)` with `req.thinking(4096)` against a real OAuth token receives **2 thinking_delta + 12 text_delta StreamEvents** for "Prove sqrt(2) is irrational." Without the `display` field the same call returns ~1 text event and zero thinking events.

## Context for the implementer

You are working in `~/Projects/wade/motosan-ai/sdks/rust`. v0.15.4 is on crates.io (shipped earlier today with the initial `StreamEventType::ThinkingDelta`/`ThinkingDone` plumbing). The omission found here turns those events into no-ops for Claude Code OAuth users.

The patch is already drafted in the working tree against the `main` branch. You can either:
- Use the existing `git diff src/providers/anthropic.rs` and just commit + ship, or
- Re-derive the patch yourself by editing both `body["thinking"] = json!({...})` blocks.

**Verification discipline** (memory rule `feedback_verify_subagent_published_artifacts`): **Do NOT publish to crates.io. Do NOT push tags. Do NOT push the working branch.** The human handles tag + push + publish after reviewing your commit.

---

### Task 1: Add `display: "summarized"` to both thinking body builders

**Files:**
- Modify: `sdks/rust/src/providers/anthropic.rs` — `AnthropicRequestBuilder::build` (non-streaming, around line 327) and the OAuth streaming body builder (around line 687)

- [ ] **Step 1: Locate both sites**

```bash
grep -n '"budget_tokens": thinking.budget_tokens' sdks/rust/src/providers/anthropic.rs
```

Expected: exactly **two matches**.

- [ ] **Step 2: Add `display: "summarized"` at each site**

At each match, the surrounding code is:

```rust
body["thinking"] = json!({
    "type": "enabled",
    "budget_tokens": thinking.budget_tokens,
});
```

Add a third key:

```rust
body["thinking"] = json!({
    "type": "enabled",
    "budget_tokens": thinking.budget_tokens,
    "display": "summarized",
});
```

Above the first occurrence, add a comment explaining why (see the existing `git diff` for exact wording).

- [ ] **Step 3: Build + test**

```bash
cargo build 2>&1 | tail -3
cargo test --features anthropic 2>&1 | tail -3
```

Both must finish clean.

- [ ] **Step 4: Live integration smoke (optional, requires OAuth token)**

If you have `~/.capo/agent/auth.json` configured with an `sk-ant-oat01-*` token, run the human's standalone test:

```bash
cd /tmp/motosan-test && cargo run --release 2>&1 | tail -8
```

Expected: `thinking_delta events: 2` (or higher), `text_delta events: 10+`. If the test directory doesn't exist, skip this step and note so in the report.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/providers/anthropic.rs
git commit -m "fix(anthropic): explicitly set thinking.display = \"summarized\"

Without this, the OAuth product surface (sk-ant-oat01-* tokens via
Claude Code subscriptions) defaults the thinking display to \"omitted\"
regardless of model — zero thinking_delta SSE events emitted. Adding
display: \"summarized\" makes Anthropic stream the thinking content
properly. Matches pi's behaviour at
packages/ai/src/providers/anthropic.ts:950+968.

Direct API key callers (sk-ant-api03-*) are unaffected: they already
default to \"summarized\" for Sonnet 4.5/4.6/Opus 4.6.

Refs: docs/superpowers/plans/2026-05-23-thinking-display-summarized.md"
```

---

### Task 2: Version bump 0.15.4 → 0.15.5 + CHANGELOG

**Files:**
- Modify: `sdks/rust/Cargo.toml` — version `0.15.4` → `0.15.5`
- Modify: `CHANGELOG.md` — new entry at the top

- [ ] **Step 1: Bump version**

```bash
sed -i.bak 's/^version = "0.15.4"$/version = "0.15.5"/' sdks/rust/Cargo.toml && rm sdks/rust/Cargo.toml.bak
grep '^version' sdks/rust/Cargo.toml
```

Expected: `version = "0.15.5"`.

- [ ] **Step 2: Add CHANGELOG entry**

Insert at the top of `CHANGELOG.md`, before the previous most-recent entry:

```markdown
## [rust-0.15.5] — 2026-05-23

### Fixed

- **Anthropic provider sends `display: "summarized"` in the thinking config.** Without it the OAuth product surface (`sk-ant-oat01-*` tokens issued by Claude Code subscriptions) silently defaults the thinking display to `"omitted"` for all models — Anthropic accepts the request but returns zero `thinking_delta` SSE events. With the explicit `summarized` the OAuth tier behaves like direct API key callers and streams thinking content per-delta. Patch covers both non-streaming and streaming OAuth body builders (`sdks/rust/src/providers/anthropic.rs`). Verified end-to-end against `claude-sonnet-4-6` via a Claude Pro OAuth token.
```

- [ ] **Step 3: Build + dry-run publish**

```bash
cd sdks/rust
cargo build --release
cargo publish --dry-run --features anthropic 2>&1 | tail -5
```

All must finish clean.

- [ ] **Step 4: Commit + tag locally (do NOT push)**

```bash
git add sdks/rust/Cargo.toml CHANGELOG.md
git commit -m "release(rust): v0.15.5 — Anthropic thinking display fix"
git tag -a rust-0.15.5 -m "rust-0.15.5"
```

Do **not** `git push` and do **not** `cargo publish` without `--dry-run`.

---

### Task 3: Final verification + report

- [ ] **Step 1: Confirm artifact doesn't already exist on crates.io**

```bash
curl -sI "https://static.crates.io/crates/motosan-ai/motosan-ai-0.15.5.crate" | head -1
```

Expected: `HTTP/2 404`. If `200`, stop — someone else published.

- [ ] **Step 2: Report**

Summarise:
- Final commit SHA + tag name (local only).
- `cargo publish --dry-run` output.
- Whether you ran the live integration smoke (Task 1 Step 4) and what it printed.
- Any deviations from the plan.

**Do not push. Do not publish. Do not push the tag.**

---

## Anti-scope

1. Do NOT publish to crates.io. The human runs `cargo publish` after reviewing.
2. Do NOT push to `origin`. The human pushes after publish succeeds.
3. Do NOT also modify motosan-agent-loop or capo. Those are separate release plans.
4. Do NOT change defaults for non-Anthropic providers — `display` is Anthropic-specific.
5. Do NOT make `display` a configurable knob in this release. If a use case for `"omitted"` ever appears, add it later as `ChatRequestBuilder::thinking_display(...)`. Out of scope here.
