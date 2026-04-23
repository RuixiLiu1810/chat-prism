# Agent CLI Claude Code External UI Organization Design

Date: 2026-04-23  
Status: Proposed  
Scope: `crates/agent-cli` only

## 1. Objective

Build the `agent-cli` interactive UI to align with **Claude Code external-user behavior** from the `reference` codebase, with the smallest practical change set.

This design is intentionally not a visual reinvention. It is a behavior-mapping spec:

1. Reuse what is directly portable.
2. Rewrite only what is framework-bound (React/Ink runtime).
3. Exclude internal-only (`USER_TYPE=ant`) and non-core surface area.

## 2. Baseline and Constraints

## 2.1 Baseline

`reference/claude-code-main` external behavior is the source of truth.

## 2.2 Hard Constraints

1. Change only `crates/agent-cli`.
2. Keep `agent-core` protocol and runtime semantics unchanged.
3. Keep existing `jsonl` output mode behavior.
4. Use behavior equivalence over code copying.
5. Do not pull ant-only behavior into external-aligned CLI UX.

## 3. Evidence: External Behavior Model

## 3.1 Fullscreen Is Policy-Driven, Not Absolute

Evidence:

1. [fullscreen.ts#L112](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/utils/fullscreen.ts#L112)
2. [fullscreen.ts#L162](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/utils/fullscreen.ts#L162)

Interpretation:

1. Fullscreen is an environment policy decision.
2. UI shell must support fullscreen and non-fullscreen paths.

## 3.2 REPL Is a Session-Orchestrator, Not Just a Renderer

Evidence:

1. [REPL.tsx#L1103](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/screens/REPL.tsx#L1103) (`toolUseConfirmQueue`)
2. [REPL.tsx#L1112](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/screens/REPL.tsx#L1112) (`promptQueue`)
3. [REPL.tsx#L1157](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/screens/REPL.tsx#L1157) (`sessionStatus`)

Interpretation:

1. Session state is derived from queue and run status.
2. Approval suspension is a **waiting state** within the same session, not a fresh session.

## 3.3 Layout Uses Slot Semantics

Evidence:

1. [FullscreenLayout.tsx#L31](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/components/FullscreenLayout.tsx#L31)
2. [FullscreenLayout.tsx#L260](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/components/FullscreenLayout.tsx#L260)

Interpretation:

1. Layout contracts are explicit (`scrollable`, `bottom`, `overlay`, `modal`, `bottomFloat`).
2. Business logic should not be entangled with placement details.

## 3.4 Input Feel Comes from a Dedicated Input State Machine

Evidence:

1. [useInputBuffer.ts#L27](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/hooks/useInputBuffer.ts#L27)
2. [useArrowKeyHistory.tsx#L63](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/hooks/useArrowKeyHistory.tsx#L63)
3. [useHistorySearch.ts#L15](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/hooks/useHistorySearch.ts#L15)
4. [useCommandKeybindings.tsx#L37](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/hooks/useCommandKeybindings.tsx#L37)

Interpretation:

1. Good CLI feel is mostly input-state orchestration, not visual framing.

## 3.5 External vs Internal Behavior Is Explicitly Diverged

Evidence:

1. [REPL.tsx#L107](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/screens/REPL.tsx#L107)
2. [commands.ts#L49](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/commands.ts#L49)
3. [commands.ts#L343](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/commands.ts#L343)

Interpretation:

1. External alignment requires excluding ant-specific branches and feature gates.

## 4. Target Organization for `crates/agent-cli`

## 4.1 Layers

1. `Shell Orchestrator Layer`
2. `Layout Slot Layer`
3. `Transcript/Semantic View Layer`
4. `Input State Machine Layer`
5. `Event Bridge Layer`
6. `Terminal Capability Layer`

## 4.2 Proposed Module Boundaries

1. `tui/shell.rs`
2. `tui/layout.rs` (new)
3. `tui/transcript.rs` (new)
4. `tui/input.rs` (existing, upgraded)
5. `tui/input_buffer.rs` (new)
6. `tui/history_search.rs` (new)
7. `tui/event_bridge.rs` (existing, upgraded)
8. `tui/view_model.rs` (existing, upgraded)
9. `tui/fullscreen_policy.rs` (new)
10. `tui/suggestions.rs` (new, minimal v1)

## 5. Migration Mapping Table

## 5.1 Directly Portable Behavior

| Behavior | Reference Source | Agent CLI Target | Rule |
|---|---|---|---|
| Session status machine (`waiting/busy/idle`) | `REPL.tsx` | `tui/shell.rs` | Preserve semantics exactly |
| Approval/prompt queue continuity | `REPL.tsx` | `tui/event_bridge.rs`, `tui/view_model.rs` | Keep same-session continuation |
| Input buffer + undo | `useInputBuffer.ts` | `tui/input_buffer.rs` | Keep data model and debounce semantics |
| Arrow-key history + draft restore | `useArrowKeyHistory.tsx` | `repl.rs`, `tui/input.rs` | Preserve draft-before-history behavior |
| History search (`Ctrl+R`) | `useHistorySearch.ts` | `tui/history_search.rs` | Preserve query/match/cancel semantics |
| Command keybinding dispatch | `useCommandKeybindings.tsx` | `command_router.rs`, `tui/input.rs` | Keep action-to-command mapping model |

## 5.2 Rewrite with Original Intent (Framework-Bound)

| Behavior | Reference Source | Agent CLI Target | Rewrite Reason |
|---|---|---|---|
| Slot layout (`scrollable/bottom/overlay/modal`) | `FullscreenLayout.tsx` | `tui/layout.rs` | React/Ink component runtime not portable to Rust |
| Transcript rendering model | `Messages.tsx` | `tui/transcript.rs` | JSX tree replaced by semantic line model |
| Fullscreen policy switches | `fullscreen.ts` | `tui/fullscreen_policy.rs` | Same policy intent, Rust terminal environment |
| Terminal title/tab-status hooks | `use-terminal-title.ts`, `use-tab-status.ts` | `header_renderer.rs`, capability helper | Hook model rewritten as optional terminal capability calls |
| Suggestions UI | `PromptInputFooterSuggestions.tsx` | `tui/suggestions.rs` | Overlay rendering adapted to text UI constraints |

## 5.3 Explicit Non-Goals

| Item | Reason |
|---|---|
| `src/ink/*` rendering core migration | Framework-level runtime, not minimal CLI migration |
| ant-only branches (`USER_TYPE=ant`) | Out of external baseline |
| Buddy/voice/survey/internal notifications | Not required for external CLI core UX alignment |
| Large plugin/marketplace parity | Outside current scope |

## 6. Data Flow Design (Agent CLI)

1. User input enters `Input State Machine`.
2. Input resolves to command action or prompt submission.
3. Prompt submission triggers existing turn execution path.
4. Runtime events enter `Event Bridge`.
5. `Event Bridge` normalizes event payloads into semantic updates.
6. `ViewModel` updates session state and timeline.
7. `Layout` renders slot content from `ViewModel` snapshot.
8. Session status is recalculated continuously (`waiting/busy/idle`).

## 7. State Model Design

## 7.1 Core Session State

1. `is_loading`
2. `waiting_for_approval`
3. `active_dialog` (if any)
4. `session_status` derived from state above

## 7.2 Input State

1. `input_buffer`
2. `history_cursor`
3. `draft_snapshot`
4. `search_query`
5. `search_match_index`

## 7.3 Transcript State

1. Timeline entries (`user`, `assistant`, `semantic`)
2. Per-entry detail expansion state
3. Optional unseen-divider/jump marker (v2)

## 8. Error and Recovery Design

1. `approvalRequired` maps to waiting state, not terminal error.
2. Tool or provider errors append semantic error entries with optional detail.
3. Unknown command surfaces suggestion and keeps input focus.
4. Render degradation falls back to plain streaming output path.
5. Resume behavior keeps active session timeline continuity.

## 9. Testing Surface

## 9.1 Unit Tests

1. Session status derivation.
2. Queue-to-state transitions (`suspended` continuity).
3. Input buffer undo/debounce behavior.
4. History navigation and draft restoration.
5. History search transitions.
6. Event mapping to semantic transcript entries.

## 9.2 Integration Tests

1. Multi-turn with approval suspension and continuation in same session.
2. Slash-command path + normal prompt path coexistence.
3. `jsonl` mode regression check.
4. Fullscreen-policy switching behavior in supported environments.

## 10. Minimal-Change Rollout Plan

1. Introduce `UiSessionState` and queue-driven status in `tui/shell.rs`.
2. Add input buffer/history/search state modules.
3. Replace ad-hoc rendering with slot-based layout contract.
4. Upgrade event bridge for semantic timeline entries and detail expansion.
5. Add optional terminal capability helpers (title/tab status).
6. Stabilize with tests and keep classic/jsonl paths intact.

## 11. Definition of Done

1. Behavior aligns with Claude Code external baseline for session continuity and input feel.
2. No `agent-core` protocol changes required.
3. `jsonl` path remains unchanged.
4. No ant-only behavior leaked into `agent-cli` default UX.
5. Test suite covers session, input, and event mapping invariants.

## 12. Scope Guardrails

1. Any new UI behavior must have an explicit reference evidence anchor.
2. Any behavior without evidence is deferred.
3. Cosmetic changes that do not support behavioral parity are out of scope.

## 13. Visual Fidelity Addendum (External Baseline)

This section defines the **visual behavior contract** for color, background, motion, and icons.
It is not optional polish. It is part of behavioral parity.

## 13.1 Color System Contract

Evidence:

1. [theme.ts#L4](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/utils/theme.ts#L4)
2. [theme.ts#L55](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/utils/theme.ts#L55)
3. [theme.ts#L63](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/utils/theme.ts#L63)
4. [theme.ts#L115](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/utils/theme.ts#L115)
5. [theme.ts#L440](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/utils/theme.ts#L440)

Requirements:

1. `agent-cli` must use semantic color roles instead of hard-coded ad-hoc colors.
2. Minimum required roles for parity:
   - `text`, `subtle`, `success`, `warning`, `error`
   - `userMessageBackground`, `userMessageBackgroundHover`
   - `messageActionsBackground`
   - `selectionBg`
   - `claude` + shimmer companion color for spinner row
3. ANSI fallback profile is mandatory, matching reference intent for low-color terminals.

Implementation note for minimal change:

1. Introduce a lightweight `tui/theme.rs` with role tokens and two runtime palettes:
   - truecolor profile
   - ANSI profile
2. No full theme switch UI is required in this phase.

## 13.2 Background Layering Contract

Evidence:

1. [theme.ts#L163](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/utils/theme.ts#L163)
2. [theme.ts#L488](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/utils/theme.ts#L488)
3. [FullscreenLayout.tsx#L31](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/components/FullscreenLayout.tsx#L31)
4. [FullscreenLayout.tsx#L260](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/components/FullscreenLayout.tsx#L260)

Requirements:

1. UI background must be layered by slot semantics:
   - transcript layer (`scrollable`)
   - input/controls layer (`bottom`)
   - transient overlay layer (`overlay`)
   - blocking interaction layer (`modal`)
2. User message blocks and action-selected blocks must be visually distinguishable by background, not just prefix symbols.
3. Selection highlight must use dedicated `selectionBg` semantics, not inverse-color hacks.

Implementation note for minimal change:

1. Add style-level rendering helpers to transcript lines for `user`, `assistant`, `semantic`, `selected`.
2. Keep content model unchanged; only style mapping is added.

## 13.3 Motion and Animation Contract

Evidence:

1. [Spinner.tsx#L119](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/components/Spinner.tsx#L119)
2. [SpinnerAnimationRow.tsx#L72](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/components/Spinner/SpinnerAnimationRow.tsx#L72)
3. [SpinnerAnimationRow.tsx#L103](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/components/Spinner/SpinnerAnimationRow.tsx#L103)
4. [SpinnerAnimationRow.tsx#L131](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/components/Spinner/SpinnerAnimationRow.tsx#L131)
5. [useStalledAnimation.ts#L69](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/components/Spinner/useStalledAnimation.ts#L69)

Requirements:

1. Spinner animation is time-based and frame-driven, not random character swaps.
2. Reduced-motion mode must disable active animation and use stable static glyphs.
3. Stalled-state feedback must transition by intensity (or equivalent semantic fallback), not abrupt color flashing.
4. Transcript rendering must avoid frame-wide flicker by preferring local redraw over full repaint.

Implementation note for minimal change:

1. Add a tiny `AnimationClock` abstraction in `tui`:
   - active: fixed interval frames
   - reduced: single static frame
2. Keep only spinner-row motion in phase 1; do not animate unrelated regions.

## 13.4 Iconography and Glyph Contract

Evidence:

1. [figures.ts#L4](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/constants/figures.ts#L4)
2. [figures.ts#L9](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/constants/figures.ts#L9)
3. [figures.ts#L26](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/constants/figures.ts#L26)

Requirements:

1. Use a centralized icon table for all status glyphs; no inline scattered literals.
2. Keep platform fallback behavior for unsupported glyphs (e.g., circle variant fallback).
3. Define and freeze first-phase icon roles:
   - turn marker
   - running/waiting/completed state markers
   - fast/action hint marker
   - direction markers (`up/down`)
4. Unknown glyph support must gracefully degrade to ASCII-safe alternatives.

Implementation note for minimal change:

1. Add `tui/icons.rs` and route all rendering prefixes through it.

## 13.5 Status Color/Label Consistency

Evidence:

1. [use-tab-status.ts#L21](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/ink/hooks/use-tab-status.ts#L21)
2. [use-tab-status.ts#L27](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/ink/hooks/use-tab-status.ts#L27)
3. [use-tab-status.ts#L32](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/ink/hooks/use-tab-status.ts#L32)
4. [use-tab-status.ts#L37](/Users/liuruixi/Documents/Code/reference/claude-code-main/src/ink/hooks/use-tab-status.ts#L37)

Requirements:

1. Session status labels must remain semantically stable:
   - `idle`
   - `busy`
   - `waiting`
2. Header status color must follow the same semantic mapping across all UI modes.
3. If terminal-level tab-status capability is unavailable, the same semantic state must still be visible in header text.

## 14. Visual Test Matrix (Addendum)

## 14.1 Snapshot and Behavior Tests

1. Theme role mapping snapshot test (truecolor/ANSI).
2. Spinner reduced-motion test (animated vs static frame behavior).
3. Icon fallback test for non-Unicode-safe environments.
4. Background contrast test for `user` vs `messageActions` highlighting.

## 14.2 Manual Verification

1. Narrow width terminal: no broken wrapping or phantom line breaks.
2. Approval suspension flow: no repaint storms, no heading reset.
3. Scrollback readability: semantic lines remain visually separable.

## 15. Updated Definition of Done (Visual Scope)

In addition to Section 11:

1. Color rendering is role-based and parity-aligned with reference intent.
2. Background layering matches slot semantics (`scrollable/bottom/overlay/modal`).
3. Spinner motion follows frame-driven rules with reduced-motion fallback.
4. Iconography is centralized with platform-safe fallback behavior.
