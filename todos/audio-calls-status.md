# Audio Calls: Current Status vs Spec

Reference: `todos/pika-audio-calls-spec.md`

## Executive Summary

The spec claims Phases 0-8 are complete. In reality, Phases 0-8 were only validated against an **in-memory relay** (`InMemoryRelay`) -- a process-local `mpsc::channel` where no bytes ever hit the network. Real MOQ network transport has now been implemented and the infrastructure is deployed, but **no end-to-end call has been tested through the real relay with actual STT/LLM/TTS invocation**.

## What Actually Works (Validated)

### Transport Layer (new work, validated against real relay)

- **MOQ relay** (`moq.justinmoon.com:443`): Running on hetzner, TLS certs fixed, QUIC/UDP/443 listening.
- **NetworkRelay** (`crates/pika-media/src/network.rs`): Bridges async moq-native QUIC client to sync `mpsc::Receiver<MediaFrame>` interface. Behind `network` feature flag.
- **Pub/sub through real relay**: `network_relay_test` example -- 21/21 frames delivered through `moq.justinmoon.com`.
- **Full-duplex encrypted audio through real relay**: `duplex_test` example -- two parties (Alice/Bob) each publish+subscribe with frame encryption/decryption. ~98% frame delivery over 10 seconds. 1 crypto error per side (expected: warmup frames before keys established).
- **Auto-selection**: `call_runtime.rs` uses `MediaTransport` enum -- `NetworkRelay` for `https://` URLs, `InMemoryRelay` for `ws://` test URLs. Existing tests unchanged.

### Bot Deployment (deployed, not call-tested)

- **marmotd on streambot**: Deployed with NetworkRelay support. Sidecar starts in ~1s, connects to relay.primal.net, processes incoming Nostr events.
- **CryptoProvider fix**: Both `ring` (nostr-sdk) and `aws-lc-rs` (quinn/moq-native) in dep tree. Explicitly install `ring` as default rustls CryptoProvider at startup.
- **CallMediaTransport in daemon.rs**: `publish_tts_audio_response` and `start_stt_worker` auto-select NetworkRelay for `https://` moq_url, InMemoryRelay for tests.
- **Infra**: `~/code/infra` flake.nix points at `openclaw-marmot/audio-transport` branch. outputHashes for moq-lite, moq-native, pika-media added to streambot.nix.

### Pre-existing (in-memory only, per previous agent)

- Call signaling state machine (invite/accept/reject/end over MLS/Nostr)
- Opus encode/decode pipeline
- Jitter buffer
- MLS-derived frame encryption/decryption
- cpal audio capture/playback (iOS/Android, behind `#[cfg]`)
- Bot echo flow (Phase 3) -- InMemoryRelay only
- Bot STT pipeline (Phase 4) -- InMemoryRelay only
- Bot TTS response (Phase 8) -- InMemoryRelay only

## What Has NOT Been Tested

### Critical gaps

1. **No real call has gone through the MOQ relay.** The duplex_test proves the transport works, but it's two `NetworkRelay` instances in the same process publishing/subscribing synthetic encrypted payloads. No MLS signaling, no Nostr, no call_runtime.rs, no bot.

2. **No STT/LLM/TTS invocation over network transport.** The bot's STT and TTS pipelines were only tested with InMemoryRelay. Nobody has verified that audio frames arrive at the bot via NetworkRelay, get decoded, transcribed, sent to an LLM, synthesized back to speech, and published back.

3. **No cross-process call.** Both sides of every test run in the same process. There is no test where process A publishes and process B subscribes through the real relay with call signaling.

4. **No iOS app call through real relay.** The iOS app has never made a call that goes through `moq.justinmoon.com`. The `call_moq_url` config value needs to be set to `https://moq.justinmoon.com/anon` and tested on a real device.

5. **No pika-cli call commands.** `pika-cli` only handles messaging (invite, send, listen). It cannot initiate or receive calls, so automated CLI-to-bot call testing is not possible without adding call subcommands.

## Phase-by-Phase Reality Check

| Phase | Spec Status | Actual Status |
|-------|------------|---------------|
| 0. Control scaffold | Complete | **Real.** Signaling works over MLS/Nostr. |
| 1. MOQ media plumbing | Complete | **Partially real.** In-memory session + Opus path tested. NetworkRelay implemented and validated for pub/sub, but not wired into a real call flow. |
| 2. Push-to-talk audio | Partial | **Same.** Rust audio backend implemented, no real device QA. |
| 3. Bot echo | Complete | **In-memory only.** Echo flow works with InMemoryRelay. Not tested over real MOQ relay. |
| 4. Bot STT -> text | Complete | **In-memory only.** STT pipeline works with InMemoryRelay. Not tested with real audio arriving over network. |
| 5. Full duplex audio | Complete | **Transport proven, integration not.** duplex_test shows bidirectional encrypted frames work through real relay. But no actual call with signaling + call_runtime + jitter buffer has gone through the real relay. |
| 6. Encryption hardening | Complete | **Real.** Frame encryption/decryption works. Validated in duplex_test over real relay. |
| 7. Android port | Complete | **Assumed real** (compiles, passes CI). Not verified with real calls. |
| 8. Bot full duplex voice | Complete (in-memory) | **In-memory only.** STT->LLM->TTS->Opus pipeline wired but only tested with InMemoryRelay. |

## What Needs to Happen Next

### Minimum to prove the feature works end-to-end

1. **Test from iOS app to bot over real relay.** Set `call_moq_url: "https://moq.justinmoon.com/anon"` in pika config. Initiate a call from the iOS app to the bot. Verify:
   - Bot receives call invite via Nostr/MLS
   - Bot subscribes to caller's audio via NetworkRelay
   - STT receives frames and produces transcript
   - LLM generates response
   - TTS synthesizes audio
   - Bot publishes response via NetworkRelay
   - Caller hears audio response

2. **Or: add call commands to pika-cli** for scriptable testing without the iOS app. This would allow `pika-cli call-start --group $GID` → bot accepts → verify audio flows both directions.

### Before merging to master

- openclaw-marmot `audio-transport` branch should be validated with a real call before merging to master
- infra flake.nix branch pin (`/audio-transport`) should revert to default once merged

## Files Changed

### pika (audio branch)

| File | What |
|------|------|
| `crates/pika-media/Cargo.toml` | Added moq-native, moq-lite, tokio, bytes, url behind `network` feature |
| `crates/pika-media/src/network.rs` | `NetworkRelay` -- async QUIC to sync mpsc bridge |
| `crates/pika-media/examples/network_relay_test.rs` | Pub/sub test against real relay |
| `crates/pika-media/examples/duplex_test.rs` | Full-duplex encrypted audio test against real relay |
| `rust/src/core/call_runtime.rs` | `MediaTransport` enum, auto-select based on moq_url scheme |

### openclaw-marmot (audio-transport branch)

| File | What |
|------|------|
| `marmotd/Cargo.toml` | pika-media git dep (audio branch) + rustls dep |
| `marmotd/src/main.rs` | rustls CryptoProvider init (ring/aws-lc-rs conflict fix) |
| `marmotd/src/daemon.rs` | `CallMediaTransport` enum, `_with_transport` variants, auto-select |

### infra

| File | What |
|------|------|
| `flake.nix` | openclawMarmotSrc points at `audio-transport` branch |
| `flake.lock` | Updated to latest audio-transport rev |
| `nix/hosts/streambot.nix` | outputHashes for moq-lite, moq-native, pika-media |
