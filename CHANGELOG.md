# Changelog

This is the FORTUNA project changelog. It follows [Keep a Changelog](https://keepachangelog.com/)
style. Each build track maintains its own **subsystem subsection** under
`## [Unreleased]`, so concurrent edits touch distinct sections and rarely
collide; the verifier reconciles the subsections on merge. Dates are UTC. One
concise bullet per logical change; newest-relevant first.

## [Unreleased]

### Cognition belief-pipeline & perps (fortuna-cognition / fortuna-ledger / fortuna-core, Track C)

The `prob_claims/v1` scalar-belief foundation + perp strategies (design
`docs/design/perp-strategies-and-scalar-claims.md`). Verifier-gated ACCEPT
(slices 1a + 1b + 2a + funding kernel) and MERGED to main @2809aea, 2026-06-13.
Slice 2b (`funding_forecast` producer) gated ACCEPT (dispersion-widening
mutation-proven) and MERGED to main @f949554, 2026-06-13.

#### Added

- **perp basis-v2 — turnkey example config + a parse guard** (`config/fortuna.example.toml`, `fortuna-live`
  compose tests; docs/test only, no runtime change): a fully-documented, commented `[perp_event_basis_v2]`
  stanza so the operator can switch the v2 trader (+ the coupled funding poller) on with a one-line config
  edit — `perp_market` + the bracket ladder, plus every DC-1..DC-6 knob shown at its operator-ENDORSED
  conservative default with an inline explanation + the I7 "DATA-ONLY until the soak proves the edge" note.
  A new `v2_all_knobs_parse_from_toml` test sets all 17 knobs to non-default values and asserts each binds,
  guaranteeing the shipped stanza is turnkey-correct (a typo'd/renamed field is caught). The DC-1..DC-6
  defaults are now operator-endorsed as the Sim-stage starting operating point (recorded in GAPS); v2 stays
  DATA-ONLY/Sim (I7) — a declared edge still waits on the measured soak result.
- **slice-3b-v2 + A2d follow-on — the daemon WIRE-IN (v2 strategy registration + the funding pipeline live)**
  (`fortuna-live` compose/boot/daemon/main, ADDITIVE + GATED, **0 deletions**): makes the perp basis-v2 strategy
  AND the funding resolve/score + poller pipeline LIVE in the daemon. (1) A `[perp_event_basis_v2]` config
  section + `build_perp_event_basis_v2_config` (mirrors rung-0; `perp_market` + `ladder` required, the 16 DC
  knobs optional with the DC defaults) + the `Option` field on `DaemonToml`; the strategy is composed at BOTH
  runner sites, gated on the section's presence. (2) `resolve_and_score_funding_beliefs` is now called PER
  SEGMENT as a sibling of `persist_scalar_beliefs` (same scalar-pool gate, same alert-never-crash posture; a new
  `funding_score_id_base` counter threaded EXACTLY like `scalar_belief_id_base`, +5/resolved per the fn
  contract). (3) The funding-rates poller is spawned in `main` as an independent cancellable task (host-pinned
  public GET, NO creds), gated on `[perp_event_basis_v2]`, stopped on daemon shutdown via a `watch` cancel.
  EVERY change is additive (0 deletions) and gated — a daemon WITHOUT `[perp_event_basis_v2]` is BYTE-IDENTICAL
  to today (the `daemon_smoke` suite passes UNCHANGED). 5 new tests (4 compose-builder + 1 gating). The v2
  trader stays DATA-ONLY/Sim (I7) — an edge is DECLARED only after operator endorsement of the DC defaults + a
  v2 e2e fixture. NOTE: the poller gate is coupled to the v2-strategy presence (enabling `[perp_event_basis_v2]`
  enables the funding poller); a separate `[funding_poller] enabled` flag is the documented split if
  v2-without-poller is ever wanted.
- **A2d slice-3 Part 2 — the funding-rates POLLER** (`fortuna-live::funding_poller`, additive): the missing
  piece that FILLS the `funding_rates_historical` store (Part 1 store + Part 3 resolve→score loop are merged;
  production was EMPTY until this — nothing wrote the store outside tests). A PUBLIC-GET, NO-creds, host-pinned
  poller. `poll_funding_rates_once` fetches the venue funding history (all markets), validates each entry's
  shape (UNTRUSTED, spec 5.11 — non-finite rate / empty ticker / unparseable funding_time ⇒ quarantine + count
  + alert + skip), and idempotently inserts each well-formed `{market_ticker, funding_time, funding_rate,
  mark_price}` via `FundingRatesHistoricalRepo` (ON CONFLICT DO NOTHING; `Ok(false)`→skipped_dup), with
  `captured_at` = the injected clock's now. `next_funding_poll_at` is a PURE 8h-boundary scheduler (04/12/20
  UTC, strictly-after, day-rollover; total — degrades to `now` at the i64 extremes, never panics).
  `run_funding_poller` is the Clock-driven loop (backfill once, then poll past each boundary; cancellable via a
  `watch` signal; the injected Clock is the timing authority — a SimClock test is deterministic). HTTP path:
  option (b) — a minimal host-PINNED UNAUTHENTICATED `reqwest` GET (`KineticsPublicFetch`, base const
  `external-api.kalshi.com/trade-api/v2`, reusing the fortuna-venues DTO) — chosen because the signed Kalshi
  transport mandates a key and the endpoint is public (openapi: no 401/403); the URL is built from the pinned
  base + a const path + config params, NEVER from a payload (the track-D SSRF lesson). `funding_rate` stays
  forecast-domain f64; `mark_price` stored VERBATIM (no f64→`Cents`). No `panic`/`unwrap`; a fetch failure is
  alert-and-continue. The `drive()` wire-in is the additive follow-on (Part 3 precedent). 11 tests
  fixture-grounded (backfill 100 rows; idempotent re-poll 0 inserted/100 dup; quarantine a malformed entry
  keeping siblings; fetch-failure alert; mark_price verbatim; captured_at=poll-time; the boundary set),
  MUTATION-PROVEN (captured_at→funding_time reds the captured_at test). This completes **A2d slice-3
  END-TO-END** (store + poller + resolve/score → forecasts scored against accruing ground truth). Depth caveat:
  the venue launched 2026-06-03, so backfill is SHALLOW (~11 days, ~64% zeros) — the poller makes the store
  GROW; the beats-baselines verdict accrues over the soak (an I7 forward-validation gate is time-gated).
- **T5.B8 telemetry — perp basis-v2 A10 diagnostics → `MetricSample`s** (`fortuna-runner`, additive): the
  V5-deferred "richer named-MetricSample emission." A new additive `Strategy::metric_samples()` default
  (`Vec::new()`, mirroring the `drain_*` idiom — so the other 5 strategies are unchanged) is overridden by
  `PerpEventBasisV2` to emit ~8 headline GAUGE samples from its A10 `V2Eval` snapshot (cdf_divergence ×10_000,
  sigma_tau ×1e6, anchor whole-dollars, horizon_ms, obs_count, anchor_stale, regime one-hot, active), each
  carrying a `market=<perp_market>` label; `metrics_export()` drains every strategy's `metric_samples()`
  (deterministic registration order, appended after the existing fixed metrics). Every f64→i64 scale is
  finite-guarded + documented in the metric `help` (the dashboard divides back); not-ready ⇒ a single
  `active=0` (so the dashboard distinguishes "inactive" from "missing series"). No money/gate/order path
  touched (diagnostic integers); no `panic`/`unwrap`. 4 tests; VERIFIER-ACCEPT (`docs/reviews/2026-06-14-T5.B8.md`
  — full battery + DST green, purely additive, 0 deletions). Remaining T5.B8: the per-bin emission + the
  margin-state telemetry (needs daemon/paper wiring), the ROTA §9.2 perps panel (track-B), and the kill-switch
  perp-flatten (track-A's `fortuna-killswitch` crate — plan ready, escalated for the ownership/sequencing call).
- **perp basis-v2 §3.3 SLICE V5 — measured informativeness + diagnostics (A7 + A10)**
  (`fortuna-runner::perp_event_basis_v2`, additive; the LAST v2 slice — the v2 strategy is now COMPLETE):
  "measure that the perp leads, don't assume it." Per candidate bin, A7 computes an `InfoVerdict` from
  whole-book FRESHNESS (the only cleanly cross-instrument signal; DC-6 — per-level age is unavailable, and
  perp-vs-bracket spread/depth are different units so they are RECORDED, not gated): perp side = the perp
  book's `as_of` (via `core.books`) or `funding.obs_at` when the perp book is unplumbed; bracket side = the
  bin book's `as_of`. The verdict feeds the V4 EV gate and can ONLY make it more conservative — BracketLeads
  (bracket strictly fresher, both fresh) ⇒ VETO (configurable); Unfavorable (perp book absent, either side
  stale, or no bin book) ⇒ raise `adverse` by `info_adverse_penalty` then re-apply the strict EV gate;
  PerpFavorable ⇒ unchanged. The conservative default is NEVER PerpFavorable when perp leadership cannot be
  established. A10: emits the model-vs-implied CDF divergence (a Kolmogorov sup-distance over the
  canonically-ordered cumulatives) + `q_j` + the A7 verdict + per-bin freshness/spread/depth into the
  `V2Eval` snapshot and the proposal `thesis` (the §9 "C produces the numbers" half; the richer named-
  `MetricSample` emission + realized band-coverage are deferred to the telemetry slice T5.B8, and the ROTA
  §9.2 display is track-B). Documents the rung-0 FALLBACK: v2 degrades to propose-nothing on any
  unavailable/stale/incoherent input, and the rung-0 strategy remains the fallback basis path. I6/I7
  preserved; A7 never up-sizes or up-weights. 11 new adversarial tests (the three verdicts, the veto, the
  penalty skips/overcomes, the conservative default, the CDF divergence hand-checked), MUTATION-PROVEN
  (defaulting perp-absent to PerpFavorable reds the perp-absent tests). NO `compose.rs` wire-in (track-A
  follow-on). With V5 the v2 strategy (§3.3 A3/A6/A9/A5/A4/A8/A7/A10) is COMPLETE — DATA-ONLY, Sim-stage; a
  declared edge awaits operator endorsement of the DC defaults + a v2 paired-cycle e2e fixture (I7).
- **perp basis-v2 §3.3 SLICE V4 — horizon gating + the per-bin EV gate (A5 + A4 + A8)**
  (`fortuna-runner::perp_event_basis_v2`, additive; the FIRST v2 slice that PROPOSES): extends V3 with
  (A5) per-bracket horizon regimes — τ = `core.markets[bracket].close_at − core.now` (DC-4 resolved
  in-lane, no compose change), classified Direct (≤4h) / VolAdjusted (4–48h) / Disabled (>48h or
  τ-unknown/expired) — and REPLACES V3's per-step σ stand-in with the horizon-scaled `σ_τ = σ_step·√(τ/Δ)`
  (Δ = an EWMA of the BRTI `obs_at` tick-spacing), clamped. THREE load-bearing vetoes ⇒ propose nothing:
  the >48h/τ-unknown horizon, a STALE BRTI anchor (A6: `now − obs_at > max_anchor_age_ms`), and an
  incoherent ladder (A9). Then the (A4+A8) per-bin EV gate: `EV_j = q_j − ask_j − fee_round_trip(ask_j) −
  slippage − reserve − adverse` (DC-3 defaults; the fee is the cents-ceil fee-trap maker rate so promo-$0
  never zeroes it), emitting ONE UNSIZED `Passive`/Buy/Yes maker leg joining each clearing bin's best YES
  bid when `EV_j > threshold` (strict) — `fair_value = round(q_j·100)` clamped `[1,99]` (the one documented
  f64→`Cents` boundary), deduped on `(market, limit)`. Each `q_j` is mapped back to its market by STRIKE
  (the kernel returns canonical price order, not catalog order). I6 (unsized — the harness sizes) + I7 (Sim,
  no auto-promotion) preserved; the EV is an honest f64 edge claim, never a size. No `panic`/`unwrap`; every
  degenerate/missing/stale input degrades to propose-nothing. 19 new adversarial tests (regime boundaries,
  σ_τ scaling, the 3 vetoes, EV clears/rejects/at-threshold-strict, multi-bin + dedup, no-bid/no-ask skip,
  fee-trap >0, no-panic), MUTATION-PROVEN (EV `>`→`>=` reds the at-threshold test). NO `compose.rs` wire-in
  (track-A follow-on). Slice V4 of 3 (V5 = A7 informativeness + A10 emission remains).
- **perp basis-v2 §3.3 SLICE V3 — the model layer (A3 + A6 + A9 + σ)** (`fortuna-runner::perp_event_basis_v2`,
  additive, DATA-ONLY, Sim-stage, proposes NOTHING): a NEW propose-only strategy succeeding rung-0's median
  signal with the v2 fair-probability model wired onto live `PerpTick`/`OrderBook` data. On each matching
  `PerpTick` it (A6) anchors on the BRTI reference (`funding.reference_price` ×10000 → BTC dollars, NEVER the
  perp mark), (DC-1) folds the anchor into a bounded EWMA-σ estimator (N=64 ring, λ=0.94, ≥20 returns to
  activate, σ clamped to `[floor,ceiling]`), (A9) gates the ladder via `validate_ladder_no_arb` (incoherent ⇒
  no pricing), (A3) prices the per-bracket `q_j` via `bracket_fair_probs(SettlementModel{anchor,σ})`, and
  (A10) computes the rung-0 implied median as a DEMOTED health diagnostic — all stored in a public `V2Eval`
  snapshot for inspection/telemetry. It emits ZERO proposals (the per-bin EV gate is the V4 slice) and never
  auto-promotes (I7). The √τ horizon-scaling of σ is deliberately deferred to V4/A5; V3 uses the per-step σ so
  the `q_j` wiring is exercised + testable now. No `panic`/`unwrap`/`expect`; f64 is forecast-domain only (no
  `Cents` touched — V3 proposes nothing); `bin_prob` reused verbatim from rung-0. 13 adversarial tests
  (σ-readiness gating, the `q_j`↔kernel wiring pin, the load-bearing A6 anchor-not-mark assertion, the A9
  incoherent-ladder gate, degenerate-anchor no-panic), MUTATION-PROVEN (pricing off the mark reds the A6 +
  wiring + degenerate-anchor tests). NO `compose.rs` wire-in — the strategy is unit-tested standalone; the
  track-A `[perp_event_basis_v2]` registration is a documented follow-on (like the kernel + the `drive()`
  wire). Slice V3 of 3 (V4 = A5 horizon + A4/A8 EV gate; V5 = A7 informativeness + A10 emission).
- **A2d SLICE 3 Part 3 — the resolve→score loop** (`fortuna-live::daemon` + `fortuna-ledger`, additive;
  `drive()` UNTOUCHED): `resolve_and_score_funding_beliefs(pool, now, score_id_base)` drains every DUE
  (`horizon <= now`), CAPTURABLE `funding_forecast` scalar belief and, per belief, resolves it set-once
  against the realized rate then writes FIVE `belief_scores` legs — the forecast's `crps_pinball` + the
  four A2d baselines (`crps_pinball:carry_forward|last_rate|rw_estimate|rw_persistence`) — so the §9.1
  ROTA scorecard reads the edge gate straight off the rows. Every baseline anchor comes off the persisted
  fan, NEVER an evidence parse (spec 5.11): `estimate` = `v@0.50`; `rw_band` = `(v@0.90−v@0.50)/Z90`
  clamped ≥0 (the producer's own dispersion, inverted); `last_realized` = the PRIOR 8h window's realized
  rate, degrading to the CURRENT realized when the prior window is uncaptured — a NON-fabricating fallback
  that only TIGHTENS the gate (the two persistence baselines anchor at truth, so funding_forecast can't
  earn a spurious edge from a missing window). New `ScalarBeliefsRepo::unresolved_due(producer, now_iso,
  limit)` is the work queue (`realized_value IS NULL AND horizon <= now`, oldest-first, bounded batch).
  Defensive per-belief SKIP (bad `event_key` / uncaptured rate / malformed fan → skip, never a panic or
  batch-abort) + idempotent (set-once `resolve` + a UNIQUE(belief_id,rule_id) catch → a re-run resolves 0,
  no dup-key). 4 `#[sqlx::test]` (happy 5-leg, idempotent rerun, uncaptured-skip, open-window-skip),
  fixture-grounded on the public KXBCHPERP capture; `.sqlx` cache regenerated; MUTATION-PROVEN (neutralizing
  the `resolve` reds BOTH the resolved-assertion AND the idempotency `count==0` rerun). NOT yet wired into
  `drive()` — that one additive line (mirroring `persist_scalar_beliefs`) is a deliberate follow-on. Part 3
  of 3 (Part 2 = the public-GET poller, track-D's `fortuna-sources`).
- **A2d SLICE 3 Part 1 — the realized-funding store** (`fortuna-ledger`, additive): an APPEND-ONLY
  `funding_rates_historical(market_ticker, funding_time, funding_rate, mark_price, captured_at)` table
  (UNIQUE(market_ticker, funding_time), `fortuna_refuse_mutation` trigger — a finalized 8h rate never
  changes, I5) + a `FundingRatesHistoricalRepo`: `insert` (ON CONFLICT DO NOTHING → idempotent re-poll,
  returns inserted?), `realized_rate(market, funding_time)` (the resolve/score loop's ground truth),
  `latest_funding_time(market)` (the poller's backfill cursor). `funding_rate` is f64 cognition-domain
  (not money); `mark_price` is the venue per-contract dollar STRING stored verbatim. Compile-checked
  sqlx (`.sqlx` cache regenerated); 5 `#[sqlx::test]` tests incl. the append-only-trigger refusal +
  idempotent re-insert. This is the realized-funding source the verifier bus (961fa7a) found public
  (`GET /margin/funding_rates/historical`) — Part 1 of 3 (Part 2 = the poller; Part 3 = the
  resolve→score loop).
- **perp basis-v2 §3.3 — the fair-probability kernel (V0–V2)** (`fortuna-cognition::basis_v2`,
  additive pure-f64 kernel): the §3.3 per-bracket fair-probability model (A3) + the ladder
  no-arbitrage guard (A9). `normal_cdf` (Φ via the in-house A&S 7.1.26 erf — no new dependency,
  replay-deterministic) + `lognormal_cdf` (F(price)=Φ(ln(price/S₀)/σ), None on non-finite/≤0);
  `bracket_fair_probs(bins, SettlementModel{anchor,σ})` → per-bin `q_j` (Between→F(cap)−F(floor),
  Greater→1−F(floor), Less→F(cap)) over the rung-0 canonical price order, all-or-nothing on
  degenerate input; `validate_ladder_no_arb` → monotone-implied-CDF + YES-sum coherence. LOAD-BEARING
  A3 no-circularity: `q_j` reads ONLY the strikes, never `BracketBin.prob` (the implied mid) —
  mutation-PROVEN (reading `.prob` reds `fair_probs_independent_of_implied_prob`; re-verified by the
  controller). σ/τ are CALLER-injected (the kernel invents no modeling constant); the crossed-quote
  check is documented as the strategy layer's job. 22 adversarial tests. Pure f64, no money/IO. The
  v2 STRATEGY wiring (V3–V7: horizon-gate, EV gate, informativeness, CDF diagnostics, multi-leg
  propose) needs 6 operator design-calls (DC-1..DC-6, ledgered in GAPS) + the e2e is fixture-gated.
- **funding_forecast §2.6 A2d SLICE 2 — the 4-baseline unified edge gate**
  (`fortuna-cognition::funding_baselines`, additive): adds `compare_against_baselines` +
  a `BaselineComparison` that scores funding_forecast against FOUR naive baselines
  side-by-side via `crps_pinball` — carry-forward (estimate point), last-realized-rate
  (persistence point), an estimate-anchored random-walk fan, and a `last_realized`-anchored
  PERSISTENCE random-walk fan — with `beats_all` (strict `<` on every leg; a tie does not
  beat) as the §2.6 A2d gate (funding_forecast stays DATA-ONLY until it clears, I7). The RW
  band is caller-INJECTED (σ·√horizon — the kernel invents no constant); the fan uses the
  producer's pinned standard-normal multipliers (replay-deterministic, not an erf-inverse).
  ROBUSTNESS (operator call): funding_forecast's own dispersion is already √(time-remaining)-
  shaped, so the estimate-anchored RW near-twins it — the persistence-anchored RW (a distinct
  anchor) was added so the gate isn't a self-comparison; the docs flag the estimate-RW as the
  weak leg. A non-finite `rw_band` → `InvalidPrediction` (NOT silently clamped — `f64::NAN.max(0.0)`
  is `0.0`, which would hide the bug); off-grid q → `InvalidPrediction`. SLICE 1
  `compare_against_carry_forward` is untouched. 25 tests (5 carry-forward + 20 new), MUTATION-
  PROVEN (each `beats_*` guard flip reds exactly its leg's tests). Pure f64, no money/DB/loop.
  SLICE 3 (the resolve/score loop + `belief_scores` rows + ROTA §9.1) remains.
- **funding_forecast §2.6 A2d SLICE 1 — the carry-forward baseline-comparison kernel**
  (`fortuna-cognition::funding_baselines`, post-EXIT scoring refinement, the edge /
  I7-spirit gate): a pure, deterministic kernel — `compare_against_carry_forward(forecast,
  estimate, realized) -> CarryForwardComparison` — that scores funding_forecast's scalar
  fan AND the carry-forward baseline (the venue ESTIMATE projected FLAT, a degenerate
  Scalar at `estimate` over the SAME q-levels) side-by-side via the existing
  `crps_pinball` proper rule, returning `{forecast_crps, carry_forward_crps,
  beats_carry_forward}` (strict `<`; a TIE does NOT beat). funding_forecast has no edge —
  and stays DATA-ONLY (no promotion past Sim, I7) — unless it MEASURABLY beats this bar.
  KERNEL-FIRST (mirrors `basis.rs`): f64 forecast-domain, NO money/DB/loop touch, reuses
  the scoring engine (no scoring math written). 5 adversarial tests (centered-forecast
  beats far carry-forward; on-target carry-forward beats a wild forecast; comparison
  computed; perfect-zero + tie-does-not-beat; non-Scalar → KindMismatch). MUTATION-PROVEN:
  flip the `<` guard → exactly the 2 directional tests red, the other 3 stay green. SLICE
  2 (last-rate + random-walk baselines) and SLICE 3 (the resolve/score loop + `belief_scores`
  rows + ROTA §9.1) remain — see GAPS.
- **funding_forecast §2.6 A2b — the fixed seven-quantile fan** (`fortuna-runner`,
  post-EXIT scoring refinement, binding design §2.6 A2b): the producer's
  `PredictiveDistribution::Scalar` now carries EXACTLY the seven quantiles
  `{0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95}` (was an unfixed 3-point
  `{0.1,0.5,0.9}` fan), so the body + both tails are characterized for CRPS and
  band-coverage. SAME dispersion model — `band = DISPERSION_SCALE·√(remaining/window)`
  evaluated at the standard-normal multipliers (∓1.645 / ∓1.282 / ∓0.674 / 0); q
  strictly increasing, v non-decreasing (`validate_scalar`-clean by construction);
  band still widens with time-remaining and collapses to `p` at `remaining == 0`.
  A new `a2b_emits_exactly_the_seven_fixed_quantiles` pins the q-vector +
  monotonicity (MUTATION-PROVEN: it RED against the 3-point producer, GREEN after).
  The existing fan-shape tests are unaffected (they read `q_at(0.1/0.5/0.9)`, all
  retained). The ROTA display is generic (band-check reads q=0.1/0.9, still
  present; median via `clean_quantiles`) — NO track-B change; only `daemon_smoke`'s
  count assertion moved 3→7. NO money/gate/exec touch (CRPS/quantiles are f64
  forecast-domain). A2d (baseline-beat CRPS) is the next §2.6 slice — see GAPS.
- **Triage tier — 2 mutation-coverage follow-ons closed** (`fortuna-cognition`,
  test-hardening per the verifier bus 2026-06-13; additive): (1) a fractional-token
  cost vector (`anthropic_triage_cost_ceils_a_fractional_token_vector`, input 1100 /
  output 1040 tok) pins the triage cost CEIL — the prior test used 1000/1000 → exact
  1.0/5.0 legs, so a ceil→floor/round/trunc mutation did NOT red; the new vector
  asserts 8¢ (floor/round/trunc undercharge to 6 or 7). (2) a new assertion that the
  malformed-output path STILL debits the budget
  (`anthropic_triage_malformed_output_still_debits_the_budget` — `record_spend`
  precedes the verdict parse, so burned tokens book even when the verdict errors),
  exposed via a read-only `AnthropicTriageMind::spent_today_cents()` accessor mirroring
  `AnthropicMind`'s. BOTH mutation-proven IN THIS ITERATION: the ceil→floor mutation
  reds (1) only; zeroing the debit reds (2) only. Behavior unchanged — the impl was
  already correct; these pin it so a future regression reds.
- **Demo-flip Phase 2 — `compose_kalshi_runner` + `ActiveRunner` + boot gate**
  (`fortuna-live` + `fortuna-runner`, additive — docs/design/kalshi-demo-flip.md):
  a `venue = "kalshi" / stage = "paper"` daemon that composes a real `KalshiVenue`
  (mock funds, real DEMO venue) over the SAME deterministic `SimRunner` core made
  venue-generic in Phase 1. `compose_kalshi_runner` (+ a `_with_transport`
  injection seam the tests drive a `MockKalshiTransport` through — NEVER the live
  API) reads the ESTABLISHED demo creds `KALSHI_API_DEMO_KEY_ID` +
  `KALSHI_DEMO_PRIVATE_KEY_PATH` (the SAME two vars the fixture recorders read —
  the path is routing data, the file CONTENT is the `Secret`-wrapped, never-logged
  RSA key); builds `KalshiSigner` + `ReqwestKalshiTransport(KALSHI_DEMO_BASE_URL)`
  + `KalshiVenue`; runs the synthesis arm at `Stage::Paper` with the runner
  allowlist `&[Stage::Sim, Stage::Paper]` (I7: LiveMin/Scaled still refused at
  construction). An `ActiveRunner` enum {Sim, Kalshi} + delegation resolves the
  `compose_runner`/`compose_kalshi_runner` return-type split; `main.rs` routes by
  `[daemon].venue`. The boot gate (`validate_bootable`): `venue = "kalshi"`
  REQUIRES `stage = "paper"` + a `[kalshi]` section; sim/live_min/scaled refused
  (promotion is a human action, I7). Sim path byte-unchanged (A3 — DST corpus
  replays identically). `fortuna-invariants` ADD-ONLY: 2 new I7 tests pin the new
  seam (`new_with_venue(&[Sim, Paper])` ACCEPTS Paper ONLY via the explicit
  allowlist, still REFUSES LiveMin/Scaled) + a mechanical `faults → Option`
  adaptation in a non-assertion helper — no assertion weakened (operator-waive
  flagged in GAPS). The CODE is complete + battery-green (fmt, clippy --workspace
  --all-targets -D warnings, test --workspace, run-dst); the LIVE demo run stays
  operator-gated (creds in `.env` + the T4.2 fixture checklist + `[kalshi].series`
  tickers — the operator flips demo in the morning).
- **Demo-flip Phase 1 — SimRunner is now venue-generic** (`fortuna-runner` +
  `fortuna-venues` + `fortuna-live`, additive — docs/design/kalshi-demo-flip.md):
  `SimRunner<J>` → `SimRunner<V: Venue = SimVenue, J>`, so the runner drives ANY
  `Venue`, not just the sim. A new `Venue::account() -> (cash, reserved)` (default
  `balance()` + 0; SimVenue delegates to `inspect_totals`) replaces the 3
  SimVenue-only `inspect_totals` calls in the runner. A venue-injecting
  `new_with_venue(.., venue, clock, allowed_stages)` is the seam; `new_with_journal`
  routes THROUGH it with a SimVenue + `&[Stage::Sim]` (ONE construction path → the sim
  path is byte-identical, A3). `report()` is now async; `RunnerConfig.faults` is
  `Option`. SIM PATH PROVEN byte-unchanged: the full DST corpus (run-dst exit 0) + the
  156-result workspace suite green, and an ADD-ONLY invariant pins that `SimRunner::new`
  STILL refuses `Stage::Paper` (the Kalshi demo opens Paper only via the explicit
  `new_with_venue` seam — Phase 2). The KalshiVenue adapter is already trait-complete,
  so Phase 2 (`compose_kalshi_runner` + boot gate) is the remaining code; the live demo
  run is operator-blocked (demo creds + the T4.2 fixture checklist).
- **3-tier cognition models + ModelRegistry + triage seam** (`fortuna-cognition` +
  `fortuna-live` + `fortuna-runner`, additive — spec 5.9 tiering): `[cognition]` now
  carries three REAL model fields — `synthesis_model` (deep, default Opus), `mid_model`
  (NEW, default Sonnet), `triage_model` (promoted from tolerated-but-UNREAD to a real
  field, default Haiku) — and a `ModelRegistry` in the mind layer (fortuna-cognition) maps
  each role's tier → model as the SINGLE source of truth the daemon consults. `mind_from_env`
  is parameterized by model, so each ROLE runs on its tier: synthesis on Opus, and the daily
  RECONCILIATION now on a SEPARATE `mid_model` mind (Sonnet) instead of borrowing the
  synthesis Opus mind (the overkill the operator flagged). Each tier binds `AnthropicMind`
  only with `ANTHROPIC_API_KEY` (else `StubMind`) and shares the `[cognition]` budget rails
  (per-cycle + daily; reconciliation is once-daily so the daily total rises by at most one
  mid-tier cycle); I6 propose-only unchanged; sim path byte-unchanged. The TRIAGE SEAM is
  built: a `TriageMind` trait + `StubTriageMind` (mirroring the veto mind) + a
  `TriageDecision::Mind` variant whose async `assess` runs the cheap tier in the cognition
  cycle BEFORE the expensive frontier mind — cost accounted (even on a plain decline), and a
  provider failure surfaces as `CycleError::Triage` (the synthesis arm degrades mechanical-
  only, never a coerced verdict). PROVEN: synthesis-mind == Opus + reconciliation-mind ==
  Sonnet, DISTINCT (MUTATION-PROVEN: route reconciliation on Opus → RED, executed); a
  parse/default guard; a registry-lookup test; 4 cycle tests (accept→synthesis,
  decline→no-synthesis, cost-accounted, failure-surfaces). The daemon now COMPOSES the
  triage on `triage_model`: an Anthropic Haiku triage (`AnthropicTriageMind`, its own
  `[cognition]` budget rails) when `ANTHROPIC_API_KEY` is present, else `AlwaysAccept`
  (byte-unchanged). 4 more tests pin the Haiku triage: escalate true/false →
  Accepted/Declined, cost-from-usage-tokens, and a budget breach + a malformed output both
  surface (never a coerced verdict). The `compose_runner` triage injection is compiler +
  clippy verified (an unused `triage` param fails `-D warnings`).
- **Scalar-belief EGRESS persisted + Sim-soak PerpTick FEED** (slices 4d + 4e,
  `fortuna-live` daemon/main + new `perp_feed`, additive): closes the slice-4
  finding — the producers composed in 4c now actually PRODUCE and PERSIST. Each
  segment `drive()` drains `drain_pending_scalar_beliefs()` and writes them to
  the `scalar_beliefs` ledger via `persist_scalar_beliefs` (the table ROTA §9.1
  groups by `producer`), gated on a composed scalar producer (`[funding_forecast]`
  / `[perp_event_basis]`) and fail-closed to no-persist otherwise; the scalar
  drain runs OUTSIDE the synthesis-refresh block (funding_forecast is independent
  of the synth arm), and the scalar id space (`01SCB…`) advances independently of
  the binary (`01BLF…`). The new `PerpTickFeed` (4e) replays RECORDED kinetics
  `ticker` frames (`[funding_forecast].ticker_feed_jsonl`) one PerpTick per
  segment through the 4b `inject_perp_tick` seam, so `funding_forecast` fires in a
  Sim soak (the Sim loop sources only `BookSnapshot`s — the producers are
  otherwise inert). The binary `BeliefDraft` path and `tick()` are byte-unchanged
  (A3). PROOF: a DB integration test drives a recorded PerpTick end-to-end and
  asserts the persisted `scalar_beliefs` row (unit `"rate"`, the {0.1,0.5,0.9}
  quantile fan) — MUTATION-PROVEN (break the egress → 0 rows → RED, executed). 5
  new tests (4 `perp_feed` parser tests against the real 489-frame capture: 74
  tickers → 74 PerpTicks, venue-stamped, loops, zero-ticker/missing-file fail
  closed; + the e2e); all 8 `drive()` smokes updated to the 15-arg signature.
- **Daemon registration of the perp strategies** (slice 4c, `fortuna-live`
  compose/boot/daemon, additive — 489 insertions / 0 deletions): opt-in
  `[funding_forecast]` and `[perp_event_basis]` config sections compose the two
  perp strategies into `compose_runner` alongside the mechanical/synthesis arms
  (same gate/exec path, I1), mirroring the `[mech_extremes]` precedent. The
  `perp_event_basis` bracket ladder is config-supplied (`key = market →
  { kind = between|greater|less, floor_dollars, cap_dollars }`, strictly
  validated) — sidestepping the absent `Market` strike metadata (live-market-list
  catalog is a later sub-slice). Neither is veto-enrolled (funding_forecast
  proposes nothing; perp_event_basis stays out so no veto mind is required). Both
  are INERT in pure-sim until a producer injects PerpTicks (the 4b seam) — the
  composition is the deliverable. 11 tests incl. a `compose_runner` boot test
  asserting both register only when configured (fail-closed otherwise).
- **`SimRunner::inject_perp_tick`** (slice 4b, `fortuna-runner`, additive): the perp
  INGESTION seam. `EventPayload::PerpTick` has no producer in the deterministic
  `tick()` loop (which sources only `BookSnapshot`s), so the perp strategies would
  be inert in the daemon. This publishes an `EventOrigin::External` `PerpTick` onto
  the bus for the next `tick()` to dispatch through its EXISTING `new_events` read —
  so `tick()` itself is UNTOUCHED (the record/replay determinism contract and every
  existing DST recording are unaffected; the full DST corpus re-ran green to prove
  it). A Sim-soak test drives the REAL `funding_forecast` through a runner tick: it
  produces a scalar belief BECAUSE it saw an injected `PerpTick`, and nothing
  without one. The same seam carries the live kinetics feed
  (`KineticsPerpObservation` → `inject_perp_tick`).
- **`KineticsPerpObservation`** (slice 4a, `fortuna-venues::kinetics::perp_observation`,
  additive): the venue-side half of a `PerpTick`, built VERBATIM from a WS `ticker`
  frame — `MarketId` + `PerpMarks` (venue settlement; no conservative mark) +
  `FundingObservation` (rate→`Decimal` estimate, `next_funding_time`, reference
  price, capture `obs_at`). The venue crate stays BUS-FREE: this returns perp-domain
  components, and the producer (a later sub-slice) adds the `venue` id to make the
  bus event. 4 tests (synthetic exact-mapping + field-swap guards, recorded-frame
  re-derivation, malformed→`Err`). The foundation for the PerpTick producer —
  WITHOUT a producer the perp strategies are inert (slice-4 architectural finding,
  see GAPS). NEXT: the scripted PerpTick source (Sim soak) + daemon registration.
- **`perp_event_basis` STRATEGY** (slice 3b-strategy, `fortuna-runner::perp_event_basis`,
  additive): the propose-only, mechanical, Sim-stage bracket trader. On a `PerpTick`
  it rebuilds bin probabilities from `core.books` (YES mid `(bid_or_0 + ask_or_0)/2`
  — an absent quote counts as the 0c floor, so the live `0 bid / Nc ask` far tails
  keep their `ask/2` mass and the strategy reproduces the kernel's validated basis),
  calls `compute_basis`, and proposes ONE maker-only (`Urgency::Passive`) UNSIZED
  `Cents` leg (I6 — no qty; the harness sizes) on the bin containing the perp
  forecast, gated by the fee-trap (`fair = limit + premium`, clamped ≤99). It holds
  its OWN bracket catalog (`MarketId → BracketStrike`); no `fortuna_venues::Market`
  widening (live catalog-population is the slice-4 daemon concern). 14 mutation-pinned
  unit/e2e tests + a DST oracle that independently recomputes the verdict in lockstep
  with `bin_prob`. VALIDATED on live DEMO data: the committed e2e (cycle …753775,
  basis −$55.53) + a fresh independent cycle (…754035, basis +$55.08), both with
  perp/ladder agreement <0.1%.
- **`perp_event_basis` basis kernel** (slices 3 + 3b, `fortuna-cognition::basis`):
  the deterministic forecast-quality basis signal — `bracket_implied_median` (a
  **KXBTC** price-level bracket ladder's YES bid/ask → normalized probabilities →
  0.5-crossing interpolation) + `compute_basis` (perp mark − implied median,
  gated past the assumed-fee floor). Slice 3b refined the kernel to the REAL
  3-strike-type ladder grounded in the live capture: a `BracketStrike` enum
  {`Between`{floor,cap}, `Greater`{floor}, `Less`{cap}} with `BracketBin{kind,
  prob}`; a 0.5 crossing landing in an OPEN tail returns `None` (no finite width
  to interpolate — conservative, no fabricated point). The kernel now has ZERO
  money-type touch: `compute_basis` takes the perp mark as caller-supplied `f64`
  BTC-dollars (the per-contract→BTC ×10000 boundary is the caller's), so it is
  pure f64-cognition. The implied-median reduction (`sum_p`) is taken over the
  SORTED bins, so the median is a pure function of the ladder MULTISET,
  independent of caller input order (a DST-found float-determinism wrinkle: a
  non-associative input-order sum could flip the 0.5 crossing at an exact
  cum==0.5-at-a-bin-boundary tie). 14 mutation-pinned synthetic tests + a NEW
  real-data e2e (`basis_live_fixture.rs`) on the committed paired cycle — implied
  median $63,961.53 vs perp $63,906.00 → basis −$55.53 (two independent price
  sources agree <0.1%). The composite fixture lives in `fixtures/perp-basis/`
  (a recorder-DERIVED perp+ladder pair for the basis/cognition layer, NOT a
  single Kinetics DTO capture — kept OUT of `fixtures/kinetics-perps/` so the
  venue DTO-coverage tripwire `every_fixture_parses_into_its_typed_dto`, which
  requires every fixture there to classify, is not tripped; operator-directed
  location, the tripwire's "every DTO fixture accounted for" guarantee intact).
  The bracket-TRADER strategy (the sized `Cents` bracket-leg trade) stays
  fixture-gated.
- **`funding_forecast` strategy** (slice 2b, `fortuna-runner`): a zero-capital
  scalar belief-producer — on a `PerpTick` it forecasts the next funding rate
  directly from the recorded venue estimate (`finalize_funding_rate(estimate)`;
  the estimate IS the running TWAP, never re-derived) and emits a
  `PredictiveDistribution::Scalar` quantile fan whose dispersion widens with
  time-remaining-in-window (a documented rung-0 model, CRPS-measured). Proposes
  NOTHING (I6). A live-data CRPS test scores a recorded estimate → forecast
  against a recorded realized rate; exact-window calibration is deferred to the
  operator-queued paired fixture (the test pins the gap executably, never
  fabricates). DST arm over tick/gap/window-roll/clamp chaos.
- **Perp-strategy seam** (slice 2a, additive): `EventPayload::PerpTick` + the
  `FundingObservation` type (`fortuna-core`), `ScalarBeliefDraft`
  (`fortuna-cognition::scalar_beliefs`), the `drain_scalar_beliefs()` default
  Strategy-trait method + the runner's `pending_scalar_beliefs` buffer
  (`fortuna-runner`) — the plumbing the `funding_forecast` strategy (2b) rides.
  Bus events replay byte-stable (the `Decimal` rate preserves scale). The binary
  `BeliefDraft` / `drain_beliefs` path is byte-unchanged.
- **Scalar belief type + swappable scoring** (`fortuna-cognition::scoring`,
  slice 1a): `PredictiveDistribution {Binary, Categorical, Scalar{quantiles,
  unit}}` + `RealizedOutcome` + the swappable `ScoringRule` trait; `BrierRule`
  + `CrpsPinballRule` (native CRPS = mean pinball / quantile loss); `ScoreError`;
  full `validate()` (strict-(0,1) binary p, categorical sum≈1, ≥2
  strictly-increasing non-crossing quantiles). Additive — the binary
  `BeliefDraft` path is byte-unchanged. 54 tests incl. a proper-scoring proptest.
- **Scalar-belief storage** (`fortuna-ledger`, slice 1b): append-only
  `scalar_beliefs` (immutable claim + one-time resolution; `producer`
  first-class for the ROTA scorecard) and `belief_scores` (rule-tagged
  `(belief_id, rule_id)` score, FK → `scalar_beliefs`, unique per rule);
  `ScalarBeliefsRepo` (exactly-once `resolve`, mirroring `resolve_and_score`) +
  `BeliefScoresRepo`. Migration `20260613000002_scalar_beliefs.sql` with
  append-only DB triggers. 7 live-PG tests.
- Deterministic funding-forecast kernel (`fortuna-core::perp`): `FundingWindow`
  (running TWAP of recorded premiums; premium-as-input never re-derived) +
  `finalize_funding_rate` (±2 % clamp, 0.01 % zero threshold). 13 tests.

#### Deferred

- Live-market bracket catalog (slice 4e-future): populate `perp_event_basis`'s
  bracket ladder from the live Kalshi market list instead of config (coordinate
  with track A). NOT needed for composition — the strategy holds its own
  config-supplied catalog (the `KalshiMarket` floor/cap DTO is unnecessary). The
  daemon composition itself (slices 4b–4e: registration in `compose_runner` /
  `daemon.rs`, the PerpTick injection seam, and the recorded Sim-soak feed) is
  DONE and listed under Added above.
- F5–F9 (Aeolus weather → belief) — ✅ LANDED (track-E, merged @bdea003): the
  `aeolus_*` cognition modules (dedup / strict v2 parser / world-forward match /
  propose-only emission / Brier+CRPS reliability) are on main. No longer deferred.

### Ingestion & data sources (fortuna-sources, Track D)

The news-aggregation / weather-signal ingestion subsystem (`crates/fortuna-sources`)
and its daemon seam (`crates/fortuna-live` `ingestion.rs` / `boot.rs`). Off by
default — merged code activates zero ingestion until an operator opts in (see
`docs/runbooks/ingestion-ops.md`). No model is anywhere on the ingestion path.

#### Added

- Fail-closed `[sources.<id>]` config (`SourceConfig` / `SourceKind`): unknown
  kinds/fields, non-https URLs, and anything not runnable in Phase A are hard
  errors, never defaults (D1).
- `FetchClient` HTTP substrate: SSRF-safe host pin (`HostPin`), https-only,
  conditional GET (ETag / If-Modified-Since → 304 ⇒ empty), and a GCRA
  politeness rate-limit (D2).
- Layer-1 `StructuralValidator` (refuse future-dated / republished / over-volume
  per tick) plus the Layer-0 dossier template (D3).
- `NwsSource` adapter — NWS active alerts (`feed = "alerts"`) and Area Forecast
  Discussions (`feed = "afd"`), emitting `nws.*` signals, with dossier and real
  fixtures (D4).
- `RssSource` adapter — any RSS/Atom via feed-rs, emitting `rss.item`; Fed/SEC
  dossiers (D5).
- `CalendarSource` adapter — BLS macro release schedule (`feed = "schedule"`,
  iCalendar) and latest-numbers RSS (`feed = "latest"`) (D6).
- Layer-2 corroboration (`corroborate`) — near-duplicate clustering that
  collapses syndication so one wire story carried by many outlets is one origin;
  built as a standalone pass, not yet wired into the live ingestion tick (D8).
- `IngestionScheduler` — the validator-WIRED ingest core: per-source cadence,
  the live Layer-1 hard gate (refuse-and-quarantine on the path), per-source
  `Health` machine with operator-only `rearm`, deterministic capped exponential
  backoff, and `SourceMetrics` (D9).
- Config-driven `build_scheduler` factory plus the daemon `[ingestion]` seam
  (default-off; the trading daemon is byte-unchanged when the section is absent)
  (D10).
- **Phase A merged to main @ `f31aaa8`** (NWS + RSS + Calendar; GDELT deferred).
- Generic per-source auth header (`auth_header` / `auth_env`): `x-api-key` and
  any scheme drop in by name; the secret is env-only and redacted (F1).
- `NwsClimateSource` adapter (`feed = "climate"`) — the NWS CLI
  (Climatological Report–Daily) two-hop grader, the official daily max/min
  settlement record; emits `nws.cli` carrying the raw productText (F2).
- `AeolusSource` adapter (kind `aeolus`) — the operator-owned probabilistic
  temperature-forecast vendor; `x-api-key` auth, env-only secret; emits
  `aeolus.forecast` (the raw envelope, untouched) with real live-endpoint
  fixtures (F3).
- Climate grader wired into the factory — scheduler-validated and reachable
  through config (F4).
- OBS-1 ingestion telemetry data surface (`IngestionTelemetry`): per-source
  `SourceTelemetry`, process-wide `FunnelCounts`, and a bounded (256), newest-
  first `recent` feed of redacted `SignalRecord`s — the observability
  contract §2 snapshot.
- OBS-2a funnel loop-stages — `IngestionCore` / `IngestionWiring` now fill the
  funnel's `normalized` / `deduped` / `persisted` / `persist_failures` stages and
  expose `telemetry(now)`, so the funnel is complete end to end (those stages
  read 0 in OBS-1). The `Arc<RwLock>` publish that exposes the snapshot to ROTA
  is OBS-2b (deferred).
- OBS-3 `SourceTelemetry.domain_tags` — populated from the `source_registry`
  admission via a new `domain_of` resolver on `build_scheduler` (parallel to
  `tier_of`), so the per-source telemetry carries its domain (weather|macro|…).
  No more empty placeholder fields in the telemetry surface.
- OBS-2b telemetry publish — `run_ingestion_loop` now publishes the snapshot into
  a shared `IngestionTelemetryHandle` (`Arc<RwLock<IngestionTelemetry>>`) each
  tick ("one writer, many readers", §2); `IngestionTelemetry` derives `Default`
  for the empty pre-first-tick state. The daemon creates the handle (inert when
  ingestion is off) and logs the final funnel at shutdown. The ROTA read endpoint
  (OBS-2c) is track B's harness.
- Design docs: `docs/design/aeolus-fortuna-source-contract.md` (rev 3,
  reconciled with the Aeolus producer handoff) and
  `docs/design/ingestion-observability-contract.md` (telemetry + ROTA-views
  contract for track-B).

#### Fixed

- Unified the URL parser across the fetch path — the host pin is now built from
  the same WHATWG `url` parser (`reqwest::Url` / `url::Url::parse().host_str()`)
  the HTTP client and redirect handling use, removing the hand-rolled
  `host_of_https` (see Security).

#### Security

- **Critical SSRF "parser-differential" fixed at root cause before merge** — a
  mismatch between a hand-rolled host extractor and the HTTP client's WHATWG URL
  parser was eliminated by deleting `host_of_https` and unifying on one parser;
  cleared by 29 adversarial vectors. The injection surface (ingestion) treats all
  fetched content as untrusted data, never instructions (spec 5.11).
- Per-source auth secrets are env-only (resolved by the binary, never the lib),
  marked sensitive (`HeaderValue::set_sensitive`) so the `http` crate prints
  `Sensitive`, and elided as `<redacted>` in manual `Debug` — never in config,
  repo, logs, or audit payloads.

#### Deferred

- D7 `GdeltSource` — external IP rate-limit; interim is `rss` against GDELT's
  `format=rss`.
- OBS-2 — the loop-side funnel stages (`normalized` / `deduped` / `persisted`)
  and the `Arc<RwLock>` snapshot publish (fortuna-live); OBS-3 — `domain_tags`
  from the registry.
- F4b — release-aware cadence — ✅ LANDED (track-E, merged @0e20681): the scheduler
  consumes `next_run_at` (band-clamped so an absurd hint can't break steady cadence).
- F10 — Aeolus `source_registry` row + dossier finalization + v1→v2 fixture
  migration.
- F5–F9 — ✅ LANDED (track-E, merged @bdea003; cognition, reassigned C→E): F5 dedup,
  F6 the strict v2 μ/σ→p parser, F7 world-forward match, F8
  belief→calibration→gates→sizing, F9 the Layer-3 `source_reliability` scoring the
  ROTA scorecard depends on. No longer deferred.

### Domain-analysis personas (fortuna-cognition, Track E)

Persona analysts (meteorologist + macro economist) that reason over UNTRUSTED
signals and emit calibration-scored beliefs. Verifier-gated ACCEPT and MERGED to
main @2668291, 2026-06-13. No model action is ever execution — personas propose.

#### Added

- Persona belief consumption (`persona_beliefs`, E.4): the μ/σ→p backbone +
  artifact→`BeliefDraft` fan-out into the GATED belief pipeline (never orders —
  I6), plus the `SectionKind::DomainAnalysis` context section.
- Persona scoring + promote/retire (`persona_scoring`, E.5): calibration Brier vs
  both baselines (raw + market) + CLV; `propose_promotion` returns a
  RECOMMENDATION-ONLY `PersonaPromotionProposal` (the daemon never self-promotes —
  the I7 analog; a human acts on the proposal). Mutation-proven gate.
- The trusted/untrusted firewall (E.3a core): the persona's method rides the Mind
  `system_charter`; untrusted signals are assembled only as `<context-item>` data,
  never as instructions.
- End-to-end meteorologist proof + macro-economist generalization (one mechanism,
  two domains) + the persona-authoring operator runbook + a seeded persona-runner
  DST arm (budget throttle, signal absence, schema-invalid findings).

### Trading core, venues & exec

_Owned by Tracks A / B / C / E — see their entries below._

## Track A — venue / exec / recovery

Prior to this log (gated, on main): M3 rearm notices; T4.2 (i) Kalshi WS dial
slices 1-2 + 4-5 + concrete transport (see `docs/reviews/t42-wsdial-gate-2026-06-13.md`,
`t42-redial-gate-2026-06-13.md`, `m3-rearm-gate-2026-06-13.md`).

### 2026-06-14 — Market-back discovery wired into the live daemon (`[discovery]`, opt-in) — amendment part 1b (completes the ingestion→beliefs amendment)

**Added (default-OFF; extends part 1a).** Per the operator amendment + spec §5.12, a MARKET-BACK
sub-step in `drive()`, placed BEFORE the synthesis edge-refresh: run the deterministic `prefilter`
over the venue `catalog`, dedup already-edged listings (`current_edges_for_market`), normalize survivors
via the same `Mind` (`market_back_discovery`; the §5.12 budget cap lives INSIDE it), persist each
NEW-event draft as a canonical `events` row (`01EVT…`), and for each proposed edge card AUTO-CONFIRM the
LOW-STAKES ones — `confirmed_by = "discovery:auto"` ⇒ `EdgeTier::Confirmed` ⇒ the synthesis arm prices it
THIS SAME segment — while persisting HIGH-STAKES edges as PROPOSED (`confirmed_by = None`) and routing a
`MessageKind::Review` alert to #fortuna-review. The auto-confirm boundary is EXACTLY spec §5.12:252
(`high_stakes == mapping != Direct || deterministic_score < 1.0`; "deterministic checks score them;
#fortuna-review confirms the high-stakes ones"). Auto-confirmed edges feed only BELIEFS — orders still
cross the universal gate I1 (propose-only, I6).

- **Extends** the part-1a `[discovery]` config (prefilter knobs: `category_allowlist`,
  `min_volume_contracts`, `min_category_quality`) + `DiscoveryWiring` (`prefilter`, `catalog`,
  `event_id_base`, `edge_id_base`). Edge-card event_ids resolve via a `new:{market_id}`
  placeholder→minted-id map; an UNRESOLVABLE event_id alerts + skips (no dangling edge). No-panic
  (match/let-else, `wrapping_add`); EXISTS-guarded event create; dedup re-run-safe.
- **PROD GAP (T4.2/operator):** the daemon has no live venue catalog wired (`main.rs` sets
  `catalog: Vec::new()`), so market-back is INERT in production (no mind call, no events/edges, no alert)
  until the Kalshi adapter supplies a catalog. (World-forward (1a) is the prod-active signal→belief path
  meanwhile.) Ledgered in GAPS.
- **e2e (mutation-proven, the amendment's gate):** `discovery_market_back_auto_confirms_and_synthesis_
  drafts_a_belief` supplies a test catalog (a real sim market with a book), scripts a StubMind
  `NormalizationBatch` (Direct + matching source/horizon ⇒ deterministic 1.0 ⇒ auto-confirm), enables the
  synthesis arm with a believing_mind on the DETERMINISTIC minted event_id, runs `drive()`, and asserts
  ≥1 `events` row + a `confirmed_by='discovery:auto'` edge + a synthesis belief on that event — the full
  signals/catalog→event→confirmed-edge→synthesis-belief chain. The synthesis belief CANNOT arise without
  the auto-confirmed edge (compose asserts 0 edges; the edge arrives via the segment-1 refresh). MUTATION:
  `discovery=None` ⇒ 0 events/edges/belief ⇒ RED (verified). code-architect blueprinted; code-reviewer
  clean (no high-conf issues). Full battery green (test --workspace 1496/0; run-dst 200 0-violations).

### 2026-06-14 — World-forward discovery wired into the live daemon (`[discovery]`, opt-in) — amendment part 1a

**Added (default-OFF).** Per the operator amendment ("drive the ingestion→beliefs loops") + spec §5.12,
a `[discovery]` opt-in WORLD-FORWARD step in `drive()`: each segment reads fresh signals
(`SignalsRepo::recent_by_kind` over `signal_kinds`, within `window_hours`, capped at `max_signals`),
turns them into `<context-item>` blocks, and hands them to one `world_forward_discovery` call (the §5.12
daily cost cap + the unscoreable rule live INSIDE it). Each returned candidate is persisted as a `watch:`
event (EXISTS-guarded — `EventsRepo::create` is a pure INSERT); the SCOREABLE candidates' beliefs fan out
through the existing `persist_beliefs` path, attributed to a pre-built `StrategyId("world-forward")` (the
I7 gate/scoring boundary). This is the path that makes ingested SIGNALS produce beliefs in production —
no venue catalog needed. Sits after the persona step, before `route_alerts` (no synthesis-edge dependency).

- **Boot loader (fail-closed):** the curated `SourceRegistry` is loaded ONCE at boot
  (`SourceRegistryRepo::load_all`); an out-of-range `trust_tier` REFUSES to boot (no silent default). The
  discovery `StrategyId` is built once at boot (no fallible id construction on the loop path). The
  discovery mind is the same synthesis `Mind`. `DiscoveryWiring` owns the `DiscoveryBudget` across segments.
- **No-panic / I6 / default-off:** the daemon block is match/let-else/filter_map throughout (no
  unwrap/expect); data-only (signals → `watch:` events + beliefs, never orders); absent `[discovery]` /
  `enabled=false` ⇒ `None` ⇒ the step never runs (all sibling `drive()` smokes pass `None`).
- **e2e (mutation-proven):** `discovery_world_forward_persists_watchlist_events_and_beliefs` seeds a
  scoreable registry source, inserts a signal, scripts a `StubMind` `WatchlistBatch` (one scoreable + one
  unscoreable candidate), runs ONE `drive()` segment, asserts 2 `watch:` events + exactly 1 belief (the
  unscoreable candidate's belief refused — "no beliefs nobody can grade"). MUTATION: `discovery=None` ⇒ 0
  ⇒ RED (verified). code-architect blueprinted; code-reviewer clean (no high-conf issues). Full battery
  green (test --workspace 1495/0; run-dst 200 0-violations). NEXT (amendment part 1b): market-back
  (catalog→edges→synthesis) — extends this `[discovery]`/`DiscoveryWiring`; catalog-gated, see GAPS.

### 2026-06-13 — Persona analysis step wired into the live daemon (`[personas]`, opt-in)

**Added (default-OFF).** Per `docs/design/persona-live-wiring-handoff.md` (Track-E→Track-A
handoff), a `[personas]` opt-in step in `drive()`: each segment reads the signals the
loaded personas care about (`SignalsRepo::recent_by_kind` over the union of
`reads_signal_kinds`, within `window_hours`, capped at `max_signals`), hands them to one
`run_due_personas` call (§4 firewall + cost budget + schema validation live INSIDE it),
and for each produced artifact persists a `domain_analyses` row (`01PAN…` id) + fans out
binary beliefs through the existing `persist_beliefs` path (attributed to a single
pre-built `StrategyId("domain-analysis")` — the I7 gate/scoring boundary). Mirrors the
scalar-drain failure posture: any read/persist failure ALERTS (routed in-segment) and
CONTINUES — never crashes the loop (persona analyses/beliefs are the calibration
substrate, not the money path). The block sits between the scalar-drain block and
`route_alerts`.

- **Boot loader (fail-closed):** for each `[[personas.persona]]`, read `persona.md` +
  `schema.json`, `PersonaDef::parse`, fetch the registry HEAD, and `validate_against` it
  — a hash/version/status mismatch (or missing row) REFUSES to boot (a tampered method
  never runs, §6). `PersonasWiring` bundle (pool, schedules, `PersonaScheduleState`,
  `DiscoveryBudget`, the synthesis `Mind`, the pre-built strategy, knobs) owned across
  segments like `ReviewWiring`. The persona strategy id is built ONCE at boot (no fallible
  id construction on the loop path); the daemon block is no-panic (match/let-else/
  filter_map throughout, no unwrap/expect).
- **Default-off byte-identical:** absent `[personas]` or `enabled = false` ⇒ `None` ⇒ the
  step never runs (proven by all 9 existing `drive()` smokes passing `None`).
- **I6/§4 inherited:** the wiring only moves SIGNALS (untrusted data) + persists outputs;
  the trusted method never enters this code; no order/size/price is emitted (DATA →
  BeliefDrafts → the same universal gate, propose-only).
- **e2e (mutation-proven):** `drive_persists_persona_analysis_and_beliefs_when_wired`
  registers the shipped meteorologist, inserts an `aeolus.forecast` signal whose payload
  yields a date-bearing region, scripts a `StubMind`, runs ONE `drive()` segment with the
  wiring, and asserts 1 `domain_analyses` row + exactly 3 beliefs citing that `analysis_id`.
  MUTATION: `personas = None` ⇒ 0 rows ⇒ RED (verified). Full battery green (test
  --workspace 1491/0; run-dst 200 0-violations). Slice 3 (weekly-review promote/retire
  verdict folding) deferred per the handoff — separable, not a blocker.



**Fixed (live WS path).** `KalshiWsTransport::signed_request`
(`crates/fortuna-venues/src/kalshi/ws_transport.rs`) hand-built the upgrade
`Request<()>` with only the three KALSHI-ACCESS-* auth headers, relying on the
false belief that tungstenite adds the standard WS upgrade headers. It does NOT
for a pre-built request, so `connect_async` always failed
`Protocol(InvalidHeader("sec-websocket-key"))` — the live socket never connected.
Now `signed_request` starts from `ws_url.into_client_request()` (which generates
`Sec-WebSocket-Key/Version`, `Upgrade`, `Connection`, `Host`) and layers the auth
headers on top. This was invisible to unit tests ("no live socket in tests"); the
operator-directed FIRST LIVE EXERCISE surfaced it.

**Why.** Operator set the demo creds and directed the live handshake. Driving it
caught a real defect that blocked every live WS connection.

**Tests-first.** New regression `signed_request_carries_the_mandatory_websocket_
upgrade_headers` (RED before the fix, GREEN after); the existing auth-header test
is unchanged (not weakened). Protected crate untouched.

**Live-proven (demo, READ-ONLY).** The signed handshake now returns "OK — 101
upgrade, authenticated" against `wss://external-api-ws.demo.kalshi.co`. New
operator-run tool `crates/fortuna-venues/examples/kalshi_ws_handshake.rs` —
demo-only (hard-coded endpoints + a `contains("demo")` guard), read-only
(`GET /markets` + orderbook subscribe, NO orders), secrets never printed. Residual:
0 streamed frames in-window (only future-dated demo markets were open — no live
book yet); the handshake + subscribe paths themselves work.

### 2026-06-13 — F16a: Kalshi cancel-reconcile hardened via the order list

**Changed.** `KalshiVenue::cancel` (`crates/fortuna-venues/src/kalshi/adapter.rs`).
On a DELETE-200 ack whose single reconcile GET reads stale `Resting`/`Unknown`
(the recorded F16/F3 race — DELETE acked `reduced_by:"1.00"`, GET ~360ms later
still `resting`), cancel() now reconciles ONCE against the order LIST
(`GET /portfolio/orders`, new `cancel_reconcile_status_via_list`) — the
authoritative terminal surface — and maps: list `Canceled`→`Ok(())`,
`Executed`→`Rejected` (fills via `fills_since`), still-stale/absent/list-error→
`Timeout` (the safe fallback). A genuinely-canceled order that read stale now
resolves to a definite `Ok` instead of a false `Timeout`. The first-DELETE-404 →
`NotFound` path is unchanged (no ack ⇒ claim nothing).

**Why the order list, not recancel-404.** `fixtures/kalshi/README.md` finding-16
suggested "treat recancel-404 as proof-of-canceled"; the fixtures REFUTE it — the
404 bodies for already-canceled, already-EXECUTED, and never-existed orders are
byte-identical (`orders__cancel_already_canceled` == `_executed` == `_unknown_id`),
so that heuristic would MASK A FILL. The list status is the safe discriminator
(`portfolio__orders_list` carries the same id `canceled` and other ids `executed`).
README finding-16 annotated with this correction.

**Tests** (verbatim recorded bodies; no fabrication): stale→list-canceled→Ok;
stale→list-EXECUTED→Rejected (safety headline, **mutation-proven** — flip the
Executed arm to `Ok(())` ⇒ that test reds); stale→absent→Timeout; the two existing
stale tests extended to the 3-call flow (Timeout preserved, not weakened). Full
`fortuna-venues` suite green; protected crate untouched; no new dep, no constructor
change. **Deferred (F16b, GAPS):** the full multi-attempt bounded-backoff poll —
needs an injected Sleeper + a recorded multi-stale fixture (never fabricated).

### 2026-06-13 — T4.5 slice: gate-verdict badge (/api/rota/v1/build) — `7ed3138`

**What.** New `/api/rota/v1/build` endpoint exposing the LATEST gate verdict
parsed from the verifier's `docs/reviews/*.md` — the local operator console's
build-health badge (design §7 cut it from v1 for "no parser"; T4.5 re-includes
it). New `RotaState.reviews_dir` capability (mirrors `perishable_dir`; main.rs
wires `docs/reviews`; a deployed daemon lacks `docs/` → "unknown"). `parse_verdict_token`
finds `verdict:` anywhere in a line (line-start AND mid-line `Base: … Verdict: X`
headers) and validates the ACCEPT*/BLOCK vocabulary (no prose false-positives);
`latest_gate_verdict` picks the newest-by-mtime `.md` carrying a verdict (the
rolling GATE-FINDINGS bus + verdict-less files skipped); bounded 8KB read; no-panic.

**Tests.** Parser units over every real format (+ mid-line, ACCEPT-WITH-CONDITIONS,
a prose-guard) + a deterministic populated-path scanner test (`File::set_modified`)
+ endpoint + degraded. code-reviewer ACCEPT (1 should-fix folded: the mid-line miss).

**Correction.** The iteration-14 validation note over-claimed the *discovery joins*
(a) as BUILDABLE-NOW; per design §4/§12 they are deferred (queries unwritten,
triage-recall not-in-v1) and discovery observability is track-B's — corrected in
GAPS/queue/§10. So the buildable track-A T4.5 surface is now COMPLETE: audit-recents
(gates+settlement) + this badge. Remaining: (c) WS counters, (d) money model — both
operator/verifier-blocked (GAPS).

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (1391 passed, 0 failed); run-dst.sh 200 (0 violations). (One run hit a
transient sqlx-test temp-DB-name collision in the pre-existing cognition test — a
known parallel-`#[sqlx::test]` flake, not this slice; green on re-run.)

### 2026-06-13 — T4.5 slice: /settlement.recent_watchdog_events — `9558d56`

**What.** Second T4.5 build slice (design §5), mirroring the gates slice.
`view_settlement` (rota.rs) merges `recent_watchdog_events` when the R5 pool is
present: the audit `watchdog` rows (sub-kinds settlement_overdue / dispute_freeze /
orphaned_position) → `{audit_id, at, kind (the sub-kind), market_ref}`, newest-first.
New `recent_watchdog_events_page` (runtime sqlx, `payload->>'kind'` text-extract).
No verdict filter — every watchdog row is an event.

**Honest/degraded.** Daemon-shaped "settlement" base view preserved (`views_from`
untouched — the fortuna-live views test still asserts the array is absent there); no
pool → explicit `available:false`; errors neutral. The bus `settlement_overdue` event
is a separate kind; the audit table carries `watchdog`.

**Tests (populated-path).** Seed all 3 watchdog sub-kinds + a non-watchdog row; assert
only the watchdog rows surface newest-first with the full §5 shape (first/middle/last
pinned), foreign kind excluded; available-but-empty; degraded-no-pool. code-reviewer
ACCEPT (clean faithful mirror).

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (1387 passed, 0 failed); run-dst.sh 200 (0 violations).

### 2026-06-13 — T4.5 slice: /gates.recent_rejections (audit-recents) — `59fa594`

**What.** First T4.5 build slice (design §5). `view_gates` (rota.rs) now merges a
`recent_rejections` sub-surface when the R5 read pool is present: recent per-check
gate REJECTIONS from the audit `gate_decision` trail, mapped to `{audit_id, at,
check, reason, intent_ref}`, newest-first. New `recent_gate_rejections_page`
extracts fields as TEXT in SQL (`payload->>'check'` etc.) — runtime sqlx, the
`audit_tail_page` precedent (off the sqlx-offline cache).

**Why.** The first of the BUILDABLE-NOW T4.5 pieces (the audit pool it was deferred
behind is live). Surfaces *why* orders were gate-rejected for the operator.

**Honest/degraded.** The daemon-shaped "gates" base view is preserved (`views_from`
untouched). No pool → explicit `available:false`, never fabricated; errors log
neutral detail, never raw sqlx. The bus `gate_reject` event is a separate kind
(live stream); the audit table carries `gate_decision`.

**Tests (populated-path, T4.5 TEST RULE).** Seed real `gate_decision` rows (2
Rejects + a Pass + the foreign `gate_reject` kind); assert only the 2 Rejects
surface newest-first with the full §5 shape, Pass+foreign excluded; available-but-
empty when no rejects; degraded-no-pool unavailable. code-reviewer ACCEPT.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (1384 passed, 0 failed); run-dst.sh 200 (0 violations).

### 2026-06-13 — T4.5 ROTA deferred panels: validation + slice plan (no code)

**What.** Validation-only iteration for T4.5 (deferred ROTA trading-side panels): a
code-explorer map of rota.rs/views.rs/ledger + the design §5 contracts, recorded as
fit-validation notes in `docs/design/rota-dashboard.md` §10 ("T4.5 validation").

**Findings.** Three pieces are BUILDABLE-NOW (the R5 audit pool they were deferred
behind is live): (e) audit-recents — `/gates.recent_rejections` is clean (`gate_reject`
audit kind, payload `{intent,check,reason}`), `/settlement.recent_watchdog` has a
two-path sink nuance to resolve first; (a) discovery joins (tradability/edges +
shadow-triage); (b) gate-verdict badge (low value). Two are BLOCKED and ledgered as
operator/verifier asks in GAPS: (c) WS gap/resync counters need the operator-run live
dial wired into `drive()`; (d) the full §5 money model needs an operator/design call to
surface the mark-loop `AccountView` via a SimRunner accessor. Ownership confirmed: these
are track-A trading-side surfaces (the cognition panel + §9 presentation are track-B).

**Next.** Build order: (e) /gates.recent_rejections → (e) settlement → (a) joins → (b)
badge, each with a populated-path `#[sqlx::test]` (the T4.5 TEST RULE).

**Battery.** Docs-only (no `.rs` touched) — the code battery is unchanged from the green
`fbbf861` state this session; `cargo fmt --check` clean. No code, no new tests.

### 2026-06-13 — fix: scope kinetics-DTO suite past track-C's basis fixture (main was red)

**What.** `kinetics_dto.rs`'s `every_fixture_parses_into_its_typed_dto` exhaustively
globs `fixtures/kinetics-perps/`; track-C's slice-3b commit (`2c17295`) added the
cross-venue basis composite `paired_cycle_btc_perp_vs_kxbtc.json` there (perp +
co-recorded KXBTC bracket, for `perp_event_basis`) — not a kinetics endpoint DTO, so
the exhaustive test failed `UNCLASSIFIED`. Added a documented `NON_KINETICS_FIXTURES`
exclusion (skip that one stem before the counter).

**Why.** This failed on **main** (pre-existing, confirmed against the main worktree —
the verifier's disk-deferred merge battery missed it), so `cargo test --workspace` was
red for every track. Correct scoping, not a weakening: every real kinetics fixture is
still classified + parsed + counted, `seen == table.len()` still exhaustive
(code-reviewer confirmed). GAPS-ledgered; the cleaner fix (relocate the basis fixture
out of the kinetics dir) is a track-C/verifier follow-up.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (0 failed); run-dst.sh 200 (0 violations). code-reviewer ACCEPT.

### 2026-06-13 — T4.2 (iii) Cluster 2 tail: recorded 409→AlreadyExists — `1e96d20`

**What.** One round-trip test in `kalshi_recorded_roundtrip.rs`:
`recorded_place_duplicate_client_order_id_resolves_to_already_exists`. `place()`
over the operator-recorded duplicate-409 fixture (nested
`{"error":{"code":"order_already_exists",...}}`) → resolve-by-coid GET →
`VenueError::AlreadyExists{existing}`.

**Why.** Closes clearance item 7. The 409→AlreadyExists routing was covered
synthetically (`kalshi_adapter.rs`) with a PLACEHOLDER code; this drives the real
nested wire body that placeholder awaited — idempotent place, never a false success.

**No vacuous re-tests.** Items 5 (unauth GET /markets) + 12 (legacy
`/portfolio/orders` write family) are closed by CITED existing coverage, not new
tests: `markets()` round-trips ×5 in `kalshi_adapter.rs` (the unauth distinction is
a venue property, not mock-exercisable); the adapter writes via
`/portfolio/events/orders` exclusively (item 16) and the legacy body is DTO-identical
to v2. Clearance tally now PASSes 5, 7, 12; the 2(iii) checklist is done bar the
operator-run live WS handshake.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (1325 passed, 0 failed); run-dst.sh 200 (0 invariant violations).
code-reviewer ACCEPT (sound, no issues). Protected crate untouched.

### 2026-06-13 — T4.2 (iv) kill-switch LIVE `freeze --venue kalshi` wiring — `7f69b81`

**What.** `crates/fortuna-killswitch` `main.rs` gains the live Kalshi freeze path
(replacing the stub): read the switch's own env creds → `load_kalshi_creds` (new in
`lib.rs`, pure, fail-closed) → `KalshiSigner` → `ReqwestKalshiTransport` →
`KalshiVenue` → `freeze_cancel_and_report_positions` on a self-spun current-thread
tokio runtime, with `RealClock`. New `tests/kalshi_live_wiring.rs` (9 tests).

**Why.** The machinery (`4e3a484`) was proven over a real `KalshiVenue` via a mock
transport; this is the binary actually wiring the production transport so the
operator can run a real demo freeze (the 27-item clearance is now signed on main).

**I4 (held, proven executably).** `i4_killswitch_independence` stays GREEN: `tokio`
is NOT in the structural forbidden set and is already transitive via
`fortuna-venues` (the direct dep adds zero packages); a self-spun one-shot reactor
for the HTTP cancels is not the daemon event loop; the sim `self-test` path is
byte-unchanged (operational layer) and the behavioral layer passes. "tokio for IO at
the edges."

**Fail-closed + secret-safe.** All three `FORTUNA_KILLSWITCH_KALSHI_*` env vars are
required (base URL never defaulted — prod vs demo must be explicit); a missing/blank
value or unreadable/empty PEM refuses before any venue call (exit 4). `KalshiCreds`
has a hand-written redacting `Debug` (mutation-tested); errors name only the env var
/ path, never key material.

**Operator dep (GAPS).** New env var `FORTUNA_KILLSWITCH_KALSHI_BASE_URL` (added to
`.env.example`); requested operator.md addition via GAPS. The live exercise itself is
operator-run.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (143 bins, 1324 passed, 0 failed, `i4_killswitch_independence` ok);
run-dst.sh 200 (4 corpus + 200 seeds, 0 invariant violations). code-reviewer ACCEPT
(1 must-fix [dead `RealClock.now()`] + 1 should-fix [exit-code assert] folded).
Protected crate untouched.

### 2026-06-13 — T4.2 (v) A2: Slack Socket Mode envelope loop — `f52ee66`

**What.** `crates/fortuna-ops/src/socket.rs` gains the ack-first listener LOOP
over a mockable `SlackSocketTransport`/`SlackSocketConn` (mirrors the Kalshi WS
dial seam `kalshi::dial`). `run_socket_loop`: connect → pump (ack → dedup →
dispatch) → redial. New `tests/socket_loop.rs` (12 tests) + 5 inline units.

**Why.** A1 was the pure decision logic; the loop is what actually receives,
acks, dedups, and survives reconnects against a recorded/mock transport — the
production-shaped listener minus the live socket (slice B).

**Safety teeth.** ack-FIRST before any sink touch (the 3s deadline; proven by a
shared ack-vs-sink ordering log); bounded envelope-id dedup ring — a
durably-handled envelope is suppressed but a `SinkError`-failed halt is left
UNrecorded so a Slack redelivery RE-ATTEMPTS it (code-reviewer should-fix folded
+ regression-tested); `SocketDial` capped-exponential reconnect surviving
transport loss AND the `disconnect`/refresh_requested lifecycle WITHOUT
escalating on planned refreshes; cancel watch (prompt mid-pump + mid-backoff).
I2 preserved end-to-end (a re-arm on the socket is acked but REFUSED). Untrusted
data: malformed frames skipped, no panic, no ack.

**Notes.** `SlackEnvelope.envelope_id` is now `#[serde(default)]` (hello/disconnect
protocol frames carry none). Two faithful Slack-vs-Kalshi differences ledgered for
B: no client subscribe step; no app-level keep-alive (B's real tokio-tungstenite
transport must set a WS ping/pong timeout so a half-open socket surfaces as a recv
error). ZERO new fortuna-ops dep.

**Remaining (GAPS).** B (operator-gated) = daemon wiring (HaltRequestSink → gate
halt path; EphemeralSender → SlackRouter) + real WSS transport + `[slack.socket_mode]`
config + `FORTUNA_SLACK_APP_TOKEN` + operator-run live.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (134 bins, 1209 passed, 0 failed); run-dst.sh 200 (4 corpus + 200
seeds, 0 invariant violations; ingest_dst 5/5; daemon_smoke 15/15). code-reviewer
ACCEPT (1 should-fix folded). Protected crate untouched.

### 2026-06-13 — T4.2 (v) A1: Slack Socket listener decision logic — `ca5082d`

**What.** New `crates/fortuna-ops/src/socket.rs` (+14 tests) — the Slack inbound
interactivity DECISION LOGIC (built to docs/research/ops/slack-api-2026-06-09).
`dispatch_envelope` routes block_actions / slash to handlers.

**Safety teeth.** I2 re-arm REFUSED (no halt path; `HaltRequestSink` exposes only
`request_halt` — code-reviewer confirmed airtight); allow-list (fail-closed empty;
absent user = no) + optional team restriction (WrongTeam); halt-only routing to
an injected sink (NOT the I4 killswitch); untrusted-data (action_id ENUM-matched,
reason bounded 500c opaque, panic-free indexing).

**Dep-clean.** Injected `HaltRequestSink`/`EphemeralSender` traits → ZERO new
fortuna-ops dep, no fortuna-runner/gates import.

**Remaining (GAPS).** A2 = the ack-first envelope loop + WS transport mock
(dedup/reconnect); B = daemon wiring + real WSS (tokio-tungstenite) + config +
`FORTUNA_SLACK_APP_TOKEN` + operator-run live.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (133
targets, 0 failed); run-dst.sh 200 (0 violations; daemon_smoke 15/15).
code-reviewer ACCEPT (2 must-fixes folded). Protected crate untouched.

### 2026-06-13 — T4.2 (iv) kill-switch Kalshi freeze machinery — `4e3a484`

**What.** `crates/fortuna-killswitch/tests/kalshi_freeze.rs` (1 test; test-only) —
proves the I4 freeze-and-cancel works over the REAL `KalshiVenue` adapter via a
mock transport (no live socket): open_orders → cancel each (DELETE + reconcile
GET → canceled) → KillReport(2 cancelled, 0 failed); 5 transport calls; the
flat-file journal records the freeze.

**I4.** Mock + `block_on` (no tokio runtime); `fortuna-venues` already a killswitch
dep → ZERO new crate → `i4_killswitch_independence` invariant test verified GREEN.

**Remaining (next slice, ledgered GAPS).** The live `freeze --venue kalshi` wiring
(FORTUNA_KILLSWITCH_* creds + ReqwestKalshiTransport on a current-thread tokio
runtime — I4 analysis flagged for verifier); live exercise operator-run after
clearance.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (132
targets, 0 failed, incl. i4_killswitch_independence); run-dst.sh 200 (0 violations;
daemon_smoke 15/15). code-reviewer ACCEPT. Protected crate untouched.

### 2026-06-13 — T4.2 (iii) Cluster 2/3: Kalshi auth-401 routing — `fe86cb5`

**What.** +1 parametric test in `kalshi_recorded_roundtrip.rs`: each recorded 401
auth-gateway body (bad-sig / unknown-key / missing-header / skew) → `balance()` →
`VenueError::Rejected` with the venue code surfaced; two needles use the `code=`
prefix so the auth path also proves G1 structured extraction discriminately.

**Verdicts.** Clearance item 3 → PASS; item 2 adapter-mapping half (skew 401 →
`header_timestamp_expired` → Rejected). code-reviewer ACCEPT. Battery green (131
targets, 0 failed; run-dst.sh 200 0-violations; daemon_smoke 15/15).

### 2026-06-13 — T4.2 (iii) Cluster 2: Kalshi exec round-trips — `811e383`

**What.** `crates/fortuna-venues/tests/kalshi_recorded_roundtrip.rs` (4 tests;
test-only) — transport round-trips driving place/cancel/fills through a scripted
`MockKalshiTransport` over the operator-recorded response bodies.

**Asserts.** place()→recorded 201→VenueOrderId; place()→recorded nested 400→
Rejected with the venue code structure-carried (G1 e2e); the cancel STALE-READ
RACE (F16)→Timeout, never a false success off the lagged reconcile GET;
fills_since round-trips the recorded fills (taker yes/52c/fee 2c, coid resolved
via GET order).

**Verdicts.** Clearance items 6, 8-routing, 15, 19-roundtrip → PASS. REMAINING C2:
409-dup-resolve routing, unauth GET, legacy order family; then Cluster 3.

**Ledgered.** Cancel-hardening follow-up (poll-until-terminal + recancel-404-as-
canceled) — safe today (Timeout → caller reconciles); see GAPS.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (131
targets, 0 failed); run-dst.sh 200 (0 violations; daemon_smoke 15/15).
code-reviewer ACCEPT. Protected crate untouched.

### 2026-06-13 — G1 fix: Kalshi error_reason nested-object extraction — `b2087fc`

**What.** `crates/fortuna-venues/src/kalshi/dto.rs` — `error_reason` now
structure-extracts the nested `{"error":{"code","message","details"}}` body
(`KalshiErrorBody.error: Option<serde_json::Value>`), the commonest recorded 4xx
shape (17/19). The 429 string shape and the flat shape are unchanged.

**Why.** Closes gap **G1** that the 2(iii) Cluster-1 clearance exposed — the
venue's error code now reaches diagnostics structured (`code=order_already_exists;
...`) instead of a raw-JSON dump. Diagnostic quality; HTTP-status routing was
already correct. Zero blast radius (dto.rs-internal).

**Tests.** TDD red-first: new `error_reason_extracts_the_nested_error_object`
(kalshi_dto.rs); `recorded_nested_4xx_...` tightened to require the `code=` prefix.
The 3 pre-existing error_reason tests unchanged + green.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (130
targets, 0 failed); run-dst.sh 200 (0 violations; daemon_smoke 15/15).
code-reviewer ACCEPT. Protected crate untouched.

### 2026-06-13 — T4.2 (iii) Cluster 1: Kalshi paper-clearance — `f7206a4`

**What.** `crates/fortuna-venues/tests/kalshi_recorded.rs` (18 tests; test-only) —
the FIRST tests to load the operator-recorded `fixtures/kalshi/` bodies (every
prior adapter test used doc-derived samples), asserting the adapter parses the
real wire per the README findings. Plus the 27-item clearance record
`docs/design/track-a-kalshi-paper-clearance.md` (operator-signed gate; UNSIGNED).

**Why.** Queue 2(iii): an executable, operator-signable clearance that the adapter
handles the wire the venue ACTUALLY sent — `venue=kalshi` stays boot-refused until
signed.

**Verdicts.** Cluster 1 PASS: 1,7,8,9,10,13,14,16,17,18,20,21. PENDING: Cluster 2
(transport round-trips), Cluster 3 (auth-skew, WS live handshake). UNCOVERABLE
(re-capture): demo/prod parity, STP maker mode, cursor stability/expired,
settlement units, populated series fee fields, maintenance-window status.

**Adapter gaps EXPOSED (ledgered GAPS, not fixed here).** G1 nested error body not
structure-extracted (diagnostic quality; routing correct). G2 no exchange-status
DTO/method (halt rails). Both resolve before promotion.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (127
targets, 0 failed); run-dst.sh 200 (4 corpus + 200 seeds, 0 violations;
daemon_smoke 15/15). code-reviewer pass folded in (C1 doc path; C2 legacy-family
label). Protected crate untouched.

### 2026-06-13 — T4.2 (ii) book-driven recorded-stream replay into PaperVenue — `e6dd7ec`

**What.** New integration test `crates/fortuna-runner/tests/recorded_replay.rs`
(7 tests; test-only, no production change). Drives the production replay seam
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
qualifying input.

**Fixture-blocked (ledgered in GAPS, never fabricated).** (1) Trade-through
replay — no public trade frame was recorded (quiet market); paper maker fills are
trade-driven (spec 11). (2) Structural-arb replay — a single-market recording
cannot complete a bracket; needs a multi-market bracket fixture.

**Battery.** `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D
warnings`; `cargo test --workspace` (126 targets, 0 failed); `scripts/run-dst.sh
200` (4 corpus + 200 seeds, 0 invariant violations; daemon_smoke 15/15;
ingest_dst 5/5). code-reviewer pass folded in. Protected crate untouched.

**Shared docs.** No architecture/runbook change warranted (test-only; the replay
seam and strategies are unchanged production code). BUILD_PLAN T4.2 progress
noted (box stays unticked — slices iii–v remain); queue item 2(ii) marked done.

### ROTA observability console (fortuna-ops, Track B)

The read-only operator single pane of glass (`crates/fortuna-ops/src/rota.rs`,
`assets/rota/`). Mission 2: total observability. Read-only doctrine absolute (zero
mutating endpoints), gold-on-black, honest nulls; every board screenshot-verified
with real rows (archived under `docs/reviews/rota-visual/`). Live status matrix:
`docs/design/rota-observability.md`.

#### Added

- Local bringup harness (`crates/fortuna-ops/examples/rota_local.rs`): seeds a
  GUARDED throwaway Postgres (`ROTA_LOCAL_DATABASE_URL` only, never the operator's
  DB) + a representative snapshot, serves the console — the reusable screenshot
  rig. The 7 original boards (health/money/gates/cognition/settlement/streams/audit)
  screenshot-verified with real rows.
- Generic `boardTable` renderer for the D-contract `{title, columns, rows, summary}`
  envelope, with a data-driven `pill` column flag — reused by every ingestion board.
- **V2 Sources Health** (`GET /api/rota/v1/ingest_sources`) — per-source health /
  polls / accepted / drop-by-reason / 304-rate / quarantines; surfaces the
  AFD-firehose. Now also the source_registry admission attributes **Domains**
  (`domain_tags`, joined; honest null "—" when untagged) + **Tier** (`trust_tier`),
  surfaced in `merge_ingest_views`'s `sources_board` after track-D's OBS-3 populated
  them — "what this source is and how trusted", beside its counters.
- **V1 Live Signal Feed** (`GET /api/rota/v1/ingest_feed`) — recent signals
  newest-first with their (redacted, esc()'d) data + accept/drop status pills.
- **V3 Ingest Funnel** (`GET /api/rota/v1/ingest_funnel`) — the pipeline as a stage
  table (fetched → validated → normalized → persisted) with retention % + drop-offs.
- **Discovery — Events board** (`GET /api/rota/v1/discovery`, mission item 4 "the
  canonical events we have, the markets under them") — the events ledger with each
  event's status + DISTINCT mapped-market count (a LEFT JOIN to
  `market_event_edges`, supersession-safe). A fortuna-ops runtime-sqlx query (the
  audit-tail pattern). Benchmark snapshots + per-event drill-in are follow-ons.
- **Database board** (`GET /api/rota/v1/db`, mission item 5 "honest visibility into
  the actual tables — counts") — an exact `COUNT(*)` sweep over every one of the 24
  ledger tables (incl. the `scalar_beliefs`/`belief_scores` scalar plane), busiest-
  first, with a `{tables, total_rows}` summary. The table
  names are query literals (UNION ALL, no interpolation — zero injection surface);
  a genuinely-empty table shows a real `0`, never an omitted row. A fortuna-ops
  runtime-sqlx query (the audit-tail pattern). NOTE (GAPS): exact COUNT is accurate
  at Sim scale — swap to `pg_class.reltuples` when `audit`/`signals` grow; per-table
  drill-in (recents / columns) is a follow-on.
- **Personas board** (`GET /api/rota/v1/personas`, mission item 1 "how beliefs are
  formed — the roster of analysts"; track-E §20.1 registry half) — every
  (persona_id, version) grouped by persona, newest version first, with domain, tier,
  lifecycle status (a `pill`: active→green, retired→dim), the method-file integrity
  hash (8-char prefix), the signal kinds it reads (`reads_signal_kinds` flattened),
  and effective date, plus a `{personas, versions, active}` summary. A fortuna-ops
  runtime-sqlx query (the audit-tail pattern); all columns are operator-authored
  config (not untrusted data). The §20.1 SCORECARD half (per-persona Brier/CLV/
  verdict) is data-blocked on track-E persona scoring — ROTA surfaces it when the
  data lands, never a fabricated score (GAPS).
- **Domain Analyses board** (`GET /api/rota/v1/analyses`, mission item 1 / track-E
  §20.2 "the whole process") — the analysis-artifact ledger newest-first: which
  persona (`id@version`) analysed which `region_key`, when, at what cost (dollars
  via the `cents` flag), the `content_hash` replay anchor (8-char prefix), and the
  supersession status, with an `{analyses, open, cost_cents}` summary. A fortuna-ops
  runtime-sqlx query (audit-tail pattern). UNTRUSTED-DATA BOUNDARY: this view renders
  STRUCTURAL METADATA ONLY — the `findings` / `signal_manifest` JSONB (untrusted
  model/signal output) are not selected or exposed; the per-artifact expander (where
  the esc/JSON-encode discipline applies) is a §20.2 follow-on (GAPS).
- **Forecasts scorecard** (`GET /api/rota/v1/forecasts`, track-C §9.1 "the outcomes
  of the whole process") — the scalar-forecast calibration headline: per (producer,
  scoring rule) the mean score (CRPS, lower=better) over RESOLVED forecasts, the
  resolved count, and the unit, with a `{producers, rules, scored}` summary. A
  `scalar_beliefs ⋈ belief_scores` runtime-sqlx aggregate (audit-tail pattern).
  SCORE METADATA ONLY — the untrusted `quantiles`/`provenance` JSONB are not selected
  or exposed; the recent-forecast feed + `coverage_bps` + sparkline are §9.1 follow-
  ons (GAPS). Degrades honest-`unavailable` until track-C's daemon persist (slice 4)
  writes the tables — never a fabricated score.
- **Working Orders board** (`GET /api/rota/v1/working_orders`, mission item 3 "trades
  being executed" — the live side) — the intents currently resting at the venue
  (submitted / acked / partially-filled, not yet terminal): market, side, action,
  limit (dollars), qty, filled, status, submitted-at, with a `{working}` summary. A
  `views_from` board shaped daemon-side from `runner.manager().intents()` filtered by
  `IntentStatus::is_working()` (the same ROTA seam as Strategy P&L; a pure panic-free
  read — daemon snapshot byte-unchanged, daemon_smoke 15/15). Empty when nothing rests
  (honest). With Recent Fills + Strategy P&L, mission item 3 (trades) is substantially
  covered; unrealized PnL remains the mark-loop gap.
- **Persona Scorecard board** (`GET /api/rota/v1/persona_scores`, track-E §20.1
  outcomes half — now unblocked by the merged persona runtime) — per persona, the
  calibration of its resolved beliefs: n_resolved, mean Brier (lower=better), mean
  CLV bps (higher=better), aggregated from the `beliefs` table grouped by
  `provenance->>'persona_id'`, with an honest `evaluating (n/60)` verdict. A pure
  AVG/COUNT projection — the §11 PROMOTABLE/RETIRE verdict + the raw/market baselines
  + calibration_quality are NOT computed in ROTA (unpersisted / cognition logic;
  omitted, never faked). Completes the Personas board's two halves (registry +
  scorecard). Honest-`unavailable` until the persona runner is daemon-wired.
- **Telemetry board** (`GET /api/rota/v1/telemetry`, mission item 6 "the Prometheus
  stack on the console") — the metric series the daemon exports (the same
  `MetricsRegistry` the `/metrics` exposition is rendered from), grouped by subsystem
  (ingest/gate/exec/state/venue/killswitch/cognition/…), one row per series with its
  type + integer value. R2-clean: the daemon shapes it via the new
  `MetricsRegistry::telemetry_board` (an additive `views["telemetry"]` key, daemon
  snapshot byte-stable) and ROTA serves it via `read_view` — the handler never parses
  Prometheus text. Completes the operator's single-pane-of-glass across all six
  mission areas (cognition, pipeline, trades, discovery, DB, telemetry).
- **Forecast Feed board — RICH scalar-belief feed** (`GET /api/rota/v1/forecast_feed`,
  track-C §9.1 + the operator "completely see the belief and everything" want, 2026-06-13):
  the recent scalar beliefs newest-first, each a click-to-expand `<details>` (the `/cognition`
  belief-panel precedent). The SUMMARY line carries producer · event · q=0.5 median · unit ·
  resolved/pending pill · → realized (honest null while pending); EXPAND reveals the WHOLE
  quantile FAN (every q/v pair) + the producer's EVIDENCE (its work — e.g. estimate /
  point_forecast / remaining_candles) + provenance. Reads `ScalarBeliefsRepo::recent`
  (newest-first by ULID belief_id; NO ledger change). The live daemon wraps
  `{"provenance":…,"evidence":…}` into the single `provenance` column (persist_scalar_beliefs)
  — SPLIT back here (both-keys detection; a non-wrapped row is shown WHOLE as provenance, never
  partially nulled). UNTRUSTED-DATA BOUNDARY (spec 5.11): `clean_quantiles` reads only the
  numeric q/v (malformed entries dropped, never raw-rendered); evidence + provenance are
  `truncate_evidence` size-capped and JSON-`esc()`'d — rendered as DATA, never interpreted. The
  scalar companion to the binary `/cognition` panel; completes §9.1's two halves (scorecard +
  rich feed). Screenshot-verified with real rows
  (`docs/reviews/rota-visual/rota-forecast-feed-rich-2026-06-13.png`).
- **Forecasts scorecard — band coverage** (§9.1 calibration metric): the Forecasts
  scorecard gains a quantile-band coverage column — per (producer, rule), the fraction
  of resolved forecasts whose realized outcome fell inside the 0.1–0.9 band (a
  well-calibrated producer ≈ 80%). Reads only the q0.1/q0.9 boundary numbers from the
  fan for the band check (the raw fan stays unexposed); a missing quantile degrades
  honestly to not-covered. Mean CRPS + coverage are now the two calibration measures.
- **Domain Analyses board — belief fanout** (§20.2): the Analyses board gains a
  `beliefs` column counting how many beliefs were built from each analysis
  (`beliefs.provenance ->> 'analysis_id'`) — the cognition pipeline's downstream
  output per artifact. A correlated `COUNT(*)` (no content exposed; the untrusted-data
  boundary holds). The full per-belief expander remains a follow-on.
- **Persona Pipeline board** (`GET /api/rota/v1/persona_pipeline`, track-E §20.4) — per
  persona, the cognition pipeline funnel: analyses produced → beliefs fanned out →
  beliefs resolved, over the persona-registry universe (a LEFT-JOIN aggregate; an idle
  persona reads honest 0s). The conversion at each stage is the pipeline-health signal.
  Counts only — no content exposed. (Universe is the registry: a persona attributed but
  not registered is omitted — it still appears in the scorecard.)
- **Cognition board — provenance legibility** (§20.3 / mission item 1): the per-belief
  expander now renders a LABELED one-line provenance summary (`persona id@version ·
  model · cost · analysis · run`) above the raw JSON dump — "which source/persona drove
  this belief," the reasoning made legible. A `provenance_summary` handler helper
  extracts the known keys into an additive `prov` field; the JS escapes every value.
  Pure JSONB field-extraction for display (no cognition computation); the whole
  provenance is still served. Cross-references the Personas/Analyses boards via the
  surfaced persona_id/analysis_id.
- **Strategy P&L board** (`GET /api/rota/v1/strategies`, mission item 3 "realized
  PnL per strategy") — per-strategy realized PnL / fees / fills / open exposure,
  shaped daemon-side from `runner.digest_snapshot()` (the same attribution the
  daily digest uses, no runner change) in the `views_from` ROTA seam, served via
  `boardTable` with money columns as dollars. A losing strategy renders honestly
  (negative). Unrealized PnL stays the mark-loop gap; working orders
  (`runner.manager().intents()`) is the remaining trades follow-on.
- **Recent Fills board** (`GET /api/rota/v1/fills`, mission item 3 "trades being
  executed") — the executed trades from the durable `fills` ledger, newest-first
  (time/market/side/action/qty/price/fee/maker-taker). A runtime-sqlx query (the
  audit-tail pattern, no fortuna-live touch) + a new data-driven `cents` column
  flag on `boardTable` so money columns render as dollars. A fill carries no
  strategy/PnL (ledgered): per-strategy P&L (a views_from board) + working orders
  + the honest unrealized-PnL gap (no mark loop) are follow-ons.
- **OBS-2c — V1/V2/V3 now render LIVE daemon data.** `merge_ingest_views`
  (fortuna-live `views.rs`) shapes the daemon-published `IngestionTelemetryHandle`
  (track-D OBS-2b) into the three board envelopes each ROTA segment, merged at the
  snapshot-composition site (`main.rs`, non-blocking `try_read`). Honest gate: an
  unticked / ingestion-off telemetry merges nothing, so the boards stay degraded and
  the daemon snapshot is byte-unchanged (daemon_smoke 15/15). Unit-tested to produce
  the exact screenshot-verified envelopes; ROTA stays a pure snapshot reader
  (fortuna-ops gains no fortuna-sources dependency).
- Cognition board **belief lifecycle** — status distribution (open/resolved/
  superseded/abandoned) + the resolved beliefs' calibration outcome (mean Brier/CLV)
  via a real `GROUP BY`/`AVG` (runtime sqlx).
- Loop-file rule 6 — the operator doc-discipline directive (own docs + targeted
  shared-doc edits + this changelog; no staleness), part of DoD.

#### Deferred / blocked (ledgered in GAPS)

- **D V6** full belief→strategy→PnL — schema-blocked (no belief→trade link); ROTA
  surfaces the calibration edge proxy (CLV), never a fabricated dollar PnL.
- **C** `/forecasts`,`/perps` and **E** `/personas`,`/analyses`,`/persona_pipeline`
  — built as their tables/data land.
