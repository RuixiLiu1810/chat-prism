# Agent CLI Claude Code External UI Implementation Plan

> **Execution mode:** Inline execution in this repo, task-by-task with checkpoints.  
> **Baseline:** `docs/superpowers/specs/2026-04-23-agent-cli-cc-external-ui-organization-design.md`  
> **Scope lock:** `crates/agent-cli` only.

## Goal

Implement Claude Code external-aligned terminal UI behavior in `agent-cli` with minimal code movement:

1. Session continuity (`suspended` stays in same conversation flow)
2. Slot-based layout semantics
3. Input state machine parity (buffer/history/search/command binding)
4. Semantic-first transcript with expandable details
5. Visual fidelity contract (theme roles, background layering, motion, icon fallback)
6. No regression in `jsonl` and classic paths

## Architecture Summary

Keep runtime behavior and provider contract unchanged.  
Add/upgrade only CLI UI composition modules:

1. `tui/shell.rs` orchestrates session status and turn lifecycle.
2. `tui/layout.rs` renders `scrollable/bottom/overlay/modal` semantics.
3. `tui/transcript.rs` renders semantic timeline.
4. `tui/input*.rs` handles input buffer/history/search and key actions.
5. `tui/event_bridge.rs` maps runtime events to semantic updates.
6. `tui/theme.rs` and `tui/icons.rs` implement visual role contracts.

## File Plan

### Create

1. `crates/agent-cli/src/tui/layout.rs`
2. `crates/agent-cli/src/tui/transcript.rs`
3. `crates/agent-cli/src/tui/theme.rs`
4. `crates/agent-cli/src/tui/icons.rs`
5. `crates/agent-cli/src/tui/input_buffer.rs`
6. `crates/agent-cli/src/tui/history_search.rs`
7. `crates/agent-cli/src/tui/suggestions.rs`

### Modify

1. `crates/agent-cli/src/tui/mod.rs`
2. `crates/agent-cli/src/tui/shell.rs`
3. `crates/agent-cli/src/tui/view_model.rs`
4. `crates/agent-cli/src/tui/event_bridge.rs`
5. `crates/agent-cli/src/tui/input.rs`
6. `crates/agent-cli/src/repl.rs`
7. `crates/agent-cli/src/command_router.rs`
8. `crates/agent-cli/src/header_renderer.rs`
9. `crates/agent-cli/src/main.rs`

## Task Breakdown

### Task 1: Session Status Core (`waiting/busy/idle`)

**Files:** `tui/shell.rs`, `tui/view_model.rs`, `tui/event_bridge.rs`  
**Outcome:** `suspended`/approval becomes same-session waiting state.

- [ ] Add explicit `UiSessionStatus` state derivation.
- [ ] Ensure `approvalRequired` maps to waiting status.
- [ ] Keep turn continuation in same timeline, no heading/session reset.
- [ ] Add unit tests for status transitions and suspended continuity.

### Task 2: Slot Layout Contract

**Files:** `tui/layout.rs`, `tui/shell.rs`, `tui/mod.rs`  
**Outcome:** UI uses slot semantics: `scrollable/bottom/overlay/modal`.

- [ ] Introduce layout API with slot render order and boundaries.
- [ ] Route current shell render path through layout contract.
- [ ] Preserve non-fullscreen default path behavior.
- [ ] Add narrow-width rendering tests (wrapping and clipping sanity).

### Task 3: Input State Machine Parity

**Files:** `tui/input_buffer.rs`, `tui/history_search.rs`, `tui/input.rs`, `repl.rs`  
**Outcome:** Claude Code-like command-line feel in Rust CLI constraints.

- [ ] Implement buffered input snapshots with undo semantics.
- [ ] Implement history navigation with draft restore.
- [ ] Implement `Ctrl+R` history search state transitions.
- [ ] Wire key action mapping into shell loop.
- [ ] Add focused tests for all input transitions.

### Task 4: Semantic Transcript + Detail Expansion

**Files:** `tui/transcript.rs`, `tui/view_model.rs`, `tui/event_bridge.rs`  
**Outcome:** Default semantic readability; details expandable on demand.

- [ ] Normalize event payloads to semantic timeline entries.
- [ ] Implement detail toggling per semantic node.
- [ ] Keep user/assistant/system semantic hierarchy stable.
- [ ] Add tests for event mapping and expansion behavior.

### Task 5: Visual Fidelity Contract

**Files:** `tui/theme.rs`, `tui/icons.rs`, `tui/transcript.rs`, `tui/shell.rs`, `header_renderer.rs`  
**Outcome:** Role-based color/background/motion/icon parity intent.

- [ ] Add theme role tokens (`text/subtle/success/warning/error/...`).
- [ ] Support truecolor + ANSI fallback palettes.
- [ ] Add centralized icon table with Unicode/ASCII fallback.
- [ ] Add spinner frame clock with reduced-motion static fallback.
- [ ] Ensure status colors/labels stay semantically consistent.

### Task 6: Command and Discoverability Alignment

**Files:** `command_router.rs`, `tui/input.rs`, `tui/suggestions.rs`  
**Outcome:** Command discoverability and predictable command interaction.

- [ ] Keep slash-command parsing semantics stable.
- [ ] Add lightweight suggestions rendering for command/file candidates.
- [ ] Ensure unknown command feedback includes best suggestion.

### Task 7: Integration + Non-Regression

**Files:** `main.rs`, `output.rs`, `tui/*` tests  
**Outcome:** Stable rollout with classic/jsonl compatibility.

- [ ] Verify classic mode path untouched.
- [ ] Verify jsonl mode output format unchanged.
- [ ] Add multi-turn integration tests for approval suspension and continuation.
- [ ] Run clippy/build/test gates for `agent-cli`.

## Validation Matrix

1. `cargo test -p agent-cli`
2. `cargo clippy -p agent-cli -- -D warnings`
3. `cargo run -p agent-cli -- --ui-mode tui` manual flow:
   - normal prompt turn
   - tool call with approval suspend
   - `/approve ...` then same-session continuation
4. `cargo run -p agent-cli -- --output jsonl` regression check

## Commit Strategy

1. Commit A: session status core + tests
2. Commit B: slot layout integration
3. Commit C: input buffer/history/search
4. Commit D: semantic transcript + detail expansion
5. Commit E: visual fidelity roles/icons/motion
6. Commit F: command discoverability enhancements
7. Commit G: integration and regression hardening

## Risk Controls

1. Keep fallback to existing streaming/classic path until slot renderer is stable.
2. Guard new visual capability paths behind terminal feature checks.
3. Avoid full repaint loops; favor localized redraw for spinner/transcript updates.
4. Treat Unicode glyph support as optional with ASCII-safe fallback.
