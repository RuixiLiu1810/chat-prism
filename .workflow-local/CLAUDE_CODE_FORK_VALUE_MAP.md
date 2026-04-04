# Claude Code Fork Value Map

## Purpose

This document captures the most valuable ideas from `claude-code-fork-main`
without turning that fork into a direct integration target.

Rule:

- borrow capability patterns
- do not re-bind ClaudePrism to Claude-specific runtime assumptions

## High Value

### 1. Unified Query / Turn Engine

Representative files:

- `claude-code-fork-main/src/QueryEngine.ts`

Why it matters:

- conversation state lives in one executor
- abort, permission denials, usage, prompt assembly, and tool lifecycle are coordinated together
- UI does not directly own core turn execution logic

What ClaudePrism should borrow:

- keep one runtime-owned turn executor per conversation
- avoid scattering turn state across provider adapters and UI stores
- treat "submit message" as a complete workflow, not a transport call

Priority:

- high

### 2. Message Adapter Layer

Representative files:

- `claude-code-fork-main/src/remote/sdkMessageAdapter.ts`

Why it matters:

- external SDK / remote messages are normalized before rendering
- tool results, stream events, status, compact boundaries, and assistant text are treated as different semantics
- provider payload shape does not leak into the UI

What ClaudePrism should borrow:

- continue strengthening provider-neutral event adapters
- keep raw provider responses out of React components
- maintain an explicit internal message model

Priority:

- high

### 3. Permission System As Runtime Infrastructure

Representative files:

- `claude-code-fork-main/src/cli/structuredIO.ts`
- `claude-code-fork-main/src/services/tools/toolExecution.ts`
- `claude-code-fork-main/src/remote/RemoteSessionManager.ts`

Why it matters:

- permission is not just a dialog
- hooks, classifiers, SDK prompts, remote bridges, and persisted rules all participate
- allow/deny/ask decisions are part of the tool execution pipeline

What ClaudePrism should borrow:

- keep permission handling in runtime, not just widget state
- evolve from approval buttons toward permission rules and decision provenance
- preserve session-scoped authorization as a first-class concept

Priority:

- high

### 4. Review-First Diff Semantics

Representative files:

- `claude-code-fork-main/src/components/diff/DiffDialog.tsx`
- `claude-code-fork-main/src/components/FileEditToolUseRejectedMessage.tsx`
- `claude-code-fork-main/src/screens/REPL.tsx`

Why it matters:

- file edits are reviewable artifacts, not invisible side effects
- rejected or blocked file edits still produce something inspectable
- diff is a first-class surface, not a fallback debugging aid

What ClaudePrism should borrow:

- continue pushing edit requests toward reviewable change objects
- blocked writes should remain inspectable through diff/proposed change flows
- do not reduce edit failures to plain text explanations

Priority:

- high

## Medium Value

### 5. Session / History As Working Identity

Representative files:

- `claude-code-fork-main/src/assistant/sessionHistory.ts`
- `claude-code-fork-main/src/services/SessionMemory/sessionMemoryUtils.ts`
- `claude-code-fork-main/src/services/SessionMemory/prompts.ts`

Why it matters:

- session history is more than a list of old conversations
- it supports continuity, summaries, compaction recovery, and working identity

What ClaudePrism should borrow:

- keep session identity visible across list, tab, and active drawer
- consider structured session memory later, but do not jump there before tool/review flows are stable

Priority:

- medium

### 6. Central Coordination State

Representative files:

- `claude-code-fork-main/src/state/AppStateStore.ts`

Why it matters:

- overlays, permission mode, pending plan verification, remote callbacks, and launch state are coordinated centrally

What ClaudePrism should borrow:

- centralize only the state that truly coordinates runtime behavior
- avoid recreating the full giant store shape

Priority:

- medium

## Low Value / Use Carefully

### 7. Remote Session Harness

Representative files:

- `claude-code-fork-main/src/remote/RemoteSessionManager.ts`

Why it matters:

- useful if ClaudePrism later introduces remote workers or channel-mediated approvals

Why it is not urgent now:

- current ClaudePrism priorities are local runtime quality and provider-neutral agent UX

Priority:

- low

## Do Not Copy Directly

### 1. Claude-Specific Auth / CCR / Bridge Details

Do not import:

- Claude-specific auth flows
- CCR-only external metadata assumptions
- Anthropic transport details

Reason:

- these would re-bind ClaudePrism to the exact runtime we are trying to decouple from

### 2. Ink / Terminal UI Shell

Do not import:

- REPL terminal presentation
- terminal-specific focus and overlay mechanics

Reason:

- ClaudePrism is a Tauri desktop app with different UI constraints

### 3. Monolithic AppState Shape

Do not copy the full store structure.

Reason:

- it solves many terminal/remote concerns we do not need
- direct copying would add complexity faster than value

## Migration Order For ClaudePrism

### Tier A: Must Continue

1. strengthen provider-neutral event adapter
2. continue maturing runtime-level permission logic
3. continue review-first edit flow and diff semantics

### Tier B: Worth Doing After Tier A

1. richer session identity and recovery affordances
2. structured session memory only after current editing/review loops are stable

### Tier C: Parked

1. remote session bridge concepts
2. large-scale state architecture borrowing
3. Claude-specific auth / transport behavior

## Product Rule

Claude Code feels strong not because one model is magical, but because these systems reinforce each other:

- turn engine
- message adapter
- permission pipeline
- diff review
- session continuity

ClaudePrism should copy that systems thinking, not Claude branding or Claude transport.
