# Review: track-c T5.B2 + T5.B3 + T5.B5 (perp core types, perp gate arm, MarginSim) — 2026-06-12
Base: 9466a08  Head: 885e0c2 (branch track-c)  Verdict: ACCEPT-WITH-GAPS
(conditional on: (1) the standing operator waive for the protected-crate additions —
queued in GAPS, contents verified pure-additive below; (2) operator adjudication of F1)
Protected crate touched: YES — pure additions (628 insertions / 0 deletions, numstat-verified)

Gate attempt #3 (disk-full killed #1; cold-cache watchdog stall killed #2). Full tier.
All commands run in worktree /tmp/fortuna-gc detached at 885e0c2 with
CARGO_TARGET_DIR=/tmp/fortuna-gate-target. Track-C-authored commits in the range:
7bf22df, e061348, af70ce4, 88f832e, 17f6f87, a1f2924, 9be57b7, 26472ea, 885e0c2.
Other commits in the range (1900ff2, b0b7db3, f4b4a54, 8467d0f, 59f6292, 70e7d04,
927a700, 14288a0, 9942344) are main-side, previously gated, reached track-c by rebase.

## Criteria (fixed before reading the diff: spec 5.15, BUILD_PLAN T5.B2/B3/B5,
## docs/design/kinetics-perps-module-plan.md, research.md §4-6, orchestration.md)

### A — T5.B2 core types
- A1 PerpPrice integer ten-thousandths end-to-end, checked ops, Decimal only at
  boundaries (spec 5.15 "Price domain"): PASS — crates/fortuna-core/src/perp.rs;
  mechanical sweep zero f64/unwrap/expect/panic in new src; from_dollars_{floor,ceil,exact}
  are the only Decimal entry points.
- A2 Type-level Cents/PerpPrice separation, conversion rounding against us: PASS —
  PerpValue::to_cents_floor (gains) / to_cents_ceil (costs) split is doctrine and
  test-pinned (prop_to_cents_floor_le_ceil_within_one_and_exact_iff_divisible).
- A3 Signed PerpPosition / FundingAccrual / MarginAccountView conservative marking:
  PASS — min-of-marks uPnL, unmarked_flag on missing conservative mark; funding
  amount floored against us both directions (test vectors: long pays -7c on the
  0.0001 x $6.26 x 100 case; asymmetric 105/-106 long/short pair).
- A4 Property tests run, case counts not shrunk: PASS — 38 tests incl. 7 property
  suites (tests/perp.rs result: ok. 38 passed); no ProptestConfig override and no
  proptest.toml anywhere in the repo (default 256 cases).

### B — T5.B3 gate arm (spec 5.15 "New gates" + "Capital and exposure")
- B1 Margin headroom vs DEDICATED envelope, worst case = liquidation loss not
  premium: PASS — required = mm_curve(worst-case notional) x safety multiplier
  (validated >= 100), worst-case notional = max(|pos|,|pos+delta|) along the fill
  path x max(limit, conservative mark) ceiled; available = margin-account equity
  minus funding drag; notional beyond the last curve tier REFUSED (spec sentence
  implemented literally). No premium-style loss bound anywhere on the perp arm.
  The max()-path conservatism is pinned (asset_notional_cap_uses_worst_case_post_position).
- B2 Liquidation-distance floor, boundaries: PASS with Minor F2 — enforcement
  mutation-proven (floor disabled => liquidation_distance_floor_rejects_thin_buffer
  + _is_monotone_in_config RED); exactly-at-floor equality NOT pinned (mutation
  `<` -> `<=` survives all 36 tests).
