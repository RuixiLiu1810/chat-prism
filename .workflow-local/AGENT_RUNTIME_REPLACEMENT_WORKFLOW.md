# Agent Runtime Replacement Workflow

## Goal

Replace the current Claude Code-specific runtime with a provider-agnostic agent runtime, while preserving the existing desktop UX as much as possible during the migration.

This is not a "swap one chat API key" task. The current application depends on an agent runtime that provides:

- streaming output
- multi-turn continuation
- tool calling
- local file/system actions
- session recovery
- proposed changes visibility

The replacement must preserve those capabilities in a stable order.

## Product Decision

### Runtime Classes

The backend model layer must support two runtime classes:

1. `responses` runtime
   - canonical provider: OpenAI
   - endpoints:
     - `POST /v1/responses`
     - `POST /v1/responses/{response_id}/cancel`
   - strengths:
     - streaming
     - `previous_response_id`
     - tool calling
     - custom function tools
     - MCP tools
     - multi-turn conversation state

2. `chat_completions` runtime
   - target providers: MiniMax first, DeepSeek optional later
   - endpoints:
     - `POST /v1/chat/completions`
   - strengths:
     - streaming
     - tool calling
     - broad OpenAI-compatible ecosystem support
   - limitations:
     - no native `previous_response_id`
     - local runtime must persist full message history
     - cancel/continuation semantics are provider-specific

Product rule:

- `responses` provider = full-fidelity agent path
- `chat_completions` provider = degraded-but-usable agent path
- never assume that `GET /models = Responses-compatible`

### Performance Gap Truth

When MiniMax or another `chat_completions` provider feels worse than the
original Claude Code experience, do not reduce the explanation to "the model is
weaker".

The gap is usually a combination of:

1. runtime class difference
   - `chat_completions` does not provide the same native continuation/cancel/session semantics as `responses`
2. default execution bias
   - Claude Code is tuned to treat many requests as work instructions, not chat prompts
3. permission and review friction
   - if the local runtime is still maturing, edit/write flows feel heavier and more hesitant
4. message/event adapter maturity
   - raw tool/status/stream semantics influence the "agent feel" directly
5. session continuity maturity
   - continuity is not just history replay; it includes working identity and compact/recovery behavior

Project rule:

- optimize runtime behavior before blaming provider quality
- compare `model + runtime` against `model + runtime`, not provider names alone

### Model-Facing Runtime Repair

For weaker `chat_completions` providers, the next priority is not provider
expansion but fixing the quality of information the runtime sends to the model.

Execution order:

1. Phase 1 Bundle: information quality + hard execution guard
   - rewrite `AGENT_BASE_INSTRUCTIONS` with explicit context-marker semantics
   - sanitize model-facing tool feedback so internal control JSON never leaks
     into provider-visible tool messages
   - use task-aware `tool_choice`
   - enforce a runtime hard guard so `selection_edit` cannot silently fall back
     to `write_file`
2. Phase 2 Bundle: tool reliability
   - fix streamed tool-call id/name assembly
   - enrich tool error/context feedback and tool descriptions
   - increase edit-path token/output budgets
3. Phase 3 Bundle: continuity + fallback
   - preserve tool context in transcript/history reconstruction
   - add multilingual fallback intent detection
   - set task-aware turn loop limits

Status update (2026-04-03):
- `Phase 1/2/3 Bundle` first-pass implementation completed and validated locally
- follow-up work now belongs to later structural/runtime hardening tracks rather
  than this information-quality bundle

### MiniMax Gap Closure Plan

The next improvement cycle for `chat_completions` providers should focus on
shrinking the behavior gap with the original Claude Code experience.

Execution order:

1. Phase A: execution bias hardening
   - build a clearer edit-intent router
   - make selection-edit requests prefer reviewable file changes instead of prose fallbacks
   - status: first pass landed
     - frontend now passes a structured `turnProfile`
       - `task_kind`
       - `selection_scope`
       - `response_mode`
       - `sampling_profile`
     - backend now resolves `TurnProfile` first, and only falls back to weak prompt heuristics when no stronger context exists

2. Phase B: permission friction reduction
   - move from "approval + retry" toward pending approval with in-place continuation
   - keep session-scoped permission decisions first-class
   - status: completed for the current migration scope
     - approval cards no longer synthesize frontend continuation prompts
     - runtime now stores pending turn state and resumes through `agent_resume_pending_turn`
     - resumed execution emits structured `tool_resumed` / `turn_resumed` events

3. Phase D: review-first diff completion
   - stabilize proposed-change generation as the main edit path
   - blocked writes should remain reviewable, not collapse into text summaries
   - status: first pass landed
     - when the model invokes `write_file`, visible assistant prose is suppressed in the chat transcript
     - review-ready diff stays the primary editing surface instead of competing with a second prose summary in chat

4. Phase C: message/event semantics strengthening
   - improve tool progress / status / completion semantics so the runtime feels more like an agent executor than a chat shell
   - status: first pass landed
     - tool execution now emits clearer stage transitions:
       - `tool_running`
       - `awaiting_approval`
       - `review_ready`
       - `tool_result_ready`
       - `responding_after_tools`
     - the top status banner now distinguishes active work, pending approval, and review-ready states instead of showing a single generic running tone

5. Phase E: session continuity round two
   - improve active working context continuity after the edit/review path is stable
   - status: first pass landed
     - active tabs now retain:
       - current work target
       - recent tool activity
     - the chat drawer surfaces these as lightweight working-memory chips so resumed sessions feel more like ongoing work instead of anonymous history

Prioritization rule:

- A/B/D before C/E
- do not add more providers before this gap-closure pass materially improves the local runtime feel

### TurnProfile / SamplingProfile Rule

For weaker `chat_completions` models such as MiniMax, do not rely on keyword
guessing plus prompt markers as the primary routing mechanism.

Frozen rules:

1. agent routing must move toward a unified `TurnProfile`
   - `task_kind`
   - `selection_scope`
   - `response_mode`
   - `sampling_profile`
2. frontend should provide explicit UI/context signals first
3. keyword checks are allowed only as weak fallback evidence
4. provider request bodies must consume `sampling_profile`
5. provider quality discussions should not proceed until this profile-driven
   runtime behavior is in place

Current status:

