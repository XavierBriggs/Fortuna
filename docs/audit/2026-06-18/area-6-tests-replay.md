# Area 6 — Test & Replay Posture

## Summary

The test corpus is large (153 test files, 16 crates), well-classified, and demonstrably mutation-resistant: invariant tests, DST harnesses, property tests, contract/fixture tests, and integration tests all exist and pass. The DST battery runs 10 seeded harnesses (including a paper-on-live DST and an ingestion-scheduler DST), regression seeds are pinned in `crates/fortuna-core/dst-corpus/`, and the mind-tests claim of 19/19 passing is **verified fresh** (`mind.rs: 19 passed; 0 failed`). Replay at the bus level is proven byte-identical (`replay_verify`). Settlement→PnL, calibration persistence, stale-book marking, demo-cannot-execute, and idempotency are all tested — though stale-book is tested only as a mark-policy concern and not as a gate-rejection path, and PnL rebuilding from audit events (not in-memory state) has no standalone test. The single most dangerous gap for demo readiness is that the **WS trade-frame path** (paper fills from a live busy-market replay) is explicitly fixture-blocked with no recorded trade frame — so the core paper-fill realism proof for `fortuna start paper-demo` depends entirely on a synthetic public-REST recording plus a `MockKalshiTransport` harness, not a real WS event stream.

---

## Findings

