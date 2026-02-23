---
summary: Intent and reasoning for consolidating marmotd into pikachat as a single agent-native CLI
read_when:
  - working on pikachat daemon mode or marmotd consolidation
  - making decisions about CLI naming, packaging, or architecture
  - evaluating how agents and bots interact with pika messaging
---

# Fold marmotd into pikachat

**Issue:** [#226](https://github.com/sledtools/pika/issues/226)
**Status:** Open

## Summary

Consolidate `marmotd` (the long-running JSONL sidecar daemon) into **`pikachat`** (renamed from `pika-cli`) so that a single binary serves all non-GUI messaging needs: one-shot CLI commands, persistent daemon mode, and remote CLI usage against a running daemon.

## Problem

Today there are two separate Rust binaries for non-GUI Pika messaging:

- **pikachat** (`cli/`, currently shipped as `pika-cli`) -- A stateless, one-shot CLI for sending messages, managing identity, and testing. Each invocation connects to relays, does its work, and exits. No background state synchronization.
- **marmotd** (`crates/marmotd/`) -- A long-running daemon that speaks JSONL over stdio. It maintains persistent relay connections, processes incoming welcomes and messages in real-time, and is the integration surface for the OpenClaw marmot plugin (which spawns it as a sidecar process).

This split creates several problems:

1. **Duplicated core logic.** Both binaries implement the same MLS/Nostr workflows (identity management, key package publishing, group creation, message send/receive) with slightly different wiring. Keeping them in sync is ongoing maintenance burden.

2. **Confusing surface area for agents.** An agent or bot developer has to understand two tools with different interaction models. pikachat (today: `pika-cli`) is approachable but can't stay connected; marmotd can stay connected but requires JSONL piping and has no standalone CLI UX.

3. **Naming fragmentation.** The project has "pika" (the app), "marmot" (the protocol), "marmotd" (the daemon), and "marmot" again (the OpenClaw npm plugin). Users encounter multiple names for what is conceptually one messaging system.

4. **Catch-up problem for one-shot CLI.** A stateless CLI that connects, sends, and disconnects can miss messages that arrived while it was offline. For reliable agent use, you almost always want a daemon running. Making the daemon a first-class mode of the same binary removes the friction of figuring this out.

## Intent

Make **`pikachat`** the single agent-native CLI for Pika messaging. It should operate in three modes from one binary:

### 1. Standalone CLI (current one-shot CLI behavior)

One-shot commands that connect, act, and exit. Good for scripting, quick sends, CI smoke tests. This is the default mode and already exists.

### 2. Daemon mode (`pikachat daemon`)

A `daemon` subcommand that does what `marmotd daemon` currently does: run as a long-lived process, maintain relay connections, process inbound welcomes/messages, and accept commands over JSONL stdio. This is the mode that OpenClaw (and any persistent agent) uses.

### 3. Remote CLI (future)

A mode where `pikachat` commands talk to an already-running daemon instance instead of connecting to relays directly. This lets agents keep a single persistent daemon and send occasional commands through the same CLI without each command needing its own relay connection and catch-up cycle. Today the daemon speaks a simple JSONL request/response protocol over stdio (with optional `request_id`); the remote CLI would reuse the same message types over a local transport (Unix socket / named pipe / TCP localhost).

## Why consolidate rather than keep separate

- **Single core, thin wrappers.** The MLS/Nostr logic is identical across modes. Having one binary with mode selection means one place to maintain, test, and release that logic.
- **Agent ergonomics.** An agent that starts with `pikachat send --to npub1... --content "hello"` can graduate to `pikachat daemon` when it needs persistence, without switching tools or learning a new protocol.
- **Simplified releases.** One binary to build, distribute, and version instead of two.
- **Default-to-daemon may be the right default.** The daemon mode is more correct for most use cases because it stays synced. The standalone mode is an optimization (useful, but secondary). Putting both in one binary lets us consider making daemon the default behavior, with a `--detached` flag for one-shot use.

## Naming and packaging

The consolidation is also an opportunity to unify naming:

- **CLI binary (decision):** `pikachat` (not `pikachat-cli`). `pika-cli` becomes legacy/renamed.
- **npm packages:** `pikachat` for the core npm package, `pikachat-openclaw` for the OpenClaw plugin. The `pikachat` name is secured on npm. This replaces the current `@justinmoon/marmot` plugin name.
- **marmotd binary:** Deprecated once `pikachat daemon` reaches parity. Per Tony: no in-place migration; treat this as a clean break.

The constraint on OpenClaw plugin naming is that it must not be called `openclaw-pika` due to OpenClaw's plugin name parsing conflicts.

**OpenClaw channel ID (decision):** `"pikachat"` (renamed from `"marmot"`). All OpenClaw config moves to `channels.pikachat.*` and reload prefixes become `reload.configPrefixes: ["channels.pikachat", "plugins.entries.pikachat"]`. This is a breaking config change and is intentionally **not** auto-migrated.

### Renaming decision: `marmot*` → `pikachat*` everywhere

Tony’s direction: **rename everything**; “marmot” should disappear from user-facing surfaces.

| Current name (today) | Target name (post-consolidation) |
|---|---|
| Binary `marmotd` | `pikachat daemon` (single consolidated binary) |
| Binary/crate `pika-cli` | Binary/crate `pikachat` |
| OpenClaw channel id `marmot` | `pikachat` |
| OpenClaw npm package `@justinmoon/marmot` | `pikachat-openclaw` |
| OpenClaw config prefix `channels.marmot.*` | `channels.pikachat.*` |
| Env vars `MARMOT_*` | `PIKACHAT_*` |
| Tool cache `~/.openclaw/tools/marmot/...` | `~/.openclaw/tools/pikachat/...` |

Notes:
- This does **not** imply renaming protocol namespaces like `pika.call` (those are part of the app/core protocol).
- The sections below document current code paths (which still use the old names) but the migration plan assumes the target naming.

## Smart send UX

A key UX goal carried forward from the existing CLI design is "smart send" -- the ability to message someone by npub without knowing group IDs:

```
pikachat send --to npub1xyz... --content "hey!"
```

This already works in `pika-cli` today (and will remain the primary `pikachat` UX): it looks up an existing 1:1 DM with the peer, or auto-creates one (fetching key package, creating group, sending welcome, then sending the message).

## What this does NOT change

- **The Pika iOS/Android apps.** They continue to use the Rust core directly via UniFFI/JNI. The CLI consolidation is purely about the non-GUI tools.
- **The Marmot protocol (MDK).** The underlying MLS library is unaffected.
- **The OpenClaw plugin architecture.** The plugin still spawns a sidecar binary; it just spawns `pikachat daemon` instead of `marmotd daemon`.
- **The JSONL protocol.** The daemon's stdin/stdout protocol stays the same. OpenClaw plugin compatibility is preserved.

---

## Research log (running)

This section is intentionally “notes-first”: it accumulates facts + constraints discovered while exploring the repo.

### 2026-02-22 — initial repo scan

Commands/files consulted:

- `./scripts/agent-brief` (high-level repo context + just recipes)
- `docs/pika-cli-design.md` (current pika-cli UX + relay defaults)
- `docs/release.md` (release pipelines; especially marmotd)
- `cli/src/main.rs` (current pika-cli surface)
- `crates/marmotd/src/main.rs`, `crates/marmotd/src/daemon.rs` (daemon CLI + JSONL protocol)
- `pikachat-openclaw/openclaw/extensions/pikachat/src/{channel.ts,sidecar.ts,sidecar-install.ts,config.ts}` (OpenClaw integration surface, sidecar lifecycle, auto-update)

Key takeaway: **marmotd is not just “message sync”** — its daemon JSONL surface includes **group init**, **typing indicators**, and a **call/audio control plane** used by the OpenClaw channel.

### 2026-02-22 -- second pass: completeness audit

Additional files consulted (beyond initial scan):

- `crates/marmotd/src/call_audio.rs` (Opus-to-WAV pipeline, silence segmentation for STT)
- `crates/marmotd/src/call_tts.rs` (OpenAI TTS synthesis, WAV decode, fixture tone)
- `pikachat-openclaw/openclaw/extensions/pikachat/src/types.ts` (multi-account resolution)
- `pikachat-openclaw/openclaw/extensions/pikachat/src/runtime.ts` (singleton plugin runtime)
- `pikachat-openclaw/openclaw/extensions/pikachat/index.ts` (plugin entrypoint; binds `configSchema`)
- `pikachat-openclaw/openclaw/extensions/pikachat/src/config-schema.ts` (schema exported at runtime via `index.ts`; incomplete vs actual runtime config reads)
- `pikachat-openclaw/openclaw/extensions/pikachat/openclaw.plugin.json` (static plugin manifest + schema; differs from runtime-exported schema)
- `rust/tests/e2e_local_pikachat_daemon_call.rs` (full FfiApp<->marmotd E2E call test)
- `.github/workflows/pikachat-release.yml` (complete CI/release pipeline)
- `scripts/bump-pikachat.sh` (version bump script)
- `Cargo.toml` (workspace-level: MDK pinning, member list)
- All scenario scripts in `pikachat-openclaw/scripts/`

New findings captured:
- Full file inventory for refactor impact (see below)
- Dependency differences table between pika-cli and marmotd
- Complete environment variable inventory for both marmotd and OpenClaw plugin
- OpenClaw config type split (typed `MarmotChannelConfig` vs ad-hoc reads for `owner`, `dmGroups`, `memberNames`)
- Detailed OpenClaw sidecar spawn sequence (command/args/version resolution)
- marmotd's `bot`, `init`, `scenario` subcommands documented
- Audio pipeline modules (`call_audio.rs`, `call_tts.rs`) documented
- E2E test binary resolution path documented
- Phase 4 scenario (full OpenClaw gateway integration) documented
- Detailed 6-phase implementation plan for Option B
- Decisions/constraints captured (see bottom)

### 2026-02-22 — third pass: integration + rename surface audit

Additional files consulted:

- `pikachat-openclaw/openclaw/extensions/pikachat/src/sidecar-install.test.ts` (installer behavior and version-compat tests)
- `pikachat-openclaw/openclaw/extensions/pikachat/src/channel.ts` (voice pipeline: `call_audio_chunk` → STT → agent → TTS)
- `tools/{pika-e2e-bot,cli-smoke,ui-e2e-local}` + `tools/lib/local-e2e-fixture.sh` (scripts that shell out to `pika-cli` + assume `.pika-cli` state)
- `.gitignore` (tracks `.pika-cli` and `.pika-cli-test-nsec`)

New findings captured:

- `call_audio_chunk` is **consumed by the OpenClaw plugin** for STT; the daemon does not emit transcripts.
- OpenClaw plugin config schema is split across **two** sources (`index.ts`/`config-schema.ts` vs `openclaw.plugin.json`) and neither matches full runtime behavior.
- `docs/release.md` (and `pikachat-openclaw/todos/ship-marmot.md`) reference `@openclaw/marmot`, but this repo’s plugin package is `@justinmoon/marmot`.
- There are additional internal scripts that must be updated when `pika-cli` is renamed to `pikachat`.

---

## Comprehensive file inventory (for refactor impact)

All files that will be touched or must be understood during the consolidation:

### Rust source

| File | Role |
|---|---|
| `crates/marmotd/src/main.rs` | Binary entry: CLI parser (`Cli` struct), `daemon`, `bot`, `init`, `scenario` subcommands, shared helpers (`load_or_create_keys`, `new_mdk`, `connect_client`, `publish_and_confirm`, etc.) |
| `crates/marmotd/src/daemon.rs` | Daemon engine: JSONL protocol types (`InCmd`/`OutMsg`), `daemon_main` loop, call signaling, media crypto, echo/audio-chunk (“stt”) workers, `run_audio_echo_smoke` |
| `crates/marmotd/src/call_audio.rs` | `OpusToAudioPipeline` + `SilenceSegmenter` — decodes Opus to PCM, segments on silence, emits WAV chunks for downstream STT (OpenClaw plugin) |
| `crates/marmotd/src/call_tts.rs` | `synthesize_tts_pcm` — OpenAI TTS (or fixture tone), WAV decode helpers |
| `crates/marmotd/Cargo.toml` | Deps: `pika-media` (with `network` feature), `rustls` (ring), `reqwest` (blocking), `hound`, edition 2024 |
| `cli/src/main.rs` | pika-cli binary: one-shot commands, `listen` tail mode |
| `cli/src/mdk_util.rs` | `load_or_create_keys`, `open_mdk` — shared helpers (duplicated from marmotd) |
| `cli/src/relay_util.rs` | `connect_client`, `publish_and_confirm`, `fetch_latest_key_package` — shared helpers (duplicated from marmotd) |
| `cli/Cargo.toml` | Deps: `nostr-blossom` (profile pic upload), no `pika-media`, no `rustls` provider, edition 2021 |

### OpenClaw plugin (TypeScript)

| File | Role |
|---|---|
| `pikachat-openclaw/openclaw/extensions/pikachat/index.ts` | Plugin entrypoint: exports plugin metadata and binds `configSchema` (currently from `src/config-schema.ts`) |
| `pikachat-openclaw/openclaw/extensions/pikachat/src/channel.ts` | Main plugin: sidecar lifecycle, event dispatch, group/DM routing, mention detection, **voice pipeline** (`call_audio_chunk` → STT → agent → TTS), Nostr profile resolution, sqlite direct reads |
| `pikachat-openclaw/openclaw/extensions/pikachat/src/sidecar.ts` | `MarmotSidecar` class: process spawn, JSONL request/response, event handler, TS types for all sidecar messages |
| `pikachat-openclaw/openclaw/extensions/pikachat/src/sidecar-install.ts` | Auto-install/update: GitHub releases, platform detection, SHA256 verification, version caching, patch-only update policy |
| `pikachat-openclaw/openclaw/extensions/pikachat/src/sidecar-install.test.ts` | Installer tests: version parsing + compatibility gate behavior |
| `pikachat-openclaw/openclaw/extensions/pikachat/src/config.ts` | `MarmotChannelConfig` type + resolver (typed subset — does NOT include `owner`, `dmGroups`, `memberNames`) |
| `pikachat-openclaw/openclaw/extensions/pikachat/src/config-schema.ts` | JSON Schema exported at runtime via `index.ts` (`configSchema`); currently incomplete vs runtime config reads |
| `pikachat-openclaw/openclaw/extensions/pikachat/src/types.ts` | Multi-account resolution: `ResolvedMarmotAccount`, `resolveMarmotAccount` |
| `pikachat-openclaw/openclaw/extensions/pikachat/src/runtime.ts` | Singleton plugin runtime holder |
| `pikachat-openclaw/openclaw/extensions/pikachat/openclaw.plugin.json` | Static plugin manifest + config schema; includes `owner`, `dmGroups`, `memberNames`, `ignorePubkeys`, per-group `requireMention` (but does **not** include `sidecarVersion`, `accounts`, or per-group `users`/`systemPrompt`) |
| `pikachat-openclaw/openclaw/extensions/pikachat/package.json` | npm package: `@justinmoon/marmot` v0.5.2 |

### CI / release / scripts

| File | Role |
|---|---|
| `.github/workflows/pikachat-release.yml` | Release workflow: tag validation, cross-platform builds, GitHub release + npm publish |
| `scripts/bump-pikachat.sh` | Version bump: `crates/marmotd/Cargo.toml` + `package.json`, cargo check, git commit + tag |
| `justfile` | Recipes: `pre-merge-marmotd`, `nightly-marmotd`, `e2e-local-marmotd`, `openclaw-pikachat-scenarios` |
| `pikachat-openclaw/scripts/phase1.sh` | Scenario: Rust<->Rust invite+chat |
| `pikachat-openclaw/scripts/phase2.sh` | Scenario: Rust harness<->Rust bot |
| `pikachat-openclaw/scripts/phase3.sh` | Scenario: Rust harness<->JSONL daemon |
| `pikachat-openclaw/scripts/phase3_audio.sh` | Scenario: audio echo smoke |
| `pikachat-openclaw/scripts/phase4_openclaw_pikachat.sh` | Scenario: full OpenClaw gateway + sidecar integration test |

### Test harnesses

| File | Role |
|---|---|
| `rust/tests/e2e_local_pikachat_daemon_call.rs` | E2E call test: FfiApp caller <-> marmotd daemon. Resolves binary via `MARMOTD_BIN` env or `target/debug/marmotd`. Contains embedded local Nostr relay. |

### Repo tools / scripts impacted by rename (pika-cli → pikachat)

These aren’t part of the OpenClaw sidecar contract, but they *will* break if we rename the `pika-cli` binary and/or the `.pika-cli` default state directory.

| File | What it assumes today |
|---|---|
| `scripts/agent-brief` | Runs `cargo run -p pika-cli -- --help` and references “pika-cli” in output |
| `tools/pika-e2e-bot` | Shells out to `cargo run -p pika-cli -- listen ...` and expects JSONL `{type: "message"|"welcome"}` lines |
| `tools/ui-e2e-local` | Boots local stack and relies on `tools/pika-e2e-bot` / `pika-cli` listen behavior |
| `tools/cli-smoke` | Smokes `cargo run -p pika-cli -- ...` across key commands |
| `tools/lib/local-e2e-fixture.sh` | Sets up test identity/state and assumes `.pika-cli` paths |
| `.gitignore` | Ignores `.pika-cli/` and `.pika-cli-test-nsec` |

---

## Current state (as implemented)

### Workspace layout & dependency pinning

From the workspace `Cargo.toml`:

- `pika-cli` and `marmotd` are both workspace members.
- MDK crates are pinned at the workspace level to avoid state-format skew:
  - `mdk-core`, `mdk-sqlite-storage`, `mdk-storage-traits` (git rev pinned).

Refactor implication: daemon/CLI consolidation should continue to use **workspace MDK dependencies** so that `pikachat daemon` (today: `pika-cli`), the app core, and any remaining compatibility binaries all share the exact same MDK version.

### Binaries / crates involved

1. **`pika-cli`** (`cli/`)
   - One-shot commands + a “tail” style `listen` command (runs for `--timeout`, prints JSON lines).
   - Default state dir: `.pika-cli`.
   - Default relay sets are embedded in `cli/src/main.rs` (matching `rust/src/core/config.rs`).

2. **`marmotd`** (`crates/marmotd/`)
   - Multi-purpose harness binary. Relevant here: `marmotd daemon` is the **long-running JSONL sidecar** used by OpenClaw.
   - Default state dir: `.state/marmotd` (but OpenClaw overrides this; see below).
   - Writes/reads:
     - `identity.json` (plaintext hex secret + pubkey)
     - `mdk.sqlite` (MLS state; unencrypted)

   Notes from `crates/marmotd/Cargo.toml`:
   - Edition: **2024**
   - Depends on `pika-media` with `network` feature (call/media layer)
   - Uses `rustls` with `ring` feature and explicitly installs ring provider in `main` (to avoid provider ambiguity with other crypto deps)
   - Depends on `reqwest` with `blocking` feature (for TTS synthesis on a std::thread)
   - Depends on `hound` (WAV encode/decode for audio pipeline)

   Additional subcommands beyond `daemon`:
   - **`bot`**: Deterministic Rust bot fixture for E2E testing. Publishes a key package, waits for a welcome, joins the group, waits for a prompt matching `openclaw: reply exactly "..."` or `ping`, sends the prescribed reply, and exits. Used by `phase2` scenario.
   - **`init`**: Import an nsec into identity.json. Writes `identity.json` to `--state-dir`. Warns if existing identity has different key or if `mdk.sqlite` exists.
   - **`scenario`**: Test harness subcommands (`invite-and-chat`, `invite-and-chat-rust-bot`, `invite-and-chat-daemon`, `invite-and-chat-peer`, `audio-echo`). These orchestrate multi-party MLS test flows.

   Internal modules:
   - **`call_audio.rs`**: `OpusToAudioPipeline` wrapping `SilenceSegmenter` — decodes inbound Opus frames to PCM, detects silence boundaries (configurable via `MARMOT_SILENCE_RMS_THRESHOLD` env var, default 500.0), emits WAV chunks for STT transcription. Constants: `SILENCE_DURATION_MS=700`, `MAX_CHUNK_MS=20000`, `MIN_CHUNK_MS=500`.
   - **`call_tts.rs`**: `synthesize_tts_pcm` — calls OpenAI TTS API (or generates a 440Hz fixture tone when `MARMOT_TTS_FIXTURE=1`). Returns `TtsPcm { sample_rate_hz, channels, pcm_i16 }`. WAV decode handles both normal and streaming WAVs (OpenAI sets `data_chunk_size=0xFFFFFFFF`).

3. **OpenClaw Marmot plugin** (`pikachat-openclaw/openclaw/extensions/pikachat/`)
   - TypeScript channel plugin that:
     - spawns the sidecar (`marmotd daemon` today)
     - speaks JSONL over stdin/stdout
     - **auto-installs/updates** the sidecar binary from GitHub releases

---

## marmotd daemon: exact JSONL protocol + behavior

Source of truth:
- Rust: `crates/marmotd/src/daemon.rs`
- TS types mirrored in: `pikachat-openclaw/openclaw/extensions/pikachat/src/sidecar.ts`

### Transport

- Sidecar is a process with:
  - **stdin**: JSON objects, one per line (commands)
  - **stdout**: JSON objects, one per line (events + request responses)
  - **stderr**: debug logs (OpenClaw forwards to its logger)

### Startup

On start, sidecar emits a single `ready` line:

```json
{ "type": "ready", "protocol_version": 1, "pubkey": "<hex>", "npub": "npub1..." }
```

Then it:
- runs a primary-relay readiness check (90s) against the first `--relay` arg (default `ws://127.0.0.1:18080`)
- connects to relays (initial `--relay` args; `set_relays` and `publish_keypackage` may expand/replace the in-memory relay list)
- subscribes to:
  - GiftWrap welcomes (kind 1059) addressed to it via **recipient `p` tag** filter (`since = now - giftwrap_lookback_sec`, default 3 days; `limit=200`)
  - all existing groups from `mdk.sqlite` (restart-safe)

### Allowlist / sender filtering

Daemon supports `--allow-pubkey <hex>` repeatable.

- If allowlist is empty: **open mode** (accept all senders) and daemon prints a warning.
- Filtering is enforced on:
  - welcome senders
  - decrypted group messages

Implementation detail: pubkeys from `--allow-pubkey` are normalized by `trim().to_lowercase()`, so comparisons are case-insensitive.

OpenClaw currently does **not** pass `--allow-pubkey`; it does its own filtering in TS after receiving events.

### Request/response shape

- Commands include optional `request_id`.
- Daemon replies with `ok`/`error` for most commands; `request_id` is only included in the response if it was provided on the request (and `result` is omitted if null).

Additionally, daemon emits “unsolicited” event messages (`welcome_received`, `message_received`, etc.) for real-time updates.

### Commands (stdin) — current set

All commands are JSON objects with `cmd` in `snake_case`.

| Command | Purpose | Notes |
|---|---|---|
| `publish_keypackage` | Publish kind 443 key package | Strips NIP-70 `protected` tag for broad relay compatibility; also emits `keypackage_published` event |
| `set_relays` | Update relay list | Adds relays to nostr-sdk client and connects |
| `list_pending_welcomes` | List staged welcomes | Reads from MDK pending store |
| `accept_welcome` | Accept staged welcome and join group | Also subscribes to group messages + backfills recent group backlog (last 1h, limit 200, primary relay only) |
| `list_groups` | List known groups | Uses MDK groups |
| `send_message` | Send chat message to a group | Wraps MLS, strips `protected`, publishes to relays |
| `send_typing` | Send a short-lived typing indicator | Custom kind `20067` with `expiration` (~10s) and `d=pika`; best-effort publish |
| `init_group` | Create a DM/group with a peer + send welcome(s) | Fetches up to 10 peer key packages from relays; skips invalid ones |
| `accept_call` / `reject_call` / `end_call` | Call control plane | Operates as callee; outgoing-call support is not implemented |
| `send_audio_response` | TTS response over the call media layer | Publishes encrypted media frames to MOQ/in-memory relay |
| `send_audio_file` | Send raw PCM (little-endian i16) as audio | Async publish on a worker thread; reports stats via `ok` (note: input is raw PCM i16le, not WAV) |
| `shutdown` | Graceful shutdown | Replies `ok` then exits loop |

### Events (stdout) — current set

| Event `type` | When emitted | Payload highlights |
|---|---|---|
| `ready` | once at startup | `protocol_version`, `pubkey`, `npub` |
| `keypackage_published` | after keypackage publish | `event_id` |
| `welcome_received` | when an inbound welcome giftwrap is processed/staged | `wrapper_event_id`, `welcome_event_id`, `from_pubkey`, `nostr_group_id`, `group_name` |
| `group_joined` | after accepting a welcome | `nostr_group_id`, `mls_group_id` |
| `group_created` | after `init_group` | `nostr_group_id`, `mls_group_id`, `peer_pubkey` |
| `message_received` | inbound decrypted application messages | includes `message_id`, `created_at` |
| `call_invite_received` | inbound call invite signal detected | `call_id`, `from_pubkey`, `nostr_group_id` |
| `call_session_started` | after accepting an invite | `call_id`, `from_pubkey`, `nostr_group_id` |
| `call_session_ended` | on end/reject/remote end | `call_id`, `reason` |
| `call_debug` | periodic worker telemetry | frame counters |
| `call_audio_chunk` | Audio-chunk (“stt”) worker emits segmented WAV chunk path | **OpenClaw plugin** transcribes audio; sidecar only writes WAV to a temp dir and reports the path |

### Protocol schema (exact fields)

Authoritative definitions are the Rust enums `InCmd` and `OutMsg` in `crates/marmotd/src/daemon.rs` (mirrored in TS types in `pikachat-openclaw/.../sidecar.ts`).

#### Commands (stdin)

- `publish_keypackage`: `{ cmd, request_id?, relays? }`
  - `ok.result`: `{ event_id }`
  - emits: `keypackage_published { event_id }`
- `set_relays`: `{ cmd, request_id?, relays }`
  - `ok.result`: `{ relays: string[] }`
- `list_pending_welcomes`: `{ cmd, request_id? }`
  - `ok.result`: `{ welcomes: [{ wrapper_event_id, welcome_event_id, from_pubkey, nostr_group_id, group_name }] }`
- `accept_welcome`: `{ cmd, request_id?, wrapper_event_id }`
  - `ok.result`: `{ nostr_group_id, mls_group_id }`
  - emits: `group_joined { nostr_group_id, mls_group_id }`
- `list_groups`: `{ cmd, request_id? }`
  - `ok.result`: `{ groups: [{ nostr_group_id, mls_group_id, name, description }] }`
- `send_message`: `{ cmd, request_id?, nostr_group_id, content }`
  - `ok.result`: `{ event_id }`
- `send_typing`: `{ cmd, request_id?, nostr_group_id }`
  - `ok` is best-effort and may be emitted later (publish happens on a background task)
- `init_group`: `{ cmd, request_id?, peer_pubkey, group_name? }` (default `group_name = "DM"`)
  - `ok.result`: `{ nostr_group_id, mls_group_id, peer_pubkey }`
  - emits: `group_created { nostr_group_id, mls_group_id, peer_pubkey }`
- `accept_call`: `{ cmd, request_id?, call_id }`
  - `ok.result`: `{ call_id, nostr_group_id }`
  - emits: `call_session_started { call_id, nostr_group_id, from_pubkey }`
- `reject_call`: `{ cmd, request_id?, call_id, reason? }` (default `reason = "declined"`)
  - `ok.result`: `{ call_id }`
- `end_call`: `{ cmd, request_id?, call_id, reason? }` (default `reason = "user_hangup"`)
  - `ok.result`: `{ call_id }`
  - emits: `call_session_ended { call_id, reason }`
- `send_audio_response`: `{ cmd, request_id?, call_id, tts_text }`
  - `ok.result`: `{ call_id, frames_published, publish_path?, subscribe_path?, track, local_label, peer_label }`
- `send_audio_file`: `{ cmd, request_id?, call_id, audio_path, sample_rate, channels? }` (default `channels = 1`)
  - `audio_path` is **raw PCM i16le** (not WAV)
  - `ok.result` is emitted later (publish happens on a worker thread): `{ call_id, frames_published, publish_path?, subscribe_path?, track }`
- `shutdown`: `{ cmd, request_id? }`

#### Events (stdout)

- `ready`: `{ type, protocol_version, pubkey, npub }`
- `ok`: `{ type, request_id?, result? }`
- `error`: `{ type, request_id?, code, message }`
- `keypackage_published`: `{ type, event_id }`
- `welcome_received`: `{ type, wrapper_event_id, welcome_event_id, from_pubkey, nostr_group_id, group_name }`
- `group_joined`: `{ type, nostr_group_id, mls_group_id }`
- `group_created`: `{ type, nostr_group_id, mls_group_id, peer_pubkey }`
- `message_received`: `{ type, nostr_group_id, from_pubkey, content, created_at, message_id }`
- `call_invite_received`: `{ type, call_id, from_pubkey, nostr_group_id }`
- `call_session_started`: `{ type, call_id, nostr_group_id, from_pubkey }`
- `call_session_ended`: `{ type, call_id, reason }`
- `call_debug`: `{ type, call_id, tx_frames, rx_frames, rx_dropped }`
- `call_audio_chunk`: `{ type, call_id, audio_path, sample_rate, channels }`

### Call signaling

Daemon treats some application messages as **call signals** if `msg.content` parses as JSON envelope:

```json
{ "v":1, "ns":"pika.call", "type":"call.invite|call.accept|call.reject|call.end", "call_id":"uuid", "body":{ "moq_url":"...", "broadcast_base":"...", "relay_auth":"capv1_<64hex>", "tracks":[...] } }
```

Compat parsing exists for:
- “double encoded” JSON strings (`"{...}"`)
- objects containing nested `content` or `rumor.content`

Important behavior:
- video track (`video0`) invites are rejected as `unsupported_video`
- if already in a call, invites are rejected as `busy`

`accept_call` validates the invite's `relay_auth` and rejects with reason `auth_failed` if the token is missing/malformed or doesn't match the expected value.

### Environment variables consumed by marmotd

| Env var | Where used | Purpose |
|---|---|---|
| `MARMOT_ECHO_MODE` | `daemon.rs` | If truthy (`1`), daemon uses echo worker instead of the audio-chunk (“stt”) worker (which emits `call_audio_chunk` WAV files) |
| `MARMOT_TTS_FIXTURE` | `call_tts.rs` | If `1`, skip OpenAI TTS and return a synthetic 440Hz tone |
| `MARMOT_SILENCE_RMS_THRESHOLD` | `call_audio.rs` | Override default RMS threshold (500.0) for silence detection |
| `OPENAI_API_KEY` | `call_tts.rs` | Required for real TTS (unless fixture mode) |
| `OPENAI_BASE_URL` | `call_tts.rs` | Override TTS API base URL (default: `https://api.openai.com/v1`) |
| `OPENAI_TTS_MODEL` | `call_tts.rs` | Override TTS model (default: `gpt-4o-mini-tts`) |
| `OPENAI_TTS_VOICE` | `call_tts.rs` | Override TTS voice (default: `alloy`) |

### Environment variables consumed by OpenClaw plugin

| Env var | Where used | Purpose |
|---|---|---|
| `GITHUB_TOKEN` | `sidecar-install.ts` | GitHub API auth for release fetching (avoids rate limits) |
| `MARMOT_SIDECAR_REPO` | `sidecar-install.ts` | Override release repo (default: `sledtools/pika`) |
| `MARMOT_SIDECAR_VERSION` | `sidecar-install.ts` | Pin sidecar to a specific release tag |
| `MARMOT_SIDECAR_CMD` | `channel.ts` | Override sidecar command (bypasses auto-install) |
| `MARMOT_SIDECAR_ARGS` | `channel.ts` | Override sidecar args (JSON array string) |
| `MARMOT_SIDECAR_LOG_REQUESTS` | `sidecar.ts` | If `1`, log every sidecar request/response with timing |
| `MARMOT_CALL_START_TTS_TEXT` | `channel.ts` | TTS greeting text on call session start |
| `MARMOT_CALL_START_TTS_DELAY_MS` | `channel.ts` | Delay before greeting TTS (default: 1500, clamped 0-30000) |
| `OPENAI_API_KEY` | `channel.ts` | Used for STT fallback when `runtime.stt` is unavailable (OpenAI-compatible transcription endpoint) |
| `GROQ_API_KEY` | `channel.ts` | Used for STT fallback when `runtime.stt` is unavailable (Groq OpenAI-compatible transcription endpoint) |

These env vars are part of the operational contract. Per Tony’s decision, all `MARMOT_*` env vars should be renamed to `PIKACHAT_*` as part of the clean-break migration (no backward-compat aliases).

### “Protected tag stripping” is a deliberate interop choice

Both key packages and MLS wrapper events are published with NIP-70 `protected` tags **removed** to avoid common-relay rejection.
Any consolidation must preserve this behavior unless we intentionally change the relay deployment model.

---

## OpenClaw integration: constraints that the refactor must preserve

### How OpenClaw spawns the sidecar today

In `pikachat-openclaw/.../channel.ts`:

**Command resolution** (priority order):
1. `MARMOT_SIDECAR_CMD` env var
2. `channels.marmot.sidecarCmd` config
3. Default: `"marmotd"`

If `requestedCmd` is the default `"marmotd"`, `sidecar-install.ts` always uses the managed binary (auto-download/update). If it's anything else, it resolves the command from PATH or as an absolute path, then falls back to auto-install only if not found.

**Args resolution** (priority order):
1. `MARMOT_SIDECAR_ARGS` env var (JSON array string)
2. `channels.marmot.sidecarArgs` config
3. Default: `['daemon', ...relays.flatMap(r => ['--relay', r]), '--state-dir', <baseStateDir>]`

**State dir resolution**: `resolveAccountStateDir` uses `channels.marmot.stateDir` if set, otherwise `<openclaw-state-dir>/marmot/accounts/<sanitized-accountId>`.

**Post-ready sequence**:
1. `set_relays(relays)` — push the full relay list
2. `publish_keypackage(relays)` — publish kind 443
3. If `owner` configured and no groups exist: fire-and-forget `init_group(ownerPk)` to create owner DM

**Sidecar version pinning**: `channels.marmot.sidecarVersion` can pin to a specific release tag. This is passed to `sidecar-install.ts` as `pinnedVersion`.

Therefore, to replace marmotd with pikachat in-place, we need one of:

1. Keep the command+args compatible (OpenClaw can keep using `marmotd daemon ...` during transition), **or**
2. Update OpenClaw plugin to call `pikachat daemon ...` and update its auto-install logic.

### OpenClaw’s sidecar auto-install / auto-update system

`pikachat-openclaw/.../sidecar-install.ts` implements:

- Default repo: `sledtools/pika`
- Default binary name: `marmotd`
- Downloads from GitHub Releases assets named (by platform):
  - `marmotd-x86_64-linux`, `marmotd-aarch64-linux`
  - `marmotd-x86_64-darwin`, `marmotd-aarch64-darwin`
- Verifies `*.sha256` when available
- Caches at: `~/.openclaw/tools/marmot/<tag>/marmotd`
- Keeps at most 2 versions (current + one previous)
- Caches “latest compatible tag” lookups in `~/.openclaw/tools/marmot/.latest-version` for 24 hours
- **Patch-only updates**: it only auto-updates to releases with the same `major.minor` as the plugin’s npm version.
  - e.g. plugin `0.5.x` accepts `marmotd-v0.5.y`
- If a pinned version is provided (`MARMOT_SIDECAR_VERSION` env var or `channels.marmot.sidecarVersion`), the “patch-only compatibility” gate is bypassed and that exact tag is installed.
- Windows is not supported by the installer (throws on unsupported platforms).

Refactor implication: if we switch to distributing `pikachat` as the sidecar, we need a comparable release stream and asset naming, or we need to intentionally change this update contract.

### State dir & sqlite schema coupling

OpenClaw does more than speak JSONL:

- It reads the sidecar’s sqlite directly with `sqlite3` for heuristics/context:
  - `SELECT name FROM groups WHERE nostr_group_id = x'<nostrGroupId>';`
  - `SELECT DISTINCT hex(m.pubkey) ... JOIN groups g ... WHERE g.nostr_group_id = x'<nostrGroupId>';`

So **file names and basic schema table/column names are part of the integration contract**:

- state dir must contain `mdk.sqlite`
- tables `groups` and `messages` must continue to exist (or OpenClaw must be updated)

### Voice call pipeline contract (audio chunks → STT → TTS)

End-to-end, voice calls in OpenClaw work like this today:

1. **marmotd** receives encrypted Opus frames over MoQ and runs the “stt worker” (audio chunker).
2. On silence boundaries, marmotd writes a WAV chunk to a temp directory and emits:
   - `call_audio_chunk { call_id, audio_path, sample_rate, channels }`
   - `audio_path` is under: `${TMPDIR}/marmotd-audio-<call_id>/chunk_<seq>.wav`
3. **OpenClaw plugin** (`channel.ts`) transcribes that WAV file:
   - Preferred: `runtime.stt.transcribeAudioFile(...)` (OpenClaw runtime STT)
   - Fallback: direct OpenAI-compatible HTTP call using `OPENAI_API_KEY` or `GROQ_API_KEY`
   - The plugin deletes the chunk WAV file after transcription.
4. Plugin routes the transcript to the agent (same path as inbound chat messages).
5. Plugin delivers a spoken response via one of two paths:
   - Preferred: `runtime.tts.textToSpeechTelephony(...)` → writes a **raw PCM i16le** temp file → `send_audio_file(call_id, pcmPath, sampleRate)`
   - Fallback: `send_audio_response(call_id, tts_text)` (sidecar performs OpenAI TTS internally)

Refactor implication: `pikachat daemon` must preserve `call_audio_chunk` emission semantics (including WAV chunk format and `sample_rate`/`channels` fields), and `send_audio_file` must continue to accept **raw PCM i16le** (not WAV).

### Config keys used by the OpenClaw plugin (observed)

There is a **split** between the typed `MarmotChannelConfig` (in `config.ts`) and the actual config keys read at runtime (in `channel.ts` via raw `cfg.channels.marmot.*`).

#### Typed config (`config.ts` → `MarmotChannelConfig`)

- `relays` (string[])
- `stateDir` (string?)
- `sidecarCmd` (string?)
- `sidecarArgs` (string[]?)
- `sidecarVersion` (string?)
- `autoAcceptWelcomes` (boolean, default true)
- `groupPolicy` (`"allowlist"` | `"open"`, default `"allowlist"`)
- `groupAllowFrom` (string[])
- `groups` (Record<string, { name?: string }>)

#### Ad-hoc config reads in `channel.ts` (NOT in typed config)

These are read via raw `cfg.channels.marmot.*` access, bypassing the typed resolver:

- `owner` (string | string[]) — bot owner pubkey(s). Gets `CommandAuthorized=true`. Falls back to first entry in `groupAllowFrom`.
- `dmGroups` (string[]) — group IDs that route to main session instead of isolated group session.
- `memberNames` (Record<string, string>) — map of pubkey/npub to display name, overrides Nostr profile resolution.
- `ignorePubkeys` (string[]) — defined in `openclaw.plugin.json` schema but not observed in channel.ts runtime code.

#### Security / pairing policy hooks (config path strings)

Even though DMs are currently “not implemented” in the channel plugin, `channel.ts` wires up OpenClaw’s pairing policy UX with *stringly-typed config paths*:

- `policyPath`: `channels.marmot.dmPolicy`
- `allowFromPath`: `channels.marmot.allowFrom`
- Pairing allowlist entries are normalized by stripping a `marmot:` prefix (e.g. `marmot:npub1...`).

Refactor implication: if the channel/plugin id becomes `pikachat`, these should become `channels.pikachat.dmPolicy`, `channels.pikachat.allowFrom`, and `pikachat:`.

#### Multi-account layout (used by `types.ts`)

Config is merged as:

- base: `channels.marmot.*`
- overlay: `channels.marmot.accounts[<accountId>].*` (if present)

The per-account object can include:

- `enabled` (boolean, default true)
- `name` (string, optional)

Refactor implication: if we rename the channel ID to `pikachat`, all of these paths move to `channels.pikachat.*`.

#### Per-group config (read ad-hoc from `groups[groupId]`)

- `requireMention` (boolean) — if true (default for groups), bot only responds when mentioned.
- `users` (string[]) — per-group sender allowlist, layered on top of `groupAllowFrom`.
- `systemPrompt` (string) — per-group system prompt override.

#### Config schema sources (currently inconsistent)

There are **two** schema definitions in-tree today, and neither matches the full set of keys that `channel.ts` reads:

1. **Runtime-exported schema**: `pikachat-openclaw/.../index.ts` sets `configSchema` to `marmotPluginConfigSchema` from `src/config-schema.ts`.
   - Includes: `relays`, `stateDir`, `sidecarCmd`, `sidecarArgs`, `sidecarVersion`, `autoAcceptWelcomes`, `groupPolicy`, `groupAllowFrom`, `groups.{name}`.
   - Does **not** include: `owner`, `dmGroups`, `memberNames`, `ignorePubkeys`, per-group `requireMention`/`users`/`systemPrompt`, or `accounts`.
2. **Static manifest schema**: `openclaw.plugin.json` defines a different schema (used by OpenClaw tooling / metadata).
   - Includes: `owner`, `dmGroups`, `memberNames`, `ignorePubkeys`, per-group `requireMention`.
   - Does **not** include: `sidecarVersion`, `accounts`, or per-group `users`/`systemPrompt`.

Refactor implication: when we rename the plugin/channel, we should explicitly choose a *single* source of truth for validation and make the other one match (or remove it), otherwise config drift will continue.

Refactor implication: since this is a clean-break migration, all plugin config should move from `channels.marmot.*` → `channels.pikachat.*`.

---

## Distribution / release reality today (marmotd lane)

From `docs/release.md`, `scripts/bump-pikachat.sh`, and `.github/workflows/pikachat-release.yml`:

- marmotd has its own tag family: `marmotd-vX.Y.Z`
- assets per release: platform binaries + `*.sha256`
- npm package is published from `pikachat-openclaw/openclaw/extensions/pikachat`.
- In this repo today, the published npm name is **`@justinmoon/marmot`** (see `pikachat-openclaw/.../package.json`).
- Note: `docs/release.md` and `pikachat-openclaw/todos/ship-marmot.md` still reference `@openclaw/marmot`; those references are stale.
- `scripts/bump-pikachat.sh` bumps both:
  - `crates/marmotd/Cargo.toml`
  - `pikachat-openclaw/openclaw/extensions/pikachat/package.json`

This coupling is why OpenClaw’s sidecar auto-update can be patch-only constrained by the plugin version.

### CI / validation lanes that encode the contract

From `justfile`:

- `pre-merge-marmotd` runs `cargo clippy -p marmotd` and `cargo test -p marmotd`.
- `nightly-marmotd` runs:
  - `just e2e-local-marmotd`
  - `just openclaw-pikachat-scenarios`

`e2e-local-marmotd`:

- Builds `marmotd` and then runs a **pika_core** ignored test suite: `rust/tests/e2e_local_pikachat_daemon_call.rs`.
- That test suite spawns the daemon as a subprocess and speaks the JSONL protocol.

Refactor implication: when we migrate the sidecar from `marmotd` to `pikachat daemon`, we either:

- update these tests to spawn `pikachat daemon`, or
- keep a compatibility `marmotd` binary that continues to satisfy the existing tests.

### OpenClaw-marmot scenario harness also depends on the `marmotd` binary

`just openclaw-pikachat-scenarios` runs shell scripts that invoke `cargo run -p marmotd -- scenario ...`:

- `invite-and-chat` (Rust↔Rust)
- `invite-and-chat-rust-bot` (Rust harness ↔ deterministic Rust bot process)
- `invite-and-chat-daemon` (Rust harness ↔ JSONL daemon over stdio)
- `audio-echo` (media-layer echo smoke)
- plus a phase4 script that boots a real OpenClaw gateway process configured to spawn the sidecar.

Refactor implication: if we remove the `marmotd` crate/binary entirely, we must re-home these scenario commands somewhere (e.g. `pika-cli dev scenario ...`), or rewrite the scripts/tests to use another harness.

### E2E test binary resolution

`rust/tests/e2e_local_pikachat_daemon_call.rs` resolves the daemon binary via:
1. `MARMOTD_BIN` environment variable (if set and non-empty)
2. Otherwise: `target/debug/marmotd` (relative to repo root, computed from `CARGO_MANIFEST_DIR`)

The `just e2e-local-marmotd` recipe does `cargo build -p marmotd` then sets `MARMOTD_BIN=$PWD/target/debug/marmotd`.

This test also:
- Spawns a local Nostr relay (in-process, via tokio + tokio-tungstenite)
- Spawns a local MoQ relay (`moq-relay` binary, must be on PATH)
- Creates an FfiApp caller that exercises the full MLS + call signaling + media path against the daemon
- Tests both text messaging (ping/pong) and call signaling (invite/accept/media/TTS/end)

### Scenario phase4 (full OpenClaw integration)

`pikachat-openclaw/scripts/phase4_openclaw_pikachat.sh` boots a real OpenClaw gateway process that:
- Uses the config to spawn the sidecar binary
- Exercises the complete plugin lifecycle (ready, set_relays, publish_keypackage, welcome, accept, messaging)

This is the most comprehensive integration test and validates the OpenClaw <-> sidecar <-> relay chain end-to-end.

---

## Dependency differences: pika-cli vs marmotd

| Aspect | pika-cli (`cli/`) | marmotd (`crates/marmotd/`) |
|---|---|---|
| Rust edition | 2021 | 2024 |
| `pika-media` | **not** a dependency | dependency with `network` feature |
| `rustls` provider | **no explicit setup** (no ring/aws-lc-rs conflict) | Explicit `ring::default_provider().install_default()` in main |
| `reqwest` | not used directly | `blocking` + `json` + `rustls-tls` features (for TTS) |
| `hound` | not used | WAV encode/decode for audio pipeline |
| `nostr-blossom` | dependency (Blossom file upload for profile pics) | not used |
| tracing setup | `EnvFilter` (respects `RUST_LOG`), `warn` default | Fixed `Level::INFO`, no env filter |

Refactor implication: adding `pikachat daemon` means `pikachat` must pick up `pika-media`, `rustls` provider setup, `reqwest` blocking, and `hound`. This will increase the binary size and compile time. The rustls provider conflict is particularly subtle -- both `ring` and `aws-lc-rs` are in the dep tree (nostr-sdk uses ring, quinn/moq-native uses aws-lc-rs), and rustls panics if both try to auto-install.

---

## Consolidation requirements (what “parity” actually means)

If the goal is “OpenClaw can spawn `pikachat daemon` and everything keeps working”, then `pikachat daemon` must cover at least:

- the **full JSONL command set** above
- the **full event set** above
- the call signal parsing + busy/video rejection behavior
- typing indicator behavior (custom kind + expiration)
- keypackage + wrapper event publish behavior (including the NIP-70 `protected` tag stripping)
- state dir layout and sqlite file naming

---

## Proposed implementation details (document-only; no code)

This section will evolve as we continue research.

### High-level approach

1. Implement `pikachat daemon` as the new home of the marmotd sidecar engine.
2. Preserve the existing JSONL protocol as a compatibility contract with OpenClaw.
3. Provide a migration/transition story for:
   - **binary name** (`marmotd` → `pikachat`)
   - **release tags/assets** (marmotd-v* lane → ???)
   - **npm package naming** (`@justinmoon/marmot` today → `pikachat-openclaw` per intent)

### Suggested code organization (if we want minimal duplication)

Create a shared Rust library crate that owns the daemon implementation and its protocol types, then have binaries call into it.

```text
crates/
  pika-sidecar/        # new: library crate
    src/
      protocol.rs      # JSONL command/event structs + serde
      daemon.rs        # stdin/stdout loop, request_id routing, subscriptions
      mdk_state.rs     # identity.json + mdk.sqlite helpers (can reuse cli logic)
      call/            # call signaling + audio integration (pika-media)
  marmotd/             # becomes: harness + thin wrapper around pika-sidecar (Option A)
  ...
cli/
  src/
    main.rs            # adds `daemon` subcommand calling pika-sidecar
```

Rationale:
- lets `pikachat daemon` and `marmotd daemon` share the exact same engine during migration
- keeps OpenClaw protocol compatibility testable while we change distribution

### CLI integration details (argv compatibility)

Goal: OpenClaw should be able to invoke one of the following without surprises:

```text
marmotd daemon --relay ... --state-dir ...
pikachat daemon --relay ... --state-dir ...
```

Given `pika-cli` (soon: `pikachat`) currently defines `--state-dir` and `--relay` on the root command, the simplest compatibility approach is:

- mark those root args as `clap` **global** (so they can appear after the subcommand), and
- add a `daemon` subcommand whose flags match `marmotd daemon` (at least `--relay`, `--state-dir`, `--giftwrap-lookback-sec`, `--allow-pubkey`).

If we do not want to restructure the CLI’s argument model, then Option A (compat wrapper) becomes more attractive.

### Migration plan sketch (Option A: compat wrapper)

1. Extract daemon engine into shared library crate.
2. Update `marmotd daemon` to call the shared engine (no behavior changes).
3. Add `pikachat daemon` calling the same engine.
4. Update the OpenClaw plugin to optionally use `pikachat` when configured (`sidecarCmd: pikachat`).
5. Once `pikachat` is proven stable in the wild, decide whether to:
   - keep `marmotd` as a long-term compat binary, or
   - remove it and update all scenario + CI lanes accordingly.

### Migration plan sketch (Option B: switch OpenClaw to pikachat)

1–3 as above (engine extraction + `pikachat daemon`).
4. Update OpenClaw’s auto-install logic to download/install `pikachat` (new asset names and tag family).
5. Deprecate `marmotd-v*` releases and stop publishing the old assets.

### Testing/validation plan (must stay green throughout)

- Keep `just pre-merge-marmotd` passing.
- Keep `just nightly-marmotd` passing:
  - `just e2e-local-marmotd` (pika_core test that spawns the daemon)
  - `just openclaw-pikachat-scenarios`

During migration, these are your canaries for:
- JSONL protocol compatibility
- call + audio plane correctness
- state dir and sqlite schema compatibility

### Packaging / compatibility options (to decide)

#### Option A — “compat wrapper” (lowest migration risk)

- Keep shipping a `marmotd` binary for OpenClaw, but turn it into a thin wrapper around `pikachat daemon`.
  - Could be either:
    - a small Rust binary that calls into shared daemon library code (preferred), or
    - a shell-style exec wrapper (less ideal cross-platform)
- OpenClaw plugin and its auto-update system keep working unchanged.
- `pikachat daemon` becomes the canonical implementation; `marmotd` becomes deprecated/compat.

Pros: minimal disruption, no immediate OpenClaw changes.
Cons: still “two binaries” in distribution (even if one is thin).

#### Option B — “OpenClaw switches to pikachat” (cleaner end state)

- Update OpenClaw plugin to:
  - auto-install `pikachat` assets
  - spawn `pikachat daemon ...`
- Deprecate marmotd release lane.

Pros: one binary to build/distribute.
Cons: requires coordinated plugin + binary release changes; update policy/versioning needs redesign.

### Recommended sequencing (tentative)

1. **Engine extraction / reuse**: move the daemon engine into a shared module/crate so the same code can be built into `pikachat daemon` and (temporarily) `marmotd daemon`.
   - There is already shared-ish logic in `cli/src/{mdk_util.rs,relay_util.rs}` that overlaps with `marmotd`:
     - `load_or_create_keys(identity.json)` — nearly identical implementations in both binaries (same `IdentityFile` struct, same read-or-generate-and-write logic)
     - `open_mdk(state_dir)/mdk.sqlite` — both use `MdkSqliteStorage::new_unencrypted`
     - `connect_client` — cli takes `&[String]`, marmotd takes single `&str` and adds more later
     - `publish_and_confirm` — cli version takes `&[RelayUrl]`, marmotd has both `publish_and_confirm` (single relay) and `publish_and_confirm_multi` (multi relay with fetch-back verification)
     - `fetch_latest_key_package` — identical logic but different signatures
     - `subscribe_group_msgs` — exists only in marmotd's main.rs, used by both scenarios and daemon
     - `check_relay_ready` — exists in both but with different retry strategies (cli simpler, marmotd reconnects fresh clients)
   - Consolidation should avoid creating a third copy of this logic.
   - The `marmotd` `main.rs` also contains the `bot`, `init`, `scenario` subcommands and their helpers (~900 lines). Per Tony: these test harnesses should ultimately live alongside the consolidated core commands (in `pikachat`, not a separate harness binary).
   - If `pikachat daemon` starts depending on `pika-media` (for call/audio), it will likely also need the same rustls provider initialization that `marmotd` does (ring vs aws-lc-rs provider ambiguity).
   - The `call_audio.rs` and `call_tts.rs` modules (~400 lines total) are tightly coupled to the daemon engine and must move with it.
2. Add `pikachat daemon` that is byte-for-byte protocol compatible.
3. Validate against existing E2E lanes:
   - `just nightly-marmotd`
   - `just openclaw-pikachat-scenarios`
   - `rust/tests/e2e_local_pikachat_daemon_call.rs` (and related)
4. Decide between Option A vs B for distribution.

### Pseudocode sketch: daemon main loop contract

```text
daemon_main(relays, state_dir, lookback, allow_pubkeys):
  load_or_create identity.json
  open mdk.sqlite
  connect nostr client + subscribe GiftWrap via p-tag filter
  subscribe existing groups from DB
  emit ready
  loop:
    select stdin_cmd | nostr_notification | worker_events
    handle stdin_cmd by mutating relay list, publishing, joining, etc
    handle giftwrap -> stage welcome -> emit welcome_received
    handle group msg -> mdk.process_message ->
      if call signal -> update call state + emit call events
      else if typing -> ignore
      else -> emit message_received
```

---

## Detailed implementation plan (Option B: direct switch to pikachat)

Based on Tony's decisions: Option B, no compat wrapper, no auto-migration; rename everything from `marmot*` → `pikachat*`; consolidate test harnesses into the same binary; keep version coupling the same as today.

### Phase 1: Engine extraction + shared crate

**Goal**: Create `crates/pika-sidecar/` library crate that contains all daemon logic.

1. Create `crates/pika-sidecar/` with:
   - `src/lib.rs` — public API surface
   - `src/protocol.rs` — `InCmd`, `OutMsg` enums (exact serde shapes preserved)
   - `src/daemon.rs` — `daemon_main()` extracted from `crates/marmotd/src/daemon.rs`
   - `src/call_audio.rs` — moved from `crates/marmotd/src/call_audio.rs`
   - `src/call_tts.rs` — moved from `crates/marmotd/src/call_tts.rs`
   - `src/mdk_state.rs` — unified `load_or_create_keys`, `open_mdk`, `IdentityFile` (currently duplicated in `cli/src/mdk_util.rs` and `crates/marmotd/src/main.rs`)
   - `src/relay.rs` — unified relay helpers: `connect_client`, `publish_and_confirm_multi`, `fetch_latest_key_package`, `subscribe_group_msgs`, `check_relay_ready`
2. `Cargo.toml` for `pika-sidecar`: edition 2024, deps = `pika-media` (network), `rustls` (ring), `reqwest` (blocking), `hound`, `mdk-core/mdk-sqlite-storage/mdk-storage-traits` (workspace), `nostr-sdk`, `tokio`, `serde`, `serde_json`, `tracing`.
3. Add `crates/pika-sidecar` to workspace members.
4. Update `crates/marmotd/` to depend on `pika-sidecar` and call its `daemon_main()` (temporary bridge while migrating).
5. Update `cli/` to depend on `pika-sidecar` for shared `mdk_state` and `relay` helpers. Remove `cli/src/mdk_util.rs` and `cli/src/relay_util.rs` (use `pika-sidecar` exports instead).
6. Validate: `just pre-merge-marmotd` and `cargo test -p pikachat` must pass (during the migration there may be an intermediate period where the crate is still named `pika-cli`).

### Phase 2: Add `pikachat daemon` subcommand

1. Add `Daemon` variant to `cli/src/main.rs` `Command` enum with flags: `--relay`, `--state-dir`, `--giftwrap-lookback-sec`, `--allow-pubkey`.
2. In `main()`, add `rustls::crypto::ring::default_provider().install_default()` at the top (before tracing init) — must happen before any TLS usage.
3. Route `Command::Daemon` to `pika_sidecar::daemon_main()`.
4. Add `pika-media`, `rustls`, `reqwest`, `hound` to `cli/Cargo.toml`.
5. Bump `cli/` edition to 2024 (optional but avoids let-chain syntax issues in shared code).
6. **Unify default state dir (decision)**: all `pikachat` commands (one-shot + daemon + future remote) use the same default state directory unless overridden. Default:
   - `${XDG_STATE_HOME:-$HOME/.local/state}/pikachat` on Unix
   - override via `--state-dir` (CLI) and `channels.pikachat.stateDir` (OpenClaw)
7. Validate: manually test `pikachat daemon --relay ws://127.0.0.1:18080 --state-dir .test-daemon` — should emit `ready` JSON line on stdout.
8. Validate: run `just openclaw-pikachat-scenarios` with the installer pinned to the locally built `pikachat` binary (spawn via `pikachat daemon ...`).

### Phase 3: Move test harness commands into pikachat (required)

Per Tony: test harnesses should live with the core commands/logic (single place for everything). Move `bot`, `init`, and `scenario` into `pikachat` (e.g. `pikachat dev bot`, `pikachat dev scenario`, `pikachat dev init`) and retire the separate `marmotd` harness crate once scenarios are ported.

### Phase 4: Update OpenClaw plugin

**New npm package**: `pikachat-openclaw` (or whatever name is chosen).

Changes to `sidecar-install.ts`:
- `DEFAULT_BINARY_NAME`: `"marmotd"` → `"pikachat"`
- `resolvePlatformAsset()`: `"marmotd-x86_64-linux"` → `"pikachat-x86_64-linux"`
- `getCacheDir()`: `~/.openclaw/tools/marmot/<tag>/marmotd` → `~/.openclaw/tools/pikachat/<tag>/pikachat`
- `parseVer()`: strip `"pikachat-v"` prefix
- `fetchLatestCompatibleRelease()`: search for releases with `pikachat-v*` tags + new asset names

Changes to `channel.ts`:
- Default `requestedSidecarCmd`: `"marmotd"` → `"pikachat"`
- Default `sidecarArgs`: `["daemon", ...]` (unchanged since `pikachat daemon` takes same shape)
- Channel ID (decision): `"pikachat"`
- Config prefix (decision): `channels.pikachat.*`
- Env vars (decision): `PIKACHAT_*` (rename from `MARMOT_*`)

Also update (to keep the plugin internally consistent):
- `index.ts`: plugin `id`/`name`/`description` and exported `configSchema`
- `src/types.ts`: `channels.marmot.accounts` → `channels.pikachat.accounts`
- `src/config-schema.ts` and `openclaw.plugin.json`: update IDs/prefixes and reconcile schema drift (see “Config schema sources” above)
- Any user-facing log prefixes / temp dir prefixes that currently include `marmot` (e.g. `mkdtempSync(..., "marmot-tts-")`)

Changes to `package.json`:
- `name`: `"@justinmoon/marmot"` → `"pikachat-openclaw"` (or new package name)
- `openclaw.channel.id`: `"marmot"` → `"pikachat"`
- `openclaw.install.npmSpec`: update to new package name

Changes to `openclaw.plugin.json`:
- `id`: `"marmot"` → `"pikachat"`

### Phase 5: Update CI/release pipeline

1. **New release workflow**: `.github/workflows/pikachat-release.yml` (modeled on `marmotd-release.yml`)
   - Tag family: `pikachat-vX.Y.Z` ("tag family" = the git tag prefix used to select releases)
   - Build targets: same matrix (linux x86_64/aarch64, darwin x86_64/aarch64)
   - Asset names: `pikachat-x86_64-linux`, `pikachat-aarch64-linux`, etc.
   - npm publish step: publish from new package path
2. **New bump script**: `scripts/bump-pikachat.sh` — bumps the `pikachat` Rust crate version and the `pikachat-openclaw` npm package version together (same coupling as today)
3. **Update justfile recipes**:
   - `pre-merge-marmotd` → updated/renamed to validate `pikachat daemon` mode
   - `e2e-local-marmotd` → `e2e-local-pikachat` (or parameterize binary name)
   - `openclaw-pikachat-scenarios` → `openclaw-pikachat-scenarios` and update scripts to use `cargo run -p pikachat -- daemon ...` or `cargo run -p pikachat -- dev scenario ...`
4. **Update e2e test**: `rust/tests/e2e_local_pikachat_daemon_call.rs` → resolve binary from `PIKACHAT_BIN` env var (falling back to `target/debug/pikachat`), invoke with `daemon` subcommand.

Also update internal tooling that shells out to `pika-cli` (rename impact):

- `tools/{pika-e2e-bot,ui-e2e-local,cli-smoke}`
- `tools/lib/local-e2e-fixture.sh`
- `.gitignore` (move `.pika-cli*` ignores to `pikachat` equivalents)
- `scripts/agent-brief`

### Phase 6: Deprecate marmotd

1. Stop publishing `marmotd-v*` releases.
2. Either:
   - Remove `crates/marmotd/` entirely (clean break), or
   - Keep it as a thin wrapper that prints a deprecation notice and execs `pikachat daemon`
3. Update README/docs.

---

## Decisions / constraints (as of 2026-02-22)

### Confirmed decisions (from Tony)

1. **Rename everything**: all user-facing `marmot*` surfaces become `pikachat*` (channel id, config prefix, env vars, tool cache paths, npm package naming, etc.).
2. **CLI binary name**: `pikachat` (not `pikachat-cli`). The existing `pika-cli` becomes legacy/renamed.
3. **Distribution end-state**: choose **Option B** — update the OpenClaw plugin + installer to install/spawn `pikachat` directly.
4. **No auto-upgrade/migration**: do **not** attempt to migrate existing `marmotd` installs or OpenClaw configs; this is a clean break.
5. **Version coupling (same as today)**: keep the patch-only within major.minor auto-update policy, and bump the `pikachat` Rust crate and `pikachat-openclaw` npm package together.
6. **Test harnesses live with core commands**: move `bot`, `init`, and `scenario` into `pikachat` (single place for all commands).
7. **Unified default state dir**: all `pikachat` commands share the same default state directory (unless overridden). Default: `${XDG_STATE_HOME:-$HOME/.local/state}/pikachat`.
8. **Release tags/assets**: use `pikachat-vX.Y.Z` git tags (“tag family” = the git tag prefix used by release automation) with assets named `pikachat-<arch>-<os>`.

---

## Additional decisions captured (2026-02-22)

1. **Schema system**: keep existing standards in place (no schema redesign). Update whatever schemas exist for the rename, but don’t attempt to introduce a new “single source of truth” system during this refactor.
2. **Schema coverage**: no change (do not expand validation to cover `channels.<id>.accounts` or per-group `users`/`systemPrompt`).
3. **Rename scope**: rename everything, including internal temp dir prefixes (e.g. `${TMPDIR}/marmotd-audio-*`, `marmot-tts-*`) and repo folder names like `pikachat-openclaw/`.
