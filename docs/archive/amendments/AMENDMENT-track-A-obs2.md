# AMENDMENT — TRACK A — OBS-2: ingestion funnel loop-stages + telemetry snapshot wiring

**Hand this to track-A's loop.** Operator-endorsed 2026-06-14. Bus (read at priority (a)):
`docs/reviews/GATE-FINDINGS-LATEST.md`. Contract: `docs/design/ingestion-observability-contract.md`
§2 ("one writer, many readers"). This is the **last open track-A build item** (BUILD_PLAN OBS-2);
the daily-belief-resolution drive-wiring is a separate commit already in the verifier's hands.

## What changed
OBS-1 (track-D) built the telemetry DATA SURFACE — the per-source VALIDATE stages — and left the
**loop stages at 0**. OBS-2c (track-B) built the ROTA READ side (the V1/V2/V3 boards project the
snapshot). **OBS-2 is the missing WRITE side:** the ingestion loop must populate the funnel's
loop-stage counts and PUBLISH the snapshot so the metrics renderer + the ROTA boards read live data
instead of zeros.

## The build (fortuna-live; the `drive()` ingestion seam — sequence AFTER the daily-resolution wiring)
1. In the ingestion segment of `drive()`, set the `FunnelCounts` LOOP stages each pass: `normalized` /
   `deduped` / `persisted` / `persist_failures` (the counts AFTER validation — OBS-1 owns the validate
   stages, you own the loop stages).
2. PUBLISH the `IngestionTelemetry` snapshot behind `Arc<RwLock<IngestionTelemetry>>` — ONE writer (the
   loop), MANY readers (the metrics renderer + the ROTA handlers via the OBS-2c reader). The writer takes
   the write lock briefly per pass; readers never block it (§2).
3. Thread the handle into the daemon composition (`main.rs`) so the dashboard state + metrics renderer get it.

## Discipline (non-negotiable — the verifier gates on the MERGED tree, mutation-proven)
- READ-ONLY observability: telemetry, NOT the money path. No order, gate, or belief touched.
- Honest counts: a stage is 0 because nothing happened, NEVER a fabricated value; the snapshot is a pure
  projection. No `unwrap`/`panic`; a publish failure ALERTS-and-continues (never crashes the loop).
- Clock-injected (`generated_at` is Clock-derived, never `SystemTime`). Untrusted payloads (5.11) never
  reach the snapshot beyond the existing redacted summary.
- Tests POPULATED-PATH: a test green under a stubbed-empty source does NOT count — drive REAL signals
  through the loop and assert the loop-stage counts move + the snapshot is readable concurrently.
- `cargo fmt --check`, clippy `-D warnings`, full `cargo test --workspace`, `scripts/run-dst.sh` all
  green; tick the OBS-2 BUILD_PLAN box with a one-line note; ledger your response in GAPS (never the bus).

## When you finish
Push; the verifier gates on the MERGED tree, mutation-proven. A BLOCK naming track-A preempts your queue.