- first pass landed
  - backend now resolves a structured `TurnProfile`
  - frontend now passes `turnProfile` explicitly to `agent_start_turn` /
    `agent_continue_turn`
  - prompt-level `[Execution route: ...]` markers were removed from the main path
  - OpenAI Responses and MiniMax `chat_completions` requests now apply internal
    sampling defaults from the resolved profile
- second pass landed
  - sampling profiles are now part of the formal agent runtime settings contract
  - Settings UI exposes `editStable / analysisBalanced / analysisDeep / chatFlexible`
    parameters
  - runtime loaders now deliver provider-specific sampling config to adapters
  - explicit UI actions such as proofread / lint-fix now pass direct
    `turnProfile` hints instead of relying on prompt wording
  - attachment/resource analysis turns now default to `analysisDeep` instead of
    the shallower balanced profile

### Chat-Layer Approval and Noise Reduction

The approval flow should behave like a chat-level interrupt, not a detail hidden
inside a tool widget.

Frozen rules:

1. approval UI belongs at the chat layer
   - render pending approval as a dedicated interrupt card near the composer /
     bottom of the message stream
   - do not embed the primary allow/deny controls inside bash/write widgets
2. tool widgets should remain lightweight execution trace components
   - running / completed / approval-required should stay visible
   - detailed approval decisions should not live inside the widget body
3. session and status surfaces should stay compact
   - keep persistent session identity lightweight
   - only elevate high-signal statuses such as failure, cancellation, or
     resume-after-approval

Current status:

- first pass landed
  - approval controls now render as a chat-level interrupt card above the
    composer
  - write/patch/shell widgets now show only light approval hints instead of the
    full decision UI
  - the session header has been compressed into a lighter single-line strip
  - the top status banner no longer competes with every active/pending state

### Structural Runtime Gap Priority

After the TurnProfile / SamplingProfile refactor, the next priority is no
longer provider expansion. The dominant remaining gap is runtime structure.

Frozen priority order:

1. selection-edit hard safety
   - for `selection_edit` turns, `write_file` must not remain a viable default
     path
   - runtime should hard-guard selection-scoped edit turns and force precise
     edit tools such as:
     - `replace_selected_text`
     - `apply_text_patch`
   - prompt wording alone is not sufficient

2. safe tool parallelism
   - current tool execution is still serial
   - the runtime should support:
     - read-only tools in parallel
     - write / mutation tools serialized
   - do not attempt arbitrary parallel writes before dependency semantics are
     explicit

3. round-budget uplift
   - the current hardcoded turn loop budget (`0..6`) is too low for realistic
     edit + tool trajectories
   - increase the round budget and pair it with clearer safeguards based on:
     - round count
     - token / output budget
     - explicit continuation affordance

4. pending-turn durability
   - pending approval / resume state must survive process restarts
   - add local persistence plus TTL / stale cleanup for abandoned pending turns

5. provider tool-schema adaptation
   - tool specs should no longer be treated as one OpenAI-shaped payload for
     every provider
   - provider adapters should own the final schema translation

### Tool Logic Correctness Follow-up

After the model-facing/runtime bundles and Sprint 2/3/4 closure, the next
remaining risk is no longer abstract "agent feel" but concrete tool-layer
correctness.

Important de-dup rule:

- do not re-open issues that are already fixed
  - `chat_completions` already uses `to_chat_completions_tool_schema(...)`
  - shell approval copy no longer incorrectly instructs users to review a diff
- treat the remaining work as a tool correctness track, not a prompt track

Execution order:

1. Tool correctness batch A
   - remove or implement ghost tool `replace_file_range`
   - add timeout and output-size caps to `run_shell_command`
   - split approval buckets so precise patch tools are not implicitly governed
     by the same bucket as whole-file `write_file`
2. Tool reliability batch B
   - make `read_file` truncation UTF-8 / line-safe instead of raw byte-cut +
     lossy tail corruption
   - add a conservative fallback path for `apply_text_patch`
     - exact match first
     - then a low-risk trim-based retry
   - add a fallback/preflight path for `rg`-dependent tools
     - `list_files`
     - `search_project`
3. Tool/UI cleanup batch C
   - remove stale Claude-specific widget aliases and dead widget branches
   - stop using global `isStreaming` for per-tool/per-tab status icons
   - remove fragile assistant/result text dedupe in chat message rendering
   - reduce approval payload coupling to large `oldContent/newContent` blobs in
     React widget logic

Current status (2026-04-03):

- validated against current code:
  - Batch A completed
    - `replace_file_range` ghost references removed
    - `run_shell_command` now has timeout + output caps
    - approval bucket split landed (`write_file` vs `patch_file`)
  - Batch B completed
    - `read_file` truncation is now UTF-8 / line-safe
    - `apply_text_patch` has a conservative trim-based fallback
    - `rg`-dependent tools now emit a clear preflight error when `rg` is unavailable
  - Batch C completed
    - stale Claude-specific widget aliases / dead branches removed
    - tool status rendering no longer depends on widget-local reads of global streaming state
    - fragile assistant/result dedupe removed
    - approval widget logic no longer copies large `oldContent/newContent` strings through `getApprovalPayload`
- removed from this track as outdated:
  - direct tool-schema adaptation bypass in `chat_completions`
  - shell approval text incorrectly pointing users to a diff

6. typed history / reasoning cleanup
   - `serde_json::Value` history remains acceptable only as a temporary bridge
   - move toward stronger internal message types
   - separate reasoning buffering from visible assistant text assembly

Project rule:

- complete the priority items above before promoting new providers
- DeepSeek remains parked while these structural gaps are still open
- do not explain runtime regressions primarily in terms of provider quality

### Closed-Loop Maturity Sprints

After the model-facing repair bundle, the dominant remaining gap is no longer
prompt quality. It is that the runtime subsystems still do not form a mature
closed loop:

- turn execution
- message adaptation
- permission runtime
- review-first edit artifacts
- session working memory

To keep the next cycle systematic and avoid re-opening already completed work,
the remaining mainline is frozen as three de-duplicated sprints:

#### Sprint 2: executioner stability

Goal:
- turn the current loop into a more stateful, resource-aware executor

Scope:
1. introduce `TurnBudget`
   - `max_rounds`
   - `max_output_tokens`
   - `consumed_tokens`
   - shared abort signal
