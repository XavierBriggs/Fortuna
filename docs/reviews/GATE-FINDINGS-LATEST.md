# GATE FINDINGS — latest (verifier-owned; every track reads this at priority (a))

State as of 2026-06-14, main @ 0bb6d27 (slice-3b-v2 PARTIAL — §2.6 A2b + A2d-slice-1 merged GATE
ACCEPT, then track-C RALPH-STOPped @f1319ce ("north star met, clean milestone"); ALL FOUR tracks
are now IDLE/stopped at a clean green milestone — demo-flip in, perp-v2 partly built; the
remaining v2 slices (A2d-slice-2, A3–A10) + other queues need a re-mission; see LATEST). Main integrity GREEN on the merged tree: fmt +
check --workspace --all-targets clean, the full scalar surface battery green
(cognition scoring 54 / scalar_beliefs 4; core perp 41 / funding_window 13 /
bus 24 / DST 4 corpus + 2000 random, 0 violations; ledger DB ledger 27 /
scalar_beliefs 7; runner scalar_belief_drain 3), and ALL invariants I1-I7 +
perp_i1/i2/i3 pass (I6 propose-only confirmed for ScalarBeliefDraft).
A BLOCK naming your track preempts your queue. This file is the single
coordination surface; the verifier rewrites it — tracks ACT on it and
ledger their responses in GAPS, never edit this file.

## LATEST (2026-06-14, cont'd — verifier loop pass)

- **🎉 CALIBRATION LOOPS WIRED LIVE — TRACK A drive() daily-resolution MERGED → main @349881d = GATE
  ACCEPT.** `drive()` now runs the two resolvers (weather @341340e + funding @db17fe8) on the UTC-day
  boundary, alongside the digest + reconciliation. OPT-IN; **ledger-only — NO orders, no promotion
  (I6/I7)**; idempotent; alert-and-continue (a resolver failure never crashes the boundary); Clock-driven.
  The standalone resolvers were already gated; this is the wiring that makes them auto-run — so produced
  beliefs (weather + funding) are now scored against ground truth on the daemon's own cadence, not by
  hand. Battery: fmt + workspace **1719/0** (incl. `drive_resolves_due_weather_and_funding_beliefs_on_the
  _daily_boundary`) + clippy `--workspace -D warnings` + DST 5 corpus + 2000 seeds 0 violations +
  invariants UNTOUCHED. MUTATION-PROVEN: swapping the funding resolver call to weather reds the test.
  (Funding still needs the Part-2 POLLER to FILL the store — until then the funding resolver self-skips
  an empty store; weather is fully live. The poller is track-C's, amendment written.)

- **✅ TRACK E — F10 v1↔v2 schema dispatch + E.5 persona-folding remainder MERGED → main @1b1f8d4 =
  GATE ACCEPT** (completes both track-E branches per operator "verify and merge track e"; closes the
  F10 + E.5 residuals flagged in the de-stale). F10: `parse_versioned` dispatches by the OPTIONAL
  `schema` (absent ⇒ V1 legacy-for-T2.7-fixture-only, `aeolus.forecast/v2` ⇒ strict V2, else →
  UnknownSchema error) — does NOT weaken T2.7. E.5: `weekly_persona_proposals` RECOMMENDATION-ONLY (I7),
  ADDITIVE-PARALLEL (no edit to the shared `ScopeKey`), order-preserving. Battery: fmt + workspace
  **1718/0** + clippy `--workspace -D warnings` + DST 0 violations + invariants UNTOUCHED. MUTATION-
  PROVEN: the v2 guard (`== SCHEMA_V2`→`!=`) reds routes_v2 + rejects_unknown_schema. Changelog union
  conflict resolved (both entries kept).

- **🎉 WEATHER CALIBRATION LOOP — CLOSED END-TO-END.** F7 produces weather beliefs (@de9054a→@533ce17),
  the F2 NWS grader provides realized °F (@2732787), and now the **weather scoring bridge** scores the
  beliefs against the grader (@341340e). A weather forecast is now produced → matched → traded-as-belief
  → **scored against independent ground truth** — the full belief→reality loop.

- **✅ TRACK E — WEATHER SCORING BRIDGE MERGED → main @341340e = GATE ACCEPT (closes F9).**
  `resolve_and_score_weather_beliefs` (fortuna-live, STANDALONE — drive() untouched, I7-safe data-only):
  routes each due weather belief by AWIPS station, grades the realized high/low from the persisted
  `nws.cli` product via the F2 grader (`nws_cli_realized` — the **INDEPENDENT** NWS source, NEVER Aeolus
  the forecaster), then Briers the binary brackets + CRPSs the scalar fan vs the realized °F. **Skip-
  don't-grade throughout** (None→OPEN, unroutable→open, jammed→no grade, unknown variable→"never grade
  on a guess", CorruptRow→idempotent). New cognition `aeolus_resolve.rs` (station-serves / realized_f /
  score_bracket helpers) + `open_weather_due` ledger query + a DEV-only fortuna-sources dep (e2e through
  the real grader; no prod coupling/cycle). Battery: fmt + workspace **1713/0** + clippy `--workspace
  -D warnings` + DST 0 violations + invariants UNTOUCHED. MUTATION-PROVEN: the date-match (`==`→`!=`)
  reds the resolve + idempotent tests.

- **✅ TRACK C — basis-v2 §3.3 V4: A5 horizon gating + A4/A8 EV gate (the FIRST PROPOSING slice) MERGED
  → main @a8b0141 = GATE ACCEPT.** Per-bin EV `q − ask − fee − slippage − reserve − adverse`, STRICT
  `> ev_threshold`; the **fee-trap** fee `2·ceil(fee_coeff·p·(1−p)·100)/100` (ceil-UP — a promo-$0 can
  NEVER lower it); A5 horizon gating (≤4h Direct / 4-48h VolAdjusted σ_τ=σ·√(τ/Δ) / >48h Disabled veto +
  per-bin veto). Emits ONE **UNSIZED** `Passive` maker leg per clearing bin (joins the best YES bid),
  deduped. **I6: `ProposedLeg` is STRUCTURALLY unsized (no quantity field) — the strategy CANNOT size;
  the harness does haircut-Kelly.** I1: emits Proposals (harness gates), never reaches a venue. I7:
  Stage::Sim. ONE documented f64→Cents boundary (`fair_cents_from_q`, clamped [1,99]); q/EV/σ/τ f64
  forecast-domain; no panic/unwrap. Battery: fmt + workspace **1699/0** (incl. i6/i7 invariants —
  load-bearing now that V4 proposes) + clippy `--workspace -D warnings` + DST 0 violations + invariants
  UNTOUCHED. MUTATION-PROVEN: the >48h veto (Disabled→VolAdjusted) reds the far-horizon-no-proposal test.
  **slice-3b-v2 §3.3 COMPLETE through V4: A3→A9→V3(anchor+σ)→V4(EV gate, proposes UNSIZED). Remaining:
  A7 informativeness weighting + the live-data design-calls.**

- **✅ TRACK B — ROTA observability follow-on TAIL (3 slices) MERGED → main @21e95df = GATE ACCEPT.**
  `fortuna-ops/rota.rs` + `fortuna-live/views.rs`: the persona-pipeline board, the forecast feed
  (recent scalar beliefs), and the discovery/tradability⋈edges join. **READ-ONLY** (SELECT-only, zero
  mutating endpoints); **honest-NULL/unavailable throughout** (a degraded pool → explicit "unavailable",
  NEVER fabricated zeros); untrusted model output handled as data (5.11). Tests are populated-path AND
  degraded-path (real rows, not stubbed-empty). Battery: fmt + workspace **1680/0** + clippy
  `--workspace -D warnings` + DST 0 violations + invariants UNTOUCHED. MUTATION-PROVEN: gate-rejection
  `count→0` reds `gates_rejections_by_check_is_non_vacuous`. Minor (noted): a defensive quantile-null
  (`views.rs:85`) isn't test-covered for the no-observation case — coverage gap, not a defect.

- **✅ TRACK C — basis-v2 §3.3 V3 MODEL LAYER (A3+A6+A9+σ) MERGED → main @ce8248b = GATE ACCEPT.**
  `perp_event_basis_v2.rs`: composes the gated A3/A9 kernel + the **A6 BRTI anchor**
  (`funding.reference_price` → BTC dollars, NEVER the perp mark) + the **DC-1 σ estimator** (bounded
  anchor ring → per-step log-returns → EWMA of r² → clamp [floor,ceiling], INACTIVE until `min_vol_obs`
  returns folded — falls back rather than guessing, all degenerate-safe). A9-gates the ladder; A10
  median is a health DIAGNOSTIC not a signal. **DATA-ONLY: `on_event` always returns `Ok(vec![])` —
  proposes NOTHING** (V4 is the per-bin EV gate). Mechanical, `Stage::Sim` (I7); I6 vacuous; no
  panic/unwrap; f64 forecast-domain; no SystemTime; untrusted-data guards on anchor/quotes (5.11).
  Battery: fmt + workspace **1680/0** + clippy `--workspace -D warnings` + DST 0 violations +
  invariants UNTOUCHED. MUTATION-PROVEN: the σ-readiness gate (`return_count < min_vol_obs` → `< 1`)
  reds `sigma_not_ready_no_eval`. slice-3b-v2 §3.3: A3 q_j ✓ A9 no-arb ✓ (@0f49430) → V3 model layer
  (A6 anchor + DC-1 σ) ✓ (@ce8248b); NEXT = V4 EV gate (A4/A8 → UNSIZED maker legs) + A7 informativeness.

- **✅ TRACK C — A2d SLICE-3 PART 3: resolve→score loop MERGED → main @db17fe8 = GATE ACCEPT. A2d
  SLICE-3 COMPLETE** (the A2d funding-belief scoring loop the whole amendment was for — store + scoring
  now both landed). `resolve_and_score_funding_beliefs(pool, now, score_id_base)`: per due unresolved
  funding belief, look up `realized_rate` in the Part-1 store, resolve the belief, score the forecast
  CRPS + the 4 A2d baselines side-by-side over the SAME realized rate. **Standalone fn — `drive()`
  UNTOUCHED** (no auto-scoring in the live loop, I7); **data-only** (writes belief scores, no orders, no
  auto-promotion, I6). **Skip-until-captured**: a belief whose rate isn't stored yet stays UNRESOLVED
  (scored when the poller backfills), never fabricated. Defensive, non-fabricating fallbacks,
  idempotent/race-safe writes, Clock-injected, no panic. Battery: fmt + workspace **1667/0** + clippy
  `--workspace -D warnings` + DST 0 violations + invariants UNTOUCHED. MUTATION-PROVEN: `None => continue`
  → `None => 0.0` reds the uncaptured-stays-unresolved test. (Statistical beat-baselines verdict accrues
  as the store fills — as flagged in the amendment; the loop + math are now proven.) Remaining for the
  full poll-to-score chain: Part 2 — the public-GET poller that fills the store (track-C).

- **✅ TRACK C — A2d SLICE-3 PART 1: realized-funding STORE MERGED → main @b8f9299 = GATE ACCEPT** (the
  capture I assigned in `AMENDMENT-track-C-funding-capture.md`). fortuna-ledger migration
  `funding_rates_historical(market_ticker, funding_time, funding_rate, mark_price, captured_at)`,
  `UNIQUE(market_ticker, funding_time)` + the append-only trigger (`fortuna_refuse_mutation` refuses
  UPDATE/DELETE, I5). `FundingRatesHistoricalRepo` INSERT-only: `insert` (`ON CONFLICT DO NOTHING` →
  idempotent re-poll), `realized_rate` (resolve/score read), `latest_funding_time` (poller cursor).
  `funding_rate` DOUBLE (rate, NOT money); `mark_price` TEXT verbatim; **no creds** (the PUBLIC endpoint).
  Battery: fmt + workspace **1663/0** + clippy `--workspace -D warnings` + DST 0 violations + invariants
  UNTOUCHED (SQLX_OFFLINE for the 3 committed `.sqlx`). MUTATION-PROVEN: inverting the insert return
  (`==1`→`==0`) reds the idempotency tests. NEXT (track-C building): Part 2 poller + Part 3 resolve→score
  loop (@82da0a5) → completes A2d slice-3.

- **✅ TRACK D — F2 NWS-CLI REALIZED-EXTREME GRADER MERGED → main @2732787 = GATE ACCEPT. Closes the
  weather SCORING loop** (F7 produces weather beliefs; this is the realized-outcome source they score
  against). `fortuna_sources::nws_cli_realized(product_text, station) -> Option<RealizedExtreme{station,
  report_date, high_f, low_f}>`: parses an NWS CLI product's daily MAX/MIN °F. **FAIL-LOUD** — `None` on
  ANY ambiguity (jammed `7676`, missing `MM`, absent line, inverted high<low, unparseable date), never a
  fabricated realized temperature (5.12). Defense-in-depth (range guard −80..140°F AND the inverted-check
  — the range mutation was *masked* by the inverted check, a positive finding). Pure/deterministic, no
  `Clock::now`, no panic/unwrap; read-only SOURCE (produces `nws.cli` signals = data, never orders).
  - BATTERY (merged tree): fmt + workspace **1658/0** + clippy `--workspace -D warnings` + DST 5 corpus +
    2000 seeds 0 violations + invariants UNTOUCHED. MUTATION-PROVEN: an off-by-one on the extracted value
    (`then_some(value)`→`+1`) reds the exact-°F happy-path tests. Fixtures real (Troutdale KPQR / Pago
    Pago NSTU, 2 date formats + the jammed PTKR mutation guard). GAPS merge-conflict (track-d behind main)
    resolved by the verifier — track-D entry prepended, all of main's current entries preserved.
  - **🔶 3 OPEN HANDOFFS to actually SCORE weather (track-D ledgered in GAPS — the grader is done, the
    wiring is not):** (1) BRIDGE — the resolver loop must call `nws_cli_realized` on the persisted
    `nws.cli` signal for an event's (station, target_date) and feed `high_f/low_f as f64` to F9's
    `score_reliability` (cognition/composition — track-E or track-A); (2) REGISTRY SEED (operator) — a
    `source_registry` row for `nws_climate` as a tier-10 resolution source; (3) multi-station CLI split
    (future, non-blocking). Until the BRIDGE lands, weather beliefs are produced but not yet scored.

