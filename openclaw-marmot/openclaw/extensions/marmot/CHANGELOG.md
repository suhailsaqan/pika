# Changelog

## 0.3.2

- Fix: use sledtools/pika as default sidecar repo for binary downloads.

## 0.3.1

- Release pipeline: tag `marmotd-v*` is now the single source of truth for versioning across Rust binary and npm package.

## 0.3.0

**Breaking: MLS library upgrade (openmls 0.7.1 -> 0.8.1)**

MDK upgraded from a pinned openmls git fork (0.7.1) to crates.io openmls 0.8.1. MLS wire formats and state are **not interoperable** across this boundary.

- All existing MLS groups will stop working. Users must create new groups after upgrading.
- The bot's MLS state must be wiped on deploy (`/home/openclaw/.openclaw/marmot/accounts/default/mdk.sqlite`).
- All pika app clients must update to the matching mdk revision before or at the same time as the bot deploy.

## 0.2.0

Initial public release.