2. propagate cancel/abort semantics beyond the stream layer into tool
   execution
3. inject tracked work-state into model context
   - `current_objective`
   - `current_target`
   - `last_tool_activity`

Status update (2026-04-03):
- completed for the current runtime scope
- landed:
  - `TurnBudget` now carries:
    - `max_rounds`
    - `max_output_tokens`
    - `consumed_output_tokens`
    - shared abort receiver
  - abort/cancel semantics now propagate into tool execution, including:
    - file reads
    - exact edit tools
    - search/list helpers
    - shell command execution
  - tracked work-state is now injected into model-facing instructions:
    - `current_objective`
    - `current_target`
    - `last_tool_activity`
    - pending approval state when present

#### Sprint 3: permission runtime hardening

Goal:
- upgrade approval from an in-memory state machine into a durable runtime

Scope:
1. persist pending turns locally
2. add TTL / stale cleanup for abandoned pending approvals
3. evolve approval state toward structured records
   - `decision`
   - `source`
   - `granted_at`
   - `expires_at`

Status update (2026-04-03):
- completed for the current runtime scope
- landed:
  - pending turns are now persisted locally under the app config directory
  - stale pending approvals and one-shot approvals now use TTL cleanup
  - approval state now records structured provenance through:
    - `ToolApprovalRecord`
      - `decision`
      - `source`
      - `granted_at`
      - `expires_at`
      - `remaining_uses`

#### Sprint 4: concurrency + compatibility + observability

Goal:
- improve throughput, provider compatibility, and local diagnosability before
  adding more providers

Scope:
1. support safe parallel tool execution
   - read-only tools in parallel
   - mutation tools serialized
2. add provider-specific tool schema adaptation for chat-completions providers
3. add lightweight local telemetry / structured execution logs for:
   - tool runs
   - error kinds
   - durations
   - approval-required outcomes

Status update (2026-04-03):
- completed for the current runtime scope
- landed:
  - read-only tools now execute in safe parallel batches
  - mutation tools remain serialized
  - chat-completions providers now use a provider-aware tool schema adapter
  - lightweight local telemetry now records tool execution outcomes, durations,
    approval-required states, and error kinds

Execution rule:
- Sprint 2 before Sprint 3
- Sprint 3 before Sprint 4
- DeepSeek remains parked until these three sprints materially improve the
  local runtime feel

Current status:
- Sprint 2 completed
- Sprint 3 completed
- Sprint 4 completed
- future work should move to the next structural/runtime track instead of
  reopening these baseline items as unfinished

### Model-Facing Information Quality Priority

Another frozen rule: do not diagnose weak agent behavior only as provider/model
weakness when the runtime is still sending low-quality instructions and noisy
tool feedback to the model.

Execution order:

1. Phase 1 bundle: information quality + hard execution guard
   - stronger base instructions
   - explicit context marker explanation
   - sanitized model-facing tool feedback
   - task-aware `tool_choice`
   - runtime hard guard for `selection_edit`

2. Phase 2 bundle: tool reliability
   - streamed tool-call id/name assembly fix
   - richer self-correcting tool errors
   - stricter tool descriptions
   - larger edit-path output budgets

3. Phase 3 bundle: continuity + fallback
   - preserve tool context in history reconstruction
   - multilingual fallback intent detection
   - task-aware turn loop limits

Project rule:

- Phase 1 is not prompt tuning; it is runtime correctness
- do not split the Phase 1 bundle into isolated micro-fixes

### Non-Goals For MVP

Do not attempt these in the first pass:

- full Claude session format compatibility
- full Claude CLI event subtype compatibility
- OpenAI hosted file search / vector store integration
- full provider marketplace
- complete rename of all `claude-*` files before the runtime works

The MVP target is a working agent loop, not a full branding cleanup.

### Compatibility Truth

Provider compatibility must be modeled explicitly:

- `reachable`: base URL + API key can hit a trivial endpoint like `/models`
- `tool-capable`: provider documents or validates tool calling
- `responses-compatible`: provider supports `/responses`
- `chat-compatible`: provider supports `/chat/completions`

The settings UI must not label a provider as "OK" unless the selected runtime mode is actually supported.

### Chat Completions Blueprint

For the full `chat completions` agent architecture and migration plan derived
from `claude-code-fork-main`, see:

- `.workflow-local/CHAT_COMPLETIONS_AGENT_MIGRATION_PLAN.md`
- `.workflow-local/CHAT_COMPLETIONS_AGENT_REGRESSION_CHECKLIST.md`

Frozen architectural rule:

- stop extending `write_file` as the default precise edit primitive
- selection-scoped edits must move to dedicated edit tools such as:
  - `replace_selected_text`
  - `apply_text_patch`
- `write_file` should become a whole-file write / final apply primitive, not
  the default tool for paragraph-scoped editing

## Current Coupling Points

These are the main hard couplings that must be replaced or wrapped:

### Frontend

- `apps/desktop/src/stores/claude-chat-store.ts`
  - sends prompts through `execute_claude_code` / `resume_claude_code`
- `apps/desktop/src/hooks/use-claude-events.ts`
  - assumes Claude stream message shape
- `apps/desktop/src/stores/claude-setup-store.ts`
  - setup/login/install flow is Claude CLI-specific
- `apps/desktop/src/components/project-picker.tsx`
  - app startup is blocked by Claude readiness
- `apps/desktop/src/components/agent-chat/session-selector.tsx`
  - session listing/history loading is Claude-specific

### Backend

- `apps/desktop/src-tauri/src/claude.rs`
  - spawns Claude CLI and parses its JSON stream
- `apps/desktop/src-tauri/src/lib.rs`
  - registers Claude-specific Tauri commands

## Target Architecture

## Layer 1: Provider-Agnostic Agent Core

Add a new backend module, conceptually:

- `agent/mod.rs`
- `agent/provider.rs`
- `agent/openai.rs`
- `agent/session.rs`
- `agent/tools.rs`
- `agent/events.rs`

Responsibilities:

- normalize all provider output into one internal event protocol
- manage local session state
- run the local tool loop
- expose Tauri commands to the frontend

## Layer 2: Provider Adapters

### OpenAI Responses Provider

The OpenAI provider is responsible only for:

- building `/v1/responses` requests
- translating local tool specs into OpenAI function tools
- reading streaming events
- returning normalized internal agent events
- holding `response_id` for continuation/cancel