- B3 Leverage caps: PASS with F4 note — config-driven, integer cross-multiplied
  (worst_notional x 10 <= equity x max_leverage_x10), mutation-proven (cap disabled
  => leverage_cap_rejects... RED); at-cap equality NOT pinned (`>` -> `>=` survives).
  NOTE TO OPERATOR: the gate rubric said "SPEC 2x" — no 2x exists anywhere in
  docs/spec.md, the plan, BUILD_PLAN, config, or ledgers. Spec 5.15(b) says
  "config-set at or below venue maxima". There is NO committed [perp] config
  (config/fortuna.example.toml has no perp section), so no operating cap value is
  pinned anywhere; absent config the arm fails closed (test-pinned). If 2x is the
  intended operating cap it must be recorded (config example + spec or directive).
- B4 Notional caps: PASS — venue cap counts existing open notional + order notional;
  per-asset cap on worst-case position notional; both test-pinned.
- B5c Funding drag subtracts from edge: PASS — net = gross(floored) - assumed_fee(ceiled)
  - drag(ceiled, scales with holding_windows >= 1 enforced); fee-trap pinned
  (assumed_fee_bps = 0 is a config validation ERROR).
- B6 Fail-closed on every error arm: PASS — enumerated: missing venue config, missing
  asset config, profile derivation failures (negative qty, position/market mismatch,
  overflow, invalid reduce-only x3), unbounded mm_curve tier, non-positive conservative
  mark (distance + price sanity), missing strategy limits, missing rate config, zero
  holding_windows, non-positive notional in edge math, plus the defensive dispatch arm
  (perp check invoked on event arm => reject) and the unreachable push() fallback
  (also rejects). Every arm returns Err => Reject; none falls through to acceptance.
- B7 I1 integrity: PASS — same GatePipeline instance (shared halts + I3 buckets,
  proven by perp_i3 cross-domain tests both directions + shared-bucket test); one
  audit GateCheckRecord per evaluated check, trail = PERP_ALL prefix ending at the
  failing check (proptest-pinned in gates + invariants); GatedPerpOrder seal:
  private fields, pub(crate) assemble() only, Serialize-only — two new compile_fail
  doc-tests green; no construction site outside fortuna-gates (grep); no model
  consult (all checks are pure functions over candidate/inputs/config).
- B8 Invariant middle untouched: PASS — pipeline.rs hunks are provably additive:
  4 new GateCheck variants + index() arms (11-14), defensive fail-closed dispatch arm,
  pub(crate) field visibility, strategy_limits delegation (same logic/message). Zero
  hunks in the bodies of checks 1-10. GateCheck::ALL still the 10-element event order,
  pinned by perp_all_has_eleven_checks_with_spec_numbering (asserts ALL.len()==10).
  No existing gate test file modified (only new tests/perp_gates.rs).

### C — T5.B5 MarginSim
- C1 Funding on the 04/12/20 UTC schedule: PASS — funding_times_between exclusive-after/
  inclusive-until, both pinned; accrual applies to balance + appends the log; flat
  positions accrue nothing. Cap +-2%: NOT enforced anywhere (venue-trust posture,
  ledgered in ASSUMPTIONS) — Minor F3.
- C2 Liquidation sim from RECORDED risk curves: FAIL AS CLAIMED — Major F1 (below).
  Engine is curve-driven and fail-closed (unbounded tier / missing mark / empty curve
  = error, all test-pinned), but nothing in the range reads any recording.
- C3 Mark-based PnL conservative: PASS — VWAP entries round against us by side,
  realized PnL floors toward -inf (sub-cent case pinned), account view at
  worse-of-marks, maintenance requirement at the HIGHER notional mark, liquidation
  closes whole account at worse-for-us mark pushed further by penalty (long down /
  short up, both pinned), negative balances modeled not clamped (-130c / -60c vectors).
- C4 Determinism: PASS — verifier scratch harness (2000 seeded steps: fills, funding
  ticks, liquidation checks), run twice as separate processes: byte-identical
  (71,234-byte trace, fnv1a=70d4f156c302ad20 both runs). Structural: BTreeMap-only
  state, no RNG, no clock reads, no HashMap iteration.
