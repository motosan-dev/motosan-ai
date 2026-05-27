# Ollama `think` Parser Fix + Housekeeping (0.15.1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the pre-existing `ollama_think` boolean coercion bug surfaced during 0.15.0 plan work (see `2026-05-17-ollama-http-wiring.md` header) — `OllamaProvider::build_request_body` currently hard-codes `body["think"] = json!(true)` whenever `self.think.is_some()`, regardless of the actual input string. So `ollama_think("yes")`, `ollama_think("no")`, and `ollama_think("low")` all produce identical bodies. Bundle a `.gitignore` macOS-noise cleanup. Ship as 0.15.1 patch (additive — no breaking surface).

**Architecture:** The setter `ClientBuilder::ollama_think(impl Into<String>)` already takes a string; the bug is purely in the serializer at `providers/ollama.rs:138-140`. Replace the unconditional `json!(true)` with a string parser:

- Truthy synonyms (`"true"` / `"yes"` / `"on"` / `"1"`, case-insensitive, trimmed) → JSON `true`
- Falsy synonyms (`"false"` / `"no"` / `"off"` / `"0"`, case-insensitive, trimmed) → JSON `false`
- Anything else (e.g. `"low"`, `"medium"`, `"high"`) → JSON string verbatim (trimmed)

This is backward-compatible: existing `ollama_think("yes")` callers still get bool `true` on the wire. New callers can opt into Ollama's string-valued reasoning levels (`"low"` / `"medium"` / `"high"`) by passing them through.

**Tech Stack:** Rust 1.82, `serde_json::Value`, existing `OllamaProvider` at `sdks/rust/src/providers/ollama.rs`. No new deps.

**Why patch (0.15.1) not minor:** purely additive — no public-API surface change (setter signature unchanged), no Cargo feature change, no new event variants. The existing bug-bug-bug behavior (always emitting bool `true`) is preserved for the most common case (`"yes"`); we just add value-type fidelity for the cases that were getting silently lost.

---

## File Structure

- **Modify:** `sdks/rust/src/providers/ollama.rs:138-140` — replace the 3-line `body["think"] = json!(true)` block with the parser.
- **Modify:** `sdks/rust/src/providers/ollama.rs` (end of file) — add a new `#[cfg(test)] mod tests` block with parser tests (file currently has NO tests block — verify with `grep -c "^mod tests" sdks/rust/src/providers/ollama.rs`).
- **Modify:** `sdks/rust/tests/ollama_http_autoswitch.rs` — extend the existing live test family with a new `#[ignore]`'d test that exercises the `ollama_think` parser end-to-end against a real Ollama server.
- **Modify:** `.gitignore` — add macOS section with `.DS_Store`.
- **Modify:** `sdks/rust/Cargo.toml` — version `0.15.0` → `0.15.1`.
- **Modify:** `sdks/rust/CHANGELOG.md` — `## [0.15.1] - 2026-05-17` entry.
- **Modify:** `AGENTS.md`, `llms.txt`, `README.md`, `sdks/rust/README.md`, `skills/motosan-ai/SKILL.md`, `skills/motosan-ai/references/rust-api.md` — version bumps.

---

## Task 1: Parse `ollama_think` string value correctly in `build_request_body`

**Files:**
- Modify: `sdks/rust/src/providers/ollama.rs` (the `build_request_body` method + a new `mod tests` block at end of file)

- [ ] **Step 1: Add a `mod tests` block with 4 failing tests**

