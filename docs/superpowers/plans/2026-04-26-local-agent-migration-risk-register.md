# Local Agent Migration Risk Register (Task 1)

## R1: Protocol Drift Between Desktop and CLI
- Impact: frontend state mismatch, turn completion not recognized.
- Mitigation:
  - Freeze `agent-event` + `agent-complete` names.
  - Introduce explicit `protocolVersion` with default fallback.
  - Add protocol tests in `agent-core/src/events.rs`.

## R2: Legacy Producer Missing `protocolVersion`
- Impact: deserialize failure if field becomes required.
- Mitigation:
  - `AgentCompletePayload.protocol_version` uses serde default to v1.
  - Accept alias `protocol_version`.

## R3: Compile Break From New Field
- Impact: all struct literals fail compilation.
- Mitigation:
  - Apply smallest-possible call-site edits only where literals are built.
  - Limit scope to `agent-core`, `agent-cli`, and desktop external runner.

## R4: Unintended Behavior Change During Migration
- Impact: regression in runtime event flow.
- Mitigation:
  - Migration-first approach: no event pipeline refactor in Task 1.
  - Only schema extension + compatibility tests + compile fixes.

## R5: Cross-Repo Contract Drift (Standalone CLI vs Desktop Parser)
- Impact: Desktop can start process but fails to map stream lines into `agent-event` / `agent-complete`.
- Mitigation:
  - Commit canonical fixture: `tests/fixtures/local-agent-ping.jsonl`.
  - Add CI guard: `.github/workflows/local-agent-contract.yml`.
  - Keep parser additive-field tolerant in `local_agent_external.rs`.

## R6: Resume Semantics During Externalization
- Impact: approval-resume may lose exact in-process continuation context.
- Mitigation:
  - Use explicit continuation prompt on `resume_local_agent`.
  - Keep session metadata/history APIs available while runtime path externalizes.
  - Record rollout gate for suspend/resume success ratio in cutover runbook.

## Exit Criteria For Task 1
- `cargo test -p agent-core events -v` passes.
- Protocol docs committed.
- No frontend TS behavior changes required.
