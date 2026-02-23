# Reliable MoQ Experiment (Demo Branch)

This branch exists to evaluate whether a Reliable-MoQ chat path is materially faster than Nostr relays for foreground typing indicators and chat message delivery.

## Decision

We are **not pursuing Reliable-MoQ chat transport** at this time.

Reason: in apples-to-apples paired A/B runs on deployed `*.pikachat.org` east/eu relays, the measured differences were small and statistically inconclusive, far below the target bar of `2-3x` improvement.

## What was implemented

- Reliable-MoQ prototype relay/client test harness:
  - `rust/tests/support/reliable_moq.rs`
- Deterministic profile tests (gap repair, dedupe, reconnect, error codes):
  - `rust/tests/reliable_moq_profile.rs`
- Network benchmark and interleaved paired A/B mode:
  - `rust/tests/perf_reliable_moq.rs`
- Report generator:
  - `scripts/reliable-moq-report`
- Recipes:
  - `just perf-reliable-moq`
  - `just report-reliable-moq`

## Latest measured evidence

From report `artifacts/reliable-moq/report-20260223T061250Z.md`:

- Paired A/B `pikachat us-east` (nostr vs mcr+moq):
  - delta (nostr - moq): `-8.7ms`
  - 95% CI: `[-18.6, 1.1]`
  - verdict: **inconclusive**
- Paired A/B `pikachat eu` (nostr vs mcr+moq):
  - delta (nostr - moq): `8.8ms`
  - 95% CI: `[-18.4, 35.9]`
  - verdict: **inconclusive**
- Paired A/B `moq us-east` (logos vs pikachat):
  - delta (logos - pikachat): `-4.4ms`
  - 95% CI: `[-13.3, 4.5]`
  - verdict: **inconclusive**

## How to rerun

Run full matrix + paired benchmark and generate report:

```sh
just report-reliable-moq
```

Useful env knobs:

- `PIKA_BENCH_MSG_COUNT` (default `20`)
- `PIKA_BENCH_RUNS` (default `3`)
- `PIKA_BENCH_PAIR_RUNS` (default `8`)
- `PIKA_BENCH_SKIP_MATRIX=1` (paired-only)
- `PIKA_BENCH_SKIP_PAIRS=1` (matrix-only)

Parse an existing benchmark log into a new report:

```sh
./scripts/reliable-moq-report --from-log <path-to-benchmark.log>
```
