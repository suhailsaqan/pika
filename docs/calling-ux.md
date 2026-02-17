---
summary: Phased plan to replace the current iOS calling UI with a cleaner full-screen UX, then evolve toward CallKit and robust audio routing
read_when:
  - refactoring iOS chat/call surfaces
  - implementing call UI state transitions
  - integrating CallKit and audio-session routing behavior
status: phase_0_4a_complete_phase_4bcd_planned
---

# Calling UX Plan

## Goals

1. Keep calling usable at every phase (start, accept, reject, mute, end).
2. Move call entry to the expected place: chat top-right toolbar.
3. Replace the current inline "debug-like" call strip with a deliberate full-screen call experience.
4. Avoid overcomplicating chat history now; defer call timeline logging to a later phase.
5. Remove current iOS warnings and deprecated API usage as part of the cleanup.
6. Integrate CallKit without breaking Rust-owned call state transitions.

## Non-Goals (For Initial Phases)

1. Full call history model in chat timeline.
2. CallKit parity on Android.
3. Multi-party or video calling UX.

## Historical Pain Points (Phases 0-3)

1. Inline call controls in `ChatView` looked unfinished.
2. Call state/status UI cluttered chat context.
3. Call affordance placement did not match expected top-right phone action.
4. iOS warning debt:
   - `AVAudioSession.CategoryOptions.allowBluetooth` deprecation
   - `AVAudioSession.recordPermission` and `requestRecordPermission` deprecations
   - main-actor isolation warnings around `manager.dispatch` in `ContentView`

## Current Status (2026-02-17)

1. Phase 0 is complete:
   - iOS deprecations/warnings shown earlier are addressed.
   - Audio category option is now `.allowBluetoothHFP`.
   - Mic permission checks use `AVAudioApplication`.
2. Phase 1 is complete:
   - Call entry moved to chat top-right.
   - Dedicated full-screen in-app call screen is shipped.
3. Phase 2 is complete:
   - Return-to-call pill is present in chat and no longer blocks input.
4. Phase 3 is complete:
   - Call timeline events are inline with messages and sorted in timeline order.
5. Phase 4A baseline is complete:
   - iOS has a global CallKit coordinator (`CXProvider` + `CXCallController`) with Rust `call_id` to CallKit `UUID` mapping.
   - Outgoing/incoming/end actions are synchronized between in-app UI, CallKit actions, and Rust state transitions.
6. Remaining major gaps:
   - Phase 4B mute/hold parity is not implemented yet.
   - Phase 4C audio-session ownership handoff to CallKit is not implemented yet.
   - Phase 4D PushKit/background incoming is not implemented yet.

## Product Direction

1. Signal-like behavior: active call gets a dedicated full-screen call UI.
2. The rest of the app remains navigable; user can return to the call screen quickly.
3. Chat thread remains message-focused for now (no immediate call event spam).
4. Build a clean path to CallKit + proper Apple audio processing/routing.

## Architecture Constraints (Current)

1. Rust remains source of truth (`docs/state.md`): native must not invent business transitions.
2. Current Rust call action surface:
   - `startCall(chatId)`
   - `acceptCall(chatId)`
   - `rejectCall(chatId)`
   - `endCall`
   - `toggleMute`
3. Current Rust `CallStatus` values:
   - `offering`, `ringing`, `connecting`, `active`, `ended(reason)`
4. Current runtime behavior:
   - `startCall` moves to `offering`.
   - `acceptCall` moves to `connecting`.
   - runtime connected/stats move to `active` and set `started_at`.
   - end/reject moves to `ended(reason)` (not immediately `nil`).

## CallKit Integration Principles (New)

1. Use one global `CXProvider` and one global `CXCallController`.
2. Keep a deterministic map between Rust `call_id` and CallKit `UUID`.
3. Treat CallKit as a control plane and system UI surface; Rust still owns domain state.
4. Make all cross-plane actions idempotent (duplicate callbacks and racey ordering are normal).
5. Fulfill/fail every CallKit action promptly; never leave actions pending.

## Phase 0: Foundation + Warning Cleanup

### Scope

1. Extract call logic from `ChatView` into dedicated components/helpers.
2. Remove deprecated APIs and silence current compiler warnings.
3. Keep behavior equivalent (no UX overhaul yet), so this phase is low risk.

### Implementation Targets

1. `ios/Sources/Views/ChatView.swift`
2. `ios/Sources/ContentView.swift`
3. `ios/Sources/CallAudioSessionCoordinator.swift`
4. `ios/Sources/TestIds.swift`

### Key Technical Changes

1. Replace `.allowBluetooth` with `.allowBluetoothHFP` in `CallAudioSessionCoordinator`.
2. Replace `AVAudioSession.sharedInstance().recordPermission` flow with `AVAudioApplication.recordPermission`.
3. Replace `requestRecordPermission` usage with `AVAudioApplication.requestRecordPermission`.
4. Resolve main-actor warnings by ensuring `dispatch` calls run from main-actor context in `ContentView`.

