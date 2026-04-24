# Agent CLI TUI Session Chrome Design (Claude Code Style)

## Context

This spec defines the next visual layer for `agent-cli` in `--ui-mode tui`, using a Claude Code-like terminal aesthetic while preserving the current non-fullscreen streaming architecture.

The focus is not feature breadth; it is visual hierarchy, rhythm, and interaction feel:

- session-scoped top chrome (header shown once per session)
- command-history transcript rhythm
- restrained terminal-native styling
- stable multiline alignment and wrapping

The target is not a chat interface. It is a polished command transcript: subtle gray command rows for user input, plain assistant text blocks with markers, and a lightweight session header shown once per session.

This is an incremental design on top of existing `agent-cli` TUI behavior already merged to `main`.

## Scope

### In Scope

1. `--ui-mode tui` only.
2. Session-scoped header: show once at session start; do not refresh per turn.
3. Non-fullscreen streaming mode remains unchanged at architecture level.
4. Transcript visual rhythm:
   - user rows (history + current input) as subtle full-line light gray command rows with `› ` prefix.
   - assistant rows with `● ` marker and hanging indent for wrapped lines.
   - visual rhythm should read as `subtle user command row -> plain assistant block -> subtle user command row -> plain assistant block`.
5. Single-line notice row beneath header.
6. Claude Code-inspired visual hierarchy:
   - Unicode pixel logo
   - warm accent used sparingly
   - monochrome-first text hierarchy
   - whitespace separation instead of box/border framing
7. Claude Code-inspired but project-specific styling:
   - do not copy Claude/Anthropic names, marks, or exact logo artwork
   - use project-specific product naming and a project-specific Unicode pixel logo

### Out of Scope

1. Changes to `--ui-mode classic`.
2. Changes to `--output jsonl` format.
3. Fullscreen frame-rendering TUI migration.
4. Right-side dashboard blocks (`Recent activity`, `What's new`) in this phase.
5. Provider/tool runtime semantics and turn-loop logic changes.
6. Direct cloning of Claude/Anthropic brand assets, product names, or exact logo artwork.

## Command Surface Impact

This phase does not add new commands. It only changes visual presentation for existing `tui` command flows.

### Preserved Commands (`tui`)

- `/help`
- `/commands`
- `/status`
- `/approve shell once|session|deny`

### Explicitly Unchanged Restrictions (`tui`)

The current `tui` restrictions remain in place:

- `/config`
- `/clear`
- `/model`

These continue to be unavailable in streaming `tui` mode and remain available via `classic` mode.

## UX Requirements

### Header Behavior

1. Header is rendered once at session start.
2. Header is not re-rendered during turns in the same session.
3. Header re-renders only when session changes.
4. Header has no border/frame background card.
5. Header uses whitespace and typographic hierarchy for separation.

### Header Layout

Left: Unicode pixel-logo block (fixed width).  
Right: three status lines, left-aligned:

1. Product name + version (highest emphasis)
2. Model/mode/billing status line (medium emphasis)
3. Project path (low emphasis)

### Notice Line

One-line informational notice under header:

- key phrase in warm accent color
- actionable hint in subtle gray
- no background block
- no separator line

The notice line must not advertise commands that are unavailable in streaming `tui` mode. If `/model` remains unavailable in `tui`, do not use `/model to switch` as the hint text.

### Design Intent

The interface should feel like a polished command-line workbench:

- terminal-native
- monochrome-first
- quiet and low-chroma
- structured by whitespace, prefixes, and indentation
- free of chat bubbles, cards, shadows, rounded containers, and heavy paneling

The design should be inspired by Claude Code's UI grammar, but must remain project-specific. Do not copy Claude/Anthropic product names, marks, or exact logo artwork.

### Transcript Rhythm

The transcript must read as command history flow, not chat bubbles.

Per round rhythm:

- user command row (subtle full-line light-gray tint)
- assistant text block (plain background)
- next user command row
- next assistant block

The transcript should stay compact but readable. It should not use large chat-like vertical gaps, message cards, separator lines, or bubble alignment.

### User Row Style

1. Prefix `› `.
2. Every historical submitted user command and the current editable input row share the same command-row visual grammar.
3. Use a subtle full-line light-gray tint across the transcript width.
4. The tint must be low-contrast and flat: no border, no rounded corners, no shadow, and no card-like padding.
5. The row should read as terminal command highlighting, not as a chat bubble, alert banner, or message card.
6. Multi-line wrapping preserves the same row background and text alignment.

### Current Input Row Style

The current editable input row is a Current Command Row, not a web-style input box.

1. It uses the same `› ` prefix and subtle full-line light-gray tint as historical user command rows.
2. It differs from historical user rows only by cursor/editing affordance.
3. It must not become a bordered textarea, floating bottom composer, rounded input field, or separate chat input panel.
4. It should remain part of the transcript rhythm instead of being visually detached from the command history.
5. Submitting input appends the command to the transcript in the same style and does not trigger header re-rendering.

If the current editable input row cannot reliably apply a full-line background due to REPL library constraints, preserve the `› ` prompt, alignment, spacing, and visual consistency first. Historical submitted user rows should still use the full-line subtle tint.

### Assistant Row Style

1. Prefix `● ` on first line only.
2. Wrapped lines align with first-line text start (hanging indent).
3. No bubble/background card.

## Visual System

### Typography

Monospace stack:

- JetBrains Mono
- SF Mono
- Menlo
- Monaco
- Consolas
- monospace

