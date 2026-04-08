# Local Agent Executable Issue Backlog (Writing-First)

## Confirmed Product Decisions

1. Priority: Writing drafting capability first.
2. Output language policy: English by default; Chinese only when user explicitly requests Chinese.
3. Workflow interaction: Mandatory checkpoints between major workflow stages.
4. Provider rollout: DeepSeek can be delayed as compatibility target (not release blocker).

---

## Delivery Strategy

Milestone order:
1. M1 (Prompt + Task Routing + Settings): make writing intent reliable and controllable.
2. M2 (Writing Toolchain): deliver section drafting/revision/consistency primitives.
3. M3 (Checkpointed Workflow): guided multi-turn drafting flow with hard checkpoints.
4. M4 (Literature + Citation integration): strengthen grounding for drafting quality.
5. M5 (Peer Review + Revision loop): review/report/response-letter workflow.
6. M6 (Provider hardening + persistence + observability): stabilize production behavior.

Release gates:
- Gate A: M1 + M2 complete and passing tests.
- Gate B: M3 complete with mandatory checkpoint behavior verified.
- Gate C: M4+M5 functional with no regression in M1-M3.

---

## Phase 1 — Domain Prompt, Language Policy, Task Routing (M1)

### AGW-001 Add biomedical domain instruction layer
- Scope: `apps/desktop/src-tauri/src/agent/mod.rs`
- Change:
  - Add `BIOMEDICAL_DOMAIN_INSTRUCTIONS`.
  - Inject after base instructions via `build_agent_instructions_with_work_state()`.
- Acceptance:
  - Domain=biomedical injects biomedical constraints.
  - Domain=general behavior unchanged.

### AGW-002 Add language policy block (English-default)
- Scope: `apps/desktop/src-tauri/src/agent/mod.rs`
- Change:
  - Add explicit policy: respond in English by default.
  - Switch to Chinese only when user explicitly asks for Chinese output.
- Acceptance:
  - Chinese prompt without explicit language request still returns English.
  - Prompt containing “请用中文” returns Chinese.

### AGW-003 Extend `AgentTaskKind` for academic workflow
- Scope: `apps/desktop/src-tauri/src/agent/provider.rs`
- Change:
  - Add: `LiteratureReview`, `PaperDrafting`, `PeerReview`.
- Acceptance:
  - Serde roundtrip for new enum variants passes.

### AGW-004 Academic intent detectors (EN/ZH)
- Scope: `apps/desktop/src-tauri/src/agent/mod.rs`
- Change:
  - Add:
    - `prompt_requests_literature_review()`
    - `prompt_requests_paper_drafting()`
    - `prompt_requests_peer_review()`
- Acceptance:
  - Unit tests cover EN/ZH examples and disambiguation cases.

### AGW-005 Route new task kinds in `resolve_turn_profile`
- Scope: `apps/desktop/src-tauri/src/agent/mod.rs`
- Change:
  - Map intent -> task kind.
  - Map sampling profiles and max rounds.
- Acceptance:
  - Drafting intents route to `PaperDrafting`.
  - Existing selection/file edit flows remain stable.

### AGW-006 Add `AgentDomainConfig` in settings backend
- Scope: `apps/desktop/src-tauri/src/settings/types.rs`, `schema.rs`, `validation.rs`, `mod.rs`
- Change:
  - Add config: `domain`, `custom_instructions`, `terminology_strictness`.
- Acceptance:
  - Config persists and validates correctly.

### AGW-007 Add domain selector in settings UI
- Scope: `apps/desktop/src/components/workspace/settings-tabs/AIAssistantTab.tsx`
- Change:
  - Dropdown: General / Biomedical / Chemistry / Custom.
  - Custom instruction input.
- Acceptance:
  - Save/reload retains selection and custom text.

### AGW-008 Regression tests for prompt/routing/language policy
- Scope: `apps/desktop/src-tauri/src/agent/mod.rs` tests
- Acceptance:
  - New tests pass.
  - Legacy tests still pass.

---

## Phase 2 — Writing Toolchain (M2, Writing-First)

### AGW-101 Create `tools/writing.rs`
- Scope: new file + module wiring in `tools.rs`
- Change:
  - Implement skeleton + shared helpers for writing tools.
- Acceptance:
  - Module compiles and dispatch hooks are wired.

### AGW-102 Implement `draft_section`
- Scope: `tools/writing.rs`, `tools.rs`
- Input:
  - `section_type`, `key_points`, `tone`, `target_words`, `citation_keys`, `output_format`
