# Agent CLI TUI Session Chrome Design (Claude Code Style)

## Context

This spec defines the next visual layer for `agent-cli` in `--ui-mode tui`, using a Claude Code-like terminal aesthetic while preserving the current non-fullscreen streaming architecture.

The focus is not feature breadth; it is visual hierarchy, rhythm, and interaction feel:

- session-scoped top chrome (header shown once per session)
- command-history transcript rhythm
- restrained terminal-native styling
- stable multiline alignment and wrapping

This is an incremental design on top of existing `agent-cli` TUI behavior already merged to `main`.

## Scope

### In Scope

1. `--ui-mode tui` only.
2. Session-scoped header: show once at session start; do not refresh per turn.
3. Non-fullscreen streaming mode remains unchanged at architecture level.
4. Transcript visual rhythm:
- user rows (history + current input) as full-width light gray command rows with `› ` prefix.
- assistant rows with `● ` marker and hanging indent for wrapped lines.
5. Single-line notice row beneath header.
6. Claude Code-inspired visual hierarchy:
- Unicode pixel logo
- warm accent used sparingly
- monochrome-first text hierarchy
- whitespace separation instead of box/border framing

### Out of Scope

1. Changes to `--ui-mode classic`.
2. Changes to `--output jsonl` format.
3. Fullscreen frame-rendering TUI migration.
4. Right-side dashboard blocks (`Recent activity`, `What's new`) in this phase.
5. Provider/tool runtime semantics and turn-loop logic changes.

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

### Transcript Rhythm

The transcript must read as command history flow, not chat bubbles.

Per round rhythm:

- user command row (light gray full-width background)
- assistant text block (plain background)
- next user command row
- next assistant block

### User Row Style

1. Prefix `› `.
2. Full-width light-gray background for every user input row (history + current input).
3. Multi-line wrapping preserves same row background and text alignment.

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

## Architecture

This design keeps existing flow and introduces a thin presentation layer refinement.

### Affected Modules

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

1. Render submitted user input as command row (background applied).
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
- full-width command row formatting
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
3. User rows (history + current) are rendered as full-width light-gray command rows with `› `.
4. Assistant responses use `● ` with hanging-indent multiline alignment.
5. Notice line appears below header with restrained accent usage.
6. No fullscreen behavior introduced.
7. Classic and jsonl paths remain behaviorally unchanged.
8. All tests and clippy gates pass.

## Rollout Notes

Ship as a single `tui`-only increment. If needed, follow with a second phase for richer session dashboard content (`Recent activity`, `What's new`) without changing the foundational rhythm defined here.
