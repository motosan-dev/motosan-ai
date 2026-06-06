# M4 Execution — Copy-Paste Subagent Prompt Sheet

One fresh subagent per task, **strictly in order 1→6** (imports + a breaking caps-shape change make order load-bearing). Paste the **shared preamble** + the **task prompt** together. Use the **review prompt** between tasks.

Plan: `docs/superpowers/plans/2026-06-06-typescript-m4-minimax-mcp-thinking.md`
Spec (context): `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§4 M4)
Depends on: M1 (#185) + M2 (#186) + M3 (#187), all merged. Work in a worktree off `main`.

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan, working in a git worktree.

Working dir: /Users/daiwanwei/Projects/wade/motosan-ai/.worktrees/typescript-m4-minimax-mcp
(branch off main; M1+M2+M3 already merged in.)
All shell commands run from: <working dir>/sdks/typescript
Plan file: docs/superpowers/plans/2026-06-06-typescript-m4-minimax-mcp-thinking.md (relative to the working dir)

Rules:
- Read the plan's "## Conventions", "## Built on M1+M2+M3", and "## Canonical homes & cross-task contract" sections first (incl. the **Binding rules**). They override anything ambiguous.
- Then read ONLY your assigned "### Task N" and execute its steps in order, TDD: write the failing test → run it (confirm it FAILS) → implement → run it (confirm it PASSES) → npm run build → commit. Do not skip the red step.
- Relative imports MUST end in .js (NodeNext). Source in src/, tests in tests/ (run by vitest, NOT tsc-checked). There is NO `npm run format` script (gate = `npm run build` + `npm run test`).
- This builds on merged M1+M2+M3. Import existing symbols (types.ts, error.ts, serialize/*, providers/*, provider.ts, client.ts, stream.ts) — never re-declare them. NOTE: the symbol you think is "missing" from a file is usually the PRE-M4 state your plan adds — trust the plan; it contains the code verbatim.
- BINDING RULES you must honor:
  * Thinking: adaptive models (claude-opus-4-8/4-7/4-6) → thinking:{type:adaptive,display:summarized} + output_config:{effort:high}, NO budget_tokens, NO temperature override; other models → thinking:{type:enabled,budget_tokens,display:summarized} AND force temperature=1.0 (override user temp). User temperature applies ONLY when thinking is absent. Replace the naive `result.thinking = req.thinking` passthrough AND fold the old standalone temperature block into the if/else-if — do not leave it duplicated.
  * MCP: mcp_servers body key (snake_case, only if non-empty); mcp_toolset items appended to the SAME tools array as regular tools (combined [...regular, ...mcp], set result.tools if EITHER non-empty); tool_choice:'none' still deletes the combined array. Serialize mcpToolConfigs as-given (no auto-all).
  * Beta headers: anthropic-beta = mcp-client beta when mcpServers set + interleaved-thinking beta when thinking set AND not adaptive (comma-joined, no spaces; header omitted when empty).
  * MiniMax is Anthropic-wire after Task 4: posts to {base}/v1/messages (base default https://api.minimax.io/anthropic), x-api-key auth (NOT Bearer), serializeAnthropicRequest, MiniMax-M2.7 default; supportsMcp:true.
- Do NOT expand scope. If blocked or the plan looks wrong, STOP and report the exact problem + failing output — do not improvise.
- Show actual command output for every run/build step. Do not claim success without green output.
```

## Task prompts

**Task 1 — MCP types**
```
Execute "### Task 1: MCP types in types.ts". Add McpServerType, McpServerConfig {type,url,name,authorizationToken?}, McpToolConfig (discriminated union: kind all|allowed|denied with mcpServerName + allowedTools/deniedTools), and extend ChatRequest with mcpServers?/mcpToolConfigs?. Match the contract's exact shapes. Extend tests/types.test.ts.
```

**Task 2 — serialize: thinking-config + MCP**
```
Execute "### Task 2: thinking-config + MCP serialization in serialize/anthropic.ts". Add ADAPTIVE_THINKING_MODELS + EXPORT modelUsesAdaptiveThinking (Task 3 imports it) + applyThinkingConfig + serializeMcpToolConfig. REPLACE the naive `result.thinking = req.thinking` passthrough and fold the standalone `if (req.temperature !== undefined)` block into the if/else-if thinking control flow (non-adaptive thinking forces temperature=1.0). Build the COMBINED tools array [...regular, ...mcp_toolset] (set result.tools if either non-empty) + the mcp_servers body key. Extend tests/serialize.anthropic.test.ts (adaptive vs enabled, temp forced 1.0, mcp_servers shape, mcp_toolset in tools, none-deletes).
```