It must not directly edit files or run shell commands.

### Chat Completions Provider

A second provider adapter class must be supported for vendors such as MiniMax:

- builds `/v1/chat/completions` requests
- maintains full local message history
- maps tool schemas into provider-compatible tool payloads
- reads streaming events and normalizes them into internal agent events
- does not rely on `previous_response_id`

DeepSeek can be considered only after MiniMax is stable, because its practical agent fit is weaker for this product goal.

## Layer 3: Local Tool Runtime

The local backend remains the executor of actual actions:

- `read_file`
- `write_file`
- `list_files`
- `search_project`
- `run_shell_command`

This preserves desktop control, file safety, and permission gating.

## Layer 4: Frontend Shell

The current UI should be preserved at first:

- tabbed chat
- message list
- tool widgets
- proposed changes
- session selector

The first migration goal is to keep UI stable while swapping the backend engine.

## External Reference Notes

### OpenRouter Skill: What To Borrow

Reference:

- https://openrouter.ai/skills/create-agent/SKILL.md

Useful ideas from that skill:

1. keep `UI -> Agent Core -> Provider Adapter` clearly separated
2. use an event-driven agent contract instead of provider-native raw payloads
3. treat streaming as item/event-oriented, not just plain text deltas
4. keep tool execution outside the provider transport itself

These points reinforce the current direction of this project.

### OpenRouter Skill: What Not To Copy

Do not directly transplant the OpenRouter skill implementation because:

1. it is a Node/TypeScript SDK tutorial, while this project's core runtime is Tauri/Rust
2. it is a from-scratch demo agent, not a migration plan for an existing desktop application
3. it does not address our local requirements:
   - snapshot integration
   - file refresh / compile loop
   - local session recovery
   - provider capability truthfulness

Project rule:

- borrow architecture principles
- do not let SDK tutorial structure override the local runtime architecture

## API Surface To Build

## Tauri Commands

Introduce a parallel agent command set:

- `agent_check_status`
- `agent_start_turn`
- `agent_continue_turn`
- `agent_cancel_turn`
- `agent_list_sessions`
- `agent_load_session_history`
- `agent_smoke_test`
- `agent_get_fast_mode` (optional; probably remove later)
- `agent_set_fast_mode` (optional; probably remove later)

### Command Mapping

Temporary mapping from current Claude commands:

- `check_claude_status` -> `agent_check_status`
- `execute_claude_code` -> `agent_start_turn`
- `continue_claude_code` -> `agent_continue_turn`
- `resume_claude_code` -> `agent_continue_turn`
- `cancel_claude_execution` -> `agent_cancel_turn`
- `list_claude_sessions` -> `agent_list_sessions`
- `load_session_history` -> `agent_load_session_history`

The frontend can migrate in two steps:

1. backend compatibility wrapper under existing names
2. frontend rename to `agent-*`

Smoke validation rule:

- for `chat_completions` providers, use an app-level smoke harness before promoting a provider
- smoke must validate: text streaming, actual local tool loop, and multi-turn continuation
- do not treat shell-side secret visibility as authoritative if in-app runtime succeeds

## Internal Session Model

The local session record should become:

```ts
type AgentSession = {
  localSessionId: string
  provider: "openai"
  projectPath: string
  tabId: string
  model: string
  previousResponseId?: string
  lastResponseId?: string
  createdAt: string
  updatedAt: string
}
```

This is intentionally local-first. Do not depend on Claude's `~/.claude/projects/...` layout.

## Internal Event Contract

Normalize provider output into a stable event protocol, for example:

- `agent-start`
- `agent-message-delta`
- `agent-message-complete`
- `agent-tool-call`
- `agent-tool-result`
- `agent-status`
- `agent-error`
- `agent-complete`

The frontend should consume these normalized events rather than provider-native payloads.

## Tool Strategy

## MVP Tool Set

Start with the smallest useful set:

- `read_file`
- `write_file`
- `list_files`
- `search_project`
- `run_shell_command`

Do not port every Claude-oriented tool immediately.

## Permission Strategy

Permissions stay local and explicit:

- read operations can be auto-allowed within project scope
- write operations should surface proposed changes
- shell commands should continue to respect the existing approval model

If a tool requires approval, the provider loop must pause and emit an approval-needed event instead of silently failing.

## Streaming Strategy

OpenAI Responses API should be used with streaming enabled.

The backend is responsible for:

- consuming SSE
- reassembling assistant text/tool events
- emitting normalized Tauri events to the frontend

The frontend must not parse raw OpenAI SSE directly.

## Setup / Settings Strategy

Replace Claude CLI setup with provider settings:

### Required Settings

- provider kind
- runtime mode: `responses` | `chat_completions`
- API key
- base URL (optional override)
- default model
- reasoning effort (only if the selected provider/runtime actually uses it)

### Status Check

`agent_check_status` should validate:

- API key present
- base URL reachable
- model string configured
- selected runtime compatibility

This should replace the current install/login concept. For OpenAI there is no CLI install step.

Connectivity probe rules:

- `responses` mode must probe `POST /responses` compatibility, not just `/models`
- `chat_completions` mode must probe `POST /chat/completions` compatibility, not just `/models`
- the UI should distinguish:
  - "Connected"
  - "Connected, but selected runtime unsupported"
  - "Not connected"

## Migration Order

## A0: Freeze The Contract

Deliverables:

- this workflow document
- coupling inventory
- target event/command schema

Gate:

- no coding against the old Claude path without checking this doc

## A1: Backend Skeleton

Deliverables:

- `agent` backend module scaffold
- normalized event types
- local session model
- no-op provider interface

Gate:

- backend compiles without changing frontend behavior

## A2: OpenAI Responses Provider

Deliverables:

- `agent_check_status`
- `agent_start_turn`
- `agent_continue_turn`
- `agent_cancel_turn`
- streaming text output

Scope:

- text-only first
- no local tool loop yet

Gate:

- user can send a prompt and receive streaming model output

## A3: Tool Loop

Deliverables:

- function tool schema generation
- local tool execution bridge
- tool result handoff back into Responses API
- approval-sensitive tools stay hard-gated until the frontend approval flow is wired

Gate:

- model can read project files and search the project through the normalized loop; write/shell remain intentionally gated until A4 approval plumbing exists

