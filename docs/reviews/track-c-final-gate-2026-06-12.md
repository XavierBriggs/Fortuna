# Review: track-c FINAL gate (T5.B6 + T5.B4 slices 1-4 + gate-fix batch + RALPH STOP) — 2026-06-12
Base: 8966a00 (merge-base main..track-c)  Head: 87b55e0  Verdict: BLOCK
Protected crate touched: yes — EXACTLY the waived batch 5 (628 insertions / 0 deletions,
`git diff 8966a00...87b55e0 -- crates/fortuna-invariants/` = 4 files, zero deletion lines);
ZERO new invariant-crate touches in the post-prior-gate delta
(`git diff --stat dc03294..87b55e0 -- crates/fortuna-invariants/` = empty).

Review environment: detached worktree /tmp/fortuna-gc @ 87b55e0, CARGO_TARGET_DIR=/tmp/fortuna-gate-target.
NOTE (environmental, not a finding against the diff): the FIRST clippy and test runs failed with
phantom `E0432 unresolved import fortuna_core::perp` / `E0282` from cross-checkout stale cache in
the shared target dir; `cargo clean -p fortuna-core -p fortuna-gates -p fortuna-venues` resolved it
and all subsequent runs were clean. Operators sharing one target dir across checkouts should expect this.

## Criteria (fixed before reading the diff; rubric = GATE-FINDINGS-LATEST fix list, spec 5.15,
## BUILD_PLAN T5.B4/B6, kinetics-perps-module-plan §B4/B6, fixtures/kinetics-perps SESSION-NOTES,
## research §8 + §13, fortuna-review checklist)

- A1 At-boundary pins + M1/M3 mutations die (gate-fix F2): PASS —
  `liquidation_distance_exactly_at_floor_passes` / `leverage_exactly_at_cap_passes`
  (crates/fortuna-gates/tests/perp_gates.rs:442,474). EXECUTED mutations:
  M1 perp.rs:699 `<`→`<=` => boundary test FAILED (RED, as required);
  M3 perp.rs:725 `>`→`>=` => boundary test FAILED (RED). Worktree restored (0 dirty files).
- A2 Funding ±2% cap (gate-fix F3): PASS — MarginSim::apply_funding REFUSES |rate| > 0.02
  (margin_sim.rs, FUNDING_RATE_CAP; error never silent clamp — correct fail direction for an
  append-only record). Test `funding_rate_beyond_venue_cap_is_error_not_clamp` green in the
  939/0 workspace run; at-cap ±0.02 accepted, beyond-cap errors, balance untouched.
- A3 perp.rs message cleanup (gate-fix F5): PASS — `'{}x{}'` → `'{n}.{d}x'` (commit 4d4d26c,
  perp.rs:727-731); fmt + clippy clean.
- A4 BINDING F1 item: PASS — RiskCurve::from_leverage_estimates reads the RECORDED
  leverage_estimates; shape test `risk_curve_from_recorded_leverage_estimates_shape`
  (margin_sim.rs:637) reads fixtures/kinetics-perps/markets__single.json via CARGO_MANIFEST_DIR;
  fixture values independently verified ({1000:5.899,...,1000000:5.8143} → 1696/1696/1696/1720 bps,
  ceil = conservative). EXECUTED fixture-binding mutation: doctored leverage 0.9 in the fixture
  => shape test FAILED (RED); restored. T5.B5 tick corrected VISIBLY in BUILD_PLAN
  ("[CORRECTED 2026-06-12 per gate fix F1, never erased: ...]"); original preserved.
- B  T5.B6 DST arms: FAIL AT GATE N — see Finding 1. Arms DO fire with executed hit counts at
  10000: ack_delay_fill 22574, api_error_retry 11503, demo_divergence 13526, funding_tick 134496,
  liquidation 3947, margin_reject 56888. run-dst.sh stage integration CONFIRMED (the stage's
  failure reds the whole battery: DST-EXIT=101 — it gates, it does not decorate). Determinism:
  per-seed digest re-run in-harness + my master-seed re-run reproduced the failure byte-identically.
  But the stage is RED at 10000 (1 failing seed) — a red DST seed is never accepted on explanation.
