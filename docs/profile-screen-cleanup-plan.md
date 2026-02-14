---
summary: Plan to redesign the iOS profile screen with Rust-owned metadata/blossom upload flows
read_when:
  - implementing profile editing on iOS
  - wiring Rust kind-0 metadata + Blossom uploads to native UI
status: in_progress
---

# Profile Screen Cleanup Plan

## Goals

1. Replace the current "My npub" sheet with a cleaner profile editor flow.
2. Keep profile data flow Rust-owned (fetch/save/upload in `rust/`).
3. Support profile photo upload via Blossom using public servers for now.

## Rust Work

1. Add a Rust-owned `my_profile` slice to `AppState` (name/about/picture URL).
2. Add profile actions:
   - `RefreshMyProfile`
   - `SaveMyProfile`
   - `UploadMyProfileImage`
3. On session start/foreground, fetch kind-0 metadata for the logged-in pubkey.
4. Save name/about by publishing kind-0 via `nostr-sdk::Client::set_metadata`.
5. Upload profile images via `nostr-blossom` with fallback servers:
   - `https://blossom.nostr.pub`
   - `https://void.cat`
6. After upload succeeds, publish updated kind-0 `picture`.
7. Add TODO to switch from hardcoded Blossom servers to user-advertised server lists.

## iOS Work

1. Redesign `MyNpubQrSheet` as a profile screen with `List` sections:
   - Avatar + upload button
   - Name editor
   - About editor
   - npub row (visually truncated + copy)
   - QR code section
   - nsec reveal/copy section
2. Wire `AppManager` methods for refresh/save/upload actions.
3. Keep existing test IDs where practical to minimize test churn.

## Validation

1. Regenerate iOS and Android UniFFI bindings.
2. Run Rust tests/build checks.
3. Run iOS tests/build checks.
