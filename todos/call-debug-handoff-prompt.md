# Prompt: Debug Pika Call Stuck on `Calling…` (Prod Bot / Streambot)

You are debugging why iOS Pika calls stay on `Calling…` when dialing the deployed OpenClaw Marmot bot.

## Goal
Find and fix the root cause so an iOS `Start Call` leads to observable server-side `call.invite` handling and transitions away from `Calling…`.

## Known good identities
- Caller identity used on iPhone resolves to:
  - hex: `2284fc7b932b5dbbdaa2185c76a4e17a2ef928d4a82e29b812986b454b957f8f`
  - npub: `npub1y2z0c7un9dwmhk4zrpw8df8p0gh0j2x54qhznwqjnp452ju4078srmwp70`
- Bot peer:
  - npub: `npub1rtrxx9eyvag0ap3v73c4dvsqq5d2yxwe5d72qxrfpwe5svr96wuqed4p38`
  - hex: `1ac66317246750fe862cf47156b200051aa219d9a37ca018690bb3483065d3b8`
- `~/code/infra/nix/hosts/streambot.nix` allowlist includes the caller pubkey.

## What was observed
1. iPhone app logs show `StartCall` actions, but no visible `call.accept`/`call.reject` path.
   - Pulled log: `/tmp/pika-device-pika.log`
   - Notable lines: `dispatch action="StartCall"` around `01:08` and `01:10` UTC.
2. Streambot `openclaw-gateway` journal during call attempts showed no call lifecycle logs, only repeating config warnings.
3. `openclaw-gateway` and `marmotd` are running and were restarted successfully.
4. Direct Nostr query confirms `kind:445` traffic exists on relays (so relay is not empty/dead).
5. The deployed `marmotd` appears not to include expected call-log strings (`call_invite`, `call_session_started`) when inspected, suggesting the running binary/build path may not be call-capable even though repo work exists.

## Important current deployment/runtime state
- Server: `ssh streambot`
- Service: `openclaw-gateway.service`
- Current sidecar process cmd (after restart):
  - `.../marmotd daemon --relay wss://relay.primal.net --state-dir ... --allow-pubkey ...`
- Sidecar currently launched with only one `--relay` argument (`relay.primal.net`).
  - If group message fanout relies on other relays, invites may be missed.

## Suspected root causes (priority order)
1. **Wrong/old sidecar build deployed** (binary lacks call-handling/logging paths).
2. **Relay coverage mismatch** (`marmotd` only on one relay while caller publishes/receives on broader set).
3. **Insufficient call publish/ingest logging in `pika` and/or `openclaw-marmot`**, making failures silent.

## Required deliverables
1. Prove whether `call.invite` rumors from the iPhone are actually published and visible to bot group path.
2. Prove whether streambot sidecar can parse and react to `pika.call` payloads.
3. If not, deploy a known call-capable `openclaw-marmot` revision and verify runtime behavior.
4. Add/enable explicit logs for:
   - Invite publish success/failure with `call_id`
   - Incoming parsed call signal type + `call_id`
   - Reject/accept reason paths
5. Re-test end-to-end and capture logs that show:
   - caller `StartCall`
   - bot receives `call.invite`
   - bot emits accept/reject
   - caller transitions out of `Offering` (`Calling…`).

## Suggested concrete steps
1. On streambot, verify exact binary provenance for sidecar in use and match to source commit.
2. In `~/code/infra`, ensure flake input pins call-capable `openclaw-marmot` commit and redeploy.
3. Consider updating sidecar args to include all relays used by channel config (not just one `--relay`) if supported.
4. Add temporary high-signal logging in both:
   - `rust/src/core/call_control.rs` (publish outcome + signal parse)
   - `openclaw-marmot` `marmotd` signal ingest path.
5. Run a fresh call attempt while tailing:
   - `ssh streambot 'journalctl -u openclaw-gateway -f --no-pager'`
   - pull iPhone `pika.log` after attempt.

## Artifacts collected
- iPhone logs/config:
  - `/tmp/pika-device-pika.log`
  - `/tmp/pika-device-pika_config.json`
- Server log slices:
  - `/tmp/streambot-openclaw-0108-0112.log`
  - `/tmp/streambot-openclaw-last400.log`
- Decompiled string scan binary copy:
  - `/tmp/streambot-marmotd.bin`

## Notes
- Current iPhone config had `call_moq_url: "https://moq.local/anon"`; this likely breaks media plane later, but the immediate blocker is signaling never advancing from `Calling…`.