- C5 Liquidations as system fills per 5.15 lifecycle: PASS within sim scope —
  Liquidation event reports closed positions; module doc assigns the mandatory
  alert + halt evaluation to the caller. The lifecycle state/alert plumbing is
  B4/B6/B8 wiring (queued), correctly not faked here.

### D — Protected crate (crates/fortuna-invariants/)
PURE ADDITIONS, verified: `git diff 9466a08..885e0c2 --numstat -- crates/fortuna-invariants/`
=> lib.rs +25/-0 (doc-test pins appended after the existing block: perp witness,
GatedPerpOrder construction compile_fail, Deserialize compile_fail); 3 brand-new
test files: perp_i1_sealed_order.rs +142/-0 (proptest: every outcome carries a
coherent PERP_ALL trail; sealed terms = candidate terms), perp_i2_drawdown_extension.rs
+256/-0 (perp mark loss alone breaches and locks until re-arm; funding paid alone
breaches; worse-for-us mark governs — venue mark cannot mask a conservative-mark
loss), perp_i3_cross_domain_halt.rs +205/-0 (perp breach halts event orders, event
breach halts perp orders, one shared bucket, halt-not-throttle). Zero deleted lines
diff-wide in the crate (grep '^-[^-]' count = 0). No existing assertion, name,
tolerance, or case count touched. Existing i1-i7 all green per-test (13 tests),
new perp pins green per-test (7 tests), 6 doc-tests green incl. all 4 compile_fail.
AUTO-FLAG: per CLAUDE.md/verifier rule every invariant-crate touch requires operator
review regardless of content; orchestration.md pre-authorizes the ADDITIONS-ONLY
pattern for T5.B3 and the implementer queued the waive in GAPS. WAIVE REQUEST
STANDS: confirm the additions.

### E — Completion accounting
- T5.B3 tick: PASS — every named element evidenced (margin headroom, liq-distance
  floor, leverage caps, funding-drag-in-edge, notional caps, liquidation-loss worst
  case, invariant ADDITIONS only). 36 spec-first gate tests + 8 margin tests + 7
  invariant pins, all executed this session. Deferral (daemon wiring of composed
  equity -> track A at B4/B5) ledgered in GAPS with owner.
- T5.B5 tick: FAIL on one element — "liquidation sim from recorded risk curves"
  (F1). All other elements evidenced (schedule, accrual log, mark-based PnL,
  whole-account liquidation, error doctrine; 20 spec-first tests executed).
- ASSUMPTIONS "flagged for verifier" adjudications:
  1. GatedPerpOrder as a SECOND sealed type (vs literal "same sealed GatedOrder",
     spec 5.15): ACCEPTED. The literal reading is self-contradictory — 5.15 also
     mandates "A PerpPrice must never carry an event-contract price nor vice versa
     (type-level separation, not convention)" while GatedOrder.limit_price is Cents.
     The I1 substance (same pipeline instance, same seal discipline, Serialize-only,
     compile-fail pinned) is preserved and now invariant-tested. RECOMMENDED: ledger
     a spec-gap entry to amend the 5.15 sentence at the next spec rev.
  2. Reduce-only doctrine (valid reduce-only passes risk gates + edge floor with a
     note): ACCEPTED — conservative direction; rejecting a de-risking close forces
     staying at higher risk; invalid reduce-only (no position / same direction /
     oversized) rejects, all pinned; halts/sanity/rate/idempotency/netting still
     apply (pinned).
  3. Checks 2/3/9 absent from the perp arm: ACCEPTED — MarginHeadroom IS the capital
     check against the 5.14 dedicated envelope; leverage + notional caps are the
     position-caps analog; perps have no canonical events. Per-strategy perp
     exposure deferral to B7 sizing is ledgered; keep it visible at B7 grading.
  4. B5 set (VWAP rounding by side, whole-account liquidation, summed per-market MM,
     funding only for held positions, apply_fill does not margin-check, no B5 DST):
     ACCEPTED — each conservative or a correct venue-split; maintenance-window
     funding deferral is timestamp-only (amount identical), fine as a B6 arm.
  5. B2 set (whole-tick avg_entry, Decimal rate as boundary artifact, sign
     convention, no position math in B2, caller-supplied pending_funding, no id,
     InstrumentKind threading deferred on ownership): ACCEPTED — but the
     record-don't-re-derive rate posture is what leaves F3 open.

