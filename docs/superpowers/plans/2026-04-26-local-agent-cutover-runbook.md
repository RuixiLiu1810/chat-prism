# Local Agent External Cutover Runbook

## Rollout Gates

1. Gate 1: desktop can execute 20 consecutive local-agent turns without protocol parse failures.
2. Gate 2: suspend/resume success rate is at least 95% on internal matrix.
3. Gate 3: no unresolved `tool_approval` regressions in manual smoke scripts.

## Rollback Triggers

1. More than 2% failed local-agent turn startups in 24 hours.
2. Repeated suspended turns that cannot resume in the same session timeline.
3. Desktop parser mismatch for `agent-event` / `agent-complete` envelopes.

## Rollback Commands

```bash
git revert <desktop-cutover-commit>
git revert <remove-inprocess-agent-commit>
```

## Incident Checklist

1. Confirm fixture validity:

```bash
head -n 1 tests/fixtures/local-agent-ping.jsonl | jq -e '.payload.type == "status"' >/dev/null
tail -n 1 tests/fixtures/local-agent-ping.jsonl | jq -e '.payload.type == "complete"' >/dev/null
```

2. Check binary discovery:
   - Verify `PRISM_LOCAL_AGENT_BIN` points to an existing executable, or `agent-runtime` is in `PATH`.
3. Check stream behavior:
   - Ensure stdout/stderr readers are line-buffered and non-blocking in `local_agent_external.rs`.
4. Check suspend/resume:
   - Verify approval flow emits `awaiting_approval` then continues on `resume_local_agent`.
5. Check secret hygiene:
   - Ensure error logs do not include API keys or bearer tokens.

## Manual Smoke Matrix

1. New turn (`execute_local_agent`) with short prompt.
2. Continue turn (`continue_local_agent`) using returned `localSessionId`.
3. Suspend and resume flow (`set_local_agent_tool_approval` + `resume_local_agent`).
4. Cancel active run (`cancel_local_agent`) and verify `agent-complete=cancelled`.
