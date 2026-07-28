# Correctness Quick-Wins Batch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix six small, independent, verified defects/gaps across the repo: a Rust UTF-8 panic + per-delta copy waste in ThinkStripper, the unpinned native model-stream termination contract, Python's missing central capability enforcement (Minimax silently drops images), the missing `py.typed` marker, the pre-push hook's unconditional full-suite + keychain-triggered live-API runs, and the unguarded publish workflows.

**Architecture:** Six tasks, **one PR per task**, no functional dependencies between them. NOT fully disjoint at the file level: Tasks 1+2 both append to the Rust CHANGELOG `[Unreleased]` section, Tasks 3+4 both append to the Python CHANGELOG `[Unreleased]` section (and both touch `client.py`/`minimax.py` in non-adjacent hunks, which git auto-merges). Parallel development is fine, but within each pair the later-merged PR should expect a trivial CHANGELOG rebase; the zero-conflict path is to merge in order 1→2 and 3→4. Each task is self-contained: red test (where testable) → fix → gates → PR. No version bumps in this plan; all changes land under `## [Unreleased]` and ride the next release wave.

**Tech Stack:** Rust (cargo, mockito, futures), Python 3.11+ (uv, pytest, ruff, hatchling), bash (git pre-push hook), GitHub Actions.

## Global Constraints