- C1 DTO/parser behaviors traceable to recordings: PASS — 8 spot-mapped, each verified:
  (1) dynamic error codes → code_matches prefix-match (dto.rs:126); fixture
      orders__create_gtc_blocked carries `user_not_found:_<uuid>`;
  (2) three error-body shapes → KineticsApiError untagged {Nested,Flat,Bare}; fixtures
      auth__bad_signature (nested) / subaccounts__create_nobody (flat) / funding__history_no_params (bare);
  (3) funding rates as JSON floats → f64 fields; verified raw: funding__rates_historical
      -0.0009593552948286 (number), ticker frame rate (number);
  (4) orderbook worst→best/best-at-END → best_bid/best_ask defensive scan; verified raw:
      depth0 bids 4.8368→6.3329 ascending, asks 7.6635→6.3416 descending; depth5 keeps it;
  (5) 409 duplicate body → verified raw: {"error":{"code":"order_already_exists",...}} status 409;
      adapter prefix-matches and resolves via list scan to AlreadyExists;
  (6) asymmetric skew window → verified raw metas: -5s/+5s/+30s = 200, -30s/±5min = 401;
      classification table encodes exactly that;
  (7) tick_size "" tolerance → parse_perp_price_opt; verified raw: markets__single tick_size "";
  (8) funding_history required start/end dates (finding 6) + `{}` content-type rule (finding 7)
      → client.rs:339,416; meta-equality-gated in kinetics_client.rs.
  FULL coverage: every *.json body classified, unclassified files fail the suite, classification
  cross-checked against recorded HTTP statuses (kinetics_dto.rs:190). EXECUTED fixture-binding
  mutation: doctored sub-tick ask "6.34165" => DTO test FAILED (RED); restored.
- C2 WS session vs recorded streams: PASS with two Minors — both .jsonl streams replay with zero
  errors / zero unknowns / zero seq gaps; snapshot normalizes recorded worst→best; synthetic
  gap/torn tests pin no-advance-until-fresh-snapshot (executed green); subscribe builders equal
  the recorded accepted commands (verified against both .meta.json subscribe_cmds); zero-pings
  tolerance is structural (nothing keys on pings; public meta records pings:0 over 5000 frames);
  user_orders treated as best-effort with REST as reconciliation truth. BUT: the `fill` channel
  has NO typed frame (Finding 3) and SESSION-NOTES no longer describe the committed private
  capture (Finding 2).
- C3 FIXTURES-GATED enforcement: PASS — kinetics has NO Venue impl and NO daemon composition;
  fortuna-live boot refuses every venue except "sim" (boot.rs:228 catch-all VenueNotBootable),
  so no live path exists to clear; live dial explicitly ledgered as composition work
  ("no live traffic from track C, ever"). No socket dials in any kinetics test (grep: zero
  tungstenite/connect/reqwest in tests; the one wss:// string is a constant equality assertion).
- C4 Honest gap ledgering: PASS with one Minor — pagination gap ledgered with its bound
  ("duplicate beyond the first 100 listed orders stays Rejected", GAPS B4 entry); the
  WS-session-before-tick promise trail preserved (35d59a0 ledger → slice 4 bbadfc0 → tick 4fd16de);
  fill-frame gap under-ledgered (Finding 3).
- C5 Sub-cent ticks / no f64 prices: PASS — PerpPrice integer ten-thousandths end-to-end;
  from_dollars_exact errors on sub-tick; PerpValue→Cents floor (gains) / ceil (costs);
  adapter money fields Cents via from_dollars_ceil/floor (against us); f64 confined to
  rates/leverage/roe (venue-payload ratios, not money); fee modeling Decimal with ceil.
- D  Completion accounting: PASS with notes — NOTE: the gate brief asserted "B4 is NOT ticked";
  the repo TICKS B4 (4fd16de) AFTER slice 4 landed, honoring the slice-3 ledger's
  "WS session remains before tick". Tick note enumerates open venue-state items (funding_history
  entry shape, notional-limit values, pagination, live-dial composition) — accurate vs §13;
  §13 items 6/9/11/12/14 remain composition/operator work (item 11 maintenance window only at
  ASSUMPTIONS:140; acceptable). B6 tick text matches the plan's five arms (demo-divergence
  included); its "all green at 2000" claim is honest at the STATED N (my fresh 2000-scenario run:
  green, 801 liquidations vs the claimed 786) — the red emerges at the gate's 10000. Only B2-B6
  boxes flipped in the whole range. RALPH STOP protocol-conformant (stop analysis in GAPS:
  queue state, owners, battery state; stopping beats inventing work).