## A4.5: Provider Settings

Deliverables:

- independent agent provider settings in the existing Settings system
- separate secret storage for the agent API key
- backend runtime reads settings first and only falls back to env vars for local dev
- connectivity probe covers the agent provider endpoint

Gate:

- the agent runtime can be configured from the app without relying on shell env vars

## A4: Frontend Decoupling

Deliverables:

- `claude-chat-store` starts calling `agent_*`
- `use-claude-events` becomes provider-neutral or wraps agent events
- project picker no longer blocks on Claude CLI readiness
- session selector reads local agent sessions

Gate:

- no user-facing dependency on Claude CLI remains on the main path

## A5: Cleanup

Deliverables:

- deprecate old `claude.rs` path
- rename stores/hooks/components from `claude-*` to `agent-*` where worthwhile
- remove dead Claude setup/install/login UI

Gate:

- app functions without Claude CLI installed

## Phase 9.5: Multi-Provider Runtime

### B0: Freeze Compatibility Matrix

Deliverables:

- explicit provider/runtime matrix in workflow and settings schema
- classify providers into:
  - full agent
  - degraded agent
  - unsupported

Initial truth set:

- OpenAI = `responses`
- Azure OpenAI = `responses` candidate
- MiniMax = `chat_completions`
- DeepSeek = `chat_completions` candidate, not first-class initially

### B1: Settings and Connectivity Honesty

Deliverables:

- settings UI explicitly shows the currently active runtime mode
- connectivity test distinguishes `reachable` vs actual runtime support
- settings UI stops showing false-positive "OK" for providers that only expose `/models`

Gate:

- a DeepSeek-style configuration can no longer appear fully valid while `/responses` still 404s

Status:

- completed for the current fixed `responses` runtime
- note: provider-selectable `runtimeMode` is intentionally deferred to B2/B3, because honesty was the urgent bug and switching runtime classes requires a real provider adapter first

### B2: Chat Completions Provider Skeleton

Deliverables:

- `agent/chat_completions.rs` or equivalent adapter
- normalized streaming event adapter
- local transcript-based continuation instead of `previous_response_id`

Gate:

- provider-neutral core can run either runtime class without changing frontend behavior

Status:

- completed
- current scope intentionally stops at skeleton level:
  - dispatcher is provider-aware
  - local transcript continuation path exists
  - settings can express `openai|minimax|deepseek`
  - `minimax` / `deepseek` are still not transport-complete and must not be marketed as working providers yet

### B3: MiniMax First Provider

Deliverables:

- MiniMax provider settings
- MiniMax-compatible tool schema mapping
- MiniMax streaming + tool loop integration

Gate:

- app can run a degraded-but-usable agent workflow without OpenAI API access

Status:

- implementation landed
  - MiniMax now uses the `chat_completions` transport path
  - local transcript continuation is active
  - streaming + tool loop are wired in code
- still pending:
  - real-key smoke verification
  - production judgement on whether MiniMax is stable enough to declare B3 fully done

### B4: Optional DeepSeek Evaluation

Deliverables:

- only after MiniMax path is stable
- decide whether DeepSeek offers enough tool/runtime fidelity to justify support

Gate:

- do not ship DeepSeek as a first-class provider if it weakens the agent promise too much

## Risks

## High Risk

### 1. Treating This As A Simple Chat API Swap

That would fail. The missing part is the agent loop, not text generation.

### 2. Rewriting Frontend Too Early

If the UI is renamed before the backend contract exists, the migration becomes noisy and fragile.

### 3. Porting Too Many Tools At Once

That slows down MVP and increases debugging surface.

## Medium Risk

### 4. Overfitting To OpenAI Event Semantics

The internal event contract must stay provider-neutral even if OpenAI is the first provider.

### 5. Keeping Claude Startup Gating Too Long

If `project-picker` still blocks on Claude readiness, the new backend can exist but remain unusable.

## Acceptance Criteria

The replacement is considered successful when:

1. the app can open a project without Claude CLI installed
2. a user can send a prompt and receive streaming output
3. the model can use local tools to inspect project files
4. tool-driven edits appear in the existing UI flow
5. chat history/session recovery works through local agent sessions

## Execution Discipline

While implementing this workflow:

1. always update `TASK_BOARD.md` before/when changing stage
2. always append key outcomes to `SESSION_LOG.md`
3. do not mix runtime migration with unrelated UI polish
4. do not delete the Claude path until the agent path is validated
5. keep MVP bounded to OpenAI Responses API + local tools


## Current Promotion Status

- OpenAI = canonical `responses` provider
- MiniMax = validated `chat_completions` provider (text stream + tool loop + multi-turn smoke passed)
- DeepSeek = not promoted; keep as optional future evaluation only


## Phase 10+: Agent Capability Hardening

### Strategic Rule

Do not define the next stage as "fully import Claude Code".

That framing is too broad and too risky because it would tend to:

- re-couple the app to Claude-specific assumptions
- mix provider/runtime migration with UX redesign and tool governance
- expand scope before the current multi-provider runtime is hardened

Project rule for the next stage:

- keep the provider-neutral runtime as the product core
- harden agent capability first
- selectively import high-value interaction patterns from `claude-code-fork-main`
- borrow capabilities, not Claude-specific coupling

### Recommended Order

#### Phase 10: Agent UX Hardening

Goal:

- make the current agent loop trustworthy and observable in day-to-day use

Focus:

- complete tool UX and result/error visibility
- fix cancel / interrupt semantics, especially for `chat_completions`
- improve provider status and failure messaging
- tighten streaming / tool / waiting state transitions

Current progress snapshot:

- tool UX observability v1 is complete:
  - tool calls now carry real input through the event layer
  - local tool names are mapped to visible widgets
  - generic tool cards now show result / error payloads instead of only input
- `chat_completions` local cancel / interrupt v1 is complete:
  - tab-scoped cancellation channels exist in runtime state
  - MiniMax can now be locally aborted without surfacing a false hard error
  - this is a local abort semantics improvement, not a provider-native remote cancel claim

#### Phase 11: Tool Governance

Goal:

- move from "tools can run" to "tools are controlled and reviewable"

Focus:

- write-file approval flow
- shell-command approval flow
- session-level permission choices
- clearer audit trail for tool execution

