# Spec: Pi UI via PTY-over-MoQ

## Status
- Draft
- Priority: first implementation track

## Goal
Stream a real remote Pi terminal session to local `just agent` using MoQ, so the local display and behavior are byte-for-byte the same as running `pi` directly on the remote host.

## Hard Requirements
- Full Pi TUI functionality with no UI reimplementation.
- Operate on the exact terminal data Pi produces (PTY stdout/stderr bytes + control sequences).
- End-to-end interactive behavior parity (keyboard, paste, resize, interrupts).

## Non-goals
- Semantic event reconstruction for display.
- Rebuilding Pi widgets in custom local TUI code.

## Why this path is first
This is the only path that guarantees strict UI/data identity by construction:
- Same Pi binary.
- Same PTY byte stream.
- Same terminal control protocol.

## High-level design

### Remote side
- New service mode on bot machine starts a PTY host process:
  - creates pseudo-terminal (`forkpty`/equivalent),
  - executes `pi` in that PTY,
  - forwards PTY output over MoQ,
  - applies client input and resize events to PTY.

### Local side
- `just agent` starts a local PTY client:
  - sets local terminal raw mode,
  - sends stdin bytes and resize events over MoQ,
  - writes received PTY bytes directly to local stdout.

Result: local terminal renders exactly what remote Pi emitted.

## MoQ channel model

Use dedicated reliable, ordered streams for terminal control and data.

- `ch0 control` (JSON, reliable ordered)
  - open/ack/close
  - heartbeat ping/pong
  - error reporting
- `ch1 stdin` (bytes, reliable ordered, client -> server)
  - raw key/paste bytes
- `ch2 stdout` (bytes, reliable ordered, server -> client)
  - PTY output bytes
- `ch3 resize` (JSON, reliable ordered, client -> server)
  - `{cols, rows}`

If MoQ layer does not offer strict reliability/ordering in current config, add it before implementation.

## Session protocol

### Open
Client sends:
```json
{
  "type": "open",
  "session_id": "uuid",
  "agent_group_id": "...",
  "term": "xterm-256color",
  "cols": 180,
  "rows": 46
}
```

Server replies:
```json
{
  "type": "open_ack",
  "session_id": "uuid",
  "server_version": 1
}
```

### Resize
Client sends on `ch3`:
```json
{ "type": "resize", "session_id": "uuid", "cols": 200, "rows": 55 }
```

### Close
Either side sends:
```json
{ "type": "close", "session_id": "uuid", "reason": "user_exit|network_error|remote_exit" }
```

Server includes remote exit code when available.

## Identity, auth, and encryption
- Reuse existing call/session auth used by encrypted audio/video path.
- Bind PTY session to authenticated user + group context.
- Reject unauthenticated opens and cross-session injection.
- Do not log raw PTY bytes by default (opt-in debug mode only, redacted).

## Terminal fidelity requirements
- Preserve all bytes from remote PTY output.
- Preserve all local input bytes (including escape sequences and bracketed paste).
- Forward `SIGWINCH` equivalent via resize channel without debouncing artifacts.
- Support Ctrl-C/Ctrl-D/Escape behavior exactly as seen in direct Pi sessions.

## Failure handling
- Heartbeat timeout closes session with explicit reason.
- Backpressure:
  - bounded queues on both ends,
  - stop reading stdin if outbound queue is saturated,
  - expose "connection slow" status.
- Reconnect behavior (phase 2):
  - optional short grace window to reattach to still-running PTY.

## Rollout plan

1. Minimal PTY transport
- Implement remote PTY host and local PTY client over MoQ.
- Open, stdin, stdout, resize, close.

2. Integrate with `just agent`
- Default `just agent` to PTY transport once stable.
- Add fallback switch:
  - `PIKA_AGENT_UI_MODE=pty` (default)
  - `PIKA_AGENT_UI_MODE=rpc`

3. Harden reliability
- Heartbeat, queue limits, structured errors, metrics.

4. Optional reattach
- Resume session by `session_id` after short disconnect.

## Testing and acceptance

### Functional
- Start session, run prompt with heavy tool usage, confirm full intermediate UI appears.
- Multi-line editor input, history navigation, slash commands, aborts.
- Resize terminal repeatedly during streaming output.

### Parity
- Golden capture test:
  - Run scripted key sequence directly in remote PTY and capture output bytes.
  - Run same script through PTY-over-MoQ and capture bytes.
  - Assert byte-for-byte equality for deterministic sections.

### Stability
- 60 minute soak test with continuous streaming + periodic resize.
- No stuck raw-mode local terminal on client crash paths.

## Risks
- MoQ reliability mode may need protocol tuning for terminal traffic.
- Cross-platform PTY behavior differs (Linux vs macOS dev).
- Raw terminal handling must be careful to avoid local shell corruption on abnormal exit.

## Exit criteria
- `just agent` shows Pi indistinguishable from direct remote terminal usage.
- Intermediate tool calls and streaming behavior are visible exactly as Pi emits them.
- PTY parity harness passes in CI or reproducible nightly lane.