| Severity | Readiness | Finding | Evidence (path:line) | Why it matters | Root cause | Recommended fix | Suggested test |
|---|---|---|---|---|---|---|---|
| P2 | SERVES | WS trade-through fill proof is fixture-blocked: no public WS `trade` frame captured; paper fill realism proven via `trades__public_recorded.json` (REST public endpoint, real data) and `MockKalshiTransport` but NOT from a live WS `trade` event | `crates/fortuna-runner/tests/recorded_replay.rs:342` comment; `GAPS.md:2179–2182`; `crates/fortuna-paper/tests/recorded_public_trades.rs:1` | Demo runs on live Kalshi data via WS; a WS `trade` frame is the canonical fill trigger (spec §11); the REST workaround proves the rule, but the actual live-WS code path under demo load has no recorded regression seed | Operator never captured a busy-market Kalshi WS session | Operator runs `record_kalshi_fixtures` on an active market per `GAPS.md:5505`; the harness exists | `crates/fortuna-runner/tests/recorded_replay.rs` test 5 (trade-through; currently asserts zero fills as placeholder) |
| P2 | SERVES | PnL cannot be rebuilt from audit events alone: no standalone test that reads the `audit` table, replays events, and reconstructs gross/net PnL; settlement→position→PnL is proven in memory (`crates/fortuna-state/tests/settlement.rs`, `positions.rs`) but no audit-manifest→PnL replay path exists | MISSING: no file in `crates/` tests parity between `audit` rows and computed PnL; `crates/fortuna-state/tests/settlement.rs:1`, `crates/fortuna-state/tests/positions.rs:47–313` | Post-incident audit "what were our fills and outcomes" requires tracing the audit table; if the in-memory and ledger representations diverge, realized PnL is unrecoverable from the audit alone | Spec 5.7 says decisions are replayable from audit; the PnL math path (fill→position→settlement) lives in `fortuna-state` not `fortuna-ledger`, so a ledger→state reconstruction is not covered | Add a `fortuna-ledger` test that seeds audit rows + settlement rows and asserts that a replay reconstructs position PnL exactly | Round-trip: insert fills + settlement into Pg; fold through `SettlementLedger::record_*`; assert `realized_pnl == Cents::new(X)` |
| P2 | SERVES | Calibration persistence round-trip between `fortuna-cognition` and `fortuna-ledger` is not covered as a single test: `calibration_params_are_versioned_append_only_config` (ledger) and `CalibrationParams` serde tests (cognition) are separate; no test reads the fitted `CalibrationParams` back from `CalibrationParamsRepo` and feeds them to `calibrate()` in one transaction | `crates/fortuna-ledger/tests/ledger.rs:1087`; `crates/fortuna-cognition/tests/calibration.rs:138`; `crates/fortuna-live/tests/compose.rs:78` | A calibration params row that serde-round-trips in isolation but fails when deserialized from Pg JSON and fed to the live calibration function would go undetected until demo | Params are stored as `serde_json::Value` in Pg; the cognition `fit_platt` / `CalibrationParams` deserialization is tested independently but not in a Pg-to-`calibrate()` end-to-end | Add a `fortuna-live` or `fortuna-ledger` test: insert params via `CalibrationParamsRepo`, fetch via `.latest()`, deserialize with `serde_json::from_value`, invoke `calibrate(0.7, &params)`, assert result ≠ 0.7 | Extend `crates/fortuna-live/tests/compose.rs:calibration_scope_builds_context_and_quality_from_the_ledger` with an assertion that the returned context's `params` drives a non-identity `calibrate()` |
| P2 | PARK | No soak test for the full paper-on-live loop (continuous ticks over simulated time, checking for memory growth or accrued state divergence) | MISSING: `crates/fortuna-runner/tests/perp_sim_soak.rs` is a single-tick soak for perp; no multi-tick time-series soak for paper-on-live | A demo session runs for hours; memory leaks or accruing state (belief cache, market map) are only caught under extended operation | The existing `daemon_smoke.rs` runs a small number of ticks; no multi-hour simulation exists | Add a 1000-tick `paper_live_multi_tick_soak` that measures heap or at minimum asserts no `OOM`-like behavior | `cargo test -p fortuna-live --test paper_live_multi_tick_soak -- --nocapture` with wall-clock timeout |
| P2 | SERVES | Stale-book does not block order submission at the gate: the stale-book path is tested as a mark-quality concern (`wide_flag = true`, `Cents::ZERO` mark) in `fortuna-state`, but no gate check in `fortuna-gates` rejects an order when the reference book is stale | `crates/fortuna-state/tests/marks.rs:67`; `crates/fortuna-gates/src/pipeline.rs:74-100` (no `BookAge` gate check); `crates/fortuna-gates/tests/pipeline.rs` (no stale-book test) | A stale book gets a conservative `wide_flag` mark but the strategy can still issue an order at any price; in live trading a stale book means the real market may have moved significantly | The gate uses the book for `PriceSanity` and `EdgeFloor` but has no `max_book_age_ms` guard; stale-book staleness is entirely in the state/mark layer | Either add a `BookAge` gate check or add a test asserting that a `wide_flag` mark forces the edge floor to zero (failing the `EdgeFloor` check); document which invariant covers this | `crates/fortuna-gates/tests/pipeline.rs`: `stale_book_wide_flag_fails_edge_floor_check` |
| P3 | BLOAT-cut | DST corpus has 7 seed files but only 3 anchor seeds with story comments; `perp_event_basis_sum_order_boundary.seed` has a story comment; the `paper-live-through-not-touch.seed` is unambiguously documented; however two seeds (`perp-curve-exceeded`, `perp-event-basis-fee-trap-boundary`) are in `dst-corpus/` without being in the DST runner (`scripts/run-dst.sh` does not explicitly replay `.seed` files by name — it relies on `fortuna-core --test dst -- --nocapture --seeds N` which presumably replays the anchor seeds) | `crates/fortuna-core/dst-corpus/README.md`; `scripts/run-dst.sh:10-12`; `crates/fortuna-core/dst-corpus/perp-curve-exceeded-11819682492387934495.seed` | If the `fortuna-core dst` harness does not actually enumerate all `.seed` files in `dst-corpus/`, regression seeds outside the three anchor seeds may pass vacuously | DST runner comment says "replays every regression seed" but the implementation may only exercise seeds the `--replay-seed` dispatch handles for `fortuna-core/tests/dst.rs`; `perp_event_basis_*` seeds would need to be replayed via `perp_event_basis_dst` harness, not `fortuna-core --test dst` | Audit `crates/fortuna-core/tests/dst.rs` for the seed enumeration logic; add an explicit seed-file iteration check in `run-dst.sh`; document which harness owns each seed | `cargo test -p fortuna-core --test dst -- --replay-seed <each corpus seed>` |