#### Phase 12: Proposed Changes / Diff Review

Goal:

- make agent edits reviewable before they become trusted workflow primitives

Focus:

- proposed changes surface
- diff review UI
- accept / reject flow
- alignment with local file editing and snapshot loop

#### Phase 13: Selective Claude Code Feature Import

Goal:

- selectively absorb mature interaction patterns from `claude-code-fork-main`

Allowed imports:

- permission dialog patterns
- session/history interaction design
- diff/proposed-changes review patterns
- event adapter organization

Explicit non-goals:

- Claude-specific login/install/auth flows
- Claude-specific directory/session assumptions
- Claude-specific provider coupling

### Fork Integration Rule

`claude-code-fork-main` should be treated as a capability reference, not a wholesale source tree to merge.

Adoption rule:

1. identify a concrete missing capability in ClaudePrism
2. locate the corresponding interaction/architecture in the fork
3. adapt it into the provider-neutral runtime and desktop shell
4. do not import Claude-specific assumptions with it

### Phase 12 update
- Successful agent `write_file` operations now bridge into the existing proposed changes workflow.
- This is the first concrete step toward making selection-edit requests enter a reviewable edit path instead of remaining prose-only suggestions.
- Next step: strengthen the agent default behavior so selection-edit intents prefer reviewable file changes over suggestion-only responses.

### Claude Code reference discipline
- During Phase 12+, use `claude-code-fork-main` as a reference source specifically for: edit-intent handling, execution-first agent behavior, permission semantics, and reviewable change UX.
- Do not reintroduce Claude-specific runtime bindings; borrow behavior patterns, not provider lock-in.

### Phase 12 update: execution bias
- The runtime now carries a provider-neutral execution-first system instruction layer inspired by Claude Code.
- Selection edit requests are no longer left to model improvisation alone; they are explicitly nudged toward targeted file edits and reviewable changes.
- Next step: wire this behavior into a more explicit review/approval UX so edit-style prompts feel reliably agentic, not merely chatty.

### Regression discipline
- After the core chat-completions migration, every major runtime change on this line must be checked against `.workflow-local/CHAT_COMPLETIONS_AGENT_REGRESSION_CHECKLIST.md`.
- Do not treat a provider-level smoke pass as sufficient if selection-aware edit safety, approval-resume semantics, review-first diff, or session continuity regress.

### Phase 13 status update

Current status:

- first-pass selective import is complete for the highest-value tracks
  - `message adapter`
    - backend event semantics were already structured
    - frontend now normalizes live `tool_result` events and restored session-history
      `tool_result` messages into one UI-safe display shape before React renders them
    - React components no longer need to directly consume mixed raw tool JSON
  - `permission runtime`
    - approval decisions are runtime-owned, persisted, TTL-governed, and resumed through pending-turn state
    - approval UI now lives at the chat layer instead of inside individual tool widgets
  - `review-first diff`
    - blocked or approval-gated edits remain reviewable through proposed changes / diff
    - chat no longer competes with diff by showing a second long prose summary for edit turns

Interpretation rule:

- Phase 13 should now be treated as a completed first-pass capability import, not
  as an open-ended umbrella task
- future work in this area should be tracked as new concrete runtime/UI tasks,
  not by re-opening the generic Phase 13 headline

### Post-Phase-13 follow-up direction

Reference note:

- `ccb.agent-aura.top/docs/` reinforced the same systems view as the local
  Claude Code fork analysis:
  - turn engine
  - message adapter
  - permission pipeline
  - review-first diff
  - session continuity
- treat that site as a secondary architectural reference, not a source of truth
  for provider-specific implementation details

Frozen follow-up priorities:

1. lightweight session memory before heavy memory systems
   - do not jump directly to a large MEMORY.md clone or heavy summarization flow
   - first add selective recall from existing work-state fields such as:
     - `current_objective`
     - `current_target`
     - `last_tool_activity`
   - goal: inject only the highest-signal working context back into the model
   - status:
     - first pass landed
       - `AgentSessionWorkState` now retains a lightweight `recent_objective`
         alongside the current objective
       - prompt assembly now injects a `[Selective session recall]` block
         instead of dumping raw work-state fields
       - recall selection now prefers:
         - pending state
         - recent objective
         - current target when task-relevant
         - last tool activity when continuity-relevant
       - the runtime no longer blindly echoes the current request back to the
         model as fake "memory"

2. permission runtime should continue evolving from state to rules
   - current status:
     - persisted approval records
     - TTL
     - pending-turn persistence
   - next maturity step:
     - clearer rule provenance / decision source
     - denial tracking
     - more explicit distinction between rule-driven allow/deny and ad-hoc turn UI

3. document ingestion outranks skill hunting
   - for PDF / DOCX / rich attachments, prioritize:
     - extraction
     - excerpt search
     - resource-aware tool paths
   - do not assume a "read docx" skill exists or should be the primary solution
   - product rule:
     - file/resource ingestion is a runtime capability
     - skills may orchestrate it later, but should not replace it
   - status:
     - first pass landed
       - pinned PDF and DOCX resources now go through a shared ingestion cache
       - PDF ingestion uses MuPDF extraction instead of filename-only placeholders
       - DOCX ingestion uses Mammoth extraction instead of falling through as an empty `other` file
       - send-time prompt assembly now adds `[Relevant resource matches: ...]`
         blocks based on local lexical search over ingested resource text
       - attachment-backed analysis turns now have a real excerpt/search path
         before shell probing
     - second pass landed
       - attachment-backed analysis with binary resources now disables provider
         tool choice (`none`) instead of leaving the model free to probe with
         shell commands
       - runtime hard guards now reject:
         - `run_shell_command` during PDF/DOCX analysis turns
         - `read_file` against attached `.pdf` / `.docx` resources during those turns
     - product rule:
       - ingestion-backed attachment analysis should consume prompt-provided
         excerpts and relevant matches first
       - shell probing is no longer part of the default analysis path
   - Claude PDF benchmark note:
     - Claude's public PDF support should be treated as a document-ingestion
       benchmark, not a shell-extraction workflow
     - externally visible behavior suggests:
       - PDF enters the model as a first-class document resource
       - page text extraction and page-image understanding are both part of the
         effective input surface
       - Claude Code / Claude clients then attach or reference those documents
         through runtime-managed attachment flows, rather than asking the model
         to invent `pdftotext`-style probing
     - implication for ClaudePrism:
       - closing the gap means improving ingestion fidelity and page-aware
         evidence surfacing
       - not teaching the model more shell recipes
     - next maturity steps for this track:
       - page-aware evidence blocks for PDF answers (`document -> page -> snippet`)
       - DOCX parity with the same excerpt/search UX contract
       - optional future path: page-image-assisted reasoning once the runtime
         supports a first-class document/media representation

