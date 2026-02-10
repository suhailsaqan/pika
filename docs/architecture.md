---
summary: Architecture overview — Rust owns state; iOS/Android render; MLS over Nostr via MDK
read_when:
  - starting work on the project
  - need to understand how components fit together
---

# Architecture

Pika is an MLS-encrypted messaging app for iOS and Android, built on the Marmot protocol over Nostr.

This doc is the "agent-level" overview. Deeper details live in `spec-v1.md` and `spec-v2.md`.

## TL;DR

- Rust is the source of truth for durable app state and business logic (routing, auth, chats, protocol, errors).
- iOS (SwiftUI) and Android (Jetpack Compose) primarily render Rust-provided state slices and forward user actions.
- iOS/Android may keep ephemeral UI-only state (text input drafts, scroll position, focus, local toggles, transient animations).
- iOS/Android sends fire-and-forget actions to Rust; Rust emits full slice updates with a monotonic `rev`.
- MLS + encrypted state machine comes from MDK (`https://github.com/marmot-protocol/mdk`); transport is Nostr relays via `nostr-sdk`.

## One-Way Data Flow (Core Invariant)

iOS/Android must not "assemble" or interpret state. One direction:

1. User does something in iOS/Android UI
2. iOS/Android calls `dispatch(action)` into Rust (does not block the UI thread)
3. Rust actor mutates internal state and increments `rev`
4. Rust emits an `AppUpdate` callback with `rev` and the full current slice value
5. iOS/Android applies the slice to its observable state; if `rev` continuity is broken, iOS/Android resyncs via `state()`

`rev` continuity rules (iOS/Android side):

- `update.rev == last_rev + 1`: apply update
- `update.rev <= last_rev`: drop (stale / already applied)
- `update.rev > last_rev + 1`: forward gap, call `state()` and replace mirrored view state

## Component Map

- **Rust core** (`rust/`, crate `pika_core`)
  - App actor (`AppCore`) owns mutable state, `rev`, relay clients, and the MDK instance.
  - Exposes a small UniFFI surface (plus JNI for Android).
- **iOS app** (`ios/`)
  - SwiftUI renderer + thin manager that bridges UniFFI callbacks to `@Observable` state.
  - Uses `ios/Frameworks/PikaCore.xcframework` built from Rust.
- **Android app** (`android/`)
  - Compose renderer + thin manager that bridges Rust updates into `mutableStateOf`.
  - Uses JNI libs from `android/app/src/main/jniLibs` + generated Kotlin bindings.
- **pika-cli** (`cli/`)
  - Useful for agent-driven testing; exercises protocol end-to-end without UI.
- **MDK** (external, `https://github.com/marmot-protocol/mdk`)
  - MLS group/message lifecycle + encrypted SQLite storage implementation.

## Rust FFI Boundary (What iOS/Android Can Do)

The FFI API is intentionally tiny (see `spec-v1.md`):

- `state() -> AppState` (snapshot including `rev`)
- `dispatch(Action)` (enqueue-only)
- `listen_for_updates(AppReconciler)` (callback for `AppUpdate` slices)

iOS/Android responsibilities:

- Store secrets (notably `nsec`) in platform keychain/keystore.
- Apply `AppUpdate` slices to observable state on the main thread.
- Enforce `rev` continuity; resync by calling `state()` when needed.
- Keep truly ephemeral UI state iOS/Android-only (focus, scroll offsets, gesture state).

Non-goal:

- iOS/Android should not own core app state (anything that changes app behavior, drives routing, affects protocol/network decisions, or must survive view recreation).
  - Example: "loading" and "in-flight" state for actions like login/create-chat should generally be Rust-owned, because it affects what the app can do next (disable/enable flows, retries, error handling).

## Nostr + MDK (Protocol Sketch)

MDK handles MLS operations and storage; Pika handles relay orchestration and routing.

Important event kinds (see `spec-v2.md`):

- `443`: MLS key package
- `444`: MLS welcome rumor (delivered via NIP-59 GiftWrap wrapper)
- `445`: MLS group message wrapper, routed by `h` tag where `h = hex(nostr_group_id)`

Subscriptions:

- Listen for GiftWrap inbox events (to receive welcomes).
- Subscribe to kind `445` filtered by `#h` across joined group ids.

Relays:

- Many public relays reject NIP-70 `protected` events, which affects key packages.
- Pika splits relays by role: normal traffic vs key package relays (see README "Relays (V2 / MDK)").

## Persistence + Secrets

- MLS/MDK state persists in an encrypted SQLite DB per identity (pubkey hex), under the app data dir.
- The DB encryption key is stored via the platform keyring; it is distinct from the Nostr secret key.
- `nsec` must not be persisted by Rust; iOS/Android provides it on restore/login and stores it securely.

## Where To Look In Code (First Stops)

- Rust FFI surface + state/action/update types: `rust/pika_core/src/`
- CLI protocol exerciser: `cli/src/main.rs`
- Android bindings generation: `just gen-kotlin` (see `justfile`)
- iOS bindings/xcframework generation: `just ios-xcframework` (see `justfile`)