### F — Ownership + hygiene
PASS — every track-C commit touches only track-C-owned files (fortuna-core perp
module, fortuna-gates, fortuna-state margin pieces, fortuna-invariants additions)
plus own ledger entries and own BUILD_PLAN boxes. The cli/live/runner/examples/
review-file changes in the range are main-side commits. fortuna-gates/src/pipeline.rs
and lib.rs edits are within "perp extensions" scope and provably additive (B8).
Cargo.lock follow-up 885e0c2 adds exactly the declared fortuna-state rust_decimal
dependency (+1 line). NOTE: completion notes cite pre-rebase hashes (56d07db,
7782f5c, b4561ca, e8fe069) that no longer exist in the rebased history — cosmetic,
the trail acknowledges rewrites elsewhere.

### G — Full battery at 885e0c2 (verbatim verdict lines)
- `cargo fmt --check` => exit 0 (clean).
- `cargo clippy --workspace --all-targets -- -D warnings` => "Finished `dev` profile
  [unoptimized + debuginfo] target(s) in 19.75s", exit 0, zero warnings.
- `cargo test --workspace` => 115 suites, "total passed: 852", 0 failed, exit 0.
  New suites: tests/perp.rs "ok. 38 passed", tests/perp_gates.rs "ok. 36 passed",
  tests/margin.rs "ok. 8 passed", tests/margin_sim.rs "ok. 20 passed".
- Invariants per-test: i1 (2), i2 (2), i3 (1), i4 (1, 29.85s), i5 (1), i6 (3),
  i7 (3), perp_i1 (1), perp_i2 (3), perp_i3 (3), doc-tests (6 incl. 4 compile_fail)
  — ALL ok.
- `scripts/run-dst.sh 10000` => exit 0 (set -euo pipefail). Core arm (re-run for the
  quotable line): "[dst] OK: 0 corpus + 10000 random seeds, zero invariant
  violations". Synthesis arm: "ok. 1 passed ... finished in 252.84s" (26,964 orders,
  131,820 cognition failures injected). Settlement arm: "ok. 1 passed ... 46.02s"
  (1,778 discrepancies, 1,843 halts exercised). Daemon smoke: "ok. 2 passed".
- Perp DST arms: none yet — T5.B6 is queued; absence is per-plan, not a finding.
  Today's coverage of the new gate checks: 36 deterministic tests + 2 proptests
  (~256 cases each: gates trail coherence, invariants outcome coherence) + the
  three perp_i3 bucket/halt tests.
- Proptest case counts: defaults everywhere (no ProptestConfig/proptest.toml).
- Mechanical sweep (track-C src diff): zero unwrap/expect/panic/todo, zero
  SystemTime/Instant/Utc::now, zero f32/f64, zero HashMap/HashSet, zero secret
  literals. place() sites all take GatedOrder; no GatedPerpOrder constructor
  outside fortuna-gates. Test-weakening sweep diff-wide: no #[ignore], no case
  reductions, no loosened tolerances; the only deleted asserts are in main-side
  verifier commits 8467d0f/9942344 (test STRENGTHENING, previously gated).
- Mutation checks (gate worktree, restored after; `git status` clean at end):
  M1 liq-distance `<`->`<=`: SURVIVED (boundary unpinned, F2).
  M2 liq-distance floor disabled: RED (2 tests) — teeth confirmed.
  M3 leverage `>`->`>=`: SURVIVED (boundary unpinned, F2).
  M4 leverage cap disabled: RED (1 test) — teeth confirmed.