4. MCP expansion should be treated as connection lifecycle work, not just tool calls
   - if external connectors are expanded later, design for:
     - connection caching
     - timeout handling
     - state visibility
     - tool discovery normalization
   - do not add new MCP surfaces as isolated one-off calls without runtime
     management

### Tool System Consolidation / Resource-Driven Tool Platform

Problem statement:

- the current tool layer still behaves like an evolving collection of point
  fixes:
  - `read_file` is still overloaded with "anything readable by path" mental
    models
  - `run_shell_command` keeps becoming the emergency fallback when the runtime
    lacks a first-class capability
  - attachment / document / edit / shell semantics are not yet separated into
    stable capability domains
- the long-term fix is not "make PDF work once", but to convert the tool layer
  into a resource-driven platform with explicit contracts

Frozen architectural direction:

1. separate `resource` from `file`
   - the runtime should first classify the target resource, then choose the
     appropriate ingestion / search / evidence path
   - baseline resource kinds:
     - `text_file`
     - `pdf_document`
     - `docx_document`
     - `image`
     - `structured_data`
     - `unknown`
   - implication:
     - PDF / DOCX should no longer be treated as "just another file for
       read_file"

2. move from generic file tools to domain tools
   - long-term tool families:
     - text tools:
       - `read_text_file`
     - document tools:
       - `read_document_excerpt`
       - `search_document_text`
       - `get_document_evidence`
       - `inspect_resource`
     - edit tools:
       - `replace_selected_text`
       - `apply_text_patch`
       - `write_file`
     - workspace tools:
       - `list_files`
       - `search_project`
     - shell tools:
       - `run_shell_command`
   - product rule:
     - document reading should be solved by document tools, not by shell
       probing and not by widening `read_file`

3. treat shell as an explicit engineering capability, not a universal fallback
   - `run_shell_command` should remain for build / debug / environment tasks
   - it must not be the default fallback for:
     - PDF reading
     - DOCX reading
     - attachment extraction
     - generic search gaps
   - controlled exception:
     - runtime-managed document fallback is allowed behind document tools when:
       - the document artifact is missing
       - or ingestion reports `image_only` / `failed`
     - this fallback must remain:
       - bounded by timeout/output limits
       - invisible as a model-authored shell step
       - initiated by runtime policy, not by free-form model command invention

4. establish a uniform tool contract
   - every tool should eventually declare:
     - capability class
     - resource scope
     - side-effect level
     - approval policy
     - review policy
     - suspend behavior
     - model-facing result shape
   - goal:
     - stop scattering tool behavior across prompt wording, event handling,
       widget conditions, and ad-hoc runtime guards

5. standardize tool result adaptation
   - each tool result should have distinct shapes for:
     - raw runtime state
     - model-facing feedback
     - UI-facing display
   - implication:
     - internal JSON should never leak directly into model reasoning or React
       rendering

6. standardize suspend / review / approval semantics
   - all high-risk or reviewable tools should fit the same state machine:
     - `tool_called`
     - `tool_running`
     - `tool_result_ready`
     - `approval_requested`
     - `suspended`
     - `resumed`
     - `completed`
   - future tool work must align with this runtime state model instead of
     inventing tool-specific interrupt behavior

Implementation phases:

Phase T1: resource / tool boundary cleanup

- narrow the semantics of `read_file`
  - short-term: fail fast on `.pdf` / `.docx`
  - medium-term: rename or replace with `read_text_file`
- keep PDF/DOCX on the document-ingestion path, not the text-file path
- freeze the product rule that shell probing is not a default document reader

Phase T2: document tooling

- add first-class document tools:
  - `read_document_excerpt`
  - `search_document_text`
  - `get_document_evidence`
  - `inspect_resource`
- these should use the existing ingestion layer first, then evolve behind the
  same contract as fidelity improves

Phase T3: tool contract platform

- introduce a registry-driven tool contract model
- centralize:
  - approval policy
  - reviewability
  - suspension behavior
  - model-facing result shaping
  - UI-facing display shaping

Phase T4: richer document ingestion

- persist page-aware document artifacts
- improve fidelity for PDF/DOCX evidence blocks
- optional future path:
  - OCR fallback
  - page-image-assisted reasoning

Execution discipline:

- do not continue solving document reading by expanding `run_shell_command`
- do not widen `read_file` into a universal binary/document reader
- future PDF / DOCX / XLSX / image handling should follow:

Execution update (2026-04-03):

- T1 completed (first pass)
  - `read_file` now fails fast on `.pdf/.docx`
  - shell is no longer the default document-reading fallback
- T2 completed (first pass)
  - added first-class document tools:
    - `inspect_resource`
    - `read_document_excerpt`
    - `search_document_text`
    - `get_document_evidence`
  - backend document tools now consume the same persisted ingestion artifacts created on the frontend
- T3 completed (first pass)
  - introduced a registry-driven `AgentToolContract`
  - approval bucket, reviewability, and parallel-safe classification now come from the shared contract layer instead of scattered matches
  - model-facing and UI-facing tool result shaping was extended to understand the new document tool family
- T4 completed (first pass)
  - page-aware document artifacts are now persisted under `.claudeprism/agent-resources`
  - backend document tools can surface `document -> page/paragraph -> snippet` evidence from those artifacts
  - for PDFs with missing or unusable artifacts, document tools may now perform a controlled internal `pdftotext` fallback instead of exposing shell probing as the primary path
  - OCR fallback remains a future fidelity upgrade rather than a blocker for this platform pass
  - resource classification
  - ingestion
  - excerpt/search/evidence
  - model/UI adapter
  - policy-driven tool exposure

Tool pipeline consolidation update (2026-04-03):

- a first-pass consolidation pass was completed to reduce scattered tool logic
  across the turn engine, event bridge, and frontend widgets