### Exit Criteria

1. Existing call flow still works.
2. Warnings shown in screenshot are gone.
3. Call code is no longer monolithic in `ChatView`.

## Phase 1: MVP UX Cleanup (Top-Right Call Entry + Full-Screen Call UI)

### Scope

1. Move "start call" to a phone button in the chat top-right toolbar.
2. Remove inline call control block from message area.
3. Present a dedicated full-screen call UI for live states.
4. Keep call state sourced from `state.activeCall` (Rust remains source of truth).

### UX Details

1. Top-right call button appears for 1:1 chats only.
2. Full-screen call UI handles:
   - `ringing`: Accept / Reject
   - `offering`, `connecting`, `active`: Mute / End
   - `ended`: short terminal state with `Start Again` and dismiss affordance
3. If another chat has a live call, call button is disabled with clear messaging.

### Implementation Targets

1. `ios/Sources/Views/ChatView.swift` (toolbar affordance + removal of inline strip)
2. `ios/Sources/ContentView.swift` (screen-level presentation via `.fullScreenCover`)
3. New call UI components under `ios/Sources/Views/Call/`:
   - `CallScreenView.swift`
   - `ChatCallToolbarButton.swift`
   - `CallPresentationModel.swift`

### Exit Criteria

1. Calls can still be started/accepted/ended.
2. Active call no longer looks like "programmer art."
3. Chat screen is visually cleaner and message-focused.

## Phase 2: Navigation Continuity While Call Is Active

### Scope

1. Allow users to navigate elsewhere while call remains active.
2. Add a compact persistent return-to-call affordance (banner/pill).
3. Ensure incoming call can surface from non-chat screens.

### Implementation Targets

1. `ios/Sources/ContentView.swift` (global overlay + routing behavior)
2. `ios/Sources/Views/Call/ActiveCallPill.swift` (or similar)

### Exit Criteria

1. Call remains controllable when user leaves the originating chat.
2. User can always get back to full call controls in one tap.

## Phase 3: Optional Chat Timeline Call Events

### Scope

1. Add lightweight call event logging in the chat thread only after UI cleanup ships.
2. Keep event volume minimal (terminal/system events only).

### Proposed Model

1. Start with terminal events only:
   - "Call ended" + reason + optional duration
   - "Missed call"
2. Avoid logging every transition (`offering`, `connecting`, etc.) to reduce noise.

### Exit Criteria

1. Chat history gets useful call context without clutter.
2. No regressions in message rendering/performance.

## Phase 4: CallKit + Advanced Audio Routing

### Phase 4A: CallKit Control Plane (Foreground-First)

#### Scope

1. Add `CallKitCoordinator` in iOS layer (`CXProvider` + `CXCallController` + UUID map).
2. Outgoing flow:
   - Request `CXStartCallAction`.
   - In provider `perform start`, dispatch Rust `startCall(chatId)`.
   - On Rust state transitions, report:
     - `reportOutgoingCall(... startedConnectingAt:)` when `connecting`
     - `reportOutgoingCall(... connectedAt:)` when `active`
3. Incoming flow (while app is running):
   - When Rust enters `ringing`, call `reportNewIncomingCall`.
   - In provider `perform answer`, dispatch Rust `acceptCall(chatId)`.
   - In provider `perform end` while ringing, dispatch Rust `rejectCall(chatId)`.
4. End flow:
   - On Rust `ended(reason)`, report CallKit end reason once and retire mapping.

#### Exit Criteria

1. System incoming/outgoing call UI appears and is interactive.
2. Answer/reject/end from CallKit drives Rust correctly.
3. In-app call UI and CallKit stay synchronized without duplicate calls.

### Phase 4B: Action Parity (Mute/Hold Semantics)

#### Scope

1. Add deterministic mute action support for CallKit `CXSetMutedCallAction`.
2. Recommended Rust API addition:
   - `setMute(isMuted: Bool)` to avoid races inherent in `toggleMute`.
3. Hold support:
   - If runtime cannot hold media yet, explicitly fail `CXSetHeldCallAction` (do not fake hold).

#### Exit Criteria

1. AirPods/system mute actions stay in sync with Rust `is_muted`.
2. No mute flip-flop from repeated toggle races.

### Phase 4C: Audio Session Ownership Handoff

#### Scope

1. Keep voice config (`.playAndRecord`, `.voiceChat`, `.allowBluetoothHFP`) but hand activation lifecycle to CallKit delegate callbacks:
   - `provider(_:didActivate:)`
   - `provider(_:didDeactivate:)`
2. Update `CallAudioSessionCoordinator` so it can run in:
   - non-CallKit mode (current behavior)
   - CallKit-driven mode (activation delegated to CallKit)
3. Add observers/handling for:
   - interruptions
   - route changes
   - foreground/background transitions
4. Add route diagnostics and explicit route selection plumbing for speaker/receiver/Bluetooth where needed.