---

## Trace / Narrative

**Classification of the 153 test files**

| Class | Count | Examples |
|---|---|---|
| Unit | ~60 | `beliefs.rs`, `calibration.rs`, `money.rs`, `marks.rs`, `positions.rs`, `fees.rs` |
| Integration / e2e | ~30 | `aeolus_e2e.rs`, `persona_e2e.rs`, `daemon_smoke.rs` (25 `#[sqlx::test]` tests), `pg_journal.rs`, `pg_audit.rs` |
| Contract / fixture | ~15 | `kalshi_recorded_roundtrip.rs`, `recorded_replay.rs`, `recorded_public_trades.rs`, `kalshi_adapter.rs`, `basis_live_fixture.rs` |
| DST (seeded-chaos) | 10 harnesses | `settlement_dst.rs`, `synthesis_dst.rs`, `perp_dst.rs`, `funding_forecast_dst.rs`, `perp_event_basis_dst.rs`, `paper_live_dst.rs`, `ingest_dst.rs`, `persona_dst.rs`, `persona_orchestrator_dst.rs`, `daemon_smoke.rs` (smoke is also in `run-dst.sh`) |
| Property (`proptest`) | ~8 | `properties.rs` (gates), `positions.rs`, `margin.rs`, `perp.rs`, `scoring.rs`, `veto.rs`, `fees.rs`, `i1_universal_gate.rs`, `i2_drawdown_human_rearm.rs` |
| Safety / invariant | 13 | `i1` through `i7`, `perp_i1–i4`, `i_paper_live_no_real_order.rs` |
| Soak | 1 | `perp_sim_soak.rs` (single-tick injection) |
| Replay | 2 | `bus.rs` (`replay_verify`), `paper_live_dst.rs` (byte-identical replay assertion) |

**Mind tests: 19/19 verified**
`cargo test -p fortuna-cognition --test mind 2>&1 | tail -5` confirms `19 passed; 0 failed`. Claim is accurate.

**DST corpus: 7 seeds, 5 named in `dst-corpus/`**
- `anchor-31337.seed`, `anchor-777.seed`, `anchor-8675309.seed` — core DST regression seeds for `fortuna-core --test dst`
- `paper-live-through-not-touch.seed` — paper-on-live determinism seed, loaded in `paper_live_dst.rs:seed_from_corpus()`
- `perp_event_basis_sum_order_boundary.seed` — used by `perp_event_basis_dst`
- `perp-curve-exceeded-11819682492387934495.seed` — apparent regression for perp DST
- `perp-event-basis-fee-trap-boundary.seed` — the 2026-06-15 fee-trap fix seed (story in GAPS.md)

`run-dst.sh` calls `cargo test -p fortuna-core --test dst -- --nocapture --seeds "$N"` and each of the six other DST harnesses individually. The core harness likely loads `dst-corpus/*.seed` at test startup per the README; confirmed by the README: "Replays every regression seed in crates/fortuna-core/dst-corpus/". It is unclear whether `perp-curve-exceeded` (which is a perp DST boundary) is replayed by `fortuna-core --test dst` or by `perp_dst`; neither harness's source was read in full.

**Stale-book path**
`crates/fortuna-state/tests/marks.rs:67` (`stale_book_still_uses_touch_but_flags_wide`) proves that a stale book produces a `wide_flag` mark and a conservative exit value. This is the correct behavior for the _state_ layer. However, `crates/fortuna-gates/src/pipeline.rs` lists 10 gate checks (lines 74–100) and none is `BookAge`. The gate does receive `book: Option<&OrderBook>` via `GateInputs` and uses it for price-sanity and edge-floor, but does not check `book.as_of` vs. `now`. This means a strategy can submit an order on a book that is hours old; the only consequence is that the mark is `wide_flag=true` which may cause the sizing to be conservative but does not block the order.