**Task 3 — beta headers**
```
Execute "### Task 3: beta headers in providers/anthropic.ts". Add buildBetaHeader (mcp-client beta when mcpServers set; interleaved-thinking beta when thinking set AND not adaptive; comma-joined; omit when empty) importing modelUsesAdaptiveThinking from ../serialize/anthropic.js. Wire it into BOTH chat() and streamImpl() request headers. Extend tests/providers-anthropic.test.ts. (If tsc flags the modelUsesAdaptiveThinking import as unused, the plan's Step 8 fallback says to drop it — follow that.)
```

**Task 4 — MiniMax Anthropic re-route**
```
Execute "### Task 4: MiniMax Anthropic-compat re-route + minimaxBaseUrl builder". Rewrite MinimaxProvider to the Anthropic-compat wire per the plan (thin delegate / serializeAnthropicRequest, POST {base}/v1/messages, base default https://api.minimax.io/anthropic, x-api-key auth + anthropic-version, Anthropic content/SSE parsing, default MiniMax-M2.7, retry preserved). Rename ClientBuilder _minimaxEndpoint → _minimaxBaseUrl (field + method + buildProvider arm + constructor option types/body) and fix any test call sites referencing the old name/endpoint. Test file tests/providers-minimax.test.ts (Anthropic-wire body to {base}/v1/messages NOT the legacy chatcompletion endpoint; x-api-key not Bearer; default MiniMax-M2.7; env-gated live).
```

**Task 5 — MCP rejection + supportsMcp caps**
```
Execute "### Task 5: MCP rejection on non-Anthropic providers". Add supportsMcp to ProviderCapabilities and update ALL factories (textOnly/withImage/fullCaps) + minimaxCaps; extend validateRequest to throw UnsupportedFeatureError when mcpServers/mcpToolConfigs set && !caps.supportsMcp. Set per-provider caps (anthropic + minimax → supportsMcp:true; openai → false). CRITICAL (Step 5): grep BOTH src AND tests for exact-shape caps assertions and fix them — incl. tests/providers-anthropic.test.ts:168 (add supportsMcp:true). Extend tests/capabilities.test.ts. (index.test.ts caps assertions are Task 6's; client-builder.test.ts inline mocks are fine — leave them.)
```

**Task 6 — exports + smoke test**
```
Execute "### Task 6: index.ts exports + M4 done-criteria smoke test". Export McpServerType/McpServerConfig/McpToolConfig from index.ts (no internal http/serialize leak). Fix the index.test.ts caps-shape assertions (add supportsMcp). Smoke test: a ChatRequest with mcpServers + thinking serializes for anthropic and is rejected for openai. Done when npm run build + npm run test are green.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-06-06-typescript-m4-minimax-mcp-thinking.md "### Task N".
Verify with evidence: (1) test written before implementation and now passes; (2) npm run build green (paste output); (3) no symbol re-declared outside its canonical home; (4) for Task 2: the naive thinking passthrough is REPLACED and the old temperature block FOLDED (not duplicated), thinking wire shape is {type:adaptive|enabled,...}; for Task 4: MiniMax posts Anthropic wire to {base}/v1/messages with x-api-key (not the legacy endpoint/Bearer); for Task 5: every exact-shape caps assertion in src AND tests updated for supportsMcp; (5) no scope creep; (6) commit exists with a conventional message. Report deviations; do not fix.
```

## After Task 6 (milestone close)

Run the plan's "## Milestone Done Criteria". When green, this is v0.7.0 — open a PR (from the worktree use `git push --no-verify` after verifying `npm run build` + `npm run test` locally; CI runs the full gate). Note: the minimaxEndpoint→minimaxBaseUrl rename is a breaking change (`feat(ts)!:`) — flag it in the v0.7.0 release notes. Next: M5 (Ollama native /api/chat + OpenAI-compat + auto-routing).
```