- backend now owns more of the tool semantics:
  - centralized policy gate via `ToolExecutionPolicyContext` and
    `check_tool_call_policy(...)`
  - normalized display payload via `AgentToolResultDisplayContent`
  - event payloads now carry both raw tool content and a UI-safe `display`
    projection
- frontend tool UI now starts from the backend `display` payload rather than
  re-deriving most states from raw JSON
- this is not yet the final form of the tool platform, but it establishes the
  intended direction:
  - one contract layer
  - one policy gate layer
  - one result display layer

Tool pipeline modularization update (2026-04-03):

- the backend tool implementation has now started moving from a monolith toward
  domain modules
- `agent/tools.rs` remains the shared façade for:
  - tool contracts
  - tool schemas
  - policy gates
  - result shaping
  - shared low-level helpers
- execution handlers are now split by capability domain:
  - `tools/document.rs`
  - `tools/edit.rs`
  - `tools/workspace.rs`
  - `tools/shell.rs`
- this is a structural stability step:
  - it does not change the external tool API
  - it makes the next pass on approval/review/suspend state much safer

Tool interruption state update (2026-04-03):

- approval / review / suspend semantics have now started converging on a single
  normalized interruption channel
- backend adds a shared `tool_interrupt` event with phases:
  - `awaiting_approval`
  - `review_ready`
  - `resumed`
  - `cleared`
- turn engine now emits that normalized interrupt event while still keeping the
  older compatibility events in place
- frontend approval state now begins from the normalized interrupt payload
  rather than reconstructing everything from multiple partially overlapping
  event families
- `PendingApprovalState` is now explicit about:
  - interrupt phase
  - governing approval tool
  - whether the suspended turn can actually resume
- this is still a first pass, not the final form:
  - legacy approval/review events still exist for compatibility
  - the next cleanup step is to let the frontend depend almost entirely on the
    normalized interrupt state and then prune redundant legacy wiring

Document ingestion plan alignment update (2026-04-03):

- aligned immediate implementation with the external `plan.md` Phase 1/2
  priorities (first pass):
  - removed the old PDF front-16-page indexing cap in frontend ingestion
  - upgraded structured PDF flattening to preserve layout order and paragraph
    breaks better
  - bumped artifact version to `v2` and made backend loader reject outdated
    artifacts so legacy truncated caches are not silently reused
  - strengthened runtime PDF fallback from one `pdftotext` attempt to a bounded
    two-strategy attempt (`default` then `-layout`)
  - removed the fallback segment hard-cut that previously kept only the first
    12 paragraphs when page breaks were unavailable

### Phase 14: Document Read Flow Simplification (In Progress)

Problem statement:

- current document reading is functionally available but cognitively noisy:
  - the model may call several adjacent tools in one question:
    - `inspect_resource`
    - `read_document_excerpt`
    - `search_document_text`
    - `get_document_evidence`
  - users see fragmented execution traces instead of a clear "read document"
    action
- this increases loop risk and makes attachment answers feel unstable even when
  ingestion artifacts already exist

Target behavior:

- for attachment/document Q&A, the runtime should feel like a single
  Claude-style document read path:
  - one primary document-read tool exposed to the model
  - runtime-managed retrieval inside that tool (excerpt + optional evidence)
  - no model-authored shell probing for exploratory extraction

Execution phases:

1. P14-A: API surface convergence
   - introduce a canonical `read_document` tool for PDF/DOCX analysis
   - keep legacy document tool handlers for compatibility, but remove them from
     default model-exposed tool specs
2. P14-B: policy + prompt convergence
   - update policy errors and instructions to recommend only `read_document`
     for document analysis
   - keep `read_file` blocked on binary resources
3. P14-C: UI convergence
   - collapse document tool widgets/labels into one primary read semantics
     (`Read document ...`)
   - keep legacy names display-compatible for older transcript entries
4. P14-D: regression + rollout guard
   - add/update tests for:
     - tool schema exposure list
     - binary attachment policy messaging
     - model-facing tool feedback for `read_document`
   - ensure local build/check/test pass before moving this phase to done
5. P14-E: UI 收口
   - default chat trace shows one high-level document step (`Read document`)
   - internal sub-steps remain available only in debug mode
   - avoid mixed multi-card traces (`inspect/gather/search/read excerpt`) in the
     default user path
6. P14-F: 状态机与可观测性
   - unify document read lifecycle status events:
     - `document_read_started`
     - `document_read_ready`
     - `document_read_failed`
   - emit per-question document metrics:
     - artifact miss rate
     - fallback rate
     - document tool rounds per question
     - end-to-end latency

Execution discipline:

- do not add more tool names to solve the same document-read task
- optimize for stable single-path behavior first, then add optional depth paths
- if a capability is internal fallback behavior, keep it runtime-managed and
  out of the model-facing tool vocabulary

Execution update (2026-04-04, first pass):

- P14-A completed
  - added canonical `read_document` tool
  - removed legacy document tools from `default_tool_specs()` exposure list
  - kept legacy handlers (`inspect_resource`, `read_document_excerpt`,
    `search_document_text`, `get_document_evidence`) for transcript/backward
    compatibility
- P14-B completed
  - prompt/policy/model-feedback guidance now points to `read_document` as the
    single document-read recommendation
  - binary attachment hard guards still block `read_file`/`run_shell_command`
    exploratory probing
- P14-C completed (first pass)
  - tool widget mapping now treats `read_document` as the primary document-read
    action and shows unified read semantics
- P14-D completed (first pass)
  - added regression test to enforce single-entry document tool exposure in
    `default_tool_specs()`
  - validation passed:
    - `cargo check --lib`
    - `cargo test --lib agent::tools::tests`
    - `cargo test --lib agent::prompt_tests`
    - `pnpm tsc --noEmit`
    - desktop build

Execution update (2026-04-04, second pass):

- P14-E completed
  - default chat rendering now collapses document sub-step traces into a single
    high-level document action card (`Read document`)
  - legacy document sub-step cards remain visible only when debug mode is on
- P14-F completed
  - runtime status events now use a unified document lifecycle:
    - `document_read_started`
    - `document_read_ready`
    - `document_read_failed`
  - telemetry now records per-question document metrics:
    - `artifact_miss_rate`
    - `fallback_rate`
    - `doc_tool_rounds`
    - `end_to_end_latency_ms`
