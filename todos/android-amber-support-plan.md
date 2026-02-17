# Android Amber Support Plan

Status: draft
Owner: pika app team
Scope: Android only (Pika + Rust core), preserve existing iOS/local-key flows

## Goals

- Add Android login/signing via Amber using NIP-55 (Intents + ContentProvider).
- Keep Rust as source of truth for auth/session/business logic.
- Do as much as possible in Rust while keeping Android interop practical.
- Preserve backward compatibility with current local `nsec` flow.

## Current State

- Rust auth/session flow assumes raw `nsec` and constructs `Keys` directly.
- Android stores/restores `nsec` in `EncryptedSharedPreferences`.
- Rust signing paths call `sign_with_keys` and rely on in-process keys.
- No Amber/NIP-55 integration exists in Pika yet.

## Constraints and Protocol Reality

- NIP-55 `get_public_key` is handshake: returns signer pubkey and signer package id.
- Operational methods needed for Pika path:
  - `sign_event`
  - `nip44_encrypt`
  - `nip44_decrypt`
  - (`nip04_*` optional for parity)
- Amber supports both UI Intent approval flow and background ContentProvider flow when permission is remembered.
- `current_user` routing is required for multi-account cases.

## Architecture

### Rust-first signer abstraction

- Introduce signer model in Rust session layer:
  - `LocalKeysSigner` (current behavior)
  - `ExternalSignerBridgeSigner` (Android Amber backend)
- Session should stop depending on direct private-key ownership for all signing operations.
- Use `nostr::NostrSigner` as the canonical trait boundary.

### Android bridge

- Android implements a UniFFI callback interface used by Rust signer bridge.
- Bridge executes Amber calls and returns typed success/error results.
- Bridge internally chooses:
  - ContentProvider fast path (remembered permission)
  - Intent/ActivityResult fallback path (interactive approval)

### Auth mode model

- Add explicit auth mode state:
  - `LocalNsec`
  - `ExternalSigner { pubkey, signer_package }`
- Keep existing `CreateAccount` as local-only for initial rollout.

## Implementation Plan

### Phase 1: Rust foundations (no behavior break)

1. Add auth mode + session metadata in Rust state/core.
2. Add UniFFI callback interface for external signer ops.
3. Add Rust `ExternalSignerBridgeSigner` implementing `NostrSigner`.
4. Add `start_session_with_signer(pubkey, signer)` path.
5. Keep old `start_session(keys)` as adapter.

### Phase 2: Replace key-coupled signing call sites

1. Migrate key-dependent signing/encryption paths to signer-backed equivalents.
2. Keep pubkey reads from session identity metadata.
3. Normalize signer errors into stable user-facing categories:
   - rejected
   - canceled
   - timeout
   - signer unavailable
   - package mismatch

### Phase 3: Android Amber integration

1. Add Amber client module in Android app.
2. Implement `get_public_key` handshake and persist:
   - signer package
   - signer pubkey
   - auth mode
3. Add external-signer secure store (parallel to existing `nsec` store).
4. Wire `Login with Amber` action in Android UI.
5. Restore session path for external signer mode on app startup.

### Phase 4: Rollout and cleanup

1. Gate Amber path behind feature flag initially.
2. Run soak period with both local and Amber modes.
3. Remove redundant assumptions that `nsec` always exists on Android.

## Android Tooling and Manual QA Flow

Use the new boot-only command path before manual QA:

- `just android-device-start`
- `just android-agent-open`
- Amber open helper:
  - `just android-agent-open APP=com.greenart7c3.nostrsigner`

`npx agent-device` real-Amber validation sequence:

1. Open Amber and ensure at least one account exists.
2. Open Pika and trigger Amber login/connect.
3. Approve handshake in Amber, verify Pika transitions to logged-in state.
4. Send message requiring signing, approve in Amber.
5. Repeat with reject/cancel and verify clean UX errors.
6. Restart app and verify restore via stored external signer descriptor.

## Testing Plan

### Rust unit tests

- Mock signer implementing `NostrSigner`.
- Cover:
  - sign/encrypt/decrypt success
  - reject/cancel/timeout failures
  - auth mode transitions

### Rust integration tests

- Start session with external signer mock.
- Verify key flows still work:
  - key package publish
  - message send
  - receive/decrypt path assumptions

### Android unit tests

- Amber contract parser tests (Intent extras + provider columns).
- Store/restore tests for mode descriptors.
- Error mapping tests.

### Android instrumentation tests

- Fake signer fixture app for deterministic CI.
- Scenarios:
  - approve
  - reject
  - timeout
  - package mismatch

### Manual QA (real Amber)

- Use `npx agent-device` against actual Amber install.
- Include multi-account `current_user` routing checks.
- Capture screenshots and logs for each failure mode.

## Acceptance Criteria

1. Android can log in and operate without persisting local `nsec` in Amber mode.
2. Outbound signing/encryption required by Pika works through Amber.
3. Rejection/cancel/timeout surfaces clear non-crashing errors.
4. Local `nsec` mode remains fully functional.
5. CI includes deterministic fake-signer coverage; manual QA passes with real Amber.

## Risks

- Some Rust paths may still assume `Keys` ownership and require deeper refactor.
- Intent-based UX can be noisy if app falls back too often from provider path.
- Multi-account selection can drift if `current_user`/stored descriptor is stale.

## Mitigations

- Keep migration incremental and compile-safe per phase.
- Centralize signer operations behind one Rust abstraction early.
- Add strict package/pubkey validation and explicit recovery UX.
- Maintain dual-mode support until Amber path is proven stable.
