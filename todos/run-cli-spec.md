# Pika Run CLI Spec (tools/pika-run)

## Goals

Provide a single, discoverable, scriptable CLI for:

- Running iOS and Android builds on simulator/emulator or physical devices.
- Enumerating available run targets (UDIDs/serials) deterministically.
- Producing stable, structured output for `scripts/agent-brief` and for other coding agents.

This CLI should become the canonical documentation surface for "how do I run the app?".
`just` remains the task runner, but `just run-*` becomes a thin wrapper over this CLI.

Non-goals:

- Replace Gradle/Xcodebuild. The CLI orchestrates them.
- Provide interactive UI selection by default. Ambiguity should fail with a list.

## Why We Need This (Problem Statement)

`just` is excellent for defining entrypoints but not for:

- Expressive argument parsing (modes, defaults, target selection).
- High-quality `--help` and stable "list targets" commands.
- Dynamic introspection ("what targets exist right now?") as first-class output.

This creates a repeated failure mode for agents:

- `scripts/agent-brief` currently runs `just --list`, which does not communicate flags,
  defaults, or target selection rules.
- Agents then make guesses and regress the run experience.

## CLI Location and Form

Implement a repo-local CLI script at:

- `tools/pika-run` (executable)

Implementation language:

- Prefer a single-file script with a real arg parser.
  Options:
  - Python (`argparse`), already used in other tools.
  - Node (TypeScript/JS) is possible but increases boot time and dependencies.
  - Rust binary is fine but adds build steps for "help" and "list targets"; not ideal for agent context.

Requirement:

- `tools/pika-run --help` and `tools/pika-run <platform> list-targets` must run quickly and not build.

## Command Overview

Top-level:

- `pika-run ios …`
- `pika-run android …`
- `pika-run doctor …` (optional; see below)

Common conventions:

- All commands support `--help`.
- All commands support `--json` where it makes sense.
- Ambiguous selection fails with exit code `2` and a copy/paste-able list of choices.
- When `--json` is used, errors are still non-zero but should include machine-readable info on stderr
  or in a JSON object (choose one approach and keep it consistent).

### `pika-run ios`

Subcommands:

1. `pika-run ios list-targets`
   - Lists:
     - Connected iPhone devices (UDID + name + OS version if available)
     - Available/booted simulators (UDID + name + runtime)
   - Default output: human readable.
   - With `--json`: structured list.

2. `pika-run ios run`
   - Modes:
     - `--sim` (simulator)
     - `--device` (physical)
   - Target selectors:
     - `--udid <UDID>` (applies to sim or device depending on mode)
   - Defaults:
     - If `--device` is explicitly passed: run on device.
     - Else: run on simulator.
   - Ambiguity handling:
     - If mode requires a device and more than one is connected: fail with list and suggestion `--udid`.
     - If mode requires a simulator and more than one matching simulator exists: fail with list and suggestion `--udid`.
   - Device-specific requirements:
     - Requires `PIKA_IOS_DEVELOPMENT_TEAM` for device signing.
     - Requires device unlocked; should wait up to N seconds then fail with explicit message.
   - Options:
     - `--console` attach device console (equivalent of current devicectl `--console`).
     - `--no-relay-override` disables writing the dev relay config.
     - `--bundle-id <id>` override bundle id (primarily for device signing edge cases).
   - Outputs:
     - Always print: chosen mode + chosen UDID + bundle id.
     - With `--json`: emit a JSON object that includes chosen mode/udid/bundle_id and app pid if available.

### `pika-run android`

Subcommands:

1. `pika-run android list-targets`
   - Lists:
     - Connected emulators
     - Connected physical devices
   - With `--json`: include serial, type, and any basic properties we can query quickly.

2. `pika-run android run`
   - Modes:
     - `--emulator`
     - `--device`
   - Target selectors:
     - `--serial <serial>`
   - Defaults:
     - If `--device` is explicitly passed: prefer physical device.
     - Else: prefer emulator if present or can be started.
   - Ambiguity handling:
     - Multiple emulators/devices in the chosen class: fail with list and suggestion `--serial`.
   - Emulator management:
     - Should be able to "ensure emulator" (start AVD + wait for boot).
     - Expose as part of `android run` rather than being a separate script.
       Rationale: prevents drift between the "ensure" script and the runner.
   - Options:
     - `--app-id <id>` override (defaults to debug id).
     - `--no-relay-override` disable config write.
     - `--adb-reverse …` optional (port forwarding), matching existing behavior.
   - Outputs:
     - Always print chosen serial + type.
     - With `--json`: include serial + type and launch result.