Append to the very end of `sdks/rust/src/providers/ollama.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChatRequest;

    fn req() -> ChatRequest {
        ChatRequest::builder()
            .message(crate::types::Message::user("hi"))
            .build()
    }

    #[test]
    fn think_truthy_strings_serialize_as_bool_true() {
        for input in &["true", "yes", "on", "1", "YES", "True", "  yes  "] {
            let provider = OllamaProvider::new("llama3", "http://x")
                .with_think(Some(input.to_string()));
            let body = provider.build_request_body(&req(), false);
            assert_eq!(
                body["think"],
                serde_json::json!(true),
                "input {input:?} should serialize as bool true, got {:?}",
                body["think"]
            );
        }
    }

    #[test]
    fn think_falsy_strings_serialize_as_bool_false() {
        for input in &["false", "no", "off", "0", "NO", "False"] {
            let provider = OllamaProvider::new("llama3", "http://x")
                .with_think(Some(input.to_string()));
            let body = provider.build_request_body(&req(), false);
            assert_eq!(
                body["think"],
                serde_json::json!(false),
                "input {input:?} should serialize as bool false, got {:?}",
                body["think"]
            );
        }
    }

    #[test]
    fn think_other_strings_pass_through_verbatim() {
        for input in &["low", "medium", "high", "custom-value"] {
            let provider = OllamaProvider::new("llama3", "http://x")
                .with_think(Some(input.to_string()));
            let body = provider.build_request_body(&req(), false);
            assert_eq!(
                body["think"],
                serde_json::json!(input),
                "input {input:?} should pass through as string, got {:?}",
                body["think"]
            );
        }
    }

    #[test]
    fn think_not_set_omits_field_entirely() {
        let provider = OllamaProvider::new("llama3", "http://x").with_think(None);
        let body = provider.build_request_body(&req(), false);
        assert!(
            body.get("think").is_none(),
            "think field should be absent when not set; got body: {body}"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --features ollama --lib providers::ollama::tests`

Expected: FAIL — three of the four tests should fail because the current `build_request_body` hard-codes `body["think"] = json!(true)`:
- `think_truthy_strings_serialize_as_bool_true` — should PASS (all map to true today by accident)
- `think_falsy_strings_serialize_as_bool_false` — FAIL (today emits bool `true` for falsy inputs)
- `think_other_strings_pass_through_verbatim` — FAIL (today emits bool `true` for `"low"` etc.)
- `think_not_set_omits_field_entirely` — should PASS (the `if self.think.is_some()` guard already handles this)

If `think_truthy_strings_serialize_as_bool_true` also fails, something in your environment is unusual — investigate before continuing.

- [ ] **Step 3: Apply the parser fix**

In `sdks/rust/src/providers/ollama.rs`, locate the existing 3-line block at lines 138-140 (verify with `grep -n 'body\["think"\]' sdks/rust/src/providers/ollama.rs`):

```rust
        // Think mode: when set, pass think=true to Ollama
        if self.think.is_some() {
            body["think"] = json!(true);
        }
```

Replace with:

```rust
        // Think mode: parse the user-supplied string into an appropriate
        // JSON value so callers can opt into either:
        //   - bool true/false (truthy / falsy synonyms)
        //   - string reasoning levels like "low" / "medium" / "high"
        //     (newer Ollama versions accept these)
        // Before 0.15.1 this hard-coded `true` for any non-None value,
        // silently flattening `ollama_think("no")` to bool true. Fixed in
        // 0.15.1 — see CHANGELOG.
        if let Some(think_str) = &self.think {
            let trimmed = think_str.trim();
            body["think"] = match trimmed.to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => json!(true),
                "false" | "no" | "off" | "0" => json!(false),
                _ => json!(trimmed),
            };
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --features ollama --lib providers::ollama::tests`

Expected: all 4 tests PASS.

Also run the broader Ollama suite to confirm no regression:

```
cargo test --features ollama,ollama_native
```

Expected: all tests PASS. The existing `tests/ollama_native_provider.rs::*` integration tests that use `ollama_think("on")` should continue to pass (the old behavior of emitting `true` is preserved for "on").

- [ ] **Step 5: Format**

Run: `cargo fmt`

Expected: zero output. If rustfmt rewraps anything, include those rewrites in the same commit (lesson from the 0.15.0 execution where long mockito lines caused a rule-6-stop at the verification gate).

- [ ] **Step 6: Commit**