- Output:
  - Draft content + citation mapping + quality notes.
- Acceptance:
  - Tool call returns stable JSON + preview text.

### AGW-103 Implement `restructure_outline`
- Scope: `tools/writing.rs`, `tools.rs`
- Acceptance:
  - Returns revised outline + rationale per section.

### AGW-104 Implement `check_consistency`
- Scope: `tools/writing.rs`, `tools.rs`
- Checks:
  - Terminology consistency, abbreviation definition/use, figure/table/ref numbering.
- Acceptance:
  - Returns structured findings with severity and location hints.

### AGW-105 Implement `generate_abstract`
- Scope: `tools/writing.rs`, `tools.rs`
- Acceptance:
  - Supports structured abstract mode and word limit control.

### AGW-106 Implement `insert_citation` (minimal)
- Scope: `tools/writing.rs`, citation integration points
- Acceptance:
  - Inserts citation marker from available reference pool.

### AGW-107 Register writing tools with contracts
- Scope: `tools.rs`
- Change:
  - Add tool specs, capability classes, approval/review policy, provider schema adaptation.
- Acceptance:
  - Tools appear in default specs only when corresponding feature flag enabled.

### AGW-108 Tool result adapter support for writing tool outputs
- Scope: `apps/desktop/src/lib/agent-message-adapter.ts`
- Acceptance:
  - Writing tool results render as clean UI summaries.

### AGW-109 Unit tests for writing tools
- Scope: `tools.rs` + `tools/writing.rs` tests
- Acceptance:
  - Happy path + invalid args + boundary checks pass.

---

## Phase 3 — Checkpointed Drafting Workflow (M3)

### AGW-201 Add workflow runtime module
- Scope: new `apps/desktop/src-tauri/src/agent/workflows/mod.rs`
- Acceptance:
  - Workflow stage enum and execution state compile and serialize.

### AGW-202 Implement drafting workflow orchestrator
- Scope: new `agent/workflows/paper_drafting.rs`
- Stages:
  1. Outline confirmation
  2. Section drafting
  3. Consistency check
  4. Revision pass
  5. Final packaging
- Acceptance:
  - Stage transitions are explicit and resumable.

### AGW-203 Mandatory checkpoint gate in runtime
- Scope: `turn_engine.rs`, `session.rs`
- Change:
  - Every stage completion emits checkpoint-required state.
  - Next stage blocked until user confirms.
- Acceptance:
  - No silent auto-advance across major stages.

### AGW-204 Add checkpoint events
- Scope: `events.rs`, `turn_engine.rs`
- Events:
  - `workflow_checkpoint_requested`
  - `workflow_checkpoint_approved`
  - `workflow_checkpoint_rejected`
- Acceptance:
  - Event stream is deterministic and logged.

### AGW-205 Add Tauri commands for workflow control
- Scope: `agent/mod.rs`
- Commands:
  - `agent_start_workflow`
  - `agent_continue_workflow`
  - `agent_checkpoint_action`
- Acceptance:
  - Commands support create/resume/pause/cancel paths.

### AGW-206 Frontend checkpoint card
- Scope: `agent-chat` components/store
- Change:
  - Render checkpoint card in chat stream.
  - Buttons: Approve Stage / Request Changes.
- Acceptance:
  - User can gate stage transitions reliably.

### AGW-207 Workflow state persistence
- Scope: `session.rs`
- Change:
  - Persist workflow type/current stage/stage outputs/pending checkpoint.
- Acceptance:
  - Restart app and continue workflow from pending checkpoint.

### AGW-208 E2E test: outline -> section draft -> checkpointed finalization
- Acceptance:
  - Full workflow passes without uncontrolled loop growth.

---

## Phase 4 — Literature + Citation Grounding (M4)

### AGW-301 Add PubMed provider
- Scope: new `apps/desktop/src-tauri/src/citation/pubmed.rs`, update `citation/providers.rs`
- Acceptance:
  - ESearch + EFetch pipeline returns normalized citation hits.

### AGW-302 Add MeSH expansion and query planner hooks
- Scope: `citation/query.rs`
- Acceptance:
  - Query plans include optional MeSH expansions and date range filters.

### AGW-303 Implement `search_literature`
- Scope: `tools/literature.rs`, `tools.rs`
- Acceptance:
  - Multi-provider results deduplicated by DOI/PMID.

