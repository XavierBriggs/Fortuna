> **track-A SELF-REVIEW — not a verifier verdict.** The authoritative gate verdict is the
> verifier's, recorded on the bus (`docs/reviews/GATE-FINDINGS-LATEST.md`): slices 1-3 were
> INDEPENDENTLY gated ACCEPT @5b93f8e / @533ce17 / @72170c6 (full battery + DST + mutation-proofs
> on the merged tree). This file is track-A's thorough pre-submission self-analysis, retained for
> its value; its conclusion is independently confirmed. Tracks must not author "Verdict" lines in
> `docs/reviews/` — that surface belongs to the verifier.

# Review: F7 live weather plug-in (track-A) — 2026-06-14
Base: bf3d57b~1 (3ed2ba1)  Head: b7cf6ee  Self-assessed verdict: ACCEPT (verifier-confirmed; see bus)
Protected crate touched: no (git diff --name-only | grep invariants => NO INVARIANTS CHANGES)

Scope: 3 commits — bf3d57b (WeatherMarketSource + KalshiWeatherSource), d1ebc45
(drive() weather step + build_kalshi_demo_transport + wiring), b7cf6ee (station_series
map slice 3). Rubric fixed from spec §3 (I1/I6), §5.2/§5.3 (gate type-enforcement),
§5.5/§5.9 (propose-only beliefs), §5.11 (untrusted data), §5.12 (event/edge model,
mapping types, confirmed-edge usage), §6 aeolus_eval, §10 security, CLAUDE.md house
rules. Closest task contract: the "F7 venue half" (3ed2ba1) + operator "every other
station" / "finish everything" (GAPS-ledgered 2026-06-14). BUILD_PLAN F7 line 1137 is
the COGNITION world-forward match (track-E, already merged @bdea003); this track-A work
is the live-daemon wiring half, distinct.