- E  Ownership / protected crate: PASS — invariants diff over the full range is EXACTLY the
  waived batch 5 (4 files, 628 insertions, 0 deletions; zero `^-` content lines); no new touches
  after dc03294; examples/ (recorder) untouched by track C (0 diff lines).
- F  Full battery: FAIL overall (because of B) — fmt CLEAN; clippy --workspace --all-targets
  -D warnings CLEAN (exit 0 after cache clean); cargo test --workspace 121 suites,
  939 passed / 0 failed / 0 ignored; fortuna-invariants per-test ALL GREEN
  (i1-i7 + perp_i1_sealed_order 1/0 + perp_i2_drawdown_extension 3/0 + perp_i3_cross_domain_halt 3/0);
  scripts/run-dst.sh 10000: stage1 core "OK: 3 corpus + 10000 random seeds, zero invariant
  violations"; synthesis-dst ok (26997 orders / 41749 proposals); settlement-dst ok (11 arms
  accounted); perp-dst FAILED (1 seed) → battery exit 101. Mutations: 4/4 die (M1, M3,
  doctored sub-tick fixture, doctored leverage fixture); WS synthetic seq-gap tests green.
  Sweeps: zero unwrap/expect/panic in new src money paths; zero HashMap/HashSet in new files
  (BTree everywhere); zero #[ignore]/proptest reductions/deleted asserts diff-wide; zero secret
  material in fixtures (auth metas carry notes only); GatedPerpOrder constructible only via the
  pipeline (sole non-gates mention is a test helper RETURNING the pipeline's product).

## Findings

- [Major] RED DST SEED in the new B6 stage at the gate's N: seed 11819682492387934495
  (master 1781262032255, PERP_DST_SCENARIOS=10000) — "liq check: margin sim (KXBTCPERP):
  notional $100068.43 exceeds the last risk-curve tier: liquidation cannot be modeled
  (under-modeled = failure, not surprise)". Reproduction: re-ran the same master seed; identical
  seed + message; battery exit 101. Mechanism: the harness's sim curve tops out at $100k
  (matching its gate-time notional cap), but post-fill mark drift in a wild scenario pushes the
  POSITION's notional past the last tier; MarginSim then fail-closes (correct, conservative
  production behavior) and the harness's invariant-1 counts the designed refusal as a scenario
  failure. This is a harness scenario-design gap, NOT a money-path defect (nothing failed open);
  but a red seed at the gate N blocks regardless. Required: extend the harness curve to cover
  every reachable notional OR model the unboundable-notional refusal as a designed arm
  (with the spec-5.15 halt), AND add the seed to the regression set (definition-of-done #3),
  then re-run run-dst.sh at 10000.
- [Minor] Fixture-doc drift: SESSION-NOTES' private-WS narrative ("fill=true ... user_orders=false
  across 21 frames", ~02:50Z re-run note) is the INVERSE of the committed
  ws__private_lifecycle.jsonl (12 user_order frames, 0 fill frames, 20 frames; meta recorded
  ~04:08Z — a later capture overwrote the stream). The load-bearing wire doc no longer describes
  the committed recording. Annotate SESSION-NOTES (which run the committed capture is from; that
  fill frames were observed live but are NOT in the corpus).
- [Minor] The WS `fill` channel — per SESSION-NOTES the reliable fill source — has NO typed frame
  in the DTO/session layer; a live fill frame degrades to Ignored{frame_type:"fill"}. Conservative
  under never-invent (shape uncaptured in the corpus), and REST fills_reconciled carries the
  liquidation classification — but unlike funding_history/notional-limits this uncaptured shape is
  not NAMED in the ledger. Ledger it explicitly: real-time fill/liquidation notification (research
  §13.10) rests on REST polling until a fill-frame capture (or PROD parity sweep) lands.
- [Minor] GAPS.md T5.B5 DONE entry (~line 1034) still reads "liquidation sim from recorded risk
  curves" — the exact wording gate-fix F1 ruled an overstatement at e8fe069. BUILD_PLAN was
  corrected visibly; the GAPS done-entry was not annotated. One-line annotation needed.
- [Info] GAPS.md:973 B4 entry heading "SLICES 1+2 LANDED, box stays unticked" contradicts the same
  entry's body "T5.B4 BOX TICKED" (trail appended honestly; heading stale).
- [Info] margin_sim.rs error-message string literals embed line-wrap whitespace runs
  ("+/-{cap}                      per window"); cosmetic only.
- [Info] Shared-target cache poisoning produced phantom compile errors on first clippy/test runs
  (E0432 on fortuna_core::perp); resolved by cargo clean -p of the three crates. Environmental.

## Commands run (verbatim verdict lines)

```
cd /tmp/fortuna-gc && git checkout -q --detach 87b55e0
CARGO_TARGET_DIR=/tmp/fortuna-gate-target cargo fmt --check
  -> FMT-CLEAN
CARGO_TARGET_DIR=/tmp/fortuna-gate-target cargo clippy --workspace --all-targets -- -D warnings
  -> Finished `dev` profile ... in 41.27s ; EXIT=0   (after cargo clean -p fortuna-core -p fortuna-gates -p fortuna-venues)
CARGO_TARGET_DIR=/tmp/fortuna-gate-target cargo test --workspace
  -> 121 x "test result: ok"; passed=939 failed=0 ignored=0
CARGO_TARGET_DIR=/tmp/fortuna-gate-target cargo test -p fortuna-invariants
  -> 11 binaries, all ok (incl. perp_i1 1/0, perp_i2 3/0, perp_i3 3/0)
CARGO_TARGET_DIR=/tmp/fortuna-gate-target scripts/run-dst.sh 10000
  -> [dst] OK: 3 corpus + 10000 random seeds, zero invariant violations
  -> [synthesis-dst] ... test result: ok (138.02s)
  -> [settlement-dst] arms {SettleClean: 922, ...}; test result: ok (22.63s)
  -> [perp-dst] arms {"ack_delay_fill": 22574, "api_error_retry": 11503, "demo_divergence": 13526,
       "funding_tick": 134496, "liquidation": 3947, "margin_reject": 56888}
  -> [perp-dst] master 1781262032255: 1 failing seed(s): [(11819682492387934495, "... notional
       $100068.43 exceeds the last risk-curve tier ...")]
  -> test result: FAILED. 0 passed; 1 failed.  DST-EXIT=101
DST_MASTER_SEED=1781262032255 PERP_DST_SCENARIOS=10000 cargo test -p fortuna-state --test perp_dst -- --nocapture
  -> SAME failing seed + message (reproduction confirmed)
PERP_DST_SCENARIOS=2000 cargo test -p fortuna-state --test perp_dst -- --nocapture
  -> ok; arms {... "liquidation": 801 ...}  (tick's 2000-green claim honest at its stated N)
Mutation M1 (perp.rs:699 < -> <=): liquidation_distance_exactly_at_floor_passes FAILED -> restored
Mutation M3 (perp.rs:725 > -> >=): leverage_exactly_at_cap_passes FAILED -> restored
Doctored fixture (markets__single ask -> "6.34165"): market_single_parses_load_bearing_values FAILED -> restored
Doctored fixture (leverage_estimates["1000"] -> 0.9): risk_curve_from_recorded_leverage_estimates_shape FAILED -> restored
cargo test -p fortuna-venues --test kinetics_ws seq_gap -> ok (1 passed)
git diff 8966a00...87b55e0 -- crates/fortuna-invariants/ -> 4 files, 628 insertions(+), 0 deletions
git diff dc03294..87b55e0 -- crates/fortuna-invariants/ -> empty
```

## Merge recommendation

DO NOT MERGE until the B6 red seed is fixed (harness curve coverage or designed-refusal arm +
halt), seed 11819682492387934495 joins the regression set, and run-dst.sh 10000 is green
end-to-end. The fix is narrow and harness-local; nothing in the adapter, gates, sim, or
invariants needs to move.

Parked operator decisions — UNCHANGED by today's findings:
1. WAIVE BATCH 5: still exactly the verified 628/0 pure-addition set; zero new protected-crate
   touches since. Today's findings give no reason to withhold the waive.
2. F1 DISPOSITION: the remediation is now EXECUTED-VERIFIED (recorded-fixture converter +
   file-bound shape test + visible tick correction). Prior recommendation (waive + correction)
   stands; add the GAPS B5 wording annotation (Finding 4) to the same disposition.