### AGW-304 Implement `analyze_paper`
- Scope: `tools/literature.rs`
- Acceptance:
  - Returns objective/methods/findings/limitations/relevance fields.

### AGW-305 Implement `compare_papers`
- Scope: `tools/literature.rs`
- Acceptance:
  - Shared/conflicting findings and methodology differences are structured.

### AGW-306 Implement `synthesize_evidence`
- Scope: `tools/literature.rs`
- Acceptance:
  - Produces theme-organized synthesis with source-linked evidence blocks.

### AGW-307 Implement `extract_methodology`
- Scope: `tools/literature.rs`
- Acceptance:
  - Extracts study design/sample/intervention/endpoints/statistics fields.

### AGW-308 Literature review workflow (checkpointed)
- Scope: `workflows/literature_review.rs`
- Acceptance:
  - PICO parse -> search -> screen -> analyze -> synthesize with checkpoints.

### AGW-309 Frontend literature panel
- Scope: new `src/components/agent/LiteratureReviewPanel.tsx`
- Acceptance:
  - User can submit PICO, select papers, trigger synthesis stage.

---

## Phase 5 — Peer Review + Revision Loop (M5)

### AGW-401 Implement `tools/review.rs`
- Tools:
  - `review_manuscript`
  - `check_statistics`
  - `verify_references`
  - `generate_response_letter`
  - `track_revisions`
- Acceptance:
  - Structured review findings with severity levels.

### AGW-402 Peer review workflow orchestrator
- Scope: `workflows/peer_review.rs`
- Acceptance:
  - Section review + stats review + report + revision assistance is checkpointed.

### AGW-403 Checklist packs integration
- Scope: new checklist assets + workflow integration
- Checklists:
  - CONSORT / PRISMA / STROBE / ARRIVE / CARE
- Acceptance:
  - Workflow can select and apply checklist, output compliance gaps.

### AGW-404 Revision tracker model in session state
- Scope: `session.rs`
- Acceptance:
  - Reviewer comments mapped to change status and evidence.

### AGW-405 E2E test: manuscript review report + response letter
- Acceptance:
  - One-click flow creates structured review + response draft.

---

## Phase 6 — Provider Hardening, Compatibility, Observability (M6)

### AGW-501 Provider-adaptive instruction shim
- Scope: `mod.rs`
- Acceptance:
  - OpenAI/MiniMax use provider-tailored hints without changing task semantics.

### AGW-502 Tool schema compatibility matrix
- Scope: `tools.rs`, `chat_completions.rs`, `openai.rs`
- Acceptance:
  - Same tool specs serialize correctly for OpenAI + MiniMax.
  - DeepSeek listed as deferred-compat target (non-blocking).

### AGW-503 Parse/retry fallback for tool-call parse failures
- Scope: `turn_engine.rs`, provider adapters
- Acceptance:
  - Parsing failures trigger bounded retry with explicit format correction hints.

### AGW-504 Budget policy per workflow stage
- Scope: `turn_engine.rs`, `mod.rs`, settings
- Acceptance:
  - Per-stage round and token budgets configurable.

### AGW-505 Workflow metrics expansion
- Scope: `telemetry.rs`
- Metrics:
  - stage_success_rate
  - checkpoint_wait_time_ms
  - unsupported_claim_rate
  - citation_traceability_rate
  - user_rewrite_rate
- Acceptance:
  - Metrics written in structured logs with workflow/stage labels.

### AGW-506 Cross-provider integration suite
- Scope: smoke + workflow tests
- Acceptance:
  - OpenAI + MiniMax pass full drafting workflow regression.
  - DeepSeek optional suite can fail without blocking release.

---

## Dependency Rules

1. AGW-001..008 must complete before AGW-201+.
2. AGW-101..109 must complete before AGW-202.
3. AGW-201..208 must complete before AGW-401..405.
4. AGW-301..309 can run in parallel with AGW-201..208 after M2 is stable.
5. AGW-501..506 starts after M3 reaches checkpoint stability.

---

## Definition of Done (Global)

1. Build passes:
   - `cd apps/desktop && cargo check`
   - `cd apps/desktop && npx tsc --noEmit`
2. New/changed tests pass for touched modules.
3. No regression in existing document-read stability and approval suspension behavior.
4. Workflow checkpoints are enforced (cannot skip stage transitions without user action).
5. English-default language policy verified in automated tests.

---

## Out of Scope (This Plan)

1. Python computation pipelines.
2. Autonomous no-checkpoint black-box workflow execution.
3. DeepSeek parity as release blocker.
