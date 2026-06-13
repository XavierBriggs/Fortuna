# Track A — changelog

Running worklog for TRACK A (venue / exec / recovery). Newest first. One entry
per committed slice: what, why, commit, battery result, any ledgered block.

`GAPS.md` remains the message bus (gaps / blocks / cross-track ledger); this file
is the worklog. Independent gate verdicts for landed slices live in
`docs/reviews/` (e.g. `t42-*-gate-*.md`, `m3-rearm-gate-*.md`).

Prior to this log (gated, on main): M3 rearm notices; T4.2 (i) Kalshi WS dial
slices 1-2 + 4-5 + concrete transport (see the gate records above).

---

## 2026-06-13 — T4.2 (ii) book-driven recorded-stream replay into PaperVenue — `fc5bd64`

**What.** New integration test `crates/fortuna-runner/tests/recorded_replay.rs`
(7 tests; test-only, no production change). It drives the production replay seam
`KalshiWsParser -> BookAssembler -> fortuna_paper::feed_stream_event ->
PaperVenue` over the operator-recorded Kalshi WS fixtures
(`fixtures/kalshi/ws__orderbook_trade_{yes,noleg}.jsonl`) and composes both
mechanical strategies (`mech_structural`, `mech_extremes`) over the replayed book.

**Why.** Queue item 2(ii): exercise the venue/exec/paper path against the
RECORDED fixtures "as if live," not doc-derived/synthetic frames.

**Asserts.** Gapless, fully-typed parse of both fixtures (0 trade frames); the
EXACT assembled book inside PaperVenue (yes 47×3 / 52×2; noleg 47×3 / 48×1,
including a transient empty book that replays clean); book-only replay yields NO
fills (a resting maker order is untouched); both strategies consume the recorded
book and abstain correctly, with liveness controls proving each fires on a
qualifying input (so the abstentions are genuine, not dead wiring).

**Fixture-blocked (ledgered in GAPS, never fabricated).** (1) Trade-through
replay — no public trade frame was recorded (quiet market); paper maker fills are
trade-driven (spec 11). (2) Structural-arb replay — a single-market recording
cannot complete a bracket; needs a multi-market bracket fixture.

**Battery.** `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D
warnings`; `cargo test --workspace` (126 targets, 0 failed); `scripts/run-dst.sh
200` (4 corpus + 200 seeds, 0 invariant violations; daemon_smoke 15/15;
ingest_dst 5/5). code-reviewer pass folded in (M1 liveness controls; m1 `gated()`
note). Protected crate untouched.

**Shared docs.** No architecture/runbook change warranted (test-only; the replay
seam and strategies are unchanged production code). BUILD_PLAN T4.2 progress
noted (box stays unticked — slices iii–v remain); queue item 2(ii) marked done.