## Criteria (fixed before reading the diff)
- C1 I6 propose-only (§5.9/§6) — PASS. The drive() F7 step (daemon.rs ~1761-2000) only
  reads signals, parses, maps station→series, GETs the day-set, builds buckets, and calls
  persist_beliefs + insert_edge. No place()/sizing/timing/routing. persist_beliefs
  (daemon.rs) is EXISTS-check + events.create INSERT + beliefs INSERT only. aeolus_eval
  "no orders placed" upheld. Evidence: code read; grep place( shows F7 step never calls it.
- C2 I1 universal gate (§5.2:123, §5.3) — PASS. GatedOrder is sealed: only constructor is
  GatedOrder::assemble, pub(crate) in fortuna-gates/src/order.rs:37; fields private; no
  Deserialize; no From/Into/public ctor outside gates (git grep verified). The auto-
  confirmed Direct edge is a market_event_edges row (DATA); synthesis_edges
  (compose.rs:319) loads it as EdgeTier::Confirmed and refresh_synthesis_edges feeds the
  arm, which still emits candidate orders through the gate pipeline. Edge makes a belief
  tradeable; it places nothing. Evidence: order.rs read; synthesis_edges read; place( sweep.
- C3 no panic in money/lib paths (CLAUDE.md) — PASS. weather.rs, aeolus_venue.rs, main.rs,
  and the daemon F7 step (lines 1761-2001): zero unwrap/expect/panic/todo/indexing (grep).
  event_grades_on parses defensively (let-else, KALSHI_MONTHS[month-1] guarded by
  1..=12); market_to_bucket uses ?/checked_add/checked_sub; the drive() step matches
  every Result/Option, alert-and-continue on parse/day_set/dedup/persist failure. Untrusted
  payload parse failure routes apply_external_alert + continue (§5.11). Evidence: grep + read.
- C4 secrets (§10/CLAUDE.md) — PASS. build_kalshi_demo_transport reads the PEM once, wraps
  in Secret, expose() reaches only KalshiSigner::new; returns Arc<dyn KalshiTransport>.
  main.rs builds ONE transport and Arc-clones it for runner + KalshiWeatherSource (no
  second read). No secret literals introduced (grep); nothing prints PEM/signer/signature.
  The discovery example's grading-station probe is GET-only and prints only public
  rules_primary/ticker. Evidence: diff read; secret-literal + print-signer sweeps clean.
- C5 no fabricated venue artifacts (CLAUDE.md) — PASS. market_to_bucket keys
  market_key = m.ticker (recorded). aeolus_bucket_edges recovers MarketId from
  aeolus:{ticker}. event_grades_on returns bool (a match key), never a constructed
  ticker — doc + tests assert this. Happy-path test asserts every edge market_id LIKE
  'KXHIGHNY-26JUN15-%' (recorded). day_set is read-only GET over recorded fixture.
  Evidence: aeolus_venue.rs + weather.rs read; daemon_smoke + kalshi_weather_source tests.
- C6 grounding honesty (§5.12; CLAUDE.md fixtures rule) — PASS (with ledgered residual).
  KNYC→KXHIGHNY is fixture-proven ("Central Park, New York" in markets__high_temp.json);
  Aeolus knyc_tmax/tmin carry station=KNYC, nws_station_id=NYC => same physical station.
  The other 6 mappings are grounded by a READ-ONLY demo probe transcribed into
  docs/research/sources/kalshi-temperature-stations.md (an allowed grounding source) and
  are DORMANT (Aeolus emits only KNYC today). City-named/ambiguous series correctly left
  None. The one live tmin mapping (KXLOWTNYC, rule says only "New York City") rests on a
  reasoned NYC-single-CLI-station inference; correct, and the residual ("non-KNYC entries
  assume Aeolus's coding scheme"; un-fixtured rules) is explicitly ledgered in GAPS.md.
  Safety claim ("fires only on the exact code => same physical station; else None =>
  never mis-resolve") holds. Evidence: research doc + GAPS + fixtures + station_series read.
- C7 fortuna-invariants/ untouched — PASS. git diff --name-only shows no invariants files.
- C8 tests prove the claims (DoD) — PASS. 3 #[sqlx::test] e2e: happy 6 beliefs/6 events/6
  Direct auto-confirmed kalshi edges + idempotent re-drive (still 6, dedup via DB
  current_edges_for_market pre-persist, not PK collision); mutation drop -T85 => exactly
  5/5 and 0 references to -T85 (genuinely reds on regression); settled June-13 (all
  Determined) => 0/0 (proves active-only filter in the plug-in). Sibling tests wired
  weather_source: None (standing mutation). Venue test proves date-matching + ONE
  read-only GET scoped to series over the recorded book. No test weakening anywhere
  (no removed asserts, no #[ignore], no proptest reductions, no loosened tolerances).
  Evidence: tests read; executed green this session (see Commands).
- C9 edge mapping validity (§5.12) — PASS. mapping=Direct, single-venue 1:1 (not the
  cross-venue/multi-leg class §5.12 reserves for human confirmation). confidence=1.0,
  proposed_by="aeolus_bucket_match", confirmed_by="discovery:auto" (distinguishable from
  human/market-back). Edge FK event_id REFERENCES events(event_id) satisfied by
  persist_beliefs creating the aeolus: event before insert_edge. recent_by_kind orders
  deterministically (received_at DESC, signal_id DESC) — no HashMap nondeterminism.

## Findings
- [Minor] BELIEF-REFRESH-PER-RUN gap (daemon.rs F7 step; ledgered GAPS.md). The per-market
  edge-dedup also gates the belief, so a later Aeolus run (new run_at/updated μ,σ) for an
  already-edged market does NOT persist a refreshed belief — and a belief persisted while
  its edge-insert failed becomes an orphan that a later drive double-persists. Fails CLOSED
  (no order, no confirmed edge; beliefs are append-only calibration substrate, not money
  path). Already ledgered as a follow-on. No action required to accept.
- [Minor] Weather beliefs attribute to the shared `world-forward` discovery strategy id
  (clean F9/I7 per-domain scoring isolation deferred); the EDGE already carries its own
  proposed_by. Ledgered. No action required to accept.
- [Minor] 6 of 7 station mappings + the day single-digit padding are grounded by read-only
  probe / 2-digit-day fixture only (no committed fixture for the non-KNYC rules or a
  single-digit day). Inert today (Aeolus emits only KNYC; recorded days are 2-digit).
  Both ledgered in GAPS.md with conservative "can only MISS, never mis-trade" rationale.

## Commands run (verbatim results)
- git diff --name-only bf3d57b~1..b7cf6ee | grep -i invariants => NO INVARIANTS CHANGES
- cargo fmt --check => exit 0
- cargo clippy -p fortuna-venues -p fortuna-live --all-targets -- -D warnings => exit 0 (Finished)
- cargo test -p fortuna-venues --test kalshi_weather_source => 6 passed; 0 failed
- cargo test -p fortuna-live --test aeolus_station_series => 6 passed; 0 failed
- cargo test -p fortuna-live --test daemon_smoke (full) => 23 passed; 0 failed
- scripts/run-dst.sh 200 => DST_EXIT=0; ingest_dst 5 passed; 0 failed; zero violations
- git grep GatedOrder (gates) => sole ctor assemble() pub(crate); no external ctor
- panic/secret/clock/f64/test-weakening sweeps over full diff => clean
