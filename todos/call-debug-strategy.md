# Audio Call Debugging Strategy

## Diagnosis: What's Actually Working vs. What's Not

### The core problem: there is no real MOQ network transport

Every "phase complete" claim was validated against `InMemoryRelay` — an in-process `mpsc::channel` pub/sub in `crates/pika-media/src/session.rs`. Both sides of a call share the same relay via `shared_relay_for()` (a global `HashMap` in `call_runtime.rs`). No bytes ever hit the network.

**What exists:**
- ✅ Call signaling state machine (invite/accept/reject/end over MLS/Nostr)
- ✅ Opus encode/decode pipeline
- ✅ Jitter buffer
- ✅ MLS-derived frame encryption/decryption
- ✅ Replay window, opaque participant labels, relay auth tokens
- ✅ cpal audio capture/playback (iOS/Android only, behind `#[cfg]`)
- ✅ Synthetic audio backend for tests (220Hz tone generator)

**What's missing:**
- ❌ **No real MOQ/QUIC client** — `pika-media/Cargo.toml` has zero deps on `moq-lite`, `moq-native`, or `quinn`
- ❌ **No network `MediaSession`** — `session.rs` only has `InMemoryRelay`; `MediaSession::new()` creates a local relay
- ❌ **MOQ relay is crash-looping** — cert sync mismatch on hetzner (see below)
- ❌ **No CLI call commands** — `pika-cli` can't initiate/receive calls, so testing requires the iOS app

### Infrastructure blocker: MOQ relay down

`moq-relay.service` on hetzner (100.73.239.5) is failing 48,000+ restart cycles because:
```
Caddy certificate for moq.justinmoon.com is not available yet.
```

The sync cert script in `~/configs/hosts/hetzner/moq.nix` expects:
```
/var/lib/caddy/.local/share/caddy/certificates/acme-v02.api.letsencrypt.org-directory/moq.justinmoon.com/
```

But Caddy is using a **wildcard cert** stored at:
```
/var/lib/caddy/.local/share/caddy/certificates/acme-v02.api.letsencrypt.org-directory/wildcard_.justinmoon.com/
```

**Fix:** Update `moq.nix` to source from the wildcard cert path, or add a specific Caddy site block for `moq.justinmoon.com` so it gets its own cert. This is a 1-line change in `~/configs`.

---

## Debugging Ladder: 6 Layers from Local to Real E2E

Each layer validates one new thing. Don't move to the next layer until the previous one works. Fast iteration = no iPhone required until Layer 5.

### Layer 0: Fix MOQ relay infra
**What:** Get `moq-relay.service` running on hetzner.
**How:** Fix the cert path in `~/configs/hosts/hetzner/moq.nix` to use the wildcard cert. Redeploy via `~/configs`. Verify:
```bash
ssh justin@100.73.239.5 "sudo systemctl status moq-relay"
```
**Validates:** The relay process starts and binds UDP/443.

### Layer 1: Bare QUIC connectivity probe
**What:** A minimal Rust binary that does a QUIC handshake with `moq.justinmoon.com:443`.
**How:** Add `crates/pika-media/examples/moq_probe.rs` (or a bin in `rust/src/bin/`). Use `quinn` directly:
```rust
// Pseudo-code
let endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
let conn = endpoint.connect(addr, "moq.justinmoon.com")?.await?;
println!("QUIC connected: {}", conn.remote_address());
conn.close(0u32.into(), b"probe");
```
**Validates:** TLS certs work, QUIC is reachable, firewall rules are OK.
**Iteration speed:** `cargo run --bin moq_probe` — seconds.

### Layer 2: Real MOQ pub/sub (no MLS, no Nostr)
**What:** Build a `NetworkRelay` transport backend for `pika-media` that uses `moq-native` to talk to a real relay. Run two processes on the same machine: one publishes synthetic Opus frames, the other subscribes and counts them.
**How:**
1. Add `moq-lite` + `moq-native` deps to `pika-media/Cargo.toml` (behind a `network` feature flag to keep the crate light for unit tests).
2. Implement a `NetworkSession` that wraps `moq-native::Session` and implements the same `publish`/`subscribe` interface as `InMemoryRelay` but over QUIC.
3. Write `crates/pika-media/examples/moq_pubsub.rs`:
   ```bash
   # Terminal 1: publish 50 synthetic Opus frames
   cargo run --example moq_pubsub -- publish --relay https://moq.justinmoon.com/anon --track test/audio0
   
   # Terminal 2: subscribe and count
   cargo run --example moq_pubsub -- subscribe --relay https://moq.justinmoon.com/anon --track test/audio0
   ```
**Validates:** Frames transit the real relay, ordering is preserved, no silent drops.
**Iteration speed:** `cargo run` — seconds. No app, no MLS, no identity.