```bash
git add sdks/rust/src/providers/ollama.rs
git commit -m "$(cat <<'EOF'
fix(rust): ollama_think now serializes per input value instead of hard-coding true

Pre-existing bug from before 0.15.0: ClientBuilder::ollama_think takes
a string but providers/ollama.rs:138-140 was hard-coding
body["think"] = json!(true) whenever self.think.is_some(), silently
flattening ollama_think("no") to bool true.

Fix parses the string:
- truthy synonyms (true/yes/on/1, case-insensitive, trimmed) → bool true
- falsy synonyms (false/no/off/0, case-insensitive, trimmed) → bool false
- anything else → string verbatim (so callers can opt into Ollama's
  newer reasoning-level enum: "low" / "medium" / "high")

Backward compatible: existing `ollama_think("yes")` / `ollama_think("on")`
callers still get bool true on the wire. New callers can pass any
non-bool value to forward verbatim.

Four unit tests added covering truthy / falsy / passthrough / unset cases.

Closes the pre-existing limitation noted in the header of
docs/superpowers/plans/2026-05-17-ollama-http-wiring.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Live integration test for `ollama_think` wire compat

**Files:**
- Modify: `sdks/rust/tests/ollama_http_autoswitch.rs` (append a new `#[ignore]`'d test)

Per the project memory rule: any HTTP/wire-behavior fix needs a `#[ignore]`'d live test in Done Criteria. The parser change emits different JSON bytes on the wire, so this counts. The test verifies "real Ollama doesn't reject the new serialization shape", not that thinking actually happens (which is model-dependent — most common pre-pulled models like `llama3.1:8b` don't support thinking, but they don't reject the `think` field either).

- [ ] **Step 1: Add the live test**

Append to `sdks/rust/tests/ollama_http_autoswitch.rs`:

```rust
#[tokio::test]
#[ignore] // Requires `ollama serve` running on localhost:11434 + OLLAMA_MODEL env var.
async fn live_ollama_think_string_parser_round_trip() {
    // Verifies the 0.15.1 fix: ollama_think("yes") and ollama_think("true")
    // both still produce a wire body the real Ollama server accepts.
    //
    // Scope honesty: most common pre-pulled models (llama3.1:8b, qwen2.5,
    // mistral) don't actually support `think` — they accept the field
    // silently and return a normal response. This test verifies "Ollama
    // doesn't reject the new serialization shape", not "thinking actually
    // happens". For the latter, set OLLAMA_MODEL to a think-capable model
    // like deepseek-r1 or qwen3.
    //
    // To run:
    //   OLLAMA_MODEL=llama3.1:8b cargo test --features ollama \
    //     --test ollama_http_autoswitch live_ollama_think -- --ignored --nocapture

    let model = std::env::var("OLLAMA_MODEL").expect(
        "set OLLAMA_MODEL to any chat model you have pulled — \
         run `ollama list` to see what's available",
    );
    let base_url =
        std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(&base_url)
        .model(&model)
        .ollama_think("yes") // parser maps to bool true on the wire
        .ollama_keep_alive("30s")
        .build()
        .expect("build client");

    let response = client
        .chat(vec![Message::user("Reply with exactly the word: pong")])
        .await
        .unwrap_or_else(|e| {
            panic!("Ollama chat failed against {base_url} with model {model} and think=yes: {e}.\nIs `ollama serve` running?")
        });

    assert!(
        !response.content.trim().is_empty(),
        "Ollama with think=yes returned empty content; \
         expected a non-empty reply. Got: {:?}",
        response.content
    );
}
```

- [ ] **Step 2: Run it locally if Ollama is available**

If `ollama serve` is running and a model is pulled:

```bash
OLLAMA_MODEL=$(ollama list | awk 'NR==2 {print $1}') \
cargo test --features ollama --test ollama_http_autoswitch \
    live_ollama_think_string_parser_round_trip -- --ignored --nocapture
```

Expected: PASS within ~30s (depending on model size and first-load).

If Ollama is not running on the executor's machine, document that you skipped this verification — the test still ships as a #[ignore]'d guard for future maintainers.

- [ ] **Step 3: Verify non-ignored tests in the same file still pass**

Run: `cargo test --features ollama --test ollama_http_autoswitch`

