# Release Check Report (2026-03-28)

## Scope
- Desktop app QA preflight
- Citation module test coverage and integration flow

## Command Results

1. `pnpm --filter @claude-prism/desktop qa:release`
- Result: PASS
- Includes:
  - TypeScript type check
  - Vitest full suite

2. `pnpm --filter @claude-prism/desktop qa:release:full`
- Result: PASS WITH OPTIONAL FAILURE
- TypeScript and Vitest: PASS
- Rust cargo check: FAIL (optional) due local env dependency:
  - missing `icu-uc` (`tectonic_bridge_icu`)

3. `pnpm --filter @claude-prism/desktop test`
- Result: PASS
- Files: 17 passed
- Tests: 170 passed

4. `pnpm --filter @claude-prism/desktop qa:release` (after Scholar panel split + label-template script)
- Result: PASS
- Type check and Vitest both green.

5. `pnpm --filter @claude-prism/desktop qa:release` (after apply-labels + calibrate --labels integration)
- Result: PASS
- Type check and Vitest both green.

## New QA Assets
- `apps/desktop/scripts/release-check-desktop.ts`
- `apps/desktop/docs/release-gui-e2e-checklist.md`
- `apps/desktop/src/components/workspace/scholar-panel.tsx`（Selection Citation 独立面板）
- `apps/desktop/scripts/generate-citation-label-template.ts`（标注模板生成）

## Known Environment Blocker
- Rust check may fail locally until ICU/harfbuzz/pkg-config env is configured.
- Suggested fix:
  - `brew install pkg-config icu4c harfbuzz`
  - set `PKG_CONFIG_PATH` to include ICU pkgconfig path.

## Next Manual Steps
1. Run GUI E2E using checklist:
   - `apps/desktop/docs/release-gui-e2e-checklist.md`
2. Validate Settings provider connectivity and keychain fallback.
3. Validate long-paragraph citation search stability in GUI.