### Layer 3: CLI-to-CLI calls (full stack, no app)
**What:** Add `StartCall`, `AcceptCall`, `EndCall` subcommands to `pika-cli`. Run two pika-cli instances with different identities, do a full call through real Nostr relays + real MOQ relay.
**How:**
1. Add call subcommands to `cli/src/main.rs`:
   - `pika-cli call-start --group <hex>` — sends `call.invite`, starts media worker
   - `pika-cli call-accept --group <hex>` — accepts incoming invite, starts media worker
   - `pika-cli call-listen --group <hex> --timeout 30` — waits for invite, auto-accepts
2. Wire `CallRuntime` with the new `NetworkSession` instead of `InMemoryRelay`.
3. Test script:
   ```bash
   # Term 1: Alice (test key) listens
   PIKA_TEST_NSEC=$(<.env grep PIKA_TEST_NSEC | cut -d= -f2)
   pika-cli --state-dir .alice --relay wss://relay.damus.io call-listen --group $GID --timeout 60
   
   # Term 2: Bob (another key) starts call
   pika-cli --state-dir .bob --relay wss://relay.damus.io call-start --group $GID
   ```
4. Both sides print `tx_frames` / `rx_frames` stats every second. Assert rx_frames > 0 on both.

**Validates:** Full signaling + key derivation + encrypted frame transport over real networks. Both directions. Same machine but cross-process.
**Iteration speed:** `cargo build -p pika-cli && ./target/debug/pika-cli ...` — seconds.

### Layer 4: CLI-to-bot calls (cross-machine)
**What:** pika-cli on your Mac calls the deployed openclaw-marmot bot on streambot.
**How:**
1. Update `openclaw-marmot/marmotd` to use `NetworkSession` (same as Layer 3).
2. Deploy updated marmotd to streambot.
3. Run:
   ```bash
   pika-cli --state-dir .test --relay wss://relay.damus.io call-start --group $BOT_GROUP
   ```
4. Bot should echo audio back (Phase 3 echo mode). Verify rx_frames > 0.

**Validates:** Cross-machine, cross-process, real relay. The actual deployed topology.
**Iteration speed:** CLI rebuild + run — seconds. Bot redeploy is slower but only needed once per code change to marmotd.

### Layer 5: iPhone-to-bot (the real thing)
**What:** Only after Layers 0–4 pass, test with the iOS app on a real device.
**How:** Build and install the iOS app. Call the bot. Listen to audio.
**What's new at this layer:** cpal on iOS (real mic + speaker), AVAudioSession, background audio.

This is the only layer that requires the iPhone. By this point, you already know:
- MOQ relay works (Layer 1–2)
- Signaling + crypto + frame transport works cross-process (Layer 3–4)
- The only new variable is iOS platform audio

---

## Recommended Execution Order

```
 Phase       Effort    Dependency
 ─────       ──────    ──────────
 Layer 0     ~30 min   Fix ~/configs moq.nix cert path, redeploy
 Layer 1     ~1 hour   quinn dep + probe binary
 Layer 2     ~1-2 days moq-lite/moq-native integration in pika-media
 Layer 3     ~1 day    pika-cli call commands + wire NetworkSession
 Layer 4     ~half day Deploy updated marmotd, run CLI→bot test
 Layer 5     ~half day iOS build + manual test
```

Layer 2 is the big one — it's the actual missing piece. Everything else is plumbing around it.

## Architecture for the Network Transport

The cleanest approach: make `MediaSession` generic over a transport trait, keeping `InMemoryRelay` for tests.

```rust
// crates/pika-media/src/transport.rs

pub trait MediaTransport: Send + Sync {
    fn connect(&mut self) -> Result<(), MediaSessionError>;
    fn disconnect(&mut self);
    fn publish(&self, track: &TrackAddress, frame: MediaFrame) -> Result<usize, MediaSessionError>;
    fn subscribe(&self, track: &TrackAddress) -> Result<Receiver<MediaFrame>, MediaSessionError>;
}

// InMemoryTransport — existing code, wraps InMemoryRelay
// NetworkTransport  — new, wraps moq-native::Session (behind `network` feature)
```

`call_runtime.rs` currently calls `shared_relay_for()` to get an `InMemoryRelay`. Change it to accept a `Box<dyn MediaTransport>` and select the impl based on config:
- Tests / synthetic mode → `InMemoryTransport`
- Real calls → `NetworkTransport`

## Quick Wins for Immediate Progress

1. **Fix the MOQ relay cert** (Layer 0) — unblocks everything downstream.
2. **Write the QUIC probe** (Layer 1) — confirms infra works, 30 min of code.
3. **Scope the `moq-native` integration** — look at `moq-native`'s API surface. How does it do pub/sub? What's the session lifecycle? This determines Layer 2 effort.

```bash
# Check if moq crates are available
cargo search moq-lite
cargo search moq-native
# Or look at the upstream repo referenced in ~/configs/flake.nix:
# github:kixelated/moq
```