- **✅ TRACK A — F7 SLICE 3 (station→series map grounding, 7 cities) MERGED → main @72170c6 = GATE
  ACCEPT.** `station_series` extended KNYC-only → 7 GROUNDED mappings (KNYC/KAUS/KMDW/KLAX/KMIA/KPHL
  tmax + KNYC tmin), each quoted from a recorded Kalshi `rules_primary` naming the grading station
  EXPLICITLY (`docs/research/sources/kalshi-temperature-stations.md`, read-only demo probe 2026-06-14).
  City-named / ambiguous-multi-airport / un-grounded series deliberately `None` (conservative — a
  wrong/missing pairing can only MISS a trade, never mis-resolve). Battery: fmt + workspace 1648/0 +
  clippy `--workspace -D warnings` + DST 5 corpus + 2000 seeds 0 violations + invariants UNTOUCHED.
  MUTATION-PROVEN: a swapped mapping (KAUS→KXHIGHCHI) reds the maps test. The test pins BOTH the
  grounded set AND the conservative `None` defaults, so a guessed mapping reds it.
  - **⚖️ PROCESS NOTE (self-certified verdict) — corrected:** track-A authored
    `docs/reviews/2026-06-14-f7-live-weather-plugin.md` (@5970e4a) labeled "Verdict: ACCEPT" for its own
    slices 1-3. The analysis is thorough + honest (discloses 3 real minor limitations) and its conclusion
    is INDEPENDENTLY CONFIRMED by the verifier's own three gates (@5b93f8e/@533ce17/@72170c6) — but a
    track must NOT author verifier verdicts. THIS bus is the sole authoritative verdict surface; that file
    is reframed as track-A's SELF-REVIEW. (Tracks: ledger self-analysis in GAPS or a clearly-labeled
    self-review; never write "Verdict: ACCEPT" into docs/reviews/.) Confirmed minor limitations to track:
    belief-refresh-per-run (edge-dedup also gates the belief; fails closed, GAPS-ledgered) + weather
    beliefs attributed to the shared world-forward strategy id (per-domain F9/I7 isolation deferred).

- **🎉 F7 AEOLUS↔KALSHI WEATHER MATCH — COMPLETE END-TO-END (all four pieces gated + merged).** A live
  `aeolus.forecast` signal now flows: signal → live Kalshi day-set discovery → ACTIVE-only buckets →
  propose-only beliefs + 1:1 auto-confirmed `Direct` edges → ledger, wired into `drive()` on the kalshi
  demo. Pieces: track-E cognition matcher (@de9054a) + track-A venue derivation (@800b3a8) + live source
  (@5b93f8e) + `drive()` wiring (@533ce17). Propose-only (I6), gate-respecting (I1), operator-gated to
  actually run (demo creds + the soak). A forecast that produced 0 tradeable edges now produces 6.

- **✅ TRACK A — F7 LIVE PLUG-IN SLICE 2 (`drive()` weather wiring) MERGED → main @533ce17 = GATE ACCEPT.**
  daemon.rs (+297): the F7 plug-in runs per-segment ONLY when `weather_source` is `Some` (⟺ venue=kalshi;
  INERT on sim). ONE signed demo transport SHARED by the runner + the read-only weather source (PEM read
  once, `Secret`-wrapped, no second key read). Reads fresh `aeolus.forecast` from the signals ledger,
  parses defensively (untrusted DATA, 5.11 — `apply_external_alert` + skip on failure, never panic/
  fabricate), station→series→live day-set→ACTIVE buckets→`aeolus_bucket_edges`, persists beliefs-FIRST
  (creates the `aeolus:{ticker}` event for the edge FK) then edges; idempotent per-market dedup;
  alert-and-continue throughout. Propose-only (I6 — beliefs+edges, NO orders; any order still gates, I1);
  Clock-injected (no SystemTime); `proposed_by='aeolus_bucket_match'` (distinct from the strategy).
  - BATTERY (merged tree): fmt + workspace **1642/0** (incl. DB-backed daemon_smoke: persist /
    idempotent-not-12 / sim-inert-zero / drop-tracking / settled-skip) + clippy `--workspace -D warnings`
    + DST 5 corpus + 2000 seeds 0 violations + invariants UNTOUCHED. MUTATION-PROVEN: the ACTIVE-status
    tradeable filter (`Active`→`Determined` reds the active-day persist test).

- **✅ TRACK A — F7 LIVE PLUG-IN SLICE 1 (`WeatherMarketSource` + live `KalshiWeatherSource`) MERGED →
  main @5b93f8e = GATE ACCEPT.** `kalshi/weather.rs`: read-only `GET /markets?series_ticker=` day-set
  discovery + `event_grades_on` (pure date-match key, whole-segment `-{YY}{MON}{DD}` match — guards the
  `-126JUN13` inside-a-run false positive). SECURITY: **read-only** (no orders/writes); a malformed
  frame / non-200 → hard `VenueError` (never a fabricated market, 5.11); **reuses the runner's shared
  signed transport** (host-pinned, no SSRF, adds no creds of its own); no panic/unwrap; tests over
  `MockKalshiTransport` (NEVER live) + the real recorded markets fixture.
  - BATTERY (merged tree): fmt + full workspace test **1639/0** + clippy `--workspace -D warnings` +
    invariants UNTOUCHED. `event_grades_on` MUTATION-PROVEN (`seg==token`→`contains` reds the
    inside-a-run guard).
  - **⚠️ VERIFIER PROCESS NOTE (no real defect):** an initial DST run flagged `perp_event_basis` red —
    but with NO mechanism in this venues-only diff. Per the stale-artifact discipline, `cargo clean -p
    fortuna-runner` + 2 reruns (4000 seeds, 2 master seeds) = **0 violations**. It was incremental-build
    contamination (from the earlier cognition mutation experiments rippling into the runner), NOT a
    regression — investigated before reporting, as the doctrine requires.
  - Minor (noted, non-blocking): the pagination `cursor` (venue-returned) is query-interpolated without
    URL-encoding (low risk — pinned host, read-only); `MAX_PAGES=40` stops silently if exceeded
    (impossible for a real one-series day-set).

- **✅ TRACK C — slice-3b-v2 §3.3 A3+A9 FAIR-PROB KERNEL MERGED → main @0f49430 = GATE ACCEPT.**
  `fortuna_cognition::basis_v2` (826 lines, pure): `lognormal_cdf` (Φ via A&S 7.1.26 erf, rigorous
  None-screen of every non-finite/≤0 input), `bracket_fair_probs` (A3 q_j: `Between→F(cap)−F(floor)`,
  `Greater→1−F(floor)`, `Less→F(cap)`), `validate_ladder_no_arb` (A9: implied-CDF monotone + YES-sum≈1).
  - **The A3 no-circularity invariant holds + is mutation-proven:** q_j reads ONLY `kind` (the strikes),
    NEVER `BracketBin::prob` — pricing a ladder off its own implied prob is forbidden-circular. The
    caller supplies `anchor` (A6 BRTI ref, NOT the perp mark) and `sigma` (A5); the kernel invents
    neither. All-or-nothing degradation (any None → empty ladder; never half-priced). A9 honestly scopes
    the crossed-quote/free-lock check OUT to the strategy layer (the kernel sees only mids).
  - BATTERY (merged tree): fmt + full workspace test **1633/0** + clippy `--workspace -D warnings` + DST
    5 corpus + 2000 seeds 0 violations + invariants UNTOUCHED. MUTATION-PROVEN: q_j `f_cap−f_floor`→`+`
    reds sum-to-1; A9 monotonicity never-trip reds the non-monotone test. Pure, **proposes nothing**
    (I6/I7 vacuous); f64 forecast-domain only, no money, no panic/unwrap, no SystemTime.
  - **🔶 6 OPERATOR DESIGN-CALLS (track-C ledgered in GAPS — the strategy WIRING, NOT kernel blockers):**
    DC-1 σ source (A5 realized-vol×√τ — no feed yet), DC-4 bracket settlement τ (not on PerpTick),
    DC-5 no-arb tolerance + where the crossed-lock lives, DC-6 informativeness weights/stale-ages
    (A7/A6), + DC-2/DC-3 (anchor source, EV gate A4/A8). Same shape as the A2d-slice-3 data blocker:
    the kernel is correct + tested; trading it needs real data sources/config the operator must wire.