#### Exit Criteria

1. No one-way-audio regressions across route switches.
2. Audio activation/deactivation order is deterministic in logs.
3. Switching between outputs/inputs remains user-controllable.

### Phase 4D: PushKit + Background Incoming (If Product Requires It)

#### Scope

1. Register VoIP push token via `PKPushRegistry` and backend plumbing.
2. On VoIP push receive, report incoming call to CallKit immediately, then reconcile with Rust call state.
3. Add pending-invite cache for startup/background race windows.
4. Enforce strict dedupe by `call_id` + UUID mapping to avoid duplicate reports.

#### Exit Criteria

1. Incoming calls ring when app is backgrounded/terminated.
2. No PushKit contract violations due to delayed/missing CallKit reporting.

## Phase 5: Apple Audio Pipeline Migration (Stub)

### Scope

1. Replace iOS call capture/playback backend from Rust `cpal` to Apple-native voice pipeline.
2. Keep Rust as owner of call signaling/state/crypto/media transport; migrate only iOS audio I/O path.
3. Add an explicit iOS audio bridge contract for 20ms PCM frames (48 kHz mono) between Swift audio engine and Rust media loop.
4. Use Apple voice-processing path (`AVAudioSession` voice-call config + voice-processing-enabled I/O) so echo cancellation/noise handling come from Apple stack.
5. Preserve user route control (receiver/speaker/wired/Bluetooth HFP) and ensure route changes do not break duplex audio.
6. Remove iOS dependence on `cpal` for production call quality.

### Exit Criteria

1. On iOS, active calls use Apple-native capture/playback path end-to-end (not `cpal`).
2. Voice-processing behavior is measurably improved in real-device tests (echo, background noise, duplex stability).
3. Route switching and interruptions are stable with no one-way audio regressions.
4. Rust call-state behavior and in-app/CallKit UX remain unchanged by audio backend swap.
5. `cpal` path remains only as non-iOS fallback/testing backend.

## CallKit <-> Rust Mapping

1. Rust `offering`:
   - CallKit call exists, outgoing transaction active.
2. Rust `connecting`:
   - call `reportOutgoingCall(... startedConnectingAt:)`.
3. Rust `active`:
   - call `reportOutgoingCall(... connectedAt:)`.
4. Rust `ringing`:
   - call `reportNewIncomingCall`.
5. Rust `ended(reason)`:
   - map reason to `CXCallEndedReason` and call `reportCall(... endedAt:reason:)`.

## Risks and Mitigations

1. Duplicate end reporting:
   - mitigate with per-call one-shot "endedReported" guard.
2. Call action races (in-app vs system buttons):
   - route all actions through one coordinator and compare against latest Rust snapshot before dispatch.
3. UUID mismatches:
   - deterministic UUID strategy with persistence for active call.
4. Mute race from `toggleMute`:
   - add `setMute` action in Phase 4B.
5. Audio activation conflicts:
   - single owner policy: either CallKit owns activation or app does, never both at once.

## Validation Matrix (Phase 4)

1. Outgoing call start from in-app button, then end from CallKit UI.
2. Incoming call answer from CallKit UI, then end from in-app UI.
3. Incoming call reject from CallKit UI.
4. Mute/unmute from in-app and system route/mute controls.
5. Route switches during active call:
   - receiver <-> speaker
   - wired headset connect/disconnect
   - Bluetooth HFP connect/disconnect
6. Interruptions:
   - another cellular/FaceTime call
   - Siri interruption
7. App lifecycle:
   - foreground to background to foreground while active call is live
8. Multi-device behavior sanity checks on real devices.

## Primary References Used For This Plan

1. Apple docs: `CXProvider` and `CXCallAction` API surface.
2. WWDC20 Session 10111 (`Advances in App Background Execution`) CallKit/PushKit flow guidance.
3. WWDC23 Session 10235 (`Tune voice processing for audio input and output`) voice-processing/audio-session guidance.
4. WWDC23 Session 10233 (`Enhance your app's audio experience with AirPods`) mute-control and `AVAudioApplication` behavior.

## Recommended Execution Order

1. Phase 0 first (safe refactor + warning cleanup).
2. Phase 1 second (visible UX win).
3. Phase 2 third (continuity polish).
4. Phase 3 fourth (now complete).
5. Phase 4A next (CallKit control plane).
6. Phase 4C after 4A (audio-session ownership handoff).
7. Phase 4B next (mute/hold action parity).
8. Phase 4D last, gated on background incoming requirements/backend readiness.
9. Phase 5 after Phase 4 is stable (Apple-native iOS audio pipeline migration).

## QA Checklist (Per Phase)

1. iOS simulator + device smoke test for call start/accept/reject/end/mute.
2. Verify mic permission prompt and denied-state UX.
3. Verify no regression in chat input, scrolling, and navigation.
4. Re-run existing iOS UI tests; extend call test IDs where needed.