**Settlement→PnL chain**
`crates/fortuna-state/tests/settlement.rs` (T1.4) proves the pending→posted→confirmed lifecycle, reversal, and conservation. `crates/fortuna-state/tests/positions.rs` proves FIFO PnL math (buy→sell→realized). `crates/fortuna-runner/tests/settlement_loop.rs` and `settlement_dst.rs` prove the full composed settlement cycle including discrepancies and halts. What is **not tested**: reading settled fills and settlement records from Postgres and rebuilding PnL from scratch (the `audit` table or `intent_events` table) — a post-incident PnL reconstruction path.

**Calibration persistence**
`crates/fortuna-ledger/tests/ledger.rs:1087` (`calibration_params_are_versioned_append_only_config`) proves that `CalibrationParamsRepo` inserts, versions, and retrieves params. `crates/fortuna-live/tests/compose.rs:78` (`calibration_scope_builds_context_and_quality_from_the_ledger`) proves that `calibration_for_scope` fetches params from the ledger and builds a `CalibrationContext`. The gap is that neither test asserts `calibrate(p, &ctx.params) != p` — i.e., that the fetched params actually deform a raw probability. The synthesis daemon smoke (`daemon_smoke.rs:982`) asserts a submitted order is produced when ledger calibration is loaded, which implies the calibration chain works end-to-end, but this is a high-level assertion.

**Demo-cannot-execute invariant**
`crates/fortuna-invariants/tests/i_paper_live_no_real_order.rs:245` (`paper_on_live_cannot_place_or_cancel_real_orders`) is a dedicated invariant test: it constructs a `PaperLiveVenue` with a `GuardedKalshiTransport` that panics on any non-GET or order-endpoint call, then drives 50 `place()` and 5 `cancel()` calls and asserts all underlying HTTP calls are GET-only. This is a strong, adversarial test. `paper_live_dst.rs` additionally asserts byte-identical replay under the corpus seed. Both SERVE the demo target.

**Replay determinism**
`crates/fortuna-core/tests/bus.rs` (`replay_verify`) proves that re-injecting recorded external events through the same handlers produces a byte-identical derived event stream. The DST harnesses (`synthesis_dst.rs:4`, `settlement_dst.rs:5`, `paper_live_dst.rs`) all assert byte-identity on replay. The `replay.sh` script wires `--replay-seed N` to the core DST harness or `replay-verify` binary for structural JSONL verification (the binary is referenced in `replay.sh` but was not confirmed built). No **full decision replay** from a live database audit dump is automated — the spec (5.7) says proposals carry manifest hashes so the decision is replayable, but the replay is checked at the runner-audit-sink level (`synthesis_loop.rs:812`) rather than by re-feeding audit rows to reconstruct outputs.

**Idempotency**
Multiple layers have idempotency tests: `resolve_and_score_funding_beliefs` has an explicit idempotency test in `resolve_and_score.rs:second_run_is_idempotent`; gate pipeline has `GateCheck::Idempotency` and `pipeline.rs:595` covers it; `reservations.rs` proves `release` is idempotent; `i4_killswitch_revocation.rs:251` proves `write_revocation` is idempotent. Weather resolution idempotency (`weather_resolve.rs:a_second_run_is_idempotent`) is also tested.

**Property tests**
`fortuna-gates/tests/properties.rs` (2 proptest invariants over arbitrary order sequences), `fortuna-state/tests/positions.rs` and `margin.rs` (proptest over FIFO PnL and margin math), `fortuna-cognition/tests/scoring.rs` and `veto.rs` (proptest over scoring and veto), `fortuna-invariants/tests/i1_universal_gate.rs` and `i2_drawdown_human_rearm.rs` (proptest for invariant safety properties). Coverage is good on numeric/financial domains; cognition proptest coverage is limited to scoring and veto.

