# Track A — completion-campaign queue (operator-directed 2026-06-13)

Authoritative for track A's (b)-priority. Re-read every iteration; the
verifier amends as gates land. All standard loop rules apply unchanged.

1. M3 REARM NOTICES (pool-released to track A; small, one iteration):
   - fortuna rearm output gains: "halt cleared in the ledger; the RUNNING
     daemon resumes only on restart — run: fortuna stop && fortuna start"
     (crates/fortuna-cli/src/main.rs rearm arm).
   - A ROTA/health surface for the same fact (simplest honest form: the
     health view exposes halt-state-vs-running-daemon divergence, or a
     last_halt_event summary). Design intent: runbooks/halt-and-rearm.md's
     four-state table must be readable off the console.

2. T4.2 BUILDABLE-NOW (in order):
   (i) Kalshi WS dial: signed handshake (recipe proven in fixtures), keep-
       alive, redial-with-resubscribe + seq-gap resync. The redial tests
       USE THE LEDGERED VENUE EVIDENCE (fixtures/kalshi/README.md, the
       2026-06-13 entry): mid-stream reset-without-close and 502-on-
       reconnect are the recorded behaviors the dial must survive. No live
       socket in any test (mock transport; the dial's first live exercise
       is operator-run).
   (ii) Venue-generic recorded-stream replay into PaperVenue: the book/
       delta WS fixtures EXIST — build the replay composition for both
       mech strategies. The TRADE-THROUGH portion is fixture-blocked (the
       public trade frame was never captured; retry pending venue
       recovery) — build the book-driven replay now, assert what book
       evidence supports, and LEDGER the trade-frame dependency precisely
       (no fabricated trade fixtures, ever).
       >> DONE e6dd7ec (2026-06-13): recorded_replay.rs (7 tests) — gapless
          fully-typed parse of both recorded fixtures, EXACT assembled book in
          PaperVenue, book-only replay = no fills, both mech strategies composed
          (abstain on the recorded book + liveness controls). Trade-through and
          a multi-market-bracket fixture ledgered in GAPS (never fabricated).
          Full battery green.
   (iii) Adapter paper-clearance VALIDATION: work the 27-item checklist
       (docs/research/venue/kalshi-api-2026-06-10/research.md) against
       fixtures/kalshi/ — each item PASS/FAIL/UNCOVERABLE with the
       fixture file cited; the 7 known gaps stay ledgered. Output: a
       clearance record the operator signs; venue=kalshi stays refused
       until that signature.
       >> CLUSTER 1 DONE f7206a4 (2026-06-13): kalshi_recorded.rs (18 tests) —
          FIRST recorded-fixture adapter tests; PASS items 1,7,8,9,10,13,14,16,
          17,18,20,21. Clearance record docs/design/track-a-kalshi-paper-
          clearance.md (UNSIGNED). Exposed 2 adapter gaps — G1 nested-error
          extraction RESOLVED (b2087fc), G2 exchange-status DTO pending. Clusters 2
          (transport round-trips) + 3 (auth-skew/WS handshake) PENDING.
   (iv) Kill-switch KalshiVenue plug: FORTUNA_KILLSWITCH_* credential
       pair, freeze --venue kalshi wiring + tests (mock transport); I4
       dependency rules absolute (no new killswitch deps); live exercise
       operator-run after clearance.
   (v) Slack Socket Mode listener: build to the research contract
       (docs/research/ops/) with mock transport tests — kill REQUESTS
       only, re-arms stay CLI-only; live exercise needs the operator's
       app token (ledger the gate).

3. T4.5 ROTA deferred panels (after T4.2: the WS gap/resync counters flip
   live as part of it; the rest per the BUILD_PLAN T4.5 entry and its
   TEST RULE — populated-path seeds, no vacuous tests).

4. Deferred backlog: the main.rs composition-root guard (rides with the
   next main.rs change per the ledgered rationale).

COORDINATION: track D will land ONE flagged minimal drive() seam commit in
fortuna-live (your files) — accommodate it on rebase, review it as a
neighbor's seam, do not rewrite it; conflicts beyond the seam = ledger +
bus, never a silent merge resolution.

## ADDED 2026-06-13: T5.B7/B8 reassigned to track A (ownership correction)

Track C correctly STOPPED — B7/B8 live in TRACK A's crates, not track C's:
T5.B7 perp strategy plugins need the Strategy trait/Proposal/CoreHandle in
crates/fortuna-runner (track A); T5.B8 kill-switch flatten is in
crates/fortuna-killswitch (track A); B8 telemetry + the funding-regime panel
are in crates/fortuna-ops. Track C delivered the in-territory INPUT: the
deterministic funding-forecast kernel (track-c @ 507b1ad: FundingWindow +
finalize_funding_rate, in fortuna-state). After T4.2 + T4.5, track A:
- T5.B7: wire perp_event_basis (Sim) + funding_forecast (consuming track C's
  507b1ad kernel — cherry-pick or merge it) + funding_carry (DATA-ONLY) as
  Strategy plugins; FEE-TRAP RULE (edge floors at assumed post-promo
  5-12bps; promo-$0 never justifies GO); I7 unchanged.
- T5.B8: kill-switch perps flatten (reduce_only IOC + cancel-all; extend the
  SEPARATE killswitch binary, I4 deps absolute); margin/funding telemetry;
  the funding-regime ROTA panel. Closes the Phase-5 EXIT.