- **Baseline:** `origin/main` @ `a5329ab` (merge of PR #241). Author every branch off `origin/main`, in a fresh worktree (`superpowers:using-git-worktrees`). The primary checkout is on a stale merged branch — do not base work on it.
- **Every task ships as its own PR** targeting `main`. NEVER push code changes directly to main (house rule: only docs/spec/notes may go direct; in this plan even the spec change rides its task's PR for atomicity). Do NOT merge PRs — report the PR URL and stop; review/merge happens outside the executor.
- **Rust gates** (run from `sdks/rust/`): `cargo fmt --all -- --check`, `cargo clippy --all-features --all-targets -- -D warnings` (`--all-targets` is mandatory — CI lints test code), and the full test suite **credential-stripped**. The Rust suite contains env-gated live tests that are NOT `#[ignore]`d (`tests/anthropic_live.rs`, `openai_live.rs`, … fire whenever their key env var is set), so every UNFILTERED `cargo test` in this plan must run as:
  ```bash
  env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID -u MINIMAX_API_KEY -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST cargo test --all-features
  ```
  (Name-filtered or `--test <target>`-scoped invocations are inherently safe — the filter excludes the live tests — and may run bare.)
- **Python gates** (run from `sdks/python/`): `uv run ruff check motosan_ai/`, `uv run ruff format --check motosan_ai/ tests/`, `uv run pytest tests/ -q --ignore=tests/integration/`.
- **Every commit passes format/lint/test for what it touches** — no exceptions for "non-code" files. Repo-wide formatting check is `treefmt --fail-on-change` from the repo root (the only reliable zero-reformat proof — a before/after `git status` cannot detect the formatter touching an already-dirty file; the pre-commit hook enforces this regardless). Shell changes additionally gate on `bash -n` + `shellcheck`; GitHub-workflow changes on `actionlint` (both available via `nix develop` — never skipped).
- **In every fresh worktree, before any push:** run `uv sync --all-extras` inside `sdks/python/` — the pre-push hook runs the full Python suite for every push and fails to collect (`No module named respx`) in an unsynced worktree, even for non-Python branches.
- **The pre-push hook runs minutes of tests and may auto-run LIVE Anthropic tests** (until Task 5 merges). Run pushes with a generous timeout / in background. After every push, verify the ref actually landed by **comparing SHAs** — each task's final step does this: `test "$(git ls-remote origin refs/heads/<branch> | cut -f1)" = "$(git rev-parse HEAD)"`. A bare `git ls-remote` exit code proves nothing (it exits 0 even when the ref does not exist), and an rtk-wrapped push can exit 0 while the hook blocked it.
- Tool call field is `input`, not `args`/`params`. Provider logic goes in `providers/` only.
- **Tracking issue: ALREADY CREATED — [#242](https://github.com/motosan-dev/motosan-ai/issues/242)** ("Correctness quick-wins batch: stream, typing, and tooling hardening", opened 2026-07-28). Do NOT create another. Shell variables do NOT survive across steps or agent shells, so **every commit block below re-resolves and validates the number itself** (the two `ISSUE=` lines repeated in each task's final step — expected value: 242); if resolution ever fails, stop — do not commit without the reference.
- **Commit messages:** the house format, as in the newest main history (`feat: add native freeform tool transport (#239)`, `fix: locate Rust package artifact in publish workflow (#239)`): a bare type from the allowed set **`fix:` / `feat:` / `refactor:`** — NO scope parentheses, no other types; the subject ends with ` (#${ISSUE})`. Breaking changes carry a `BREAKING CHANGE: …` body paragraph (its own `-m`), not a `!` marker. The `Co-Authored-By:` trailer is always the final `-m`. PR titles mirror their commit subject format. PR bodies reference `#${ISSUE}` (plain reference, not `Closes` — the umbrella issue is closed manually after all six PRs land).
- **PR bodies** (where a task says `--body "..."`): 2-4 sentences summarizing that task's **Context** paragraph, the test evidence (which new tests, suite results), plus any task-specific notes the task's PR step calls out. End with the standard `🤖 Generated with [Claude Code](https://claude.com/claude-code)` footer.

---

### Task 1: ThinkStripper — UTF-8 boundary panic fix + zero-copy fast path (PR-A)

Branch: `fix/think-stripper-utf8`

**Files:**
- Modify: `sdks/rust/src/think_stripper.rs` (feed(): lines 28-68; tests module at 71-142)
- Modify: `sdks/rust/CHANGELOG.md` (add under `## [Unreleased]`)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: no API change — `ThinkStripper::new()`, `feed(&mut self, chunk: &str) -> String`, `flush(&mut self) -> String` signatures unchanged. `client.rs` call sites (lines 217/222 via `wrap_with_think_stripper`) need no edits.

**Context:** `feed()` is on the hot path of every legacy chat stream (every provider). Two problems, both inside `feed()`:
1. **Panic (the real bug):** the `in_think` no-close-tag branch (line 37) slices `self.buf[self.buf.len() - keep..]` with `keep = 7` and **no char-boundary guard** — multi-byte UTF-8 thinking content panics. The sibling not-in_think branch (lines 50-54) has the guard; this branch is missing it.
2. **Copy waste:** the dominant no-tag path copies the chunk into `self.buf` (line 29), copies nearly the whole buffer *again* into `output` (line 55), then reallocates the tail (line 56), discarding capacity. Same realloc pattern at lines 37, 42, 61.

- [ ] **Step 1: Write the failing panic-regression test**

Append to the `tests` module in `sdks/rust/src/think_stripper.rs`:

```rust
    #[test]
    fn multibyte_thinking_content_does_not_panic() {
        let mut s = ThinkStripper::new();
        // 8 three-byte chars = 24 bytes; len - 7 = 17 is NOT a char boundary.
        assert_eq!(s.feed("<think>中文思考中文思考"), "");
        let mut out = s.feed("</think>答案");
        out.push_str(&s.flush());
        assert_eq!(out, "答案");
    }

    #[test]
    fn multibyte_passthrough_across_chunks() {
        let mut s = ThinkStripper::new();
        let mut out = String::new();
        out.push_str(&s.feed("回答是："));
        out.push_str(&s.feed("四十二。"));
        out.push_str(&s.flush());
        assert_eq!(out, "回答是：四十二。");
    }
```

- [ ] **Step 2: Run tests to verify the panic reproduces**

Run (from `sdks/rust/`): `cargo test think_stripper::tests -- --nocapture`
(The `::tests` suffix matters — a bare `think_stripper` filter also matches the 7-test `client::think_stripper_stream_tests` module.)
Expected: `multibyte_thinking_content_does_not_panic` FAILS with `panicked ... byte index 17 is not a char boundary`. `multibyte_passthrough_across_chunks` passes (that branch already has the guard).

- [ ] **Step 3: Rewrite `feed()` — boundary-guard the in_think branch, replace tail reallocs with `drain`/`split_off`**

Replace the whole `feed` method (current lines 28-68) with:

```rust
    pub fn feed(&mut self, chunk: &str) -> String {
        self.buf.push_str(chunk);
        let mut output = String::new();
        loop {
            if self.in_think {
                match self.buf.find("</think>") {
                    None => {
                        let keep = "</think>".len() - 1;
                        let mut cut = self.buf.len().saturating_sub(keep);
                        // Ensure we split on a char boundary (important for multi-byte UTF-8)
                        while cut > 0 && !self.buf.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        self.buf.drain(..cut);
                        break;
                    }
                    Some(end) => {
                        self.buf.drain(..end + "</think>".len());
                        self.in_think = false;
                    }
                }
            } else {
                match self.buf.find("<think>") {
                    None => {
                        let keep = "<think>".len() - 1;
                        let mut safe = self.buf.len().saturating_sub(keep);
                        // Ensure we split on a char boundary (important for multi-byte UTF-8)
                        while safe > 0 && !self.buf.is_char_boundary(safe) {
                            safe -= 1;
                        }
                        if output.is_empty() {
                            // Fast path: hand the buffer's allocation to the caller;
                            // self.buf becomes the freshly-split tiny tail. This removes
                            // the second full-size copy AND the full-size tail realloc
                            // (the tail alloc is ≤ a few bytes).
                            let tail = self.buf.split_off(safe);
                            return std::mem::replace(&mut self.buf, tail);
                        }
                        output.push_str(&self.buf[..safe]);
                        self.buf.drain(..safe);
                        break;
                    }
                    Some(start) => {
                        output.push_str(&self.buf[..start]);
                        self.buf.drain(..start + "<think>".len());
                        self.in_think = true;
                    }
                }
            }
        }
        output
    }
```

Notes for the implementer: `drain(..n)` shifts in place and keeps capacity; `find()` returns byte indices of ASCII needles, so `end + 8` / `start + 7` always land on char boundaries — only the *arithmetic* cuts (`len - keep`) need the boundary walk. The fast path is gated on `output.is_empty()` because output can be non-empty when a tag was stripped earlier in the same chunk.

- [ ] **Step 4: Run the module tests, then the full suite**

Run: `cargo test think_stripper::tests` → Expected: all 10 unit tests PASS (8 existing + 2 new).
Run: `env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID -u MINIMAX_API_KEY -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST cargo test --all-features` → Expected: PASS (this also runs the `client::think_stripper_stream_tests` module — client.rs:1352-1493 — which exercises the stripper through the public stream API; the `env -u` prefix keeps the suite's env-gated live tests from firing — see Global Constraints).

- [ ] **Step 5: Update CHANGELOG**

In `sdks/rust/CHANGELOG.md`, under `## [Unreleased]` add:

```markdown
### Fixed
- `ThinkStripper` no longer panics on multi-byte UTF-8 content inside an
  unterminated `<think>` block (the buffered-tail cut in the in-think branch
  lacked the char-boundary guard the other branch had). The no-tag fast path
  also stops copying every text delta a second time (the buffer's allocation
  is handed to the caller; only the tiny partial-tag tail is newly allocated),
  and the tag-found branches now trim in place instead of reallocating.
```

(If `### Fixed` doesn't exist under Unreleased yet, create it.)

- [ ] **Step 6: Gates, commit, push, PR**

Run from `sdks/rust/`: `cargo fmt --all`, then `cargo fmt --all -- --check`, `cargo clippy --all-features --all-targets -- -D warnings`, and the credential-stripped full suite `env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID -u MINIMAX_API_KEY -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST cargo test --all-features`. All must pass.

```bash
ISSUE=$(gh issue list --state open --search "Correctness quick-wins batch in:title" --json number --jq '.[0].number')
[[ "$ISSUE" =~ ^[0-9]+$ ]] || { echo "tracking issue not found — create it per Global Constraints" >&2; exit 1; }
git add sdks/rust/src/think_stripper.rs sdks/rust/CHANGELOG.md
git commit -m "fix: ThinkStripper UTF-8 boundary panic and zero-copy fast path (#${ISSUE})" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push -u origin fix/think-stripper-utf8   # long timeout — the pre-push hook runs suites
test "$(git ls-remote origin refs/heads/fix/think-stripper-utf8 | cut -f1)" = "$(git rev-parse HEAD)" \
    || { echo "push did not land on origin — the pre-push hook may have blocked it" >&2; exit 1; }
gh pr create --title "fix: ThinkStripper UTF-8 boundary panic and zero-copy fast path" --body "..."
```

PR body: state the panic repro (multi-byte content in unterminated think block), that the perf change is behavior-neutral (10/10 unit tests + stream-level tests green), and that no public API changed.

---

### Task 2: Native model stream termination — spec pin, message convention, EOF conformance tests (PR-B)

Branch: `fix/native-stream-termination`

**Files:**
- Modify: `specs/types.md` (add `### Stream termination (native)` after the `### Provider support` subsection ending at line 209; add one row to the terminal-event table at lines 287-294)
- Modify: `sdks/rust/src/providers/responses.rs` (`model_stream_adapter` at line 403, struct at 424-440, EOF branch at 632-641)
- Modify: `sdks/rust/src/providers/openai.rs:749` (call site)
- Modify: `sdks/rust/src/providers/chatgpt_codex.rs:422` (call site)
- Test: `sdks/rust/tests/openai_provider.rs` (append after the test ending at line 274), `sdks/rust/tests/chatgpt_codex.rs` (append after the test ending at line 97)
- Modify: `sdks/rust/CHANGELOG.md` (under `## [Unreleased]`)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `pub fn model_stream_adapter<S>(sse: S, provider: &'static str) -> crate::stream::BoxModelStream` — signature gains the `provider` param. Note this fn IS publicly reachable by downstream crates (`motosan_ai::providers::responses::model_stream_adapter` — `pub mod providers` → `pub mod responses`), so the parameter addition is an accepted 0.x public-API change; both in-repo call sites are updated in this task and the CHANGELOG entry names the signature change.

**Context:** The M3 truncation-vs-completion contract is unpinned on the 0.26.0 native path: the spec's terminal-event table has no Responses-mode row, `ModelStreamDelta` termination semantics are unstated, the adapter's `IncompleteStream` payload (`"responses stream ended without a terminal event"`, responses.rs:637) deviates from the convention every legacy adapter follows (`"<provider> ended without a terminal event"` — see `error.rs:45` doc and `specs/types.md` "Message convention"), and no test exercises native EOF-without-terminal. The old message string appears exactly once in the repo (responses.rs:637) — nothing else asserts it.

- [ ] **Step 1: Write the two failing EOF conformance tests**

Append to `sdks/rust/tests/openai_provider.rs` (the file already imports `MotosanError`, `collect_model_stream`, and defines `native_custom_request()`):

```rust
#[tokio::test]
async fn native_openai_stream_eof_without_terminal_is_incomplete() {
    let mut server = mockito::Server::new_async().await;
    // Text deltas but NO response.completed / response.incomplete → truncated.
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n"
    );
    let mock = server
        .mock("POST", "/v1/responses")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));

    let stream = provider
        .model_stream(native_custom_request())
        .await
        .expect("native stream");
    let err = collect_model_stream(stream)
        .await
        .expect_err("EOF without terminal must yield IncompleteStream");
    match err {
        MotosanError::IncompleteStream(msg) => {
            assert_eq!(msg, "openai ended without a terminal event")
        }
        other => panic!("expected IncompleteStream, got {other:?}"),
    }
    mock.assert_async().await;
}
```

Append to `sdks/rust/tests/chatgpt_codex.rs` (file already has `Matcher`, `MotosanError`, `collect_model_stream`, `native_custom_request()`):

```rust
#[tokio::test]
async fn native_codex_stream_eof_without_terminal_is_incomplete() {
    let mut server = mockito::Server::new_async().await;
    let sse = "data: {\"type\":\"response.custom_tool_call_input.delta\",\"call_id\":\"call_js\",\"delta\":\"console.\"}\n\n";
    let mock = server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let stream = provider
        .model_stream(native_custom_request())
        .await
        .expect("native stream");
    let err = collect_model_stream(stream)
        .await
        .expect_err("EOF without terminal must yield IncompleteStream");
    match err {
        MotosanError::IncompleteStream(msg) => {
            assert_eq!(msg, "chatgpt-codex ended without a terminal event")
        }
        other => panic!("expected IncompleteStream, got {other:?}"),
    }
    mock.assert_async().await;
}
```

If either file's existing imports don't already cover a name used above, add it to that file's `use` block — do not restructure imports.

- [ ] **Step 2: Run the new tests to verify they fail on the message assertion**

Run: `cargo test --all-features --test openai_provider native_openai_stream_eof -- --nocapture`
Run: `cargo test --all-features --test chatgpt_codex native_codex_stream_eof -- --nocapture`
Expected: both FAIL on `assert_eq!` — left side is `"responses stream ended without a terminal event"` (the adapter DOES yield IncompleteStream today; only the payload deviates). If they fail differently (e.g. no error at all), STOP — the premise changed; re-read `responses.rs:632-641`.

- [ ] **Step 3: Thread the provider name through the adapter**

In `sdks/rust/src/providers/responses.rs`:

1. Signature (line 403): `pub fn model_stream_adapter<S>(sse: S, provider: &'static str) -> crate::stream::BoxModelStream` and in the constructor literal (lines 413-420) add `provider,` as a field initializer.
2. Struct (lines 424-440): add field `provider: &'static str,` to `ResponsesModelStreamAdapter`.
3. EOF branch (lines 632-641): replace the fixed string with:

```rust
                Poll::Ready(None) => {
                    if !self.saw_terminal {
                        self.saw_terminal = true;
                        return Poll::Ready(Some(Err(
                            crate::error::MotosanError::IncompleteStream(format!(
                                "{} ended without a terminal event",
                                self.provider
                            )),
                        )));
                    }
                    return Poll::Ready(None);
                }
```

Call sites:
- `sdks/rust/src/providers/openai.rs:749` → `Ok(crate::providers::responses::model_stream_adapter(response.bytes_stream().eventsource(), "openai"))`
- `sdks/rust/src/providers/chatgpt_codex.rs:422` → `Ok(crate::providers::responses::model_stream_adapter(response.bytes_stream().eventsource(), "chatgpt-codex"))`

- [ ] **Step 4: Run the new tests, then the full suite**

Run the two `cargo test` commands from Step 2 → Expected: PASS.
Run: `env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID -u MINIMAX_API_KEY -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST cargo test --all-features` → Expected: PASS (grep confirmed nothing else asserts the old message; `env -u` prefix per Global Constraints).

- [ ] **Step 5: Amend specs/types.md**

(a) In the terminal-event table (lines 287-294), the existing rows describe the **legacy `stream()` adapters** — make that explicit so the new native row cannot be read as contradicting them. Change the two row labels (labels only, keep each row's terminal-event cell byte-identical):

- `| OpenAI |` → `| OpenAI (legacy \`stream()\` adapter) |`
- `| ChatGPT Codex |` → `| ChatGPT Codex (legacy \`stream()\` adapter) |`

Then add this row after the ChatGPT Codex row:

```markdown
| OpenAI Responses mode / ChatGPT Codex — native model API (`model_stream`) | `response.completed` or `response.incomplete` SSE event (either is a received terminal) |
```

(Do NOT claim anything new about the legacy Codex adapter's handling of `response.incomplete` — this task only labels the existing rows and documents the shared native adapter.)

(b) After the `### Provider support` subsection (insert before the `## Usage` heading at line 211):

```markdown
### Stream termination (native)

`ModelStreamDelta` streams follow the [Stream termination
contract](#stream-termination-contract): exactly one `Done { stop_reason }`
delta per successfully completed stream, emitted when the wire delivers a
`response.completed` or `response.incomplete` terminal event. When the byte
stream ends (EOF) without either terminal, the adapter yields
`MotosanError::IncompleteStream` with the standard payload
`<provider> ended without a terminal event` (provider names: `openai`,
`chatgpt-codex`). `collect_model_stream` propagates that error; its
`stop_reason` heuristic applies only to streams that did deliver a terminal.
```

- [ ] **Step 6: Update CHANGELOG**

In `sdks/rust/CHANGELOG.md` under `## [Unreleased]`:

```markdown
### Changed
- Native model streams (`model_stream_with`) now report EOF-truncation as
  `incomplete stream: <provider> ended without a terminal event`
  (`openai` / `chatgpt-codex`), matching the spec's message convention. The
  previous payload was `responses stream ended without a terminal event`.
  Match on the `IncompleteStream` variant, not the message text.
  `providers::responses::model_stream_adapter` accordingly gained a
  `provider: &'static str` parameter.
```

- [ ] **Step 7: Gates, commit, push, PR**

Run from `sdks/rust/`: `cargo fmt --all`, `cargo fmt --all -- --check`, `cargo clippy --all-features --all-targets -- -D warnings`, and the credential-stripped full suite `env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID -u MINIMAX_API_KEY -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST cargo test --all-features`.

```bash
ISSUE=$(gh issue list --state open --search "Correctness quick-wins batch in:title" --json number --jq '.[0].number')
[[ "$ISSUE" =~ ^[0-9]+$ ]] || { echo "tracking issue not found — create it per Global Constraints" >&2; exit 1; }
git add specs/types.md sdks/rust/src/providers/responses.rs sdks/rust/src/providers/openai.rs sdks/rust/src/providers/chatgpt_codex.rs sdks/rust/tests/openai_provider.rs sdks/rust/tests/chatgpt_codex.rs sdks/rust/CHANGELOG.md
git commit -m "fix: pin native model-stream termination contract and provider-named IncompleteStream (#${ISSUE})" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push -u origin fix/native-stream-termination
test "$(git ls-remote origin refs/heads/fix/native-stream-termination | cut -f1)" = "$(git rev-parse HEAD)" \
    || { echo "push did not land on origin — the pre-push hook may have blocked it" >&2; exit 1; }
gh pr create --title "fix: pin native model-stream termination contract (spec + message + EOF tests)" --body "..."
```

Done criteria note: no HTTP semantics change — the existing `#[ignore]`d live smokes (`chatgpt_codex_live.rs`, `live_chatgpt_codex_token_refresh.rs`) are unaffected and not required to run for this PR.

---

### Task 3: Python central capability enforcement + Minimax capability correction (PR-C)

Branch: `fix/python-capability-enforcement`

**Files:**
- Modify: `sdks/python/motosan_ai/provider_base.py` (extract free function; whole file is 45 lines)
- Modify: `sdks/python/motosan_ai/client.py` (import at top; two call sites in `chat_with` ~line 459 and `stream_with` ~line 506)
- Modify: `sdks/python/motosan_ai/providers/minimax.py:43` (capability downgrade)
- Modify: `sdks/python/tests/test_provider_capabilities.py:99-101` (flip minimax expectation)
- Create: `sdks/python/tests/test_capability_enforcement.py`
- Modify: `sdks/python/CHANGELOG.md` (under `## [Unreleased]`)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: module-level `validate_request(request: ChatRequest, capabilities: ProviderCapabilities) -> None` in `motosan_ai/provider_base.py` (raises `InvalidRequestError`). `BaseProvider.validate_request` behavior unchanged. **The `LlmClient` Protocol must NOT change** — providers without a `capabilities` attribute keep working (guarded with `getattr`).

**Context:** Rust validates centrally (`client.rs` `validate_for_dispatch`) and TS validates centrally (`provider.ts` `dispatchChat`/`dispatchStream`). Python only validates inside the four providers that subclass/self-call (`anthropic`, `gemini`, `gemini_code_assist`, `chatgpt_codex`). `OpenAIProvider`, `MinimaxProvider`, `OllamaProvider` and the three CLI clients declare `capabilities` but never enforce → Minimax declares `with_image()` yet `_serialize_messages` (minimax.py:74-115) only ever sends `message.content`, so images are **silently discarded**. Minimax's native wire has no image serialization at all, so its declared capability is corrected to `text_only()` (matches the TS SDK's deliberate `minimaxCaps()` decision; Rust routes Minimax through the Anthropic adapter, a different wire, so no Rust change).

- [ ] **Step 1: Write the failing enforcement tests**

Create `sdks/python/tests/test_capability_enforcement.py`:

```python
from __future__ import annotations

import pytest

from motosan_ai import Client, Message
from motosan_ai.error import InvalidRequestError
from motosan_ai.types import ChatResponse, StopReason, StreamEvent, Usage


IMG = Message.user_with_image("caption", "abc", "image/png")
PDF = Message.user_with_pdf_base64("caption", "abc")


def _block_dispatch(monkeypatch, provider):
    """Replace the provider's network/CLI entry points with recording fakes.

    Enforcement must fire BEFORE dispatch, so these must never run. If
    enforcement is missing, the fakes answer instantly and the test fails
    deterministically — no network, no CLI process, no hangs.
    """
    calls: list[str] = []

    async def fake_chat(request):
        calls.append("chat")
        return ChatResponse(content="dispatched", usage=Usage(1, 1), stop_reason=StopReason.stop)

    async def fake_stream(request):
        calls.append("stream")
        yield StreamEvent(content="dispatched", done=True)

    monkeypatch.setattr(provider, "chat", fake_chat)
    monkeypatch.setattr(provider, "stream", fake_stream)
    return calls


@pytest.mark.asyncio
async def test_minimax_image_rejected_on_chat(monkeypatch):
    client = Client(provider="minimax", api_key="k")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="image"):
        await client.chat([IMG])
    assert calls == []


@pytest.mark.asyncio
async def test_minimax_image_rejected_on_stream(monkeypatch):
    client = Client(provider="minimax", api_key="k")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="image"):
        async for _ in client.stream([IMG]):
            pass
    assert calls == []


@pytest.mark.asyncio
async def test_openai_document_rejected_on_chat(monkeypatch):
    client = Client(provider="openai", api_key="k")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="document"):
        await client.chat([PDF])
    assert calls == []


@pytest.mark.asyncio
async def test_ollama_native_image_rejected_on_chat(monkeypatch):
    client = Client(provider="ollama", ollama_native=True, model="llama3.2")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="image"):
        await client.chat([IMG])
    assert calls == []


@pytest.mark.asyncio
async def test_claude_code_image_rejected_on_chat(monkeypatch):
    client = Client(provider="claude_code")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="image"):
        await client.chat([IMG])
    assert calls == []


class _NoCapsProvider:
    """LlmClient-Protocol-shaped provider WITHOUT a capabilities attribute."""

    async def chat(self, request):
        return ChatResponse(content="ok", usage=Usage(1, 1), stop_reason=StopReason.stop)

    async def stream(self, request):
        if False:
            yield None


@pytest.mark.asyncio
async def test_provider_without_capabilities_is_not_validated():
    # The LlmClient Protocol does not require `capabilities`; central
    # validation must be skipped, not crash, for such providers.
    client = Client(provider="anthropic", api_key="k")
    client._provider = _NoCapsProvider()
    response = await client.chat([IMG])
    assert response.content == "ok"
```

- [ ] **Step 2: Run to verify they fail**

Run (from `sdks/python/`, after `uv sync --all-extras`):
`uv run pytest tests/test_capability_enforcement.py -q`
Expected: the five `rejected` tests FAIL instantly and deterministically with `DID NOT RAISE <class 'motosan_ai.error.InvalidRequestError'>` — the monkeypatched fakes answer instead of raising, and no network/CLI is ever touched. `test_provider_without_capabilities_is_not_validated` PASSES already (nothing validates today).

- [ ] **Step 3: Extract the free validation function**

In `sdks/python/motosan_ai/provider_base.py`, add the free function after the `ProviderCapabilities` dataclass and re-implement the method as a delegation (final file shape — the class body's abstract methods stay exactly as they are):

```python
def validate_request(request: ChatRequest, capabilities: ProviderCapabilities) -> None:
    """Raise InvalidRequestError for content blocks the capabilities do not support.

    Central choke point mirroring Rust's ``validate_for_dispatch`` and the TS
    ``validateRequest``: runs before any network/CLI dispatch.
    """
    for message in request.messages:
        for block in message.content_blocks:
            if isinstance(block, ImageBlock) and not capabilities.supports_image:
                raise InvalidRequestError("provider does not support image input")
            if isinstance(block, DocumentBlock) and not capabilities.supports_document:
                raise InvalidRequestError("provider does not support document input")


class BaseProvider(ABC):
    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def validate_request(self, request: ChatRequest) -> None:
        validate_request(request, self.capabilities)
```

(The method body's bare `validate_request(...)` resolves to the module-level function at call time — no rename needed; the loop body is otherwise moved verbatim.)

- [ ] **Step 4: Call it centrally in client.py**

In `sdks/python/motosan_ai/client.py`:

1. Add to the imports block — **directly after the `from motosan_ai.error import ConfigError, ...` line (line 12) and before `from motosan_ai.providers import (`** (ruff.toml enables isort rule "I"; the first-party block must stay alphabetized: error → provider_base → providers → retry → think_stripper → types):

```python
from motosan_ai.provider_base import validate_request as _validate_request
```

2. In `chat_with` (currently line ~459), directly after the `request = replace(request, model=self.model)` block and before the `_total_timeout` branch, insert:

```python
        caps = getattr(self._provider, "capabilities", None)
        if caps is not None:
            _validate_request(request, caps)
```

3. In `stream_with` (currently line ~506), directly after its `request = replace(request, model=self.model)` block and before `policy = self._retry_policy`, insert the same three lines. (`stream_with` is an async generator, so the error surfaces on first iteration — still strictly before any network/CLI I/O.)

The `getattr` guard is load-bearing: `LlmClient`-Protocol consumers (motosan-chat) and test fakes may not define `capabilities`.

- [ ] **Step 5: Downgrade Minimax's declared capability**

In `sdks/python/motosan_ai/providers/minimax.py:43`:

```python
    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()
```

In `sdks/python/tests/test_provider_capabilities.py`, replace `test_minimax_is_with_image` (lines 99-101) with:

```python
def test_minimax_is_text_only():
    p = MinimaxProvider(api_key="test")
    assert p.capabilities == ProviderCapabilities.text_only()
```

Then sweep for other references: `rtk proxy grep -rn "minimax" sdks/python/tests/ | grep -i "image\|capabilit"` — update any other test pinning Minimax image support (none expected; `test_minimax.py` has no image tests).

- [ ] **Step 6: Run the new tests, then the full suite**

Run: `uv run pytest tests/test_capability_enforcement.py tests/test_provider_capabilities.py -q` → Expected: ALL PASS.
Run: `uv run pytest tests/ -q --ignore=tests/integration/` → Expected: PASS. If an existing test breaks because it fed unsupported blocks to a now-enforcing provider, that test was depending on silent data loss — flip it to expect `InvalidRequestError` and list it in the PR body (expected flips: none found in the audit).

- [ ] **Step 7: Update CHANGELOG**

In `sdks/python/CHANGELOG.md` under `## [Unreleased]`:

```markdown
### Breaking
- Capability enforcement is now central (mirrors Rust/TS): `Client.chat*` /
  `Client.stream*` raise `InvalidRequestError` before any network or CLI I/O
  when a message carries content blocks the provider does not support.
  Previously `openai` (documents), `minimax` (images), native `ollama`
  (images) and the CLI backends (images/documents) silently dropped them.
- `MinimaxProvider.capabilities` corrected `with_image()` → `text_only()`:
  its wire serializer never transmitted images, so requests that previously
  "succeeded" with the image silently discarded now raise. (Matches the
  TypeScript SDK's declared Minimax capabilities.)
```

- [ ] **Step 8: Gates, commit, push, PR**

Run from `sdks/python/`: `uv run ruff check motosan_ai/` (if it reports I001 import-sorting, run `uv run ruff check --fix motosan_ai/` and re-check), `uv run ruff format motosan_ai/ tests/` then `uv run ruff format --check motosan_ai/ tests/`, `uv run pytest tests/ -q --ignore=tests/integration/`.

```bash
ISSUE=$(gh issue list --state open --search "Correctness quick-wins batch in:title" --json number --jq '.[0].number')
[[ "$ISSUE" =~ ^[0-9]+$ ]] || { echo "tracking issue not found — create it per Global Constraints" >&2; exit 1; }
git add sdks/python/motosan_ai/provider_base.py sdks/python/motosan_ai/client.py sdks/python/motosan_ai/providers/minimax.py sdks/python/tests/test_capability_enforcement.py sdks/python/tests/test_provider_capabilities.py sdks/python/CHANGELOG.md
git commit -m "fix: enforce Python provider capabilities centrally; Minimax is text-only (#${ISSUE})" \
    -m "BREAKING CHANGE: unsupported content blocks now raise InvalidRequestError before any network/CLI dispatch for all Python providers (previously openai/minimax/ollama and the CLI backends silently dropped them); MinimaxProvider.capabilities is corrected to text_only()." \
    -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push -u origin fix/python-capability-enforcement
test "$(git ls-remote origin refs/heads/fix/python-capability-enforcement | cut -f1)" = "$(git rev-parse HEAD)" \
    || { echo "push did not land on origin — the pre-push hook may have blocked it" >&2; exit 1; }
gh pr create --title "fix: central Python capability enforcement — Minimax was silently dropping images" --body "..."
```

---

### Task 4: Python typed package — make mypy clean, gate it, ship `py.typed` (PR-D)

Branch: `chore/python-py-typed` (branch names are outside the commit-type rule; original plan designation kept)

**Files:**
- Create: `sdks/python/motosan_ai/py.typed` (empty file)
- Modify: `sdks/python/motosan_ai/client.py` (providers import list; `_provider` annotation after line 97; ollama branch ~145; api_key else-branch ~166)
- Modify: `sdks/python/motosan_ai/providers/openai.py` (~226-230), `sdks/python/motosan_ai/providers/minimax.py` (~286-289), `sdks/python/motosan_ai/providers/anthropic.py` (169, 192, 407), `sdks/python/motosan_ai/oauth/_flow.py` (185-186, 205-210)
- Modify: `sdks/python/pyproject.toml` (classifiers, lines 10-17; dev extra, line 32; extras comment, lines 24-31)
- Modify: `uv.lock` (**repo root** — tracked; the root pyproject.toml declares a uv workspace with `members = ["sdks/python"]`, so adding mypy to the dev extra re-locks THIS file, not one under sdks/python)
- Modify: `.github/workflows/ci-python.yml` (add Type check step)
- Modify: `sdks/python/CHANGELOG.md` (under `## [Unreleased]`)

**Interfaces:**
- Consumes / Produces: nothing cross-task. Downstream type checkers (mypy/pyright — motosan-chat's `LlmClient` usage) start seeing the package as PEP 561 typed.

**Context:** The package is annotated but NOT mypy-clean: `mypy motosan_ai/` at the baseline reports **20 errors in 5 files** (verified 2026-07-28). Shipping `py.typed` bare would surface those errors into every downstream mypy run, so this task first makes mypy clean, adds a CI gate to keep it clean, and only then ships the marker. The exact edits below were applied to a baseline checkout and verified: `mypy motosan_ai/` → `Success: no issues found in 26 source files`, full unit suite still green, ruff check/format clean — every fix is a rename, an annotation, or control-flow-equivalent (no behavior change). Known non-goal: `pyright --verifytypes` completeness is ~94.7%; raising it to 100% is NOT gated here (follow-up candidate). Extras decision unchanged: the vestigial per-provider extras are KEPT (removal would break `pip install motosan-ai[anthropic]` and `ci-python-nightly.yml` uses `--extra full`) but get a clarifying comment.

- [ ] **Step 1: Add mypy as a dev dependency and confirm the red state**

In `sdks/python/pyproject.toml` line 32, extend the dev extra:

```toml
dev = ["pytest>=8.0", "pytest-asyncio>=0.23", "respx>=0.21", "ruff>=0.9", "mypy>=1.14"]
```

Then, from the **repo root**, run `treefmt` (unified formatter; `treefmt.toml` routes `*.toml` through taplo). The dev array now exceeds taplo's line width and **will be expanded to multi-line — keep the formatter's output** (content-identical, formatting only). The pre-commit hook (`scripts/pre-commit-fmt.sh`) aborts any commit that treefmt would still rewrite, so this must be clean before committing. If `treefmt` is not on PATH, enter the dev shell first (`nix develop`).

Run from `sdks/python/`: `uv sync --all-extras && uv run mypy motosan_ai/`
(The sync updates the tracked **repo-root** `uv.lock` — uv workspace lockfile, excluded from treefmt. That diff belongs to this task; commit it in Step 10.)
Expected: **20 errors in 5 files** — 11 in client.py (10× "Incompatible types in assignment" on `self._provider` because mypy infers the first branch's type `GeminiCodeAssistProvider`, plus 1× `str | None` on `self.api_key`), 3 in openai.py (`message` reused as str-then-dict), 2 in minimax.py (`text` reused as bytes-then-str), 2 in anthropic.py (`blocks` no-redef; `stop_reason` `Any | None` key), 2 in oauth/_flow.py (`create_task` Awaitable arg; `exc` reuse outside except).

- [ ] **Step 2: Fix client.py (11 errors)**

(a) Add `OllamaProvider` to the existing barrel import (it IS exported by `motosan_ai/providers/__init__.py:9`), keeping the list alphabetized:

```python
from motosan_ai.providers import (
    AnthropicProvider,
    ChatGptCodexProvider,
    ClaudeCodeClient,
    CodexCliClient,
    GeminiCliClient,
    GeminiCodeAssistProvider,
    GeminiProvider,
    MinimaxProvider,
    OllamaProvider,
    OpenAIProvider,
)
```

(b) Directly after `self._total_timeout = total_timeout` (line 97), add a bare annotation so every branch type-checks against the real union (runtime no-op — annotations in function bodies are never evaluated):

```python
        self._provider: (
            AnthropicProvider
            | ChatGptCodexProvider
            | ClaudeCodeClient
            | CodexCliClient
            | GeminiCliClient
            | GeminiCodeAssistProvider
            | GeminiProvider
            | MinimaxProvider
            | OllamaProvider
            | OpenAIProvider
        )
```

(c) In the ollama branch (~line 145), drop the now-redundant lazy import and use the barrel import:

```python
            if ollama_native:
                self._provider = OllamaProvider(
```

(replacing the two lines `from motosan_ai.providers.ollama import OllamaProvider as NativeOllamaProvider` and `self._provider = NativeOllamaProvider(`; constructor arguments unchanged).

(d) In the final else-branch (~line 166), route through a local so `self.api_key` stays `str`:

```python
            key = api_key or self._load_api_key(provider_value)
            if not key:
                raise ConfigError(f"Missing API key for provider: {provider_value.value}")
            self.api_key = key
```

- [ ] **Step 3: Fix the provider and oauth modules (9 errors)**

`providers/openai.py` (~226-230) — rename the dict local so it stops colliding with the earlier `message: str`:

```python
        choice = (payload.get("choices") or [{}])[0]
        msg_obj = choice.get("message") or {}
        content = msg_obj.get("content") or ""

        tool_calls: list[ToolCall] = []
        for tc in msg_obj.get("tool_calls") or []:
```

`providers/minimax.py` (~286-289) — rename the SSE-delta local (an earlier `text` in the function is `bytes`):

```python
                    delta_text = delta.get("content") or ""
                    if delta_text:
                        yielded = True
                        yield StreamEvent(content=delta_text, done=False)
```

`providers/anthropic.py` — annotate the FIRST `blocks` (line 169) and drop the annotation from the second (line 192):

```python
                if message.content_blocks:
                    blocks: list[dict[str, Any]] = [
                        content_block_to_dict(block) for block in message.content_blocks
                    ]
```

```python
                if message.tool_calls:
                    blocks = []
```

and at line 407 give the map key a `str` default:

```python
            stop_reason=_STOP_REASON_MAP.get(payload.get("stop_reason") or "", StopReason.other),
```

`oauth/_flow.py` (~185-186) — `create_task` wants a coroutine, `_open_browser` returns an `Awaitable`; wrap it (same eager-call semantics as before):

```python
    if _open_browser is not None:
        browser_awaitable = _open_browser(auth_url, redirect_uri)

        async def _await_browser() -> None:
            await browser_awaitable

        browser_task = asyncio.create_task(_await_browser())
```

and (~205-210) rename the reused `exc` (mypy: assignment to an except-bound name outside its block):

```python
                browser_exc = browser_task.exception()
                if browser_exc is not None:
                    callback_task.cancel()
                    with contextlib.suppress(asyncio.CancelledError):
                        await callback_task
                    raise AuthError(
                        f"OAuth browser callback helper failed: {browser_exc}"
                    ) from browser_exc
```

- [ ] **Step 4: Confirm green — types AND behavior**

Run from `sdks/python/`:
`uv run mypy motosan_ai/` → Expected: `Success: no issues found in 26 source files`.
`uv run pytest tests/ -q --ignore=tests/integration/` → Expected: PASS, zero failures (all Step 2-3 edits verified behavior-neutral against this suite at the baseline).

- [ ] **Step 5: Update pyproject.toml classifiers/extras and add the marker**

```bash
touch sdks/python/motosan_ai/py.typed
```

Classifiers block (lines 10-17) — add two entries so it reads:

```toml
classifiers = [
  "Development Status :: 4 - Beta",
  "Programming Language :: Python :: 3.11",
  "Programming Language :: Python :: 3.12",
  "Programming Language :: Python :: 3.13",
  "Topic :: Scientific/Engineering :: Artificial Intelligence",
  "Intended Audience :: Developers",
  "License :: OSI Approved :: MIT License",
  "Typing :: Typed",
]
```

Above the `[project.optional-dependencies]` table (line 24) add the comment:

```toml
# The per-provider extras are compatibility aliases: every provider uses the
# core httpx dependency, so `motosan-ai[anthropic]` == `motosan-ai`. Kept so
# existing install commands don't break.
```

- [ ] **Step 6: Verify the wheel actually contains py.typed**

Run from `sdks/python/`:

```bash
rm -rf dist && uv build --out-dir dist
unzip -l dist/*.whl | grep py.typed
```

Expected: one line showing `motosan_ai/py.typed` (hatchling includes package-dir files by default). If absent, add to pyproject.toml:

```toml
[tool.hatch.build.targets.wheel]
packages = ["motosan_ai"]
```

and rebuild until the grep hits. Delete `dist/` afterwards (`rm -rf dist`) — it must not be committed.

- [ ] **Step 7: Smoke-check typing visibility**

Run from `sdks/python/`:

```bash
uv run python -c "import motosan_ai, pathlib; p = pathlib.Path(motosan_ai.__file__).parent / 'py.typed'; assert p.exists(), p; print('py.typed present')"
```

Expected: `py.typed present`.

- [ ] **Step 8: Gate type-cleanliness in CI**

In `.github/workflows/ci-python.yml`, insert between the `Lint` step (line 28-29) and the `Test` step (line 30-31) — the `Sync deps` step already installs `--extra dev`, which now includes mypy:

```yaml
      - name: Type check
        run: uv run mypy motosan_ai/
```

- [ ] **Step 9: Update CHANGELOG**

In `sdks/python/CHANGELOG.md` under `## [Unreleased]`:

```markdown
### Added
- PEP 561 `py.typed` marker: downstream type checkers now see motosan-ai's
  annotations (previously the package was treated as untyped). `mypy
  motosan_ai/` is now clean and enforced in CI.

### Fixed
- Internal type errors surfaced while enabling `py.typed` (20 mypy errors
  across client.py, openai.py, minimax.py, anthropic.py, oauth/_flow.py) —
  all renames/annotations/equivalent control flow, no behavior change.
```

- [ ] **Step 10: Gates, commit, push, PR**

Run from `sdks/python/`: `uv run mypy motosan_ai/`, `uv run pytest tests/ -q --ignore=tests/integration/`, `uv run ruff check motosan_ai/`, `uv run ruff format --check motosan_ai/ tests/`. Then from the repo root: `treefmt --fail-on-change` must exit 0 (a before/after `git status` comparison canNOT detect the formatter touching an already-dirty file — only the flag proves zero reformatting; pyproject.toml was already taplo-formatted in Step 1). All must pass.

```bash
ISSUE=$(gh issue list --state open --search "Correctness quick-wins batch in:title" --json number --jq '.[0].number')
[[ "$ISSUE" =~ ^[0-9]+$ ]] || { echo "tracking issue not found — create it per Global Constraints" >&2; exit 1; }
git add sdks/python/motosan_ai/py.typed sdks/python/motosan_ai/client.py sdks/python/motosan_ai/providers/openai.py sdks/python/motosan_ai/providers/minimax.py sdks/python/motosan_ai/providers/anthropic.py sdks/python/motosan_ai/oauth/_flow.py sdks/python/pyproject.toml uv.lock .github/workflows/ci-python.yml sdks/python/CHANGELOG.md
git commit -m "feat: mypy-clean typed Python package with py.typed and CI gate (#${ISSUE})" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push -u origin chore/python-py-typed
test "$(git ls-remote origin refs/heads/chore/python-py-typed | cut -f1)" = "$(git rev-parse HEAD)" \
    || { echo "push did not land on origin — the pre-push hook may have blocked it" >&2; exit 1; }
gh pr create --title "feat: mypy-clean typed Python package (py.typed + CI gate)" --body "..."
```

---

### Task 5: Pre-push gate — path-scoped tests, opt-in live, loud failures, TS step (+ Rust nightly live CI) (PR-E)

Branch: `chore/pre-push-path-gate` (branch names are outside the commit-type rule; original plan designation kept)

**Files:**
- Modify: `scripts/pre-push-gate.sh` (full rewrite; 84 lines today)
- Create: `.github/workflows/ci-rust-nightly.yml`
- (No change to `scripts/setup-hooks.sh` — its shim `exec`s the tracked script, so the rewrite deploys instantly to every clone that already ran setup; verify by reading lines 21-25.)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: hook behavior contract — `RUN_LIVE=1 git push` opts into live tests; docs-only pushes skip all suites; failures print `PUSH BLOCKED: <reason>` on **stderr**.

**Context:** Today's hook (a) ignores the pre-push stdin ranges and runs the full Python+Rust suites on every push including docs-only, (b) auto-runs LIVE Anthropic tests (Python + Rust) whenever `ANTHROPIC_API_KEY` resolves — including silently pulling the Claude Code OAuth token from macOS Keychain, spending real API quota on mere keychain presence, (c) prints block reasons on stdout where the rtk wrapper has swallowed them (documented incident: push "succeeded" with exit 0 while blocked), (d) has no TypeScript step. Additionally — and this is why the new script strips credentials — the "unit" suites themselves contain **env-gated live tests that are NOT `#[ignore]`d**: `sdks/rust/tests/anthropic_live.rs` (plus `openai_live.rs`, `gemini_live.rs`, `minimax_live.rs`, …) runs its live cases whenever its API-key env var is set, and TS `tests/integration.*.test.ts` uses `(process.env.X ? describe : describe.skip)`. Without env-stripping, `RUN_LIVE=1` would not actually be the only live path. **Coverage caveat handled here:** the hook is currently the ONLY automated Rust live runner in the repo (`ci-python-nightly.yml` covers Python only), so making live opt-in without replacement would drop all automated Rust live coverage — the new `ci-rust-nightly.yml` workflow closes that hole.

- [ ] **Step 1: Rewrite scripts/pre-push-gate.sh**

Replace the entire file with:

```bash
#!/usr/bin/env bash
set -euo pipefail

# Pre-push gate: path-scoped unit tests for the SDKs touched by the pushed range.
# Live tests are OPT-IN: RUN_LIVE=1 git push
# (scheduled live coverage: ci-python-nightly.yml + ci-rust-nightly.yml).
#
# git invokes pre-push with one line per ref on stdin:
#   <local_ref> <local_sha> <remote_ref> <remote_sha>

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

ZERO=0000000000000000000000000000000000000000

block() {
    echo "PUSH BLOCKED: $1" >&2
    exit 1
}

# ---- Determine which SDKs the pushed ranges touch -------------------------
CHANGED=""
if [ -t 0 ]; then
    # Manual invocation without stdin: behave conservatively, test everything.
    CHANGED="ALL"
else
    while read -r _local_ref local_sha _remote_ref remote_sha; do
        [ "$local_sha" = "$ZERO" ] && continue # branch deletion: nothing to test
        if [ "$remote_sha" = "$ZERO" ]; then
            # New remote branch: diff against merge-base with origin/main.
            base=$(git merge-base "$local_sha" origin/main 2>/dev/null) || base=""
        else
            base="$remote_sha"
        fi
        if [ -n "$base" ]; then
            CHANGED+="$(git diff --name-only "$base" "$local_sha" 2>/dev/null || echo ALL)"$'\n'
        else
            CHANGED+="ALL"$'\n'
        fi
    done
fi

if [ -z "$CHANGED" ]; then
    echo "=== Pre-push gate PASSED (deletion-only push — nothing to test) ==="
    exit 0
fi

NEED_PYTHON=0
NEED_RUST=0
NEED_TS=0
if grep -qx "ALL" <<<"$CHANGED"; then
    NEED_PYTHON=1 NEED_RUST=1 NEED_TS=1
else
    if grep -qE '^sdks/python/' <<<"$CHANGED"; then NEED_PYTHON=1; fi
    if grep -qE '^sdks/rust/' <<<"$CHANGED"; then NEED_RUST=1; fi
    if grep -qE '^sdks/typescript/' <<<"$CHANGED"; then NEED_TS=1; fi
    # Root workspace manifests affect SDK builds without living under sdks/:
    # Cargo.toml is the Rust workspace root; pyproject.toml + uv.lock are the
    # uv workspace (members = ["sdks/python"]) and its tracked lockfile.
    if grep -qxE 'Cargo\.toml' <<<"$CHANGED"; then NEED_RUST=1; fi
    if grep -qxE 'pyproject\.toml|uv\.lock' <<<"$CHANGED"; then NEED_PYTHON=1; fi
fi

echo "=== Pre-push gate ==="

if [ "$NEED_PYTHON$NEED_RUST$NEED_TS" = "000" ] && [ "${RUN_LIVE:-0}" != "1" ]; then
    echo "=== Pre-push gate PASSED (no SDK paths in pushed range — suites skipped) ==="
    exit 0
fi

# ---- Unit suites ----------------------------------------------------------
# Hermetic: strip provider credentials so the env-gated live tests that live
# INSIDE the unit suites (tests/*_live.rs, TS integration.*.test.ts — none are
# #[ignore]d; they fire whenever their key env var is set) cannot run here.
# RUN_LIVE=1 below is the ONLY live path; it uses the ambient env untouched.
UNSET_CREDS=(-u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY
    -u GOOGLE_API_KEY -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID
    -u MINIMAX_API_KEY -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST)

if [ "$NEED_PYTHON" = "1" ]; then
    if command -v uv &>/dev/null; then
        echo "[python] unit tests..."
        env "${UNSET_CREDS[@]}" uv run pytest sdks/python/tests/ -q --ignore=sdks/python/tests/integration/ \
            || block "Python unit tests failed"
        echo "✅ Python unit tests passed."
    else
        echo "ℹ️  uv not found — skipping Python tests."
    fi
fi

if [ "$NEED_RUST" = "1" ]; then
    if command -v cargo &>/dev/null; then
        echo "[rust] unit tests (--all-features)..."
        env "${UNSET_CREDS[@]}" cargo test --manifest-path sdks/rust/Cargo.toml --all-features -q \
            || block "Rust unit tests failed"
        echo "✅ Rust unit tests passed."
    else
        echo "ℹ️  cargo not found — skipping Rust tests."
    fi
fi

if [ "$NEED_TS" = "1" ]; then
    if command -v npm &>/dev/null && [ -d sdks/typescript/node_modules ]; then
        echo "[typescript] build + vitest (pack-smoke needs dist/)..."
        (cd sdks/typescript && env "${UNSET_CREDS[@]}" npm run build && env "${UNSET_CREDS[@]}" npm run test) \
            || block "TypeScript tests failed"
        echo "✅ TypeScript tests passed."
    else
        echo "ℹ️  npm or sdks/typescript/node_modules missing — skipping TS tests."
    fi
fi

# ---- Live tests (opt-in only) ---------------------------------------------
if [ "${RUN_LIVE:-0}" = "1" ]; then
    if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
        ANTHROPIC_API_KEY=$(security find-generic-password -s "Claude Code-credentials" -w 2>/dev/null \
            | python3 -c "import json,sys; print(json.loads(sys.stdin.read())['claudeAiOauth']['accessToken'])" 2>/dev/null) || true
    fi
    [ -z "${ANTHROPIC_API_KEY:-}" ] && block "RUN_LIVE=1 but no ANTHROPIC_API_KEY available (env or Keychain)"
    export ANTHROPIC_API_KEY
    echo "[live] Python live Anthropic tests..."
    uv run pytest sdks/python/tests/integration/test_anthropic_live.py -v \
        || block "Python live tests failed"
    echo "[live] Rust live Anthropic tests..."
    cargo test --manifest-path sdks/rust/Cargo.toml --features full --test anthropic_live -- --test-threads=1 \
        || block "Rust live tests failed"
    echo "✅ Live tests passed."
fi

echo "=== Pre-push gate PASSED ==="
```

- [ ] **Step 2: Syntax-check and behavior-test the script**

```bash
bash -n scripts/pre-push-gate.sh                      # syntax OK, no output
H=$(git rev-parse HEAD)
Z=$(printf '0%.0s' {1..40})                           # the 40-zero "null sha"
# deletion-only push → skip:
printf '(delete) %s refs/heads/x %s\n' "$Z" "$H" | bash scripts/pre-push-gate.sh
# docs-only range → skip. Use an EXISTING docs-only commit from history — do
# NOT create commits and NEVER reset --hard here (the Step 1 script rewrite is
# still uncommitted in the working tree and would be wiped):
C=$(git log --format=%H -30 origin/main -- docs/ | while read -r c; do
      if ! git diff --name-only "$c^" "$c" | grep -qE '^sdks/'; then echo "$c"; break; fi
    done)
test -n "$C"   # sanity: history has docs-only commits (the committed plan docs qualify)
printf 'refs/heads/x %s refs/heads/x %s\n' "$C" "$(git rev-parse "$C^")" | bash scripts/pre-push-gate.sh
```

Expected: first invocation prints `deletion-only push`; second prints `no SDK paths in pushed range`. Then one real-range check — a range that touches ONLY `sdks/rust/**` under `sdks/` (origin/main~2..origin/main = PRs #240+#241; do NOT use ~4 — that pulls in the M4 release commit which bumped `sdks/python/` and `sdks/typescript/` files and would run the Python suite too). **Precondition:** run `uv sync --all-extras` in `sdks/python/` of this worktree first anyway (harmless here, required before push regardless):

```bash
printf 'refs/heads/x %s refs/heads/x %s\n' "$(git rev-parse origin/main)" "$(git rev-parse origin/main~2)" | bash scripts/pre-push-gate.sh
```

Expected: only `[rust] unit tests...` runs and passes; no `[python]`/`[typescript]` lines appear.

- [ ] **Step 3: Add the Rust nightly live workflow**

Create `.github/workflows/ci-rust-nightly.yml`:

```yaml
name: ci-rust-nightly

on:
  schedule:
    # Daily at 08:30 UTC (16:30 Asia/Taipei), offset from ci-python-nightly's 08:00
    - cron: "30 8 * * *"
  workflow_dispatch: {}

jobs:
  live:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: sdks/rust
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Require ANTHROPIC_API_KEY secret
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          # The live tests early-return SUCCESS when the key is unset/empty
          # (anthropic_live.rs: `let Some(client) = client() else { return }`),
          # so an empty secret would make this job green while testing nothing.
          test -n "$ANTHROPIC_API_KEY" || { echo "::error::ANTHROPIC_API_KEY secret is empty — nightly would silently test nothing"; exit 1; }
      - name: Run live Anthropic integration tests
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: cargo test --features full --test anthropic_live -- --test-threads=1
```

(`dtolnay/rust-toolchain@stable` matches `ci-rust.yml`'s toolchain step; the `ANTHROPIC_API_KEY` secret already exists — `ci-python-nightly.yml` uses it.)

- [ ] **Step 4: Commit, push, PR**

Note on what runs at push time: the installed hook is a 2-line shim that resolves `git rev-parse --show-toplevel` **at push time from the current directory** and `exec`s that tree's `scripts/pre-push-gate.sh` — so pushing from this task's worktree already runs the NEW script (branch touches no `sdks/**` → suites skipped → fast push). Pushes made from other checkouts keep running whatever version of the script their tree has.

Gates (before commit — format/lint/test applies to every commit, shell and YAML included):
- `bash -n scripts/pre-push-gate.sh` (already run in Step 2 — rerun after any late edit)
- `shellcheck scripts/pre-push-gate.sh` → zero findings
- `actionlint .github/workflows/ci-rust-nightly.yml` → zero findings (enter `nix develop` if the tool is missing — do not skip)
- `treefmt --fail-on-change` from the repo root → exit 0 (sh/yaml aren't treefmt-covered; this proves no covered file drifted)

```bash
ISSUE=$(gh issue list --state open --search "Correctness quick-wins batch in:title" --json number --jq '.[0].number')
[[ "$ISSUE" =~ ^[0-9]+$ ]] || { echo "tracking issue not found — create it per Global Constraints" >&2; exit 1; }
git add scripts/pre-push-gate.sh .github/workflows/ci-rust-nightly.yml
git commit -m "fix: path-scope the pre-push gate and make live tests opt-in (#${ISSUE})" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push -u origin chore/pre-push-path-gate
test "$(git ls-remote origin refs/heads/chore/pre-push-path-gate | cut -f1)" = "$(git rev-parse HEAD)" \
    || { echo "push did not land on origin — the pre-push hook may have blocked it" >&2; exit 1; }
gh pr create --title "fix: path-scoped pre-push gate + opt-in live + Rust nightly live CI" --body "..."
```

PR body must state: (1) live tests are now opt-in via `RUN_LIVE=1` and the automated Rust live coverage moves to `ci-rust-nightly.yml`, (2) block messages now go to stderr with a `PUSH BLOCKED:` prefix (rtk-swallowing incident), (3) no hook reinstall needed (shim execs the tracked script), (4) **this PR triggers NO CI checks** — every PR workflow is path-filtered to `sdks/**`, and neither `scripts/**` nor the nightly workflow file matches — so do not wait for status checks; the verification is Step 2's local runs plus the post-merge `gh workflow run ci-rust-nightly` / `gh run list --workflow=ci-rust-nightly`.

---

### Task 6: Publish workflow guards — python test/tag gates, typescript dispatch fix (PR-F)

Branch: `chore/publish-workflow-guards` (branch names are outside the commit-type rule; original plan designation kept)

**Files:**
- Modify: `.github/workflows/publish-python.yml` (full rewrite; 21 lines today)
- Modify: `.github/workflows/publish-typescript.yml` (guard the verify step, lines 28-32)

**Interfaces:**
- Consumes / Produces: nothing code-level. Contract: a `python-v*` tag whose version mismatches `pyproject.toml` now fails before upload; a broken tree can no longer publish to PyPI; `workflow_dispatch` on publish-typescript becomes usable.

**Context:** `publish-python.yml` is the weakest of the three publish flows: no lint/test gate (publishes whatever the tag points at), no tag-vs-version check, `workflow_dispatch` with zero guards. `publish-typescript.yml` has gates but its verify step runs `${GITHUB_REF_NAME#ts-v}` unconditionally, so manual dispatch (ref = `main`) always fails. `publish-rust.yml` (hardened post-#241) is the reference pattern — its tag guard is `if [ "${GITHUB_REF_TYPE:-}" = "tag" ]` (line 54). PyPI Trusted Publishing (OIDC) is deliberately out of scope — it needs PyPI-side project configuration; the token flow stays.

- [ ] **Step 1: Rewrite publish-python.yml**

```yaml
name: publish-python

on:
  push:
    tags: ["python-v*"]
  workflow_dispatch: {}

jobs:
  publish:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: sdks/python
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v5
      - name: Verify tag matches pyproject.toml version
        run: |
          if [ "${GITHUB_REF_TYPE:-}" = "tag" ]; then
            version=$(python3 -c "import tomllib; print(tomllib.load(open('pyproject.toml','rb'))['project']['version'])")
            expected_tag="python-v${version}"
            if [ "${GITHUB_REF_NAME:-}" != "$expected_tag" ]; then
              echo "::error::Tag ${GITHUB_REF_NAME:-<none>} does not match pyproject.toml version ${expected_tag}"
              exit 1
            fi
          fi
      - name: Sync deps
        run: uv sync --all-extras
      - name: Lint
        run: uv run ruff check motosan_ai/
      - name: Format check
        run: uv run ruff format --check motosan_ai/ tests/
      - name: Unit tests
        run: uv run pytest tests/ -q --ignore=tests/integration/
      - name: Build
        run: uv build --out-dir dist
      - uses: pypa/gh-action-pypi-publish@release/v1
        with:
          packages-dir: sdks/python/dist/
          password: ${{ secrets.PYPI_API_TOKEN }}
```

(Note kept intact from the current file: `defaults.run.working-directory` applies only to `run:` steps, so the publish action's root-relative `packages-dir: sdks/python/dist/` is correct as-is. `ubuntu-latest`'s python3 is ≥3.11, so `tomllib` is stdlib.)

- [ ] **Step 2: Guard publish-typescript's verify step**

In `.github/workflows/publish-typescript.yml`, replace the verify step (lines 28-32) with:

```yaml
      - name: Verify tag matches package.json version
        run: |
          if [ "${GITHUB_REF_TYPE:-}" != "tag" ]; then
            echo "::notice::Not a tag ref (workflow_dispatch) — skipping tag check; publishing package.json version as-is"
            exit 0
          fi
          TAG="${GITHUB_REF_NAME#ts-v}"
          PKG=$(node -p "require('./package.json').version")
          test "$TAG" = "$PKG" || { echo "tag $TAG != package.json $PKG"; exit 1; }
```

(Mirrors publish-rust.yml's `GITHUB_REF_TYPE` guard. An accidental dispatch that reaches `npm publish` with an already-published version fails at npm with `403 cannot publish over existing version` — safe.)

- [ ] **Step 3: Validate YAML**

```bash
python3 -c "import yaml,sys; [yaml.safe_load(open(f)) for f in ['.github/workflows/publish-python.yml','.github/workflows/publish-typescript.yml']]; print('yaml ok')"
```

Expected: `yaml ok`. (If PyYAML is unavailable in the ambient python3, run it via `uv run --with pyyaml python -c ...` from `sdks/python/`.)

Then the required lint/format gates (format/lint/test applies to every commit, YAML included):
- `actionlint .github/workflows/publish-python.yml .github/workflows/publish-typescript.yml` → zero findings (enter `nix develop` if the tool is missing — do not skip)
- `treefmt --fail-on-change` from the repo root → exit 0

- [ ] **Step 4: Commit, push, PR**

```bash
ISSUE=$(gh issue list --state open --search "Correctness quick-wins batch in:title" --json number --jq '.[0].number')
[[ "$ISSUE" =~ ^[0-9]+$ ]] || { echo "tracking issue not found — create it per Global Constraints" >&2; exit 1; }
git add .github/workflows/publish-python.yml .github/workflows/publish-typescript.yml
git commit -m "fix: guard the python and typescript publish workflows (#${ISSUE})" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push -u origin chore/publish-workflow-guards
test "$(git ls-remote origin refs/heads/chore/publish-workflow-guards | cut -f1)" = "$(git rev-parse HEAD)" \
    || { echo "push did not land on origin — the pre-push hook may have blocked it" >&2; exit 1; }
gh pr create --title "fix: python publish gates + typescript dispatch guard" --body "..."
```

PR body must note: the changed workflows only execute on the next `python-v*` / `ts-v*` tag push (or manual dispatch), and **this PR triggers NO CI checks at all** (every PR workflow is path-filtered to `sdks/**`; publish workflows are tag/dispatch-only) — do not wait for status checks. Reviewer should eyeball the diff against `publish-rust.yml`'s proven pattern; the real verification is the next release wave.

---

## Done Criteria (whole batch)

- The tracking issue exists and every commit subject carries its `(#N)`; type is bare `fix:` / `feat:` / `refactor:` (no scope parentheses); breaking notes ride a `BREAKING CHANGE:` body paragraph.
- Every push was verified landed: `git ls-remote origin refs/heads/<branch> | cut -f1` equals the local HEAD SHA (a bare exit-0 `ls-remote` proves nothing — it succeeds even when the ref is absent).
- Six PRs open (or merged): `fix/think-stripper-utf8`, `fix/native-stream-termination`, `fix/python-capability-enforcement`, `chore/python-py-typed`, `chore/pre-push-path-gate`, `chore/publish-workflow-guards`. Tasks 1-4 green on their path-triggered CI; Tasks 5-6 trigger **no CI checks by design** (all PR workflows are path-filtered to `sdks/**`) — their gate is the in-task local verification.
- `uv run mypy motosan_ai/` reports zero errors and the Type check step exists in ci-python.yml; the wheel contains `motosan_ai/py.typed`.
- The credential-stripped full Rust suite (`env -u … cargo test --all-features` per Global Constraints) green with the 2 new ThinkStripper tests and 2 new native-EOF tests present.
- `uv run pytest` green with `tests/test_capability_enforcement.py` (6 tests) present and `test_minimax_is_text_only` flipped.
- `specs/types.md` has the native termination subsection + Responses-mode table row.
- Both CHANGELOGs carry the new `[Unreleased]` entries (no version bump in this batch — entries ride the next release wave; the Python entries are release-noted as breaking then).
- `ci-rust-nightly.yml` dispatched once post-merge and green (this also retires half of the "no scheduled Rust live coverage" loose end; the >1h codex token-refresh smoke remains a separate, deliberate manual run).
- No live tests were required to run during implementation; the `#[ignore]`d live smokes are untouched.
