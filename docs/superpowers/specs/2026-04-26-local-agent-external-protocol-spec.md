# Local Agent External Protocol Spec (v1)

## Scope
- Defines the desktop <-> external local-agent event contract for migration cutover.
- Migration-first: preserve existing event names and payload shapes, add only minimal compatibility fields.

## Transport
- External process writes UTF-8 JSON Lines (one JSON object per line).
- Desktop consumes stdout/stderr stream and forwards to existing frontend channels.

## Event Channels
- `agent-event`
  - Payload type: `AgentEventEnvelope`
  - Shape: `{ "tabId": string, "payload": AgentEventPayload }`
- `agent-complete`
  - Payload type: `AgentCompletePayload`
  - Shape (v1):
    - `tabId: string`
    - `outcome: string` (`completed` | `cancelled` | `suspended` | `failed`)
    - `protocolVersion: number` (default `1` when missing)

## Versioning Rules
- Current protocol version constant: `AGENT_PROTOCOL_VERSION = 1`.
- Backward compatibility:
  - Missing `protocolVersion` must deserialize to `1`.
  - `protocol_version` snake_case is accepted as alias for compatibility.
- Forward compatibility:
  - Unknown JSON fields must be ignored by serde defaults.

## Migration Safety Net
- Keep existing frontend event listeners unchanged (`agent-event`, `agent-complete`).
- Keep desktop in-process path available during transition for rollback.
- External runner maps payload-only or legacy complete lines into canonical `AgentCompletePayload`.

## Acceptance Checks
- `agent-core` events tests include:
  - status payload protocol serialization/deserialization
  - complete payload missing `protocolVersion` defaulting to `1`
- Build callers that construct `AgentCompletePayload` include explicit `protocol_version` field.