Keep relatively large terminal-readable typography and comfortable line-height.

### Color Roles

Use restrained roles:

- base text: dark gray
- secondary text: medium gray
- accent: warm orange-brown
- user command row background: light gray tint
- assistant marker: deep gray/near-black

No purple gradients, no saturated panels, no heavy chroma backgrounds.

Use ANSI-compatible color roles where possible. Prefer terminal-safe colors and graceful degradation over exact hex matching.

### Spacing Guidance

Use compact but readable vertical rhythm:

1. Header block appears once at session start.
2. One blank line separates header from notice.
3. Notice line remains single-line and unboxed.
4. One blank line separates notice from transcript.
5. User command rows and assistant blocks should feel connected as one turn.
6. Completed turns may have a modest spacer, but avoid large chat-like gaps.
7. Do not use separator lines between turns.

## Architecture

This design keeps existing flow and introduces a thin presentation layer refinement.

Implementation should prefer minimal changes to the existing TUI presentation path. New modules should only be added when they reduce duplication or clarify existing rendering logic. Do not perform a broad TUI architecture refactor in this phase.

### Affected Modules

The following module list describes likely touchpoints, not a mandate to create new files. If some listed modules do not already exist, do not create them by default. Prefer modifying the existing rendering path first. Create new modules only when the current file becomes materially harder to maintain.

1. `crates/agent-cli/src/tui/shell.rs`
   - session-chrome lifecycle control
   - start-of-session header/notice rendering
   - transcript append-only streaming output

2. `crates/agent-cli/src/tui/layout.rs`
   - helper render functions for header block and notice line
   - width-aware line assembly and clipping/wrapping boundaries

3. `crates/agent-cli/src/tui/icons.rs`
   - Unicode pixel logo constant(s)
   - marker/icon conventions for transcript rows

4. `crates/agent-cli/src/tui/theme.rs`
   - text hierarchy roles
   - command-row background styling role
   - graceful no-color fallback

5. `crates/agent-cli/src/tui/transcript.rs`
   - `render_user_command_row` and `render_assistant_block`
   - hanging-indent wrapping helpers

6. `crates/agent-cli/src/tui/input.rs` and `view_model.rs` (minimal touch)
   - keep current input rendering consistent with historical user row style

### Unchanged Modules

1. `crates/agent-cli/src/main.rs` classic REPL execution path.
2. `crates/agent-cli/src/output.rs` sink behavior for non-tui paths.
3. Provider/tool execution runtime semantics.

## Data Flow

### Session Start (`tui` mode)

1. Resolve session id.
2. Check session-chrome state.
3. If session is new:
   - print header block once
   - print notice line once
   - print spacer line
4. Enter normal prompt loop.

### Per Turn

1. Render submitted user input as command row, applying the subtle row tint when terminal capabilities allow.
2. Stream assistant/tool/status output into assistant/semantic rows.
3. On wrap, maintain hanging-indent rules.
4. Do not redraw header.

### Session Switch

1. Detect session id change.
2. Reset chrome state.
3. Render new session header/notice exactly once.

## Error Handling

1. Terminal width detection failure:
   - fallback to default width (e.g. 120)
   - continue rendering

2. Color unavailable / `NO_COLOR` / low capability terminal:
   - disable color and background styling
   - keep structural layout and prefixes

3. Unicode rendering mismatch:
   - keep text readable; logo may degrade gracefully to simpler glyph layout
   - if terminal width is narrow or Unicode rendering is unreliable, collapse the logo to a compact text mark or omit it while preserving the three-line status block
   - do not fail session startup

4. Rendering helper error:
   - isolate to current line formatting
   - never terminate REPL due to presentational failure

5. Suspended approvals:
   - keep same session transcript continuity
   - do not repaint header

## Testing Strategy

Add/extend tests in `agent-cli` for `tui` path:

1. Header lifecycle tests
   - header printed once per session
   - not reprinted per turn
   - reprinted on session switch

2. User row tests
   - subtle full-line command row formatting without border, radius, shadow, or card-like padding
   - multiline user input wrapping/alignment consistency

3. Assistant row tests
   - `● ` marker behavior
   - hanging indent correctness under wraps

4. Width behavior tests
   - narrow width clipping/wrapping
   - stable output shape without jitter-causing malformed lines

5. Fallback tests
   - no-color mode output remains readable
   - Unicode/chrome fallback remains non-fatal

6. Regression tests
   - `--ui-mode classic` unaffected
   - `--output jsonl` unaffected

7. Gate checks
   - `cargo test -p agent-cli`
   - `cargo clippy -p agent-cli --tests -- -D warnings`

## Acceptance Criteria (Definition of Done)

1. In `--ui-mode tui`, header appears once at session start and remains top context for the session.
2. Header is not refreshed after each turn.
3. User rows (history + current editable input) are rendered as subtle full-line light-gray command rows with `› `, without borders, rounded corners, shadows, or card-like padding.
4. Assistant responses use `● ` with hanging-indent multiline alignment.
5. Notice line appears below header with restrained accent usage.
6. Current editable input row remains visually consistent with historical user command rows and is not rendered as a separate web-style input box.
7. No fullscreen behavior introduced.
8. Classic and jsonl paths remain behaviorally unchanged.
9. All tests and clippy gates pass.

## Rollout Notes

Ship as a single `tui`-only increment. If needed, follow with a second phase for richer session dashboard content (`Recent activity`, `What's new`) without changing the foundational rhythm defined here.