Expected: `5 passed, 2 ignored` (the original 4 mockito + this new live one stays ignored; the original live test from PR #176 also stays ignored).

- [ ] **Step 4: Format**

Run: `cargo fmt`

Expected: zero output (or wraps that should be included in the commit).

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/tests/ollama_http_autoswitch.rs
git commit -m "$(cat <<'EOF'
test(rust): live Ollama test for ollama_think string parser (gated #[ignore])

Verifies the 0.15.1 fix end-to-end: ollama_think("yes") produces a
wire body the real Ollama server accepts. Mirrors the existing
live_ollama_auto_switch_against_real_server pattern.

Gated #[ignore]; requires OLLAMA_MODEL env var (no default — pulled
models vary by machine).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Add macOS noise to `.gitignore`

**Files:**
- Modify: `.gitignore` (currently no macOS section — repo has `.DS_Store` / `docs/.DS_Store` / `sdks/.DS_Store` as untracked junk that shows up in every `git status`)

- [ ] **Step 1: Add the macOS section**

Append to the end of `.gitignore`:

```
# macOS
.DS_Store
**/.DS_Store

# IDE caches (developer-local, don't need to be tracked)
.idea/
.vscode/
```

(`.idea` and `.vscode` are pre-emptive — they don't currently exist but commonly appear when devs open the repo in JetBrains / VS Code respectively.)

- [ ] **Step 2: Verify `git status` no longer shows the junk**

Run: `git status -s`

Expected: the `?? .DS_Store` / `?? docs/.DS_Store` / `?? sdks/.DS_Store` lines should be gone (they were ignored, not deleted from disk).

- [ ] **Step 3: Commit**

```bash
git add .gitignore
git commit -m "$(cat <<'EOF'
chore: ignore macOS .DS_Store and common IDE caches

Pre-existing repo hygiene cleanup — every `git status` showed
.DS_Store / docs/.DS_Store / sdks/.DS_Store as untracked. Bundle
.idea/ and .vscode/ pre-emptively so the next dev opening the repo
in those editors doesn't have to redo this.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Bump 0.15.0 → 0.15.1 + CHANGELOG + release-checklist docs

**Files:**
- Modify: `sdks/rust/Cargo.toml`
- Modify: `sdks/rust/CHANGELOG.md`
- Modify: `AGENTS.md`, `llms.txt`, `README.md`, `sdks/rust/README.md`, `skills/motosan-ai/SKILL.md`, `skills/motosan-ai/references/rust-api.md`

- [ ] **Step 1: Bump `Cargo.toml`**

In `sdks/rust/Cargo.toml`, change `version = "0.15.0"` to `version = "0.15.1"`.

- [ ] **Step 2: Add CHANGELOG entry**

In `sdks/rust/CHANGELOG.md`, insert immediately after the `# Changelog` / intro lines and BEFORE `## [0.15.0]`:

```markdown
## [0.15.1] - 2026-05-17

### Fixed
- **`ollama_think` now serializes per input value** instead of hard-coding `body["think"] = true` for any non-None input. Pre-existing bug from before 0.15.0: `ClientBuilder::ollama_think` takes a string but `providers/ollama.rs:138-140` was flattening `ollama_think("no")` to bool `true`, silently inverting caller intent. Now:
  - Truthy synonyms (`"true"` / `"yes"` / `"on"` / `"1"`, case-insensitive + trimmed) → JSON `true`
  - Falsy synonyms (`"false"` / `"no"` / `"off"` / `"0"`, case-insensitive + trimmed) → JSON `false`
  - Anything else (e.g. `"low"` / `"medium"` / `"high"`) → JSON string verbatim (so callers can opt into Ollama's newer string-valued reasoning levels)
- Backward compatible: existing `ollama_think("yes")` / `ollama_think("on")` callers still see bool `true` on the wire.

### Changed
- `.gitignore`: added macOS `.DS_Store` patterns + `.idea` / `.vscode` IDE caches. Pure repo hygiene.

### Notes
- Four unit tests in `providers::ollama::tests` lock in the new parser behavior.
- `tests/ollama_http_autoswitch.rs::live_ollama_think_string_parser_round_trip` is a new `#[ignore]`'d live test verifying the wire body is accepted by a real Ollama server.
```

- [ ] **Step 3: Bump version strings in release-checklist docs**

Apply these substitutions (every `0.15.0` → `0.15.1` and every `v0.15.0` → `v0.15.1`) in:

- `AGENTS.md` line 5: `Rust v0.15.0 (crates.io)` → `Rust v0.15.1 (crates.io)`
- `llms.txt` line 5: `Python 0.10.0 · Rust 0.15.0` → `Python 0.10.0 · Rust 0.15.1`
- `llms.txt` line 22: `motosan-ai = { version = "0.15.0"` → `motosan-ai = { version = "0.15.1"`
- `README.md` line 29: `| Rust | ... | v0.15.0 |` → `| Rust | ... | v0.15.1 |`
- `README.md` line 37: `motosan-ai = { version = "0.15.0"` → `motosan-ai = { version = "0.15.1"`
- `sdks/rust/README.md` lines 320, 429, 492: three `motosan-ai = { version = "0.15.0"` → `motosan-ai = { version = "0.15.1"`
- `skills/motosan-ai/SKILL.md` line 8: `Multi-provider LLM SDK — Python 0.10.0 / Rust 0.15.0` → `Multi-provider LLM SDK — Python 0.10.0 / Rust 0.15.1`
- `skills/motosan-ai/SKILL.md` line 23: `motosan-ai = { version = "0.15.0"` → `motosan-ai = { version = "0.15.1"`
- `skills/motosan-ai/references/rust-api.md` line 7: `motosan-ai = { version = "0.15.0"` → `motosan-ai = { version = "0.15.1"`

Confirm completeness with:

```bash
grep -rn "0\.15\.0\|v0\.15\.0" /Users/daiwanwei/Projects/wade/motosan-ai \
    --include="*.md" --include="*.txt" 2>/dev/null \
    | grep -v "CHANGELOG\|docs/superpowers\|target/"
```

Expected: empty output.

- [ ] **Step 4: Commit**

```bash
git add sdks/rust/Cargo.toml sdks/rust/CHANGELOG.md AGENTS.md llms.txt README.md sdks/rust/README.md skills/motosan-ai/SKILL.md skills/motosan-ai/references/rust-api.md
git commit -m "$(cat <<'EOF'
chore(rust): bump 0.15.0 -> 0.15.1 + CHANGELOG + release-checklist docs

Per CLAUDE.md release process. 0.15.1 patch (vs minor) because the
fix is purely additive — no public-API surface change, no Cargo
feature change.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Run check-all + pre-push-gate

**Files:** none (verification only)

- [ ] **Step 1: Run check-all**

Run: `check-all`

Expected: `=== All checks passed ===` (Rust + Python lint + tests all green).

- [ ] **Step 2: Run pre-push-gate**

Run: `./scripts/pre-push-gate.sh`

Expected: `=== Pre-push gate PASSED ===` (4 stages including live Anthropic tests if `ANTHROPIC_API_KEY` is resolvable via direnv — typically ~50s for stage 4).

If either fails, fix the underlying issue and re-run before proceeding to Task 6.

---

## Task 6: PR → merge → tag rust-v0.15.1 → publish

**Files:** none (release operations)

- [ ] **Step 1: Create branch + push**

```bash
git checkout -b fix/ollama-think-parser
git push -u origin fix/ollama-think-parser
```

- [ ] **Step 2: Open PR**

```bash
gh pr create --base main --title "fix(rust): ollama_think serializes per input value (v0.15.1)" --body "$(cat <<'EOF'
## Summary

Closes the pre-existing `ollama_think` boolean coercion bug noted in the header of `docs/superpowers/plans/2026-05-17-ollama-http-wiring.md`. Ships as 0.15.1 patch (purely additive — no public-API surface change).

**Before:**
\`\`\`rust
if self.think.is_some() {
    body["think"] = json!(true);   // hardcoded; ignores input value
}
\`\`\`

**After:**
- truthy synonyms (\`true\` / \`yes\` / \`on\` / \`1\`, case-insensitive + trimmed) → bool true
- falsy synonyms (\`false\` / \`no\` / \`off\` / \`0\`, case-insensitive + trimmed) → bool false
- anything else (e.g. \`low\` / \`medium\` / \`high\`) → string verbatim

Backward compatible: existing \`ollama_think("yes")\` callers still get bool true on the wire. New callers can opt into Ollama's string-valued reasoning levels.

Also bundles \`.gitignore\` macOS noise cleanup.

## Test plan

- [x] 4 new unit tests in \`providers::ollama::tests\` covering truthy / falsy / passthrough / unset
- [x] 1 new \`#[ignore]\`'d live test (\`live_ollama_think_string_parser_round_trip\`) covering wire compat against real Ollama
- [x] Existing \`tests/ollama_native_provider.rs\` integration tests still pass (use \`ollama_think("on")\`, which still maps to bool true)
- [x] \`check-all\` green
- [x] \`./scripts/pre-push-gate.sh\` green (incl. live Anthropic tests)
- [x] \`cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings\` clean

## Release readiness

After merge, tag \`rust-v0.15.1\` and push to trigger \`publish-rust.yml\` → crates.io.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Note the URL printed by gh — open it to monitor CI.

- [ ] **Step 3: Wait for CI to settle, then merge**

```bash
until [ "$(gh pr view --json mergeStateStatus -q .mergeStateStatus 2>/dev/null)" = "CLEAN" ]; do sleep 30; done
gh pr merge --merge --delete-branch
```

- [ ] **Step 4: Pull main, tag, push**

```bash
git checkout main
git pull --ff-only origin main
git tag -a rust-v0.15.1 -m "rust-v0.15.1 — ollama_think string parser

Fixes pre-existing bug where ollama_think value was silently coerced
to bool true regardless of input string. Now parses the input:
truthy synonyms → bool true, falsy → bool false, anything else
(low/medium/high) → string verbatim.

Backward compatible. Also bundles .gitignore macOS cleanup.

Closes the pre-existing limitation noted in
docs/superpowers/plans/2026-05-17-ollama-http-wiring.md header."
git push origin rust-v0.15.1
```

- [ ] **Step 5: Watch publish workflow, verify crates.io**

```bash
sleep 5
RUN_ID=$(gh run list --workflow=publish-rust.yml --branch=rust-v0.15.1 --limit 1 --json databaseId -q '.[0].databaseId')
until [ "$(gh run view $RUN_ID --json status -q .status)" = "completed" ]; do sleep 45; done
gh run view $RUN_ID --json conclusion
```

Expected: `{"conclusion": "success"}`.

Then verify crates.io published the new version:

```bash
rtk proxy curl -sA "motosan-ai-check/1.0" "https://crates.io/api/v1/crates/motosan-ai/0.15.1" \
  | python3 -c "import sys,json; v=json.load(sys.stdin)['version']; print(v['num'], v['created_at'], 'yanked=' + str(v['yanked']))"
```

Expected: `0.15.1 <ISO timestamp> yanked=False`.

- [ ] **Step 6: Report back**

Report:
1. Final merge commit SHA on main.
2. crates.io published version + timestamp.
3. publish-rust.yml run URL.
4. Whether Step 2 of Task 2 (live test) was actually run, and the result if so.
5. Any deviations from the plan, with reason.
6. The PR URL.

---

## Done criteria

- [ ] All 6 tasks above complete with their final commits landed.
- [ ] motosan-ai 0.15.1 live on crates.io, not yanked.
- [ ] `cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings` produces 0 errors and 0 warnings.
- [ ] 4 new unit tests in `providers::ollama::tests` pass.
- [ ] Live test exists in `tests/ollama_http_autoswitch.rs` (gated `#[ignore]`); manual run against a real Ollama server confirmed PASS (recorded in commit message or report-back).
- [ ] `git status -s` no longer shows `.DS_Store` files as untracked.
- [ ] No regressions in `tests/ollama_native_provider.rs` (which uses `ollama_think("on")` — still maps to bool true under the new parser).
- [ ] Existing `tests/ollama_http_autoswitch.rs::ollama_with_keep_alive_routes_to_api_chat_endpoint` and friends still pass (4 mockito tests in that file unchanged).

## Out of scope for this plan

- Removing the `ollama_native` Cargo feature alias entirely (0.16.0 or later — separate breaking-change conversation).
- Adding an `OpenAIProvider::with_extra_body_params` escape hatch for OTHER OpenAI-compat servers (no consumer has asked).
- B1 (built-in Faux provider) — separate spec needed for design.
- B2 (stream error model — errors-as-events vs `Result<BoxStream, _>`) — 1.0 release planning conversation.
- `claude_code/mod.rs:445` `let _ = child.wait().await` silent-swallow fix — needs a new `StreamEvent::Error` variant, design conversation first.
- §2.3 codex parser shape audit — only triggers if capo reports codex `--provider` still empty after 0.14.3 (reactive).
- Local-only branch cleanup (`git branch -D <stale-branch>`) — pure local hygiene, no commit needed; do interactively when convenient.