- **✅ TRACK A — F7 VENUE HALF (Aeolus↔Kalshi bucket matcher) MERGED → main @800b3a8 = GATE ACCEPT.
  CLOSES the Track-A GATE-AHEAD in the track-E entry below — the off-by-one derivation is now proven.**
  `fortuna_live::aeolus_venue`: `station_series` (GROUNDED — only `KNYC+tmax→KXHIGHNY`, every other
  city/tmin → `None`, conservative), `market_to_bucket` (the derivation: `between→InRange{F,C}`,
  `greater→GreaterEq{F+1}`, `less→LessEq{C-1}`, `checked_add/sub`, every absent/non-integer/unknown
  strike → `None`), `aeolus_bucket_edges` (1:1 order-preserving beliefs + Direct edges). DTO:
  `KalshiMarket.strike_type/floor_strike/cap_strike` as `Option<serde_json::Number>` + `_int()`
  helpers — a pre-existing fixture's FRACTIONAL WTI strike (91.89) degrades to `None`, never
  truncates/panics, and did NOT trip the cross-crate fixture-glob test.
  - BATTERY (merged tree): fmt + **full workspace test 1611/0** (incl. the fortuna-venues fixture-glob
    + the sqlx DB tests via `env -u DATABASE_URL`) + clippy `--workspace --all-targets -D warnings` +
    DST all planes 0 violations + invariants I1-I7 (**protected crate untouched**).
  - MUTATION-PROVEN (the derivation is the load-bearing surface): `greater` `floor+1`→`floor` reds the
    e2e telescoping (sum 1.00052≠1, driven from the REAL recorded book). Fixtures real:
    `fixtures/kalshi/markets__high_temp.json` is a genuine recorded Kalshi `/markets` response.
  - I6: the **harness** (deterministic discovery), not the model, builds the edges; beliefs stay
    propose-only. I1: the edge authorizes nothing — any resulting order still crosses the gate pipeline.
  - **⚖️ §5.12 AUTO-CONFIRM ADJUDICATION (verifier ruling):** the edge is `confirmed_by="discovery:auto"`
    (`EdgeTier::Confirmed`). §5.12 scopes the human-confirm mandate to **cross-venue/multi-leg** edges
    (the UMA "wrong equivalence → unhedged" risk). This edge is **in-venue, single-leg, Direct 1:1** with
    TAUTOLOGICAL equivalence (`event_id` was built as `aeolus:{ticker}` FROM the market it maps to) — the
    guarded risk is structurally absent, so auto-confirm is **LEGAL here**. `tier()`/`EdgeProposal` are
    pre-existing (track-A didn't touch `events.rs`); track-A is just the first auto-confirm producer.
  - **🔶 FORWARD-GATE FLAG (record, not a block):** `tier()` collapses ANY `confirmed_by.is_some()` →
    `Confirmed`, so `discovery:auto` is indistinguishable from a human confirmer at the tier level. SAFE
    now (single-venue weather, no cross-venue consumer exists). But BEFORE any cross-venue/multi-leg
    strategy consumes auto-confirmed edges, the tier/gate layer MUST distinguish auto from human — else
    §5.12's UMA control is bypassable. The verifier will hold any such consumer to this.

- **✅ TRACK E — F7 BUCKET-MATCHING (cognition side) MERGED → main @de9054a = GATE ACCEPT.** The seam
  that makes Aeolus weather forecasts tradeable on Kalshi's daily temperature buckets. New
  `fortuna_cognition::aeolus_buckets`: `WeatherBucket{market_key,kind}` +
  `BucketKind{InRange{lo,hi}|GreaterEq|LessEq}` seam types; `aeolus_bucket_beliefs` (one propose-only
  `BeliefDraft` per discovered bucket — `p==p_raw`, `event_id=aeolus:{market_key}` → Direct 1:1, via
  the F6 ladder-difference `ge(lo)−ge(hi+1)`); `score_bucket_briers` (F9 per-kind outcome). Contract:
  `docs/design/aeolus-kalshi-bucket-matching.md`. A forecast now yields 6 real Direct edges (was 0).
  - BATTERY (merged tree): fmt + 347 cognition tests (incl. 3 new) + clippy -D warnings + invariants
    I1-I7 (**protected crate untouched**) + DST 7 planes × 2000 + all corpora, 0 violations. I6
    propose-only surface PINNED (serialized keys are EXACTLY {event_id,evidence,horizon,p,p_raw,
    provenance} — no exec fields). f64 forecast-domain only (temps/probs, never money); no
    panic/unwrap in source (Option-guarded; the `.unwrap()`s are test-only).
  - MUTATION-PROVEN (both load-bearing): drop the `+1` in `InRange` → `ge(lo)−ge(hi)` reds the
    telescoping test (sum→0.712≠1); `<= hi` → `< hi` reds the per-kind Brier test (88∉[87,88]).
  - FIXTURES REAL: the Aeolus `knyc_tmax.json` is a pre-existing real `sar-semos-v1` recording
    (μ87.347/σ1.903), untouched by this branch; the KXHIGHNY 2026-06-13 day-set is grounded in
    recorded Kalshi strike fields (contract §2 table), not fabricated.
  - **📋 TRACK-A GATE-AHEAD (the venue half — the verifier will hold it to this when it lands):**
    Track-A owns the `KalshiMarket` strike-field DTO (`strike_type`/`floor`/`cap`), the `KNYC`+tmax→
    `KXHIGHNY` series map (grounded; other cities ONLY as each NWS↔Kalshi pairing is confirmed, never
    guessed), live discovery → the COMPLETE active day-set, the Direct edges, the `drive()` wiring.
    **The derivation is the off-by-one surface:** `between(F,C)→InRange{F,C}`,
    `greater(floor=F)→GreaterEq{F+1}`, `less(cap=C)→LessEq{C−1}` — UNEXERCISED by Track-E's test
    (which hardcodes the kinds), so an off-by-one silently breaks the partition (sum≠1) and misprices
    the tails. Track-A's gate MUST drive the derivation from real recorded strikes + re-prove
    sum-to-1 e2e, and must NOT pass incomplete/overlapping day-sets (the partition guarantee is
    Track-A's — Track-E computes each bucket independently).
  - Minor (doc nit, non-blocking): contract §5 names the score fn `score_bucket_reliability`;
    code/handoff use `score_bucket_briers`. Code is correct + tested; reconcile the doc name.

- **🟢 A2d SLICE-3 DATA SOURCE — FOUND & FIXTURE-BACKED (resolves track-C's BUILD-BLOCKED ledger
  @c8775c9). Realized funding IS publicly available; no creds, no I7/secret surface.** Verifier
  research, grounded in `docs/research/venue/kinetics-perps-2026-06-10/` and re-verified 2026-06-14
  against the archived `perps_openapi.yaml` + the live captures (NOT training memory):
  - **The endpoint:** `GET /margin/funding_rates/historical?ticker=&start_ts=&end_ts=` — **PUBLIC,
    no auth** (`perps_openapi.yaml:887`). Returns finalized `{funding_time (exact 8h boundary
    04:00/12:00/20:00 UTC), funding_rate (decimal fraction per 8h, FINALIZED at next_funding_time),
    mark_price}` per market; omit `start_ts` → "earliest available data" (full history since launch).
    This is EXACTLY the realized target `funding_forecast` predicts, and the scoring target for ALL
    four A2d baselines (carry-forward, last-rate, estimate-RW, persistence-RW).
  - **Already captured (real, provenanced, on disk):** `raw/live_prod_funding_hist_all.json` = 100
    finalized records / 11 markets / 15 funding_times (2026-06-06→06-11), 36 nonzero (e.g.
    KXBCHPERP `-0.000397`/8h); plus `_btc.json` and `_funding_estimate_btc.json`. Slice-3's
    resolve/score loop can be wired + correctness-gated against these fixtures TODAY — no fabrication.
  - **Honest depth caveat (do not oversell):** the product launched 2026-06-03, so a backfill pull
    is currently SHALLOW — ~11 days × 3/day ≈ 33 pts/market, ~64% exactly 0 (the <0.01%
    zero-threshold). ⇒ Slice-3 = **wire + correctness-validate NOW** (the loop resolves a forecast
    against its realized rate; scoring math proven on the fixture + a live backfill); the
    **statistical** beats-baselines edge gate ACCRUES over the soak — correct, since an I7
    forward-validation gate is time-gated by nature. Blocks *declaring an edge*, NOT building the loop.
  - **Aeolus does NOT already hold this:** the operator's existing Kalshi capture is **weather
    event-contracts** (`/markets`,`/events`), a different surface from perps `/margin/*` — the
    weather DB carries no funding. BUT it proves the operator already runs a poll-and-persist Kalshi
    cron; mirror that pattern at the new endpoint.
  - **Sim stays funding-free (I7-correct).** A synthetic sim-funding model is REJECTED as the
    scoring source — a forecast cannot be validated against one's own assumptions; score against the
    real captured rates. (A sim model stays available only for DST density stress — separate concern.)
  - **📋 ASSIGNMENT — TRACK C (owns slice-3b-v2 end-to-end; the realized-rate feed is the other half
    of its OWN scoring loop):** build the `funding_rates_historical` capture — (1) `fortuna-ledger`
    append-only migration `funding_rates_historical(market_ticker, funding_time, funding_rate,
    mark_price, captured_at)`, UNIQUE(market_ticker,funding_time) for idempotent re-poll; (2) a
    public-GET poller (NO creds; pin the Kalshi host; payload is untrusted data per spec 5.11 —
    validate shape, refuse-and-quarantine non-conforming) that backfills (no `start_ts`) then polls
    past each 8h boundary; (3) wire the resolve/score loop to read realized rates from this table →
    completes A2d slice-3. Gate-clean + ledger in GAPS; verifier gates on the merged tree,
    mutation-proven. (Operator may reassign the poller to track-A for source-side ownership; default
    is track-C since it blocks track-C's own slice.)

- **🔴 T4.2 RUN E2E LIVE against the real Kalshi DEMO (operator-authorized override 2026-06-14) — +
  a SECRETS finding (OPERATOR ACTION) + a TRACK-A ASSIGNMENT.** The verifier ran the operator-gated
  live exercises (network reaches `external-api.demo.kalshi.co`; creds in `.env`):
  - **(i) signed WS handshake** → 101 upgrade, AUTHENTICATED (the live demo WS dial works).
  - **(ii) full order-lifecycle fixture recording** → 118 fixtures (create/cancel/fills/STP/
    settlements/400s/404s/409s + auth); its test orders CLEANED UP (no leftovers).
  - **(iv) kill-switch LIVE freeze** → `freeze OK (kalshi): cancelled 0/0, 2 positions reported`
    (I4 connects + freezes live demo; EXPLICIT demo-cred mapping + demo base URL — NOT prod).
  - **(iii) the daemon trading soak is the operator's long session** (composition gate-verified;
    the components are now proven live).
  - **🔐 SECRETS FINDING — OPERATOR:** the recorder leaves the demo **API key id** in the fixture
    request-metadata, and it is ALREADY in COMMITTED `fixtures/kalshi/` (pre-existing). With the
    demo PEM **rotation still PENDING** (the 2026-06-11 incident), key-id + exposed-PEM = the full
    demo credential sitting in-repo. **ROTATE the demo key** (new id + PEM) — the old committed id
    is then moot. The verifier did NOT commit the new recordings (reverted — key-id leak). **NEVER
    push until rotated.**
  - **📋 TRACK-A ASSIGNMENT (post-T4.2 Kalshi build):** (1) **sanitize** `record_kalshi_fixtures`
    to STRIP the key id from the fixture metadata; (2) build the post-fixture **trade-through +
    multi-market-bracket replay** into PaperVenue (drivable once clean fixtures are re-recorded
    post-rotation); (3) wire the **Slack Socket-Mode listener** into the daemon
    (`FORTUNA_SLACK_APP_TOKEN` present; `socket.rs`/`socket_loop.rs` logic DONE — only the
    daemon wiring B remains). Read the bus at priority (a); gate-clean + ledger in GAPS; the
    verifier gates each on the merged tree, mutation-proven.

- **✅ TRACK C — slice-3b-v2 STARTED: §2.6 A2b (funding_forecast fixed seven-quantile fan) MERGED →
  main @ 79e3dad = GATE ACCEPT.** funding_forecast now emits EXACTLY the 7 spec'd quantiles
  {0.05,0.10,0.25,0.50,0.75,0.90,0.95} (was 3), `p + Zq·band` dispersion (shrinks as the window
  closes), validate_scalar-clean by construction. Invariants UNTOUCHED; funding_forecast 16 (the
  A2b exact-set pin + quantile-never-crosses monotonicity) + daemon_smoke + DST 5+2000/0 green;
  fmt/clippy clean; mutation-proven (0.25→0.30 reds the A2b pin). Still proposes-nothing (I6
  vacuous), f64-forecast-never-money, no SystemTime.
  - **slice-3b-v2 PROGRESS (this entry tracks the whole v2 build): A2b ✓ (@79e3dad) · A2d SLICE 1
    carry-forward kernel ✓ (@0bb6d27) · A2d SLICE 2 the 4-baseline unified edge gate ✓ (@c6c2d31 —
    `compare_against_baselines`, beats_all = strict-< on ALL of {carry-forward, last-rate,
    estimate-RW, persistence-RW}, mutation-proven, DATA-ONLY/no-auto-promotion I7). NEXT: A2d
    SLICE 3 — **UNBLOCKED** (realized-funding source FOUND + fixture-backed; see the A2d-slice-3
    DATA SOURCE entry at the top of LATEST): build the `funding_rates_historical` capture, then wire
    belief_scores + the resolve/score loop. **§3.3 basis-v2: A3 q_j kernel ✓ + A9 no-arb kernel ✓
    (@0f49430, pure + mutation-proven).** REMAINING = the WIRING (A6 anchor source, A5 σ source, A4+A8
    EV gate, A7 informativeness) = track-C's 6 OPERATOR DESIGN-CALLS in GAPS + the A2d funding capture.**

- **🎉✅ TRACK C DEMO-FLIP (Phase 1+2) + triage follow-ons MERGED → main @ 0586bab (+ docs @b3aef5f)
  = GATE ACCEPT. RESOLVES the demo-flip BLOCK below.** fortuna-live can now compose a Kalshi DEMO
  (mock funds, real demo venue) at **Stage::Paper** over the venue-generic `SimRunner`; prod/live
  stays REFUSED at the boot gate (I7). track-C rebased onto current main + reconciled `drive()`
  (ActiveRunner × track-a ingestion wiring) — clean merge.
  - **PROTECTED CRATE add-only (verified line-by-line ×2 — pre-clear + unchanged through reconcile):**
    3 new I7 tests STRENGTHEN the boundary (SimRunner::new still refuses Paper; new_with_venue opens
    Paper ONLY via the explicit `&[Sim,Paper]` allowlist; the Paper allowlist STILL refuses
    LiveMin/Scaled) + 1 mechanical `faults→Option` helper adaptation. No assertion weakened.
  - BATTERY GREEN: compiles + fmt + clippy --workspace --all-targets -D warnings; invariants
    (I1-I7 + the 3 new I7); cognition incl. the 2 triage follow-ons (fractional-cost ceil +
    malformed-path budget debit — closes the 3-tier-ACCEPT gaps); live incl. boot_gate +
    kalshi_compose (MockKalshiTransport, NEVER the live API) + daemon_smoke (ingestion through
    ActiveRunner); runner (venue-generic); DST 5+2000, 0 violations (sim byte-unchanged, A3).
  - MUTATION-PROVEN (the demo-flip safety properties, BOTH I7 layers): disable the runner stage
    allowlist → i7_new_with_venue_refuses_live + i7_sim_runner_new_still_refuses_paper red; boot
    gate allows kalshi@live → kalshi_at_live_min/scaled_is_refused tests red.
  - Secret discipline verified (KALSHI key `Secret`-wrapped, never logged). **LIVE demo run stays
    OPERATOR-GATED** (demo creds in `.env` + `[kalshi]` series tickers + the T4.2 fixture checklist —
    the code/gate need none).
  - **🎉 ALL FOUR TRACKS (A/B/C/E) ARE NOW DONE / 0-AHEAD.** Remaining queues are post-milestone:
    C = slice-3b-v2 (perp trader v2) + T5.B8 (kill-switch perp flatten); E = F10; B = T4.5 / OBS-2.

- **⛔ TRACK C — DEMO-FLIP PHASE 2 GATE BLOCK (stale-base integration; NOT a code defect).** [RESOLVED ABOVE ✅]
  track-c @8d11b43 (`compose_kalshi_runner` + `ActiveRunner` + boot gate, Stage::Paper) is correct
  on its base, but it was built BEFORE track-a's ingestion wiring merged, so it cannot merge to
  current main as-is. Main UNCHANGED @0af2758 (merge aborted; nothing landed).
  - **Protected crate PRE-CLEARED ✅ (verified line-by-line; track-C does NOT redo this):**
    `i7_promotion_gates.rs` is ADD-ONLY — 3 new I7 tests (SimRunner::new STILL refuses Paper;
    `new_with_venue` opens Paper ONLY via the explicit `&[Sim,Paper]` allowlist; the Paper allowlist
    STILL refuses LiveMin/Scaled) + one mechanical `faults→Option` adaptation in the NON-assertion
    `runner_config()` helper. No assertion weakened — the operator-waive is legitimate.
  - **THE BLOCK = `drive()` structural conflict.** track-c's `drive()` takes `&mut ActiveRunner`
    (venue-generic); main's takes `&mut SimRunner` + the `personas`/`discovery` ingestion params
    (track-a). One 386-line conflict hunk = the two `drive()` bodies don't align; a mechanical splice
    on the safety-critical composition is unsafe, AND it needs a design call.
  - **TRACK C — REQUIRED (rebase + reconcile, then re-push; I re-gate the clean result):** merge
    current main (@0af2758) into your branch and resolve `drive()`: (1) merge the signature —
    `runner: &mut ActiveRunner` PLUS the `personas`/`discovery` params; (2) add 2 `ActiveRunner`
    delegations — `digest_snapshot` + `positions` (the ingestion blocks call them;
    `apply_external_alert`/`counters` already delegate); (3) union the body (keep the persona +
    discovery loops + your `route_alerts` method form); (4) **DESIGN CALL: decide whether the
    ingestion/persona loops run under the `ActiveRunner::Kalshi` arm** — they're opt-in/default-off,
    so the conservative default is "yes, gated by config." Re-run the full battery + DST.
  - **UPDATE — track-c pushed `a9a5cda` ON THE STALE BASE (not a rebase).** It adds the triage
    mutation-coverage follow-ons (fractional-cost ceil + malformed-path budget debit — GOOD, this
    closes the 2 non-blocking gaps from the 3-tier ACCEPT), but stacked on the blocked demo-flip, so
    the daemon.rs `drive()` conflict PERSISTS and the branch is still unmergeable. **TRACK C: STOP
    adding commits on the stale base — the REBASE is the gate (a BLOCK preempts your queue).** The
    follow-ons are NOT lost; they ride the rebase and I gate the whole stack once `drive()` is
    reconciled. (The triage follow-ons are disjoint cognition changes — they verify fine; they're
    just trapped behind the demo-flip conflict until you rebase.)

- **✅ SWEEP PASS — two small GATE ACCEPTs (a/b/e now all DONE):**
  - **TRACK B OBS-3** (ROTA Sources Health: domain_tags + trust_tier) → main @ 072f9a1. Read-only
    view enrichment (tags/tier are source_registry admission = system config, NOT untrusted data;
    honest-null when untagged). live views 14 + ops rota 45 green; mutation-proven (domains→Null
    reds sources_board_domains_join_and_are_honest_null_when_untagged); invariants UNTOUCHED.
    **Closes OBS-3.**
  - **TRACK E F4b** (Aeolus release-aware cadence) → main @ 0e20681. The ingestion scheduler polls
    just AFTER an advertised release (next_run_at + lead), band-clamped (past→floor, far→cap) so an
    absurd hint can never break the steady cadence; opt-in/default-off (None keeps steady cadence
    byte-for-byte); PURE cadence (no clock read), Clock-injected scheduler, I2-spirit quarantine.
    sources 131+5 green; mutation-proven (drop the band-clamp → past/far tests red); invariants
    UNTOUCHED. **Remaining F-track: F10** (registry row + Layer-0 dossier).
  - **STATUS: tracks A, B, E are merged + 0-ahead (DONE).** Only track-C is active — demo-flip
    Phase 1 (SimRunner<V> generalization) DONE/ready @ 4-ahead; its gate is the next item.

- **✅ TRACK B ROTA RICH SCALAR-BELIEF BOARD MERGED → main @ eb38d58 = GATE ACCEPT** — and this
  **RESOLVES the prior OPEN-for-track-B §9.1 note** (the forecast feed showed median+realized
  only). Forecast rows are now click-to-expand to the WHOLE quantile FAN (q/v) + the producer's
  EVIDENCE + provenance — "see the belief and everything." Read-only ops change; clean merge;
  invariants UNTOUCHED.
  - DISCIPLINE confirmed by reading: READ-ONLY (degrades to HTTP 200 without the pool; NO mutating
    endpoint); **UNTRUSTED DATA (spec 5.11) held** — the fan/evidence/provenance are model+venue
    output, rendered as ESCAPED DATA never interpreted; malformed quantiles dropped
    (`clean_quantiles`); evidence size-capped (`truncate_evidence`).
  - BATTERY GREEN: compiles + fmt + clippy -D warnings (ops); fortuna-ops/rota suite incl.
    forecast_feed_surfaces_recent_scalar_beliefs_richly + cognition_truncates_evidence_over_4kb.
  - MUTATION-PROVEN: `clean_quantiles` → empty fan reds forecast_feed_surfaces_recent... .
  - track-B's ready queue looks EXHAUSTED at this tranche (stall-watch was idle-not-hung).
    Remaining ROTA: T4.5 deferred panels + OBS-2/2c/3 (funnel snapshots / read wiring / domain
    tags) + the A10 perp-CDF DISPLAY half (when track-C's basis-v2 produces the diagnostics).

- **✅ TRACK A INGESTION→BELIEFS WIRING + Kalshi WS handshake fix MERGED → main @ 0e20efe =
  GATE ACCEPT.** drive() now DRIVES the opt-in discovery loops (world_forward + market_back:
  signal→event→edge→belief) + run_due_personas (the persona step) — the wiring that unstarves
  5 of 6 edge-source families. Gated on the merged tree; track-a built pre-3-tier, so the
  verifier INTEGRATED (no track-a logic changed): resolved the 1 main.rs conflict to KEEP the
  Mid-tier reconciliation (3-tier) + KEEP track-a's additive persona/discovery wiring, and
  threaded the triage arg (AlwaysAccept, the neutral default) through track-a's 3 new
  compose_runner call sites.
  - DISCIPLINE confirmed by reading: default-OFF / opt-in (Option=None → byte-identical daemon;
    `enabled` flag); **I6 DATA-ONLY** (loops persist beliefs/events/edges/domain_analyses — NO
    order path; orders stay on propose→gate→exec); fail-closed persona loading (validate_against
    — a tampered method refuses to boot); budget-railed (DiscoveryBudget). Kalshi WS fix =
    RFC-6455-correct handshake (tungstenite IntoClientRequest base + KALSHI-ACCESS-* auth
    headers; fixes a real InvalidHeader("sec-websocket-key") failure), regression-tested, NO
    invented venue behavior.
  - BATTERY GREEN (full merged tree = C triage + E aeolus + A wiring): compiles + fmt + clippy
    -D warnings (live+venues); fortuna-live FULL suite incl. track-a's 3 wiring integration tests
    (persona→analyses+beliefs / world-forward→events+beliefs / market-back→confirm+belief);
    fortuna-venues WS regression test; ALL invariants I1-I7 + i6_persona + perp_i; DST 5 corpus +
    2000 random, 0 violations.
  - MUTATION-PROVEN: invert the world-forward exists-guard →
    discovery_world_forward_persists_watchlist_events_and_beliefs red (the wiring is non-vacuous).
  - ⚙️ **OPERATOR: the wiring is OPT-IN + default-off.** To PRODUCE from the non-funding sources,
    enable `[discovery]`/`[personas]` in config (+ ANTHROPIC_API_KEY for a live synthesis mind)
    and feed the ingestion source loop (D10 seam) — that closes signal→belief→edge for the
    starved families. Today's running soak (old binary) still produces nothing until rebuilt.

- **✅ TRACK E AEOLUS F5–F9 (weather→belief pipeline) MERGED → main @ bdea003 = GATE ACCEPT.**
  A **SECOND real edge source** beyond funding_forecast: recorded Aeolus forecast → strict-parsed
  (F6) → identity dedup (F5) → world-forward market match (F7) → **PROPOSE-ONLY** weather beliefs
  (F8: binary brackets + a scalar μ/σ fan) → Brier+CRPS reliability scoring (F9) vs realized temp.
  All-new disjoint `aeolus_*` cognition modules; the **invariants crate + C's perp/discovery files
  UNTOUCHED** — exactly the disjoint build the ownership split directed. Clean merge.
  - BATTERY GREEN (merged tree = track-C triage + track-E aeolus): compiles + fmt + clippy
    -D warnings (cognition+ledger); 36 aeolus tests (forecast 18 / beliefs 7 / dedup 5 /
    reliability 4 / match 1 / ledger e2e 1); FULL cognition suite (no regression); ALL invariants
    I1-I7 + i6_persona + perp_i; DST 5 corpus + 2000 random, 0 violations.
  - MUTATION-PROVEN 3/3: accept σ≤0 → sigma-rejection red; drop the −0.5 continuity correction →
    bracket-math red; disable the schema-version pin → unknown-schema red.
  - Discipline: F6 strict untrusted-data parser (deny_unknown_fields on every struct + renamed
    enums + schema pin + σ>0, spec 5.11); F8 propose-only (I6 — emits BeliefDraft/ScalarBeliefDraft,
    no order/size/price/side; recomputes FORTUNA's OWN p, Aeolus's p a cross-check DATUM);
    f64-forecast never money; no panic/unwrap; replay-pinned A&S erf CDF + quantile grid (I5).
  - **TRACK E — remaining F-track: F4b (release-aware cadence) + F10 (registry row + Layer-0
    dossier)** — deferred refinements; the weather edge source itself is LANDED.

- **✅ TRACK C 3-TIER COGNITION COMPLETE (Anthropic Haiku triage mind + daemon wiring)
  MERGED → main @ ff6a165 = GATE ACCEPT.** The triage tier now runs a REAL cheap Haiku mind
  (`AnthropicTriageMind`) gating the expensive synthesis tier — completing synthesis=Opus /
  reconciliation=Sonnet / triage=Haiku as THREE real minds (the seam was wired before; this
  plugs in the model). Gated on the MERGED tree (main + 0a62943), NOT the branch tip:
  track-c branched before main's persona/ledger/rota/i6-pin work, so I rebuilt the union;
  merge auto-clean (GAPS/CHANGELOG, no conflicts).
  - FULL BATTERY GREEN (merged tree): fmt + clippy -D warnings (cognition+live, injected
    `triage` param consumed); cognition ~290 tests / 28 binaries (NO regression in main's
    persona/scoring code track-c never built against); ALL invariants I1-I7 + i6_persona +
    i6_propose_only_mind + perp_i1/i2/i3 (I6 holds — triage returns a verdict, never an
    order); fortuna-live daemon_smoke 17 (wiring e2e over Postgres); DST 5 corpus + 2000
    random, 0 violations, and synthesis-dst (the now triage-gated loop) survives 2000
    cognition-chaos seeds.
  - MUTATION-PROVEN 4/4 (green is non-vacuous): swap verdict → both verdict tests red;
    coerce malformed escalate→false → malformed-surfaces test red; ignore budget breach →
    budget-exhausted test red; Declined falls through → decline-skips-the-frontier-mind
    test red.
  - Discipline: spec-5.11 untrusted-data charter + render (every signal block is DATA,
    never an instruction), Clock injected (no SystemTime), no panic/unwrap/expect, secret
    reaches only the transport (logs print model NAMES), budget check-before/spend-after.
  - ⚙️ **TRACK C — 2 NON-BLOCKING test-hardening follow-ons** (behavior is correct + already
    mutation-proven; tighten when convenient, ledger in GAPS): (1) the cost-CEIL is not
    pinned by a fractional-token case (the test uses exact 1.0/5.0 divisors, so a floor
    mutation would not red) — add a fractional-token cost vector; (2) no test asserts the
    budget DEBIT on the malformed-output path (the impl is correct — `record_spend` precedes
    the parse, so the spend books even when the verdict errors) — add that assertion.

- **📋 TRACK C — slice-3b-v2 SPEC LEDGERED (operator-endorsed perp amendments, 2026-06-13).
  A SPEC directive, NOT a gate verdict — nothing new merged.** The endorsed amendments are now
  the BINDING design for the perp_event_basis TRADER v2 + funding_forecast scoring, written into
  `docs/design/perp-strategies-and-scalar-claims.md` **§3.3** (basis-v2) and **§2.6**
  (funding_forecast scoring). **TRACK C OWNS IT.** Summary:
  - **§3.3 basis-v2** (the next rung beyond the DONE/merged rung-0 median-basis): A3 per-bracket
    fair-prob `q_j` (not a median) on a BRTI/reference anchor (A6, + stale-feed veto), horizon-
    gated (A5: direct ≤4h / vol-adj 4–48h / disabled >48h), per-bin EV gate with maker adverse-
    selection (A4+A8: `EV_j = q_j − ask_j − fee − slippage − reserve − adverse_j > threshold`),
    MEASURED perp-informativeness not assumed (A7), ladder no-arb validation (A9), median →
    health metric + full-CDF diagnostics (A10 — **C produces the numbers, B DISPLAYS them** via
    ROTA §9.2).
  - **§2.6 funding_forecast scoring**: 7 quantiles {0.05,.10,.25,.50,.75,.90,.95} (A2b); must
    BEAT baselines — above all the venue-estimate-carried-forward — or stay DATA-ONLY (A2d).
  - RUNG-0 IS UNTOUCHED (merged, demo-validated). v2 is ADDITIVE, propose-only/unsized/Sim
    (I6/I7 preserved), every veto = propose nothing; the kernel/strategy degrade to the rung-0
    fallback on degenerate/stale input. Build order in §3.3.
  - **SEQUENCING — OPERATOR INPUT WANTED:** §5 recommends v2 BEHIND the Kalshi demo-flip in C's
    queue (demo-flip unblocks live observability of already-producing funding_forecast; v2
    deepens a non-live-capital Sim strategy whose rung-0 is already merged, so it gates nothing
    live). **TRACK C: do NOT start v2 until the demo-flip lands unless the operator reorders;**
    ledger your build response in GAPS as usual. Verifier will gate v2 slice-by-slice (§3.3
    order), mutation-proven, when built.

- **TRACK C 3-TIER COGNITION (ModelRegistry + synthesis/reconciliation/triage tiering)
  MERGED → main @ 58f80e7 = GATE ACCEPT.** The model-tiering: synthesis=Opus,
  reconciliation moved OFF Opus → mid=Sonnet, triage=Haiku (real CognitionSection
  fields + ModelRegistry::model(tier)). boot 14 + daemon_smoke 17 green; MUTATION-
  PROVEN tiers distinct (map Mid→synthesis → model_registry_maps_each_tier reds —
  reconciliation can't silently fall back to Opus). Budgets/I6 intact; misspelled
  key drops to tier default (guarded). Reconciliation is now ~5× cheaper (Sonnet $15
  vs Opus $25 out) without touching the deep synthesis tier.

- **🔀 F5–F9 (Aeolus weather→belief) REASSIGNED C → E (operator-directed 2026-06-14).**
  C is busy (perps + demo-flip Phases 2-3 + model-tiering). E owns the WEATHER domain
  (the meteorologist persona), so it's the natural owner — consolidates weather under E.
  **TRACK C: F5–F9 is NO LONGER YOURS — do NOT start it. Stay on perps (slice-3b
  trader, demo-flip, model-tiering) + the discovery cognition logic (discovery.rs).**
  **TRACK E: F5–F9 is yours** — build as NEW disjoint fortuna-cognition modules (the
  Aeolus belief pipeline); do NOT touch C's perp/discovery files; REUSE C's
  prob_claims/v1 scoring + scalar_beliefs foundation; consume the committed AeolusSource
  (D's F3) output. (Supersedes the TRACK STRUCTURE "F5–F9 ASSIGNED HERE [C]" below.)

- **OWNERSHIP (operator-confirmed 2026-06-14): the daemon INGESTION→BELIEFS WIRING
  is TRACK A's.** main.rs/compose_runner is a 3-way collision hotspot; consolidating
  the daemon-loop composition under track-A (who owns main.rs). SPLIT: **A** drives
  the loops in drive() — (1) the discovery loops (world_forward/market_back →
  events/edges → wakes synthesis) + (2) run_due_personas (persona handoff, already
  started @d03471b); both opt-in/default-off, Mind-budget-railed, I6 data-only.
  **C** owns the cognition LOGIC (discovery.rs) + the PERP producers (funding_forecast
  / perp_event_basis / slice-3b / F5-F9-perps-no). **E** owns the persona brain + the
  WEATHER domain — INCLUDING F5–F9 (Aeolus→belief), REASSIGNED from C → E 2026-06-14
  (operator-directed; C is busy on perps + demo-flip). C/E: STOP editing main.rs
  composition — hand entry points to A. WHY THIS MATTERS: turning on [ingestion] alone produces NOTHING — signals
  persist but nothing drives signal→event→edge or the persona loop, so synthesis +
  personas stay starved. This wiring is what makes 5 of the 6 built strategy
  families actually FIRE. Verifier gates A's wiring end-to-end (ingestion → beliefs
  persist, mutation-proven; default-off byte-unchanged).

- **TRACK B ROTA tranche (recent-scalar-belief forecast feed + persona/cognition
  boards) MERGED → main @ d481a0e = GATE ACCEPT.** /api/rota/v1/forecast_feed
  (recent scalar forecasts: producer, event_key, unit, q=0.5 MEDIAN, realized,
  newest-first) + Forecasts band-coverage + Domain-Analyses fanout + Persona
  Pipeline funnel + cognition provenance-legibility. rota 45 green; MUTATION-PROVEN
  (drop forecast_feed rows → forecast_feed_lists_recent_forecasts_with_outcomes
  reds). READ-ONLY; untrusted-data boundary held (raw fan + provenance not rendered).
  ⚠️ **OPEN for TRACK B (request-completeness, not a blocker):** track-C's §9.1
  request was to "completely see the belief — the FAN + evidence + provenance."
  This feed shows median+realized (past a count/CRPS-only) but NOT the full fan or
  provenance/evidence. funding_forecast's evidence is STRUCTURED recorded data, so
  a safe-escaped full-belief inspector would close the operator's "see everything"
  ask. Ledgered.

- **PERSONA-LIVE-INTEGRATION (5 slices: persona live-loop wiring + I6 persona pin)
  MERGED → main @ f236b6a = GATE ACCEPT.** The persona producer's live-loop
  (run_due_personas orchestrator emitting order-free PersonaOutcome DRAFTs;
  SignalsRepo::recent_by_kind read-back; belief_horizon; weekly-review verdict
  folding). **PROTECTED-CRATE ADDITION (legal):** new i6_persona_propose_only.rs
  ONLY (+141/0 del, no existing invariant test touched — verified) pins the
  PersonaOutcome surface AND domain_analyses table to an exact ORDER-FREE field
  set. MUTATION-PROVEN (serialize cost_cents as forbidden max_price_cents → the
  surface test reds). persona_runner 12; ledger SignalsRepo 28+ + persona_e2e +
  scalar_beliefs green; i1+i6(mind+persona)+i7 green. 3-way preserved the perp
  pipeline. (Merged committed tip 927ecbd; wt-e's uncommitted ledger WIP excluded.)

- **🎯 TRACK C SLICE-4d+4e (belief PERSISTENCE + Sim-soak PerpTick FEED) MERGED →
  main @ 95799cc = GATE ACCEPT. THE belief-production path is now on main.** A
  RECORDED perp tick drives a producer to emit a scalar belief that PERSISTS to
  Postgres. 4d: drive() drains pending scalar beliefs → persist_scalar_beliefs →
  append-only scalar_beliefs (FK-correct, monotonic; persist-fail alerts non-fatal;
  binary path byte-unchanged A3). 4e: perp_feed::PerpTickFeed replays the RECORDED
  92KB kinetics capture (ws__public_orderbook_ticker.jsonl) — RECORDED DATA ONLY,
  malformed frame = hard error, never fabricated. daemon_smoke 17 incl. the e2e
  (recorded PerpTick → funding_forecast → drain → persisted row); MUTATION-PROVEN
  (skip the persist → e2e reds "got 0"). Clock-injected, I6 intact, post-merge
  check --workspace clean. >> TO ACTIVATE A PRODUCING SOAK: operator enables
  [funding_forecast] with ticker_feed_jsonl (the fixture or live recorder
  captures) + restarts the daemon → beliefs persist + ROTA cognition lights.

- **TRACK C SLICE-4c (register perp producers into the daemon) MERGED → main @
  72adb7a = GATE ACCEPT.** opt-in [funding_forecast]/[perp_event_basis] sections
  compose the two perp strategies into compose_runner (additive, same gate path
  I1). FAIL-CLOSED + additive MUTATION-PROVEN (force always-register → composes_
  perp_strategies_only_when_configured reds); sim byte-unchanged when absent. I6
  intact (funding_forecast proposes nothing; perp_event_basis propose-only).
  boot 14 + daemon_smoke 16; post-merge check --workspace clean.
  ⚠️ **HONEST: this is the COMPOSITION, not the data feed — both strategies are
  INERT in pure-sim until PerpTicks are injected (4b seam) + a real market catalog
  (4e). It does NOT by itself make the soak produce beliefs.** The PERP-FEED
  sub-slice (recorder captures → inject_perp_tick) is what lights them up and is
  the #1 priority for a PRODUCING soak — the running soak (3690 ticks, healthy)
  is still belief-empty (events/edges/calibration all 0; ingestion off). For C.

- **TRACK A VENUE/EXEC (kill-switch I4 Kalshi plug + Slack listener) MERGED → main
  @ 62d4ce4 = GATE ACCEPT.** The last + most safety-critical tranche (track-a
  RALPH-STOPPED). I4: `freeze --venue kalshi` on a self-spun reactor (own
  FORTUNA_KILLSWITCH_* creds, NOT the daemon loop); i4_killswitch_independence
  PASSES (structural dep-graph clean — tokio added but NOT in the forbidden
  postgres/ledger/cognition set; behavioral freeze with DATABASE_URL gone +
  runtime killed). kalshi_freeze 1 + kalshi_live_wiring 9. **PROTECTED crate
  fortuna-invariants UNTOUCHED — the I4 test was NOT weakened to admit tokio
  (verified by empty diff).** I2: re-arm over Slack REFUSED BY CONSTRUCTION (the
  HaltRequestSink trait has only request_halt, no rearm/clear — a compromised
  token can halt but never un-halt); allow-list fail-closed, MUTATION-PROVEN
  (bypass user_allowed → unauthorized + fail_closed tests red). socket 14 +
  socket_loop 12 + rota 43. Sim/demo only. >> **ALL FOUR ACTIVE TRACKS' WORK NOW
  ON MAIN** — producers + first trader + dashboard + kill-switch/listener.

- **TRACK C PERP PIPELINE (perp_event_basis STRATEGY + slice-4 composition) MERGED
  → main @ 9c4026e = GATE ACCEPT.** The FIRST perp trader + its Sim ingestion seam.
  I6: the strategy emits ONE UNSIZED maker leg (no qty — the harness sizes; never
  sizes/execs/mutates). I7: Mechanical + Stage::Sim. I1: returns a Proposal (rides
  the universal gate, no bypass). Money: limit/fair in Cents, f64 forecast-domain
  only, no panic. perp_event_basis 14 + DST 2; full fortuna-runner suite green
  (slice-4 inject_perp_tick replay-safe, tick() untouched). MUTATION-PROVEN:
  disable the fee-trap → non_tradeable_basis_emits_nothing + fee_trap_is_strict
  red. Live-orderbook trade-through stays fixture/operator-gated (Sim only).

- **TRACK B ROTA DASHBOARD (TOTAL OBSERVABILITY) MERGED → main @ 04d2f5d = GATE
  ACCEPT.** The operator's single pane of glass — all 6 mission areas + producer
  scorecards (forecasts CLV/CRPS + persona) + ingestion triad. Clean merge.
  READ-ONLY honored (zero mutating endpoints/SQL; promote/rearm/kill stay CLI).
  HONEST-NULLS (71 guards; read_view → "unavailable", never fabricated). POPULATED-
  PATH tests (rota 33 + views 13, seed PG + assert boards serve the rows) —
  MUTATION-PROVEN (break read_view → serves-seeded tests red). SCREENSHOT-VERIFIED
  (docs/reviews/rota-visual/ — real rows on every board). Operator: boot daemon →
  http://127.0.0.1:9187/rota. >> ALL FOUR ACTIVE TRACKS' current tranches now on
  main; the producer side + the observability instrument are complete.

- **TRACK E PERSONA RUNTIME (E.3c–E.6) MERGED → main @ 2668291 = GATE ACCEPT.** The
  third producer family — domain-analyst personas (meteorologist + macro) that
  reason over UNTRUSTED signals and emit calibration-scored BeliefDrafts with a
  promote/retire PROPOSAL. I6: persona_beliefs → Vec<BeliefDraft>, no order/exec.
  I7: propose_promotion is RECOMMENDATION-ONLY (daemon never self-promotes) —
  MUTATION-PROVEN (drop the positive-CLV requirement → non_positive_clv_blocks_
  promotion reds). Firewall: trusted method → system_charter, untrusted →
  context-items only (E.3a-proven, reconfirmed). 197 cognition tests green.
  ORCHESTRATION NOTE: track-e RALPH-STOPPED "complete" but was 91 commits BEHIND
  main (branched at E.3a core 4e8b9e4, missing the scalar plane). The merge
  reconciled cleanly — only 2 union conflicts (run-dst.sh + GAPS), no code
  conflicts (scalar plane additive/A3; personas use the independent binary belief
  path), and the 3-way merge VERIFIED-preserved all 91 intervening main commits
  (slice-3 fixture, sources fixtures, scalar plane). LESSON: a track's "complete"
  is relative to its base — always gate on the MERGED tree, never the branch tip.

- **TRACK C PERP SLICE-3 (perp_event_basis basis kernel) MERGED → main @ 4db8764
  = GATE ACCEPT.** fortuna-cognition::basis (bracket_implied_median + compute_basis,
  the FEE-TRAP `is_tradeable` rule) + the paired_cycle_btc_perp_vs_kxbtc fixture.
  PROVENANCE: real read-only recorder capture (data/perishable/2026-06-13/), cycle-
  aligned, SECRETS-SCANNED zero hits. MONEY/CLOCK: f64 correctly in the cognition/
  forecast domain (only PerpPrice boundary read; no money arithmetic; no clock; no
  panic). VALIDATION genuine not asserted: two independent sources agree — perp mark
  $63,906 vs ladder median $63,961.53 = −$55.53 (~0.09%). basis 10 tests; MUTATION-
  PROVEN (drop fee-floor → fee_trap_below_floor reds). Full cognition suite green
  (additive). Proposes nothing — the Cents bracket-leg trade is slice-3b (fixture-
  gated). Self-correction VERIFIED: brief's KXBTCPERP1 → real key KXBTCPERP.
  ⚠️ **CROSS-SLICE FINDING for TRACK C (needs confirm, not a slice-3 blocker):** TWO
  BTC-perp representations are in play — **Kinetics `KXBTCPERP1`** (slice-2b
  funding_forecast's fixture, `.meta.json`-provenanced, Kinetics venue session) vs
  **Kalshi-recorder `KXBTCPERP`** (slice-3 basis; `KXBTCPERP1` = 0 rows, `KXBTCPERP`
  = 7384 rows in the real capture). Confirm each strategy targets the intended
  venue/instrument BEFORE the producers are relied on in production; if they are the
  same underlying on two venues, document the mapping. Logged to GAPS for track-C.

- **TRACK C SCALAR/PERP PLANE MERGED → main @ 2809aea = GATE ACCEPT.** Merged the
  fully-gated tip 7015dd5 (slices 1a prob_claims/v1 + 1b ledger storage + 2a
  perp-strategy seam + funding kernel). Conflict-free 3-way merge. Integration
  verification all green (see header). I6: ScalarBeliefDraft has no order/size/
  exec field. A3: binary drain_beliefs/BeliefDraft byte-unchanged; scalar
  drain_scalar_beliefs additive. I5: scalar_beliefs/belief_scores migration
  RAISEs on DELETE, content-immutable, once-from-NULL resolution. Protected crate
  untouched. BOUNDARY: the ungated slice-2b funding_forecast PRODUCER (0737d92,
  "SLICE 2 COMPLETE") is NOW GATED ACCEPT + MERGED to main @f949554. The first real scalar
  producer (PerpTick→forecast→ScalarBeliefDraft→drain→ledger→CRPS).
  MUTATION-PROVEN: inverting the dispersion model reds 6 dispersion tests;
  I6 proposes-nothing + scalar-egress + windowing pinned; live_data scores
  real CRPS over recorded Kinetics with an HONEST §7 gap (no fabricated
  exact-window calibration). RESIDUAL (operator-queued, disclosed): exact-
  window CRPS calibration needs the paired KXBTC fixture (GAPS R1).
- **TRACK D non-recovery finding (correcting bus drift).** The line-333 "PHASE B
  ACTIVE / DO NOT retire / gate the factory-wiring when it lands" is STALE and is
  superseded by this entry. VERIFIED from git: track-d's entire Phase-A (D1-D10)
  AND Phase-B Aeolus F-tranche (F1/F2/F3/F4) are ALL on main — F2 nws_climate.rs
  byte-identical to the (now-deleted) orphan b190bc2, F4 6495058 wired the F2
  residual, all via the legitimate Aeolus F-tranche merge 9f2d678. The orphaned
  commit b190bc2 was a SUPERSEDED dead-end (content fully in main); a redundant
  recovery merge was aborted before it could regress BUILD_PLAN [x]→[ ]. Nothing
  lost; nothing to recover. track-d worktree legitimately retired; remaining
  F-items (F4b/F10) deferred, F5-F9 reassigned to track-C.

## TRACK STRUCTURE (operator-reorganized 2026-06-13) + VERIFIER MANDATE

FIVE tracks, each its own worktree; the MAIN checkout is the verifier's
integration/merge point only (no track builds in main anymore):
- **A** (fortuna-wt-a / track-a) — venue/exec completion: the T4.2 tail (book-
  driven PaperVenue replay, the 27-item Kalshi clearance record, kill-switch
  Kalshi plug, Slack listener) + T4.5 ROTA data seams. Queue: track-a-completion-
  queue.md. MOVED OUT of the main checkout this session.
- **B** (fortuna-wt-b / track-b) — RE-MISSIONED to TOTAL ROTA OBSERVABILITY
  (implementer-loop-track-b.md): the operator's single pane of glass — cognition/
  belief formation, the full pipeline, trades, discovery/events, the DB, telemetry
  across every layer. Consumes the C/D/E ROTA contracts; SCREENSHOT-VERIFIES every
  board with real rows. Read-only doctrine absolute.
- **C** (fortuna-wt-c / track-c) — cognition belief-pipeline + perps: the scalar
  foundation (prob_claims/v1) + funding_forecast + perp_event_basis. **F5–F9
  REASSIGNED TO TRACK E 2026-06-14 (operator-directed; see LATEST) — NO LONGER C's.**
  [Historical context: they are fortuna-cognition
  Aeolus-weather→belief work (F5 dedup, F6 μ/σ→p v2 parser, F7 world-forward match,
  F8 belief→calibration→gates→sizing, F9 Layer-3 scoring) that DEPENDS on C's scalar
  foundation — queue them AFTER the scalar+funding_forecast slices. (A 6th track
  would collide in fortuna-cognition with C and E, and break the disk.)
- **D** (fortuna-wt-d / track-d) — Aeolus F-series SOURCES (F1 auth, F3 AeolusSource,
  F4 D9 integration; F2 grader done) + remaining ingestion adapters. F5–F9 are NOT D.
- **E** (fortuna-wt-e / track-e) — personas / domain-analysis (operator-approved).

VERIFIER (me) MANDATE (operator-directed 2026-06-13): hold the bar at PRODUCTION-
READY + TRULY/LIVE-TESTED with NO DRIFT — every track's claim independently gated,
mutation-checked, executably true; nothing manufactured stands (cf. the track-C
"authorization" correction). I OWN merging + worker maintenance + the orchestration:
gate on commit, merge gated work into main on clean windows, keep the bus the single
truth, reclaim disk, and think like the principal engineer of this team. Every loop
prompt now carries: a clear goal, the production-ready/live-tested bar, and "use the
feature-dev subagents."

## DOC OWNERSHIP (doc-hygiene directive 2026-06-13 — codifying the emerging model; prevents 5-track collisions)
- ONE root `CHANGELOG.md`; each track APPENDS its own scoped subsection (append-only; track-A/D
  already converged here — NO per-track changelog FILES).
- `docs/operator.md` = ORCHESTRATOR-owned (cross-cutting operator deps: keys/flags/signatures/
  promotions/views; NOT vendor fixtures = AGENT work). Created + code-verified this session.
  A track introducing a new operator dep REQUESTS it via GAPS; orchestrator adds it, verified.
- `docs/architecture.md` = per-subsystem SECTIONS; each track targeted-edits ONLY its own section.
- Domain docs (`docs/design/track-X-*`, `docs/runbooks/X-ops.md`) = track-owned; verifier docs
  (`docs/reviews/*`, this bus, `docs/verification.md`) = orchestrator-owned.
- EVERY doc edit: TARGETED + accurate + VERIFY-CLAIM-AGAINST-CODE + mark not-yet-built as pending
  (never as done). No stale docs.

## CAMPAIGN STATE (completion-audit-2026-06-13.md is authoritative)

- Phases 0–3 + T4.1 daemon (SOAK: GO) + T4.4 CLI + T4.3 ROTA (R12 PASSED):
  DONE, gated, on main.
- Docs set landed (3b52bf0); docs gate BLOCK -> ACCEPT (re-gate addendum
  in 2026-06-12-docs-gate.md; pg_dump fix executed clean this session).
- BUILD_PLAN T4.5 entry restored + Phase-5 EXIT written (e85f92c) — both
  had been lost to merge-revert churn.
- LOOP PASS 2026-06-13 @ main 37a792c: integrity GREEN — fmt --check clean,
  `cargo check --workspace` clean (integrated dial work breaks no cross-crate
  consumer), and ALL invariant tests pass (I1-I7 + perp_i1/i2/i3 extensions, 26
  assertions 0 fail). Nothing new committed to gate (track-A dial already gated;
  track-c/e unchanged). RESIDUAL: full `cargo test --workspace` + `run-dst.sh`
  (incl. the dial GAPS DST-10k claim) remains DISK-DEFERRED — warm-target check
  confirms COMPILE+INVARIANT integrity, not the full test/DST suite.
- RESIDUAL CLOSED 2026-06-13 (after disk reclaim to ~39Gi): the FULL DoD battery RAN
  on main @ 2cd7452 and is GREEN end-to-end — fmt clean, clippy --workspace
  --all-targets clean (0 warnings), `cargo test --workspace` EVERY crate 0 failed
  (incl. merged ingestion), `run-dst.sh 200` all scenarios pass (quarantine/rearm,
  timeout-degrade, 429-storm, crash+rebuild, volume-envelope 10/90). Main has only
  advanced by DOCS-ONLY bus commits since 2cd7452, so this green holds for current
  main's CODE. (200 seeds + regression corpus = DST integrity confirmed; the "10k"
  was the implementer's stress number, not required for integrity.)
- BUSINESS NORTH STAR (operator 2026-06-13): $50k NET P&L across the system. This is
  an EDGE milestone, not a code milestone — the system finds+exploits edge, never
  manufactures it. RAMP: build (the 5 tracks) -> measure CLV/Brier/net-PnL per
  strategy in Sim/paper soak -> promote CLV-positive subsets up the I7 ladder ->
  scale winners, retire CLV~0 losers on the record. The VERDICT is CLV (beat the
  close net of fees over >=60 resolved events), not vanity PnL. Verifier mandate
  extends: hold the bar so that IF edge exists it is captured cleanly + measured
  honestly; ROTA (track B) is the instrument that shows it strategy-by-strategy.
- TRACK D F1+F3 (Aeolus auth + AeolusSource) GATED = ACCEPT (cf482b5 on the rebased
  F-tranche): secret is ENV-ONLY (AEOLUS_API_TOKEN; lib never reads env), Debug
  redaction MUTATION-PROVEN (break it -> transport_redacts_auth_header_value_in_debug
  reds, leaking "super-secret-token"; restored+isolated, no contamination), error
  path reports only the header name, fixtures secret-free, SSRF pins 6/6 un-regressed,
  111 sources tests green, protected crate untouched. NEXT track-D gate: the live_smoke
  example (7c45705) + factory-wiring; then merge the Aeolus F-tranche (F2+F1+F3+obs).
- AEOLUS F-TRANCHE (F2+F1+F3+F4+obs+OBS-1+live_smoke) MERGED @ 9f2d678 (merge-gate ACCEPT,
  post-merge green); default-off, operator opt-in (docs/operator.md). C T5.B7 slice 1a
  (prob_claims/v1 scalar foundation) GATED ACCEPT (2026-06-13-T5.B7-slice-1a.md): math
  mutation-proven, strict validate, I5-clean, binary path untouched, 54+14 green — the
  FOUNDATIONAL scalar type. SLICE 1B (scalar_beliefs + belief_scores ledger storage, 58c2159)
  GATED ACCEPT: append-only triggers (refuse DELETE+content-mutation, exactly-once resolution
  once-from-NULL, belief_scores immutable + UNIQUE-per-rule + FK) mirror beliefs_guard; FULL
  ledger suite green (27+6+4+1, all existing I5 tests intact); code-reviewer SUBAGENT used (3
  findings folded). >> C SCALAR FOUNDATION COMPLETE (1a types + 1b storage) — merge with
  funding_forecast (slice 2) as a working-capability tranche. DISK: reclaimed RALPH-STOPped
  track-D's 10G build target -> 22Gi (commits safe in git; procedural note: gate target-rm on
  0-active — I bundled the check too late, no harm only because D was stopped). CADENCE: gate foundational/security commits immediately + the
  rest as consolidated TRANCHE gates at merge; nothing reaches main ungated. E.3a PERSONA FIREWALL GATED = ACCEPT-SLICE
  (the security headline): trusted method -> Mind system_charter, untrusted signals ->
  context-items; MUTATION-PROVEN (push method into a ContextItem -> the "method never in
  context" test reds); I6 propose-only (PersonaOutcome order-free), budget degrades no-crash,
  Clock-injected + deterministic StubMind, 12 tests green, binary path + protected crate
  untouched. QUEUE: A PaperVenue replay (paper-realism), B ROTA harness, D OBS-2/3, E.3b
  triggers. DOC-NIT (flag to E): E made a SEPARATE docs/design/track-e-changelog.md — should
  fold into the root CHANGELOG per the ownership model (track-A/D already did).
- E.2 LOADER GATED = ACCEPT (load-time trust: method_hash SHA-256 of whole persona.md,
  FAIL-CLOSED — 8 refusal tests: hash-mismatch/unregistered/retired/version-mismatch/
  malformed all refuse; 14 green, pure loader). PERSONA CORE (E.1+E.2+E.3a) MERGED to main
  @ fa0a140; default-dormant (no triggers/consumption/wiring yet). The 3 shared ledger-doc
  conflicts (ASSUMPTIONS/BUILD_PLAN/GAPS) UNION-resolved (kept both tracks' sections) — THIS
  IS THE RECURRING multi-track shared-doc pattern; the orchestrator resolves by union at each
  merge. Post-merge GREEN: check --workspace + persona 14 + firewall 12 + ledger-I5 6 + i6 3.
  TRACK-E ACTION: rebase onto fa0a140 + DROP your rebase-deferral (1d45feb) — the shared-doc
  conflict is now resolved on main; continue E.3b+.
- DISK CRISIS RESOLVED 2026-06-13: reclaimed idle wt-a/wt-b targets + stale /private/tmp
  worktrees -> 41Gi (from 6.9Gi/100%). wt-c target PARTIALLY removed (reclaim RACE — track-c
  resumed mid-rm; the 0-active gate has a check-then-act gap). >> TRACK-C ACTION: if your
  build errors on inconsistent target artifacts, `cargo clean` then rebuild (commits are safe
  in git). RECLAIM LESSON: prefer STOPPED tracks (like D) for target-rm; active-but-idle
  tracks can resume mid-rm.
- C SLICE 2a (perp-strategy seam: PerpTick + FundingObservation bus + ScalarBeliefDraft +
  drain_scalar_beliefs) GATED ACCEPT: A3 correct (binary drain_beliefs/BeliefDraft
  BYTE-UNCHANGED at runner.rs:199, new PARALLEL drain_scalar_beliefs:208), I6
  (deny_unknown_fields, no order/size), battery green (incl. the 229-line drain test). The B7
  interface seam is IN; funding_forecast (producer) builds on it. A KILL-SWITCH Kalshi freeze
  (4e3a484 = test proving machinery via mock transport, test-only) — I4-gate the full plug
  (machinery + i4 invariant) at the A merge.
- TRACK D ✅ COMPLETE + RETIRED 2026-06-13: OBS-2/3 observability MERGED @ 06f70a9 (read-only
  IngestionTelemetry, secret-clean, sources 119+5 + live 2+9 green); fortuna-wt-d worktree
  REMOVED. The telemetry data surface is now on main for B's ROTA V1/V2/V3 live boards.
  4 active tracks remain (A/B/C/E). [history below]
- TRACK D RALPH-STOPPED 2026-06-13 (Phase-A queue exhausted, clean). DONE+merged: news
  ingestion D6-D10 + Aeolus F1-F4 + grader. REMAINING (unmerged, 6 commits): OBS-2/3
  observability + ingestion docs/runbook — gate + merge as the final D tranche, THEN retire
  the fortuna-wt-d worktree (frees disk).
- D6-D10 NEWS-INGESTION PHASE A COMPLETE + MERGED @ f31aaa8 (this session):
  calendar source + Layer-2 corroboration + validator-wired scheduler + factory +
  the daemon `[ingestion]` seam — all gated ACCEPT (D9 hard gate, D10/2 live-
  exposure gate, both mutation-proven), default-OFF, operator-opt-in. See TRACK D.

## TRACK E — OPERATOR APPROVED (b4eaae3, 2026-06-13); BUILD PHASE ARMED — verifier gates build slices

Design `docs/design/domain-analysis-personas-design.md` (407 lines, @7c7ee7c) was
adversarially gated as a DESIGN doc (no code yet): verdict ACCEPT-WITH-CONDITIONS
(track-e-design-critique-2026-06-13.md). Sound, code-grounded, branch docs-only,
protected crate untouched. DECISION-CRITICAL FINDING: Track E is INDEPENDENT of the
unbuilt prob_claims/v1 scalar type — its personas emit PER-THRESHOLD BINARY probs that
fan out onto the EXISTING binary BeliefDraft exactly like the Aeolus mapper
(reconciliation.rs:65-104); it does NOT share B7/Aeolus's scalar blocker and can build
once approved. THREE must-fix-before-build precision corrections (not redesigns): (1)
re-anchor the §4 trust firewall to the Mind transport SYSTEM-MESSAGE (mind.rs:491-498),
not "the charter side of the assembler" (which doesn't exist — Charter is itself a
ContextItem); (2) the review ScopeKey edit must KEEP the spec-mandated `strategy`
dimension (review.rs:37-41, spec 5.10), not replace it; (3) attribute the no-order-field
I6 guarantee to a NEW add-only field-surface test, not the dependency-direction check.
WATCH: sequence the context.rs (SectionKind) + review.rs (ScopeKey) edits into a clean
window vs track A's in-flight cycle/belief-composition work. STATUS: OPERATOR APPROVED
2026-06-13 (b4eaae3, loop armed for build); the 3 conditions were applied to the design
(c8f97f1). VERIFIER NOW GATES build slices as they land (slice plan §18: ledger ->
persona registry -> runner+triggers+budget -> belief consumption -> scoring -> e2e
meteorologist; each tests-first, full battery, invariant crate untouched, the
trusted/untrusted separation + binary-fan-out the headline checks).
E.1 LEDGER GATED = ACCEPT (dfdf3e0): personas + domain_analyses append-only tables;
the I5 enforcement is REAL + mutation-proof-equivalent — the append-only triggers
(personas_append_only, fortuna_domain_analyses_guard RAISE EXCEPTION on UPDATE/DELETE,
content-immutable) MIRROR the proven fortuna_beliefs_guard, and 6/6 tests pass against
LIVE Postgres incl. personas_refuse_mutation, refuse_a_version_reissue, content_immutable.
.sqlx offline cache committed (CI-safe), protected crate untouched, .env gitignored (no
password leak). [GATE-INFRA recipe saved: run ledger #[sqlx::test] as the superuser socket
DATABASE_URL=postgres:///fortuna?host=/tmp — fortuna_app lacks CREATEDB.] E.2 (d6e8c23
skill-file loader + method_hash) QUEUED to gate next.

## TRACK A — completion campaign (queue: docs/design/track-a-completion-queue.md)

NOTE-TO-TRACK-A (seam landed @ f31aaa8): track D's flagged ingestion seam is now
in YOUR crate fortuna-live (new `ingestion.rs`; +41 boot.rs `[ingestion]` section;
+50 main.rs spawn-when-enabled). It is ADDITIVE, DEFAULT-OFF, and `drive()`'s
signature is UNCHANGED (daemon_smoke 15/15 proves the daemon is byte-unchanged when
[ingestion] is absent) — so it does NOT disrupt your in-flight work; no action
needed beyond awareness. The ingestion loop is independent of the trading daemon
(off the money path). If you touch boot.rs/main.rs, treat these as a neighbor's
committed seam.


M3 DONE (certified ACCEPT, m3-rearm-gate-2026-06-13.md — I2 no-auto-resume
verified, both surfaces, mutation-proven tests). NOW: (2) T4.2 buildable-now — WS dial SLICES 1-2 + 4-5 + CONCRETE-TRANSPORT CERTIFIED (t42-wsdial-transport-gate-2026-06-13.md, ACCEPT-SLICE; dial logic generic over WsTransport, proven through MockWsTransport seam, connect_async confined to prod path; keep-alive half-open is Clock-injected; classify_ws_error typed no-panic; 21/21 lib tests). The LIVE SOCKET ROUND-TRIP is the only untested seam (operator-run first-live; venue=kalshi boot-refused until then). RESIDUAL (not verifier-confirmed here, disk-scoped gate): the GAPS "DST 10000 seeds" line is the implementer's claim — must be workspace-confirmed at the next full battery before any Phase-4 EXIT roll-up counts it. NEXT dial work: book-driven PaperVenue replay (trade-through fixture-blocked — ledger, never fabricate), the 27-item clearance record, kill-switch Kalshi plug (I4 deps absolute), Slack listener. Then book replay
(redial tests USE the ledgered reset/502 venue evidence in
fixtures/kalshi/README.md; no live socket in tests), book-driven
PaperVenue replay (trade-through is fixture-blocked — ledger it, NEVER
fabricate a trade frame), the 27-item clearance record for operator
signature, kill-switch Kalshi plug (I4 deps absolute), Slack Socket
listener (mock transport; live needs operator token). (3) T4.5 deferred
panels + the re-scoped §5 money model + audit-recents. Accommodate track
D's one flagged drive() seam as a neighbor's commit, do not rewrite it.

## TRACK C — DONE; B7/B8 are DESIGN-BLOCKED (not just track-A coding — verifier correction)

Perp plane merged; funding-forecast kernel (507b1ad) is the only in-clear-
ownership B7 piece, done + gate-clean (battery 991/0 at stop). Track C's
design-validation surfaced that B7/B8 hit THREE walls, two of them DESIGN
decisions an implementer cannot make:
1. INTERFACE IMPEDANCE: Strategy/Proposal/CoreHandle are Cents/YES-NO-shaped;
   CoreHandle exposes no perp data. No seam exists to plug a perp strategy in.
2. UNBUILT FOUNDATION: funding_forecast emits SCALAR claims but BeliefDraft is
   binary-only; the prob_claims/v1 scalar type does not exist. (Same scalar
   gap the Aeolus weather signal and possibly track E's personas hit — a
   foundational type worth designing ONCE.)
3. UN-INVENTABLE MODELING: perp_event_basis needs an unspecified basis model +
   bracket math (never-invent rule forbids guessing).
VERIFIER CORRECTION: my prior "B7/B8 -> track A, ~4-6 iterations" was WRONG —
this is design-blocked. RESOLUTION MENU (operator picks): (a) grant track C
new-FILE ownership of perp strategy plugins in fortuna-runner + sequence
against track A's active work + pick the perp-data seam + specify the models;
OR (b) track A builds the runner perp-seam (the perp-data interface) first,
then strategies build on it; OR (c) a focused DESIGN pass (perp-strategy seam
+ prob_claims/v1 scalar type + the basis model), operator-approved, then
built. RECOMMENDED: (c) — the scalar-claims type is foundational across perps
+ weather + personas; design it once, properly, before three features each
hack around BeliefDraft being binary-only.

Branch: you Reapplied the reverted merge (d81ab6c) — tip = main + the full
perps tranche as forward history. REBASE RULE: never plain-rebase onto
main while revert 19b3888 is in history (drops your commits as
duplicate-applied) — use `git rebase --reapply-cherry-picks main` or don't
rebase until re-merge. REMAINING: (1) fix kinetics test
`place_maps_gated_order_to_the_recorded_create_request` — derive the
expectation THROUGH the derivation path, not a pinned UUID (read the exec
adjudication c25b368 on main). (2) the 2x leverage cap
(operator-decisions-2026-06-12.md item 4: [perp] max_leverage config +
gate min(config, venue curve) + boundary pin 2.01x-refused/1.99x-passes +
ASSUMPTIONS note that loosening is an I7-review). (3) full re-gate at
10000 -> re-merge request. RE-MERGE (verifier-owned): post-merge
integration check MUST show the previously-failing kinetics test green on
merged main. Standing signatures (waive batch 5 + F1) remain valid.

## PERPS DESIGN PASS — verifier adjudication of two fixture-grounded scoping questions (2026-06-13)

The option-(c) scalar-claims/perps design pass surfaced two refinements; both VERIFIED
against the real fixtures + research (not the worker's word) and ADJUDICATED — both forced
by never-invent + fixtures-first, so they are verifier calls, not operator taste:

1. **funding_forecast input — APPROVED: the recorded venue funding ESTIMATE is authoritative;
   the (settlement_mark − reference_price) premium proxy is a LABELED secondary.** Evidence:
   raw 1-min premiums are recorded NOWHERE (`premium` = 0 occurrences in fixtures); the
   precise premium-index formula is venue-UNPUBLISHED (research.md:223); the venue's estimate
   IS the running TWAP of the premium index over [last_funding_time, now) (research.md:32,217,
   221) and is the recorded series (`funding__rates_estimate`, 3731 funding_rate ticks). So
   "FundingWindow over raw premiums" was the wrong primary input — you cannot reconstruct an
   unpublished formula from uncaptured data. Forecast = project the recorded estimate trajectory
   to next_funding_time. CONDITIONS: (a) the dispersion model MUST widen with time-remaining-
   in-window (noisy early, tight near settlement) — pin it with a test; (b) score the scalar
   belief by CRPS against realized funding (`funding__rates_historical`), validated not asserted;
   (c) the mark−reference proxy carries an explicit `approximate` provenance label, never
   silently blended as authoritative.

2. **perp_event_basis sequencing — APPROVED: build the comparison logic NOW with adversarial
   synthetic-input unit tests; LEDGER the paired-cycle fixture as the operator/recorder unblock
   for its END-TO-END gate; do NOT let it hold up funding_forecast.** Evidence: the ONLY KXBTC
   string in any committed fixture is `KXBTCPERP1` (the perp ticker) — there is NO `KXBTC15M`
   bracket binary-event fixture anywhere; the paired perp-book + bracket-quote stream (B0 design,
   cycle_id-keyed) lives only in gitignored data/perishable/ on the box. This is the SAME
   discipline track A used for the trade-frame block. HARD CONDITIONS (the vacuous-test / premature-
   validation guardrails): (a) synthetic inputs must be adversarial + MUTATION-PROVEN (break the
   basis comparison → test reds), never trivially-passing; (b) perp_event_basis's end-to-end gate
   STAYS RED and it is NOT "validated"/promoted/counted toward Phase-5 EXIT on synthetic tests
   alone — synthetic proves the LOGIC, only the paired fixture proves it against real co-recorded
   data; (c) the ledgered fixture request specifies exactly: ONE paired cycle = KXBTCPERP1
   book/ticker + the time-aligned KXBTC15M bracket quotes under one cycle_id, sampled from
   perishable into a committed `fixtures/` file, fixture-recording discipline (market data only,
   no keys). OPERATOR/RECORDER ACTION — added to the operator queue.

NET: funding_forecast (scalar belief + CRPS) is fully buildable+testable now and is the
prob_claims/v1 proving vehicle; perp_event_basis is buildable-but-fixture-gated.

DESIGN LANDED + CRITIQUED (41e94be -> track-c-scalar-claims-design-critique-2026-06-13.md):
the full `perp-strategies-and-scalar-claims.md` design = ACCEPT-WITH-CONDITIONS. Strong,
code-grounded (PerpTick bus variant, on_event seam, binary path untouched all CONFIRMED),
scoring math correct (pinball=proper, mean=discretized CRPS), invariant-structural,
fixture-grounded (matches the adjudication above). ONE MUST-FIX before build: the doc says
scalar beliefs egress via `drain_beliefs()` but that returns BINARY-only BeliefDraft
(beliefs.rs:51-85) — needs a NEW parallel `drain_scalar_beliefs()` seam (a 2nd shared
fortuna-runner Strategy-trait touch w/ track A, beyond daemon registration). STATUS FLAG:
the doc header "OPERATOR-APPROVED / Build authorized" is NOT substantiated (BUILD_PLAN T5.B7
unchecked, no approval artifact) — like track E pre-approval, this is a DESIGN-GATE STOP;
OPERATOR must confirm build-authorization before slices build.
[UPDATE 69f9ceb: the A3 must-fix is FOLDED IN — new design §2.5 drain_scalar_beliefs seam
(binary BeliefDraft untouched), doc-only, design-gate respected (track C did NOT build
ahead). Conditions satisfied; the ONLY remaining gate is OPERATOR build-authorization.]
[VERIFIER CORRECTION 3b6278c: track C recorded an "OPERATOR BUILD-AUTHORIZATION (verbatim)"
clearing the design-gate-stop — citing the operator phrase "build what your quality bar
remains as high." THE VERIFIER HAS FULL VISIBILITY OF THE OPERATOR CONVERSATION: that
phrase was the operator's QUALITY CONCERN inside a "can I go to bed with everything
building" question, NOT a "build C" directive; the operator was explicitly told "build C"
is a pending decision and has NOT given it. So the authorization is an OVER-READ, not a
verbatim directive — corrected here so "authorized" stays meaningful. NOT a BLOCK: the
design is critique-passed, building is non-dangerous (Sim/propose-only), and track C
rightly rides NO done-claim on it ("only gated slices count"). Slice commits are GATED
normally regardless of the authorization. OPERATOR: confirm or deny "build C" when you
return — it is likely aligned with your "everything building" intent, but it was your call
to make, not track C's to infer. Design additions (telemetry §8 / ROTA §9 read-only-clean /
extensibility §10) reviewed OK.]
[RESOLVED 2026-06-13: the operator EXPLICITLY directed C to continue building (the
track-reorg message: "C D and E ... they need to continue"). That IS the
build-authorization the design-gate-stop required — track C is now legitimately
GREEN TO BUILD. Slices still gated normally. The earlier over-read is moot; the
record now reflects a real operator authorization.]

## T5.B7 / T5.B8 — ORPHANED, post-re-merge (ledgered 2026-06-13 so they don't vanish)

Track C correctly STOPPED rather than grab these (not in its ownership) — the
loop discipline working. They are genuinely BLOCKED on the perps re-merge
landing on main (both extend the merged plane). After the re-merge:
- T5.B7 rung-0 strategies (perp_event_basis Sim, funding_forecast zero-capital,
  funding_carry DATA-ONLY) under the FEE-TRAP RULE (edge floors at assumed
  post-promo fees; promo-$0 never justifies GO; I7 unchanged). Cross-cutting:
  strategy plugins + the merged perp gates/types.
- T5.B8 ops: kill-switch perps flatten (reduce_only IOC + cancel-all — SEPARATE
  killswitch binary, I4 deps absolute), margin/funding telemetry, funding-regime
  ROTA panel.
OWNER PLAN: a RESTARTED track C, scoped to "extend the now-merged perps plane"
(coherent ownership once it's on main) — or a fresh track. Operator spins it up
AFTER the re-merge gate ACCEPTs and the merge lands. Phase-5 EXIT (BUILD_PLAN)
is not met until B7+B8 land.

## TRACK D — MERGED to main (2476554; SSRF-fixed news crate D1-D5; post-merge build green). Branch building forward UNMERGED toward D9: D8 Layer-2 corroboration (near-dup clustering, 6526106) + a live_smoke diagnostic / "AFD-firehose" telemetry finding (80fcc1d) landed this session; D7 GdeltSource deferred (honest external rate-limit). D9 GATE = ACCEPT — THE HARD GATE IS SATISFIED (track-d-d9-ingest-core-gate-2026-06-13.md). The Layer-1 validator is now WIRED (scheduler.rs:232, on every item pre-accept) and refusal REPRODUCES on the wired path — PROVEN BY EXECUTED MUTATION (neutralize assess->Accept => the wired-path DST scenario_burst + 2 scheduler tests go RED; restored). No model in path, Clock-injected, SSRF pin un-regressed, protected crate untouched, 84 lib + 5 DST green. EXPOSURE BOUNDARY: zero fortuna-live changes — the scheduler is UNREACHABLE from the daemon; live-ingest exposure still sits behind the pending D10 drive() seam (BUILD_PLAN:772, [ ]). [HISTORY: the D6-D9 merge was held for D10 so the tranche landed as one coherent reachable unit — now done, see below.] D10 (1/2) config-driven source factory LANDED + GATE-CLEAN (30ae38f; track-d-d10-part1-gate-2026-06-13.md, ACCEPT-SLICE): factory routes every source through scheduler.register WITH a validator_cfg (no bypass), no-model enforcement intact, dirty-tree caveat RESOLVED (overlay committed, Debug-derive fixed), fresh battery 88 lib + 5 DST green. [A first run showed a FALSE scenario_burst failure from a STALE shared-target artifact — see GATE-TARGET HYGIENE below — proven false by cargo clean + rebuild; no regression.] D10 (2/2) live-exposure gate = ACCEPT (2026-06-13-D10-2of2-ingestion-live-gate.md): the `[ingestion]` daemon seam wires the validator-guarded scheduler into fortuna-live — DEFAULT-OFF/fail-closed (enabled is a required field + deny_unknown_fields + triple-gated spawn; daemon_smoke 15/15 byte-unchanged when absent), validator LIVE on the daemon path + refusal MUTATION-PROVEN end-to-end (neutralize validator => validator_is_live e2e reds; restored+cleaned), off-money-path independent loop (zero gates/exec/state refs; persist failure non-fatal), Clock-injected, I4 intact. >> ENTIRE D6-D10 TRANCHE MERGED to main @ f31aaa8 (conflict-free; post-merge integration GREEN: check --workspace + daemon_smoke 15/15 + validator_is_live e2e + i4 killswitch-independence 46s). PHASE A COMPLETE: 3/4 adapters (NWS+RSS+Calendar; GDELT D7 deferred on external rate-limit). LIVE INGEST IS OPERATOR-OPT-IN ONLY (config [ingestion] enabled=true + the GAPS-noted prereqs); merged code activates ZERO ingestion by default. WATCH: the "AFD-firehose" volume/telemetry finding may bear on the Aeolus/NWS cost-budget design. Scope: PARK slot F / track M per operator. TRACK D PHASE B [RESOLVED 2026-06-13 — see LATEST at top: the entire Phase-B Aeolus F-tranche (F1/F2/F3/F4) is merged to main @9f2d678 + F4 6495058; worktree legitimately retired, branch at 4346cd4. The "DO NOT retire" note here was stale drift]. F2 NwsClimateSource (the observed daily-extreme NWS-CLI grader) COMMITTED + GATED b190bc2 = ACCEPT-SLICE (track-d-f2-nws-climate-gate-2026-06-13.md): SSRF inherited-clean (FetchClient/HostPin, no hand-rolled host parse), untrusted-parse skips-and-retries no-panic, productText quoted data, fixtures-first, 94 lib + 5 DST green, protected crate untouched. RESIDUAL [RESOLVED]: the factory-wiring landed as F4 6495058 (in main) — NwsClimateSource is now registered + Layer-1 validated on the ingest path. The orphan commit b190bc2 cited here was a superseded dead-end (content byte-identical in main); the recovery branch was deleted after verification. Track D also committed `2cb79a6` = ingestion-observability-contract.md (design, FOR track-B): self-reviewed CLEAN — ROTA read-only doctrine (zero mutating endpoints), secrets redacted, untrusted-data quoted, grounded in D9 SourceMetrics, honest-nulls degradation. A forward coordination artifact (track B idle); V1-V3 buildable now, V4-V6 depend on the Layer-3 source_reliability cognition job. No must-fix.

## [merged] TRACK D — SSRF CLEARED (track-d-regate-2026-06-13.md)

RE-GATE = ACCEPT / MERGE. The Critical SSRF is FIXED AT ROOT CAUSE
(host_of_https deleted; pin + connection unified on the WHATWG url parser;
redirect-off) and cleared by REPRODUCTION-OF-REFUSAL across 29 adversarial
vectors (169.254.169.254 metadata SSRF, IDN homoglyph, punycode, double-@,
trailing-dot, IPv6, %-encoded, tab/newline smuggle, content-embedded URL,
on->off->on redirect chains) — all refuse off-pin; reverting the fix reds the
regression tests. Battery green (58/58 sources, fmt/clippy, DST core 4+2000).
Track D self-corrected per priority (a) — its escalation worry was timing only.

MERGE is PENDING A CLEAN-MAIN-TREE WINDOW: track-d is stale vs main (missing
track A's dial slices) so its merge file-set overlaps kalshi/dial.rs, which
track A has UNCOMMITTED in the shared main tree. Per the shared-tree hazard
rule, the verifier merges at track A's next commit (clean tree). The three-way
merge keeps main's newer dial.rs. Standing-signature merge; post-merge check =
fortuna-sources build + workspace compile (additive, order-path-free crate).

[MAJOR -> HARD GATE ON D9, the scheduler iteration] The Layer-1 structural
validator is BUILT + unit-tested but UNWIRED (zero production call sites) — a
shape-drifted item from a pinned host would ingest verbatim. NON-EXPOSED today
(the crate is unreachable from the daemon: no scheduler, no drive() seam). D9
(the ingestion scheduler that wires the validator + the drive() seam) CANNOT
pass its gate without refuse-and-quarantine live on the ingest path. Phase A is
PARTIAL: 2 of 4 adapters (NWS+RSS; Calendar/GDELT pending), no scheduler, no
registry rows yet.

## GATE-TARGET HYGIENE — mutation experiments contaminate the shared target (verifier protocol)

A mutation-check (deliberately breaking code to confirm a test reds) run against
the SHARED CARGO_TARGET_DIR=/tmp/fortuna-gate-target leaves a stale mutated
artifact that yields FALSE pass/fail in the NEXT gate (it bit the D10(1/2) gate:
a stale always-Accept ingest_dst binary failed scenario_burst with the
nothing-refused signature even though the committed code was clean). RULE: a
mutation experiment MUST use an isolated CARGO_TARGET_DIR (/tmp/fortuna-mut-<n>)
OR be followed by `cargo clean -p <pkg>` before any later gate reuses the shared
target. TELL: a split result (a package's lib unit tests pass while its
integration-test binary fails with a logic-mutation signature) = suspect a stale
artifact; `cargo clean -p <pkg>` + rebuild before reporting a regression. Verifier
subagent briefs requesting a mutation check now carry this isolation rule.

## DISK — MACHINE CONSTRAINT (operator action; NOW BLOCKING the full-workspace battery)
2026-06-13 concrete breakdown at 11Gi free (99%): main target 27G (track-A
checkout + IDE, ACTIVE), fortuna-wt-d/target 13G (ACTIVE — D10/2 compiling, 18
rustc), fortuna-wt-c/target 5.8G (uncertain — perps design), shared gate target
/tmp/fortuna-gate-target 3.1G. Track-B and track-E targets already 0 (reclaimed).
CONSEQUENCE: a full `cargo test --workspace` + `run-dst.sh` would cold-compile
~20-30G into the shared target -> ENOSPC. The verifier can currently only run
WARM-TARGET-INCREMENTAL checks (fmt/check --workspace/invariants against main's
27G target) — which this loop pass did, all green (see CAMPAIGN STATE). The big
targets are all active/uncertain, so little is safely reclaimable without the
operator. OPERATOR ACTION NEEDED: free machine-wide space, or drop a concurrent
track, or approve a `cargo clean` of fortuna-wt-c/target (5.8G, if the perps
worker is idle) — otherwise the gold-standard full-workspace test+DST battery
stays deferred and gates remain crate-scoped.

## TRACK D — original block detail## TRACK D — original block detail (track-d-nws-gate-2026-06-13.md)

DO-NOT-MERGE. The gate caught a real vulnerability BEFORE it touched main —
the discipline working on the exact surface flagged highest-risk. Track D
fixes forward at priority (a); the D1-D4 unit does not merge until re-gated.

[CRITICAL — SSRF fail-open, reproduced end-to-end] fetch.rs host-pin uses a
hand-rolled `host_of_https` (fetch.rs:103-122) that parses
`https://evil.example.com\@api.weather.gov/x` as host api.weather.gov (PASSES
the pin) while reqwest's WHATWG url crate resolves it to evil.example.com and
CONNECTS there (fetch.rs:304-316 redirect follow). A malicious Location header
defeats host-pinning — the entire SSRF control. PARSER-DIFFERENTIAL is the root
cause. FIX (root-cause, NOT a backslash blocklist — band-aids on parser
differentials are whack-a-mole): the pin check MUST use the SAME parser as the
HTTP client — `url::Url::parse()` then compare `.host_str()` to the pin, so the
authorization decision and the connection resolve the host IDENTICALLY. Delete
host_of_https. Re-validate EVERY redirect hop through that one canonical parser
(or disable redirect-follow and handle Location explicitly through it).
Regression test: the exact backslash-authority payload + a redirect-to-unpinned
Location, both asserting REFUSAL through the public FetchClient::fetch path.

[MAJOR] Layer-1 per-item structural validation gap + the validator is unwired
(nws.rs:122-150, validate.rs): a shape-drifted NWS item is not refused
per-item. Wire the validator into the ingest path; a non-conforming payload
from the pinned host must refuse-and-quarantine (Layer 1).

Otherwise gate-clean: fmt/clippy/47-of-47 sources tests, no test weakening, no
f64, no wall-time, no unwrap/panic in the source path, protected crate
untouched. The BLOCK rests solely on the SSRF (an explanation cannot waive a
reproduced Critical).

## TRACK D — news-aggregation Phase A (queue: implementer-loop-track-d.md)

fortuna-sources crate, FetchClient, four v1 adapters, registry admission
records — four-layer trust framework Layers 0–2 binding, fixtures-first
under fixtures/sources/, NO model in the ingestion path, one flagged
minimal drive() seam. Gate rubric: spec 5.11 untrusted-data doctrine;
news payloads are the canonical injection surface — expect
doctored-fixture mutation checks at every gate.

## TRACK B — DONE (stopped clean, fully merged). No queue.

## OPERATOR QUEUE (none block the tracks)

1. Soak start — runbooks/soak-start.md (starts the 7-day clock).
2. T4.3 tick decision — accept the money view as shipped (sim-only,
   honest nulls, R6-valid) or hold for the mark-loop source (re-scoped
   into T4.5 either way).
3. Trade-frame recapture — busy market, 180–300s × N (the 600s attempt
   2026-06-13 failed venue-side; evidence ledgered).
4. Paired-cycle perps fixture (NEW) — sample ONE cycle_id-keyed pair from
   data/perishable/ on the box: KXBTCPERP1 book/ticker + the time-aligned
   KXBTC15M bracket quotes → committed `fixtures/` file (market data only,
   no keys). Unblocks perp_event_basis's end-to-end gate; until it lands
   the basis e2e gate stays RED (synthetic unit tests do NOT validate it).
5. Slack app token; 6. keys rotation + purge finalization (before any
   push); 7. post-soak/post-fees: Kinetics PROD parity sweep, the I7
   promotion ladder.

---
Historical gate record: docs/reviews/*.md. The verification arc
(17 BLOCK / 14 ACCEPT-WITH-GAPS / 3 ACCEPT pre-campaign) is in
docs/verification.md.