---

## Self-Adversarial Pass

**Am I overstating the stale-book finding (P2)?** Potentially. The gate does use the book for `PriceSanity` (price within band vs. mid) and `EdgeFloor` (edge vs. fair_value). If the book is stale and the `fair_value` the strategy computed is also stale, both checks use the same stale data consistently and the gate still passes. The real risk is that the strategy's `fair_value` diverges from the real market while the book object passed to the gate is stale — which happens in normal operation when book updates are delayed. The `max_book_age_ms` in `MarkPolicy` is the only guard. This is a design choice, not necessarily a bug, but it is undocumented in gate comments and untested. Severity P2 is appropriate but the finding could be interpreted as P3 "document this design decision."

**Am I understating the WS trade-through gap?** No. The GAPS.md explicitly calls this out (`GAPS.md:2179-2182`: "trade-through stays RED until the busy-market trade-frame capture"). The REST public-trades proof (`recorded_public_trades.rs`) is a genuine real-data test and satisfies the doctrinal requirement. The gap is narrower than it appears: the WS _book_ path is fully proven; only the WS _trade_ path is fixture-blocked.

**Is the "PnL rebuild from events" finding real?** Yes. Reading `fortuna-state/tests/positions.rs` and `settlement.rs` confirms they use in-memory structures, not Pg. There is no test in any `tests/` directory that seeds Pg with `fills`, `intent_events`, and `settlement_chain` rows and then reconstructs `realized_pnl`. The `pg_journal.rs` test proves recovery (OrderManager can fold from Pg), but the fold is of intent states, not position PnL.

**Did I miss any major test classes?** The CLI integration test (`fortuna-cli/tests/cli_integration.rs`) was not read; it likely covers command-level behavior. The `shutdown.rs` test in `fortuna-live` was not read. These are minor omissions; the overall classification is sound.

**False positives?** The calibration persistence finding (P2) might be conservative — the daemon smoke `synthesis_arm_trades_with_ledger_calibration_and_an_injected_mind` (line 982) does assert an order is placed, which is downstream of calibration loading and applying. If that test passes, the chain works. The gap is test legibility and mutation-resistance, not functionality. Downgrading to P3 is defensible.

---

## Open Questions for the Lead

1. **Does `fortuna-core --test dst` enumerate all files in `dst-corpus/` or only the three anchor seeds?** If `perp-curve-exceeded` and `perp-event-basis-fee-trap-boundary` are not replayed by any active harness call in `run-dst.sh`, the battery passes vacuously on those regressions. The `run-dst.sh` calls for `perp_event_basis_dst` but that is the randomized harness, not a seed-replay.

2. **Is there an automated gate or CI step that fails if `replay.sh` cannot reconstruct a recording?** The `replay-verify` binary is referenced in `replay.sh` but no CI step was found that runs it against a stored recording. If the bus replay is only exercised in `bus.rs` unit tests (not against a real session recording), it is not protecting the live-decision audit chain.

3. **Who owns the WS trade-frame capture?** `GAPS.md:5505` says it is in the operator queue. Is this blocked on calendar (the operator needs to be at a terminal during active trading hours) or on tooling? If tooling, `record_kalshi_fixtures` may already be ready.

4. **Is the stale-book design intent documented in the gate spec section (5.3)?** If the decision was intentional (stale book → conservative mark → zero sizing haircut → zero edge → EdgeFloor rejects), that chain should be documented and a test added that exercises the full path. The current test only covers the mark layer.

5. **Is there a post-incident runbook for PnL reconstruction from the `audit` + Pg tables?** If not, add one before enabling live trading — the in-memory state is not persisted between restarts except through the intent journal, which covers order state but not position PnL.