- Determinism: scratch double-run byte-identical (fnv1a=70d4f156c302ad20).
- Disk: started 33Gi free, ended 16Gi (track-C implementer battery ran concurrently
  in fortuna-wt-c) — above the 10GB alert line, watch next firing.

## Findings
- [Major] F1: T5.B5 tick element "liquidation sim from recorded risk curves" has no
  executed evidence — reproduction: `grep -rn "risk__parameters\|fixtures/kinetics"
  crates/fortuna-state/ crates/fortuna-gates/ crates/fortuna-core/` => zero hits;
  test curves are invented tiers [[1,000,000c, 500bps],[5,000,000c, 800bps]]
  (= 12.5-20x leverage) while the recording (fixtures/kinetics-perps/
  markets__single.json leverage_estimates) shows max 5.899x => ~1695 bps — the
  invented numbers are MORE PERMISSIVE than reality; the leverage_estimates ->
  RiskCurve converter exists nowhere. MITIGATION: spec 5.15 demo/prod divergence
  ("risk-parameter and fee values are always read from the TARGET environment at
  runtime, never baked from fixtures") makes runtime ingestion at B4 the CORRECT
  design — but then the tick and GAPS wording overstate what shipped. DISPOSITION:
  operator either (a) waives with a one-line tick/GAPS correction ("curve-DRIVEN
  liquidation sim; recorded-curve ingestion + converter land with B4") + a B4
  contract item (converter + shape test against the recording), or (b) unticks B5
  pending the ingestion. Recommend (a).
- [Minor] F2: At-boundary semantics unpinned — mutations M1/M3 survive the full
  suite; "distance == floor passes" and "leverage == cap passes" (spec: "at or
  below") are convention, not tests. Fix: two exact-boundary tests (the perp_config
  numbers admit exact-equality vectors with minor adjustment).
- [Minor] F3: No sanity bound on funding rates entering FundingAccrual::accrue /
  MarginSim::apply_funding — research §4 caps |rate| at 2% per 8h; a corrupted
  recording or DTO bug propagates silently into sim balances. Posture ("record
  reported rates, never re-derive") is ledgered, but a REJECT on |rate| > 0.02
  manufactures no discrepancy — it catches impossible inputs. Ledger + fix at B4
  DTO or sim entry.
- [Minor] F4: No [perp] section in config/fortuna.example.toml — operating
  leverage/notional/fee-assumption values are unpinned (arm fails closed absent
  config, so this is safe but un-operable); the gate directive's "2x" cap exists
  nowhere in the repo. Operator decision needed; record it in the example config.
- [Minor] F5: Leverage rejection reason formats max_leverage_x10=50 as "5x0"
  (perp.rs:729-730, `{}x{}` with /10 and %10) — garbled audit string, should read
  "5.0x". Cosmetic, audit-clarity only.
- [Note] F6: Ledger completion notes cite pre-rebase commit hashes; cosmetic.

## Commands run (verbatim, all from /tmp/fortuna-gc at 885e0c2 with
## CARGO_TARGET_DIR=/tmp/fortuna-gate-target)
- cargo fmt --check                                          => exit 0
- cargo clippy --workspace --all-targets -- -D warnings      => exit 0, 0 warnings
- cargo test --workspace                                     => 852 passed / 0 failed / 115 suites
- cargo test -p fortuna-invariants                           => 20 tests + 6 doc-tests, all ok
- ./scripts/run-dst.sh 10000                                 => exit 0 (all four arms)
- cargo test -p fortuna-core --test dst -- --seeds 10000     => "[dst] OK: 0 corpus + 10000
                                                                random seeds, zero invariant violations"
- 4 mutations in crates/fortuna-gates/src/perp.rs (M1-M4 above), each followed by
  cargo test -p fortuna-gates --test perp_gates, worktree restored via git checkout --
- scratch determinism harness (outside repo), 2 process runs  => byte-identical
