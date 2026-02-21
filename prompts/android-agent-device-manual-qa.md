# Android manual QA via agent-device (Pika)

This is intended for a coding agent (or a human driving `agent-device`) to do exploratory QA.
It is NOT meant to run in CI.

## Preconditions
- An Android emulator or device is connected (`adb devices` shows it).
- App is installed: `just android-install`
- Optional helper: `just android-agent-open` boots a target and opens Pika via `agent-device`.

## Recommended agent-device session

1. Open the app:
   - `agent-device --platform android open com.pika.app`
2. Smoke routing:
   - If on Login: tap `Create Account`.
   - Confirm Chat list screen is shown (top bar title `Chats`).
3. Deterministic offline path (note-to-self):
   - Tap `My npub` (person icon).
   - Copy or read the displayed `npub...`.
   - Close dialog.
   - Tap `New Chat` (+ icon).
   - Paste your own `npub` and press `Start chat`.
   - Confirm chat title is `Note to self`.
4. Messaging:
   - Type `hi` and press `Send`.
   - Confirm the message bubble appears immediately.
   - If delivery indicator shows `!`, press retry (if surfaced) or just note it; publish failures are acceptable offline.
5. Navigation:
   - Tap back arrow to return to `Chats`.
   - Confirm the new chat appears in the list.
6. Logout and restore:
   - Tap Logout.
   - Confirm you return to the Login screen (`Pika` title).
   - Relaunch app and confirm session restore behavior is as expected (if nsec was stored).

## Amber signer check (real external signer)
1. Open Amber:
   - `agent-device --platform android open com.greenart7c3.nostrsigner`
2. Ensure at least one Amber account exists (create/import manually if needed).
3. Return to Pika:
   - `agent-device --platform android open com.justinmoon.pika.dev`
4. Exercise the Amber login/signing entrypoint once available in Pika and confirm:
   - Pika receives `pubkey` from Amber.
   - Signing approval in Amber completes and Pika advances.
   - Reject path in Amber surfaces a clear error in Pika without crashing.

## Useful agent-device commands
- `snapshot -i -c` to get clickable elements quickly.
- `find <text> click` for buttons/labels.
- `screenshot --out .tmp_android_qa.png` for bug reports.
