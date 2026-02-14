# Step 3 Debug Plan (Laptop -> Deployed OpenClaw Bot)

Date: 2026-02-14

Problem: `interop_openclaw_voice` creates a chat but the call never reaches `CallStatus::Active` (likely no `call.accept` received).

Goal: deconstruct into layers and make a single run answer "which contract failed".

## Contracts (in order)

1. Relay acceptance: our publishes land on at least 1 relay.
2. MLS group membership: bot receives + processes the welcome and is actually in the group.
3. Call signaling: bot sees `call.invite` and publishes `call.accept` (or `call.reject`).
4. MOQ runtime/media: after accept, MOQ session connects and frames flow.

## Phase 1: Add Wire Taps (one run = full diagnosis)

Changes (in this repo):
- `rust/src/bin/interop_openclaw_voice.rs`
  - Print call status transitions with timestamps.
  - Never hard-block on "ping/pong" (log it, but continue).
  - Spawn a Nostr observer that prints when:
    - we publish any `Kind::MlsGroupMessage` with the chat's `h=<chat_id>` tag
    - the bot publishes any `Kind::MlsGroupMessage` with `h=<chat_id>`
  - On timeout, print: last toast, last call status, state dir, relay lists.

Acceptance:
- On failure, output clearly indicates one of:
  - "we never published group messages" (relay rejection / connectivity)
  - "we published but bot never published in the group" (bot not in group or bot broken)
  - "bot published but not accept" (signaling logic / bot policy)
  - "accept observed but call not Active" (MOQ runtime/auth issues)

How to run:
```bash
cd ~/code/pika/worktrees/audio
cargo run -p pika_core --bin interop_openclaw_voice -- \
  npub1z6ujr8rad5zp9sr9w22rkxm0truulf2jntrks6rlwskhdmqsawpqmnjlcp
```

## Phase 2: Probe Relays (don’t guess)

Use the existing relay probe tool to see which relays reject protected events:
```bash
cd ~/code/pika/worktrees/audio

# kind 443: key package
cargo run -p pika_core --bin relay_probe -- wss://relay.damus.io --kind 443
cargo run -p pika_core --bin relay_probe -- wss://relay.primal.net --kind 443

# compare: unprotected publish
cargo run -p pika_core --bin relay_probe -- wss://relay.damus.io --kind 443 --unprotected
```

Notes:
- MDK historically marked key packages (kind 443) as NIP-70 protected, and many popular relays reject them.
- Welcomes + group messages are not expected to require NIP-70, so if *those* fail, it is likely relay policy unrelated to NIP-70 (or bot not subscribed / sidecar down).

Acceptance:
- We have an explicit "relay set" we trust for Step 3 (not vibes).

## Phase 3: Ensure Bot Actually Sees The Same Relays

If Phase 1 indicates we publish group messages but bot never publishes in the same chat:
- Expand streambot marmot plugin `relays` to include the relay set from Phase 2.
- Restart OpenClaw so it re-reads config.

Server checks:
```bash
ssh streambot 'ps aux | rg -n \"openclaw|marmotd\" || true'
ssh streambot 'journalctl -u openclaw-gateway -n 200 --no-pager'
ssh streambot 'jq -c \".plugins.entries.marmot.config.relays\" /home/openclaw/.openclaw/openclaw.json'
```

Acceptance:
- Phase 1 "bot published group message for this chat" becomes true.

## Phase 4: Split Signaling vs MOQ Runtime

If Phase 1 shows bot publishes in the chat but caller never reaches Active:
- If an accept is observed (either via app state or the observer), focus on MOQ:
  - surface the exact runtime error into the interop output (toast/state dump)
  - verify `call_moq_url=https://moq.justinmoon.com/anon`

Acceptance:
- `CallStatus::Active` reached + `tx_frames` increases.

## Optional: MDK Upgrade (Issue #168 line of attack)

MDK upstream added an option to make the key package NIP-70 protected tag optional (for relay compatibility).
If we upgrade MDK in Pika, key packages can be published unprotected, enabling use of "popular" relays and reducing relay split-brain.

Acceptance:
- `key_package_published ok=true` on Damus/Primal/nos.lol without needing special relays.
