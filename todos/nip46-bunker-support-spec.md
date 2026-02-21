# NIP-46 Bunker Support Spec

## Scope Clarification

There are two different "remote signer" roles:

1. **Client role**: Pika logs in using a remote bunker signer (NIP-46).
2. **Signer role**: Pika itself acts as a bunker for other clients (what Primal iOS is doing).

This spec focuses on **(1) client role** first, because it directly extends current auth/login flows.

## What Primal iOS Is Doing

Observed in source:

- `nostrconnect://` deeplink entrypoint: `Primal/Common/DeepLinking/Scheme/RemoteSigningScheme.swift`
- Sign-in UI selects local user + trust level, then initializes connection:
  - `Primal/Scenes/RemoteSigner/RemoteSignerSignInController.swift`
  - calls `SignerConnectionInitializer.initialize(..., connectionUrl, trustLevel, nwcConnectionString: nil)`
- App runs a remote-signer service and session/permission/event repositories:
  - `Primal/State/Managers/RemoteSigning/RemoteSignerManager.swift`

From this, Primal is primarily implementing **signer role (bunker)** UX.

## NIP Mapping

- **NIP-55**: Android signer app integration via Intents/ContentProvider (`nostrsigner:`). This is what Amber uses.
- **NIP-46**: Nostr Connect / bunker protocol (`bunker://` and `nostrconnect://`, kind `24133`).
- **NIP-47**: Wallet connect (NWC), not signer auth. Related but separate.

## Current Pika Status

Already implemented:

- Rust-owned external signer state machine and busy/toast ownership.
- Bridge signer ops: `get_public_key`, `sign_event`, `nip04_encrypt/decrypt`, `nip44_encrypt/decrypt`.
- Android Amber adapter implementing NIP-55 transport.

Not implemented yet:

- NIP-46 transport/session for login (relay-based bunker communication).
- iOS external signer path (currently only nsec restore/login in `ios/Sources/AppManager.swift`).

## Proposed Architecture (Client Role / NIP-46)

### 1. Rust-first signer backend abstraction

Keep Rust as auth owner. Add a second signer backend beside Android bridge signer:

- `LocalNsec` (existing)
- `ExternalSignerBridge` (existing Amber / NIP-55)
- `NostrConnectBunker` (new NIP-46)

### 2. New Rust actions/state

Add actions:

- `BeginBunkerLogin { uri: String }`
- `RestoreSessionBunker { descriptor: BunkerDescriptor }`

Add state/auth mode variant:

- `AuthMode::BunkerSigner { user_pubkey, remote_signer_pubkey, client_pubkey, relays, descriptor_id }`

Keep descriptor fields sufficient for native persistence + deterministic restore.

### 3. New Rust module for bunker handshake + signer

Add `rust/src/bunker_signer.rs`:

- Parse `bunker://...` (and optionally `nostrconnect://...` as phase 2).
- Create/load client keypair used for NIP-46 channel.
- Perform connect + `get_public_key` handshake.
- Build `NostrSigner` instance that uses NIP-46 methods for sign/encrypt/decrypt.
- Map bunker errors to user-visible auth errors.

Implementation route:

- Add dependency on `nostr-connect` crate (same ecosystem as `nostr-sdk`).
- Prefer library implementation over custom protocol implementation.

### 4. Persistence model

Persist only Rust-derived descriptor data in native secure stores.

- Android: extend `SecureAuthStore` with `BUNKER` mode descriptor.
- iOS: replace nsec-only keychain store with auth descriptor store (nsec + bunker variants).

No UI-local source of truth.

### 5. Platform UI changes

- Android `LoginScreen`: add "Login with Bunker" input/scan path that dispatches `BeginBunkerLogin`.
- iOS `LoginView`: add equivalent bunker entry + QR scan option.
- Both platforms: button busy state comes only from Rust `busy.logging_in`.

### 6. Restore flow

On app launch:

- Native loads stored auth descriptor.
- Dispatches appropriate Rust restore action:
  - `RestoreSession` for nsec
  - `RestoreSessionExternalSigner` for NIP-55
  - `RestoreSessionBunker` for NIP-46

### 7. Error handling

Normalize bunker handshake/runtime errors into typed categories:

- invalid URI
- relay unreachable / timeout
- rejected / unauthorized
- signer mismatch
- invalid response

Rust emits toast text; native only renders state.

## QA Plan

1. Rust unit tests:
- URI parsing and validation
- handshake happy path
- handshake failure mapping
- restore path and auth state persistence fields

2. Rust integration tests:
- mock NIP-46 signer server over test relay
- end-to-end sign/encrypt/decrypt calls through bunker signer

3. App compile checks:
- `cargo test --test app_flows`
- Android `:app:compileDebugKotlin`
- iOS build + relevant tests

4. UX validation:
- Busy states from Rust only
- No native-owned login flow state for bunker

## Optional Phase 2: Pika as Bunker (Signer Role)

If desired, mirror Primal's capability later:

- Accept `nostrconnect://` deeplinks
- Let user choose account + trust policy
- Persist per-app permissions and sessions
- Approve/reject pending requests in-app

This is a larger feature set than client-role bunker login and should be planned separately.