## Environment and Config

We should treat `.env` as "remembered defaults" and CLI flags as explicit overrides.

Rules:

- `.env` and `.env.local` are loaded only to fill defaults; they must not override already-set process env.
- CLI flags override everything else (including env).

Recommended env variables (align with existing practice):

- iOS:
  - `PIKA_IOS_DEVELOPMENT_TEAM`
  - `PIKA_IOS_BUNDLE_ID` (default debug id)
  - `PIKA_IOS_DEVICE_UDID`
  - `PIKA_IOS_CONSOLE`
- Android:
  - `PIKA_ANDROID_SERIAL`
  - `PIKA_ANDROID_APP_ID`
  - `PIKA_ANDROID_AVD_NAME`

## Defaults Policy (Explicit and Enforced)

Defaults should be stable and agent-friendly:

- iOS default: simulator.
- Android default: emulator (start one if none exists).

If a user explicitly requests device mode (iOS `--device`, Android `--device`), do not start/choose emulator.

If ambiguity exists in the requested class (multiple devices), fail with:

- Exit code `2`
- A list of exact `--udid/--serial` values.

## Integration With `scripts/agent-brief`

`scripts/agent-brief` should add sections (in addition to `just --list`) that pull
run-target information directly from the canonical CLI:

- `./tools/pika-run --help` (or `./tools/pika-run ios --help` and `android --help`)
- `./tools/pika-run ios list-targets`
- `./tools/pika-run android list-targets`

Design constraints for agent brief:

- These commands must not require building the app.
- These commands must not require `nix develop`.
  If `adb`/`xcrun` are missing, list-targets should still print a clear error and exit non-zero.
  Agent-brief already tolerates failures and includes output.

Optional enhancement:

- Use `--json` mode and have `agent-brief` render a concise table for agents.
  (Only do this if it reduces noise; avoid huge blobs.)

## Relationship to `justfile`

Keep `just` as the entrypoint for high-level workflows (build, test, QA, E2E).
However, for anything that behaves like a user-facing CLI, prefer `tools/pika-run`.

Concrete changes planned:

- `just run-ios *ARGS` becomes:
  - `./tools/pika-run ios run {{ARGS}}`
- `just run-android *ARGS` becomes:
  - `./tools/pika-run android run {{ARGS}}`

`just --list` stays, but agents should rely on `pika-run --help` for run semantics.

## What Else in `justfile` Should Move?

Heuristic: if a `just` recipe is effectively a CLI with flags, modes, and selection logic,
it likely belongs in `tools/pika-run` (or another dedicated CLI).

Likely candidates:

- iOS/Android "target selection" logic:
  - `tools/android-emulator-ensure`, `tools/ios-sim-ensure`, and the run scripts
    should be consolidated under `pika-run` to avoid drift.
- "Doctor" commands:
  - `doctor-ios` could become `pika-run doctor ios` so the same CLI can explain missing runtimes/SDKs.
- "Device automation help" does not need to move; it is already an external CLI (`agent-device`).

Probably should stay in `just`:

- Build graph recipes (`ios-xcframework`, `android-rust`, `gen-kotlin`, etc.).
  Rationale: these are implementation details of the build pipeline, not user-facing selection logic.
- QA aggregates (`qa`, `pre-merge`).
  Rationale: CI/workflow oriented, not interactive CLI semantics.

Other areas with similar limitations (potential future CLIs):

- E2E orchestration: there are already `tools/ui-e2e-local` scripts. If these expand in complexity,
  they may deserve a dedicated `tools/pika-e2e` CLI rather than growing `just` flags.
- Relay/bot interop scripts: if interop becomes a matrix of modes/targets, consider a CLI.

## Exit Codes

Standardize across the CLI:

- `0`: success
- `1`: operational failure (tool missing, build failed, install failed, launch failed)
- `2`: user input needed (ambiguous targets, missing required flag/env like dev team in device mode)

## JSON Output (Optional But Recommended)

Support `--json` for:

- `list-targets`
- `run`

Suggested shape:

```json
{
  "platform": "ios",
  "action": "run",
  "mode": "sim",
  "target": {"udid": "...", "name": "..."},
  "bundle_id": "com.justinmoon.pika.dev",
  "result": {"launched": true, "pid": 12345}
}
```

This makes it easy for agents to reason about what actually happened.

## Implementation Notes / Constraints

- macOS bash is 3.2; avoid bash-isms in any shell wrappers.
- Keep `list-targets` and `--help` fast and dependency-light.
- When device launch fails due to lock state, the CLI should explicitly say "unlock the device"
  and optionally retry for a short window.

