# Spec: Pi UI via RPC Parity Bridge

## Status
- Draft
- Priority: second implementation track (after PTY-over-MoQ)

## Goal
Deliver Pi's full interactive TUI behavior while running the agent remotely, using the exact Pi RPC/session event model end-to-end.

## Hard Requirements
- Full Pi TUI functionality (same interaction model as `pi` interactive mode).
- Operate on Pi's native data model, not summarized/reformatted deltas.
- No lossy event translation (no custom `text_delta`-only protocol).

## Non-goals
- Recreating Pi UI manually in `tools/agent-tui/agent-tui.mjs`.
- Maintaining custom event formats that only approximate Pi behavior.

## Why this path
This keeps semantic parity with Pi by transporting the exact JSON events Pi already emits in RPC mode (`AgentSessionEvent`), including `message_*`, `tool_execution_*`, queueing state, retries, and extension UI requests.

## Current gap
Today the bridge reduces Pi output to custom markers (`__PI_EVT__` + simplified kinds), which drops fidelity and cannot power full Pi UI behavior.

## Architecture

### Remote side (bot machine)
- Run Pi in RPC mode: `pi --mode rpc --no-session ...`.
- Bridge process reads:
  - RPC events/responses from Pi stdout.
  - Client commands from transport.
- Bridge forwards raw JSON lines unchanged (wrapped in transport framing only).

### Local side
- New local Node runner: `tools/agent-rpc-parity-ui/`.
- Uses Pi's official SDK/RPC client shape and renders with Pi's real interactive UI components.
- Sends all user actions as RPC commands (prompt, steer, follow_up, abort, state/session/model ops).
- Handles extension UI request/response flow.

## Transport framing (over Marmot messages)
Use framed payloads to support ordering, chunking, and replay safety.

Envelope (JSON):
```json
{
  "v": 1,
  "session_id": "uuid",
  "stream": "rpc_event|rpc_response|rpc_request|control",
  "seq": 42,
  "frag_index": 0,
  "frag_count": 1,
  "payload_b64": "..."
}
```

Rules:
- `payload_b64` is the exact Pi RPC JSON line bytes for rpc streams.
- Sender increments `seq` per stream.
- Receiver reassembles fragments, enforces monotonic sequence, dedupes by `(session_id, stream, seq)`.
- Max payload per Marmot message should stay under conservative relay limits; fragment when needed.

## Pi data parity contract
- Forward every Pi RPC event/response line without semantic mutation.
- Forbidden transformations:
  - Dropping event types.
  - Truncating tool args/results.
  - Coalescing or splitting JSON events.
  - Rewriting field names.

Optional metadata may be added only in outer envelope.

## UI fidelity contract
To claim parity, local experience must match Pi's behavior for:
- streaming text and thinking visibility
- tool call lifecycle and streaming tool updates
- steering/follow-up queues
- abort semantics
- slash command behavior (supported by RPC)
- extension UI dialogs and status hooks

## Implementation plan

1. Replace bridge protocol with raw RPC forwarding
- Add framed transport in bridge and local listener.
- Preserve exact Pi RPC JSON payloads.

2. Build local RPC parity client
- Add robust command/response correlation ids.
- Add reassembly, sequence checks, retransmit/reconnect policy.

3. Integrate real Pi UI layer
- Reuse Pi interactive components and behavior; do not hand-roll equivalent widgets.
- Keep command mapping 1:1 with RPC commands.

4. Add transport reliability safeguards
- heartbeat/ping
- reconnect + session resume strategy
- bounded buffers and backpressure

5. Wire into `just agent`
- Add mode switch:
  - `PIKA_AGENT_UI_MODE=pty` (default once PTY track lands)
  - `PIKA_AGENT_UI_MODE=rpc`

## Testing and acceptance

### Functional tests
- Prompt with tool-heavy task; verify full tool lifecycle is visible.
- Queue steering/follow-up while streaming; verify semantics match Pi.
- Abort during tool run; verify outcome matches Pi.
- Extension UI requests (select/input/confirm/editor) round-trip correctly.

### Parity tests
- Capture raw RPC event transcript from direct local `pi --mode rpc`.
- Capture transcript via bridge for same scripted prompt sequence.
- Assert event type order and payload equivalence (allowing envelope fields only).

### Performance targets
- Median event propagation latency < 250ms on healthy relays.
- No dropped or duplicated events after 30 minute interactive session.

## Risks
- Marmot message size/ordering constraints may require careful chunking and reorder buffering.
- Achieving "real Pi UI" via RPC may still require upstream-compatible adapter work.
- Higher complexity than PTY for strict visual parity.

## Exit criteria
- Users report no observable loss of intermediate Pi behavior compared to direct Pi for covered scenarios.
- Automated parity harness passes against recorded RPC traces.
