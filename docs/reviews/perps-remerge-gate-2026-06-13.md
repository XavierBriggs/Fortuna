# Review: perps re-merge package (track-c) — 2026-06-13

Base: caefc14 (main tip) / merge-base 7431e5b  Head: e027250 (track-c tip)
Verdict: ACCEPT — MERGE
Protected crate touched: YES (pure additions; signed waive batch 5 — automatic-BLOCK rule converted by operator-decisions-2026-06-12.md item 1)

Worktrees: /tmp/fortuna-grc (track-c) + /tmp/fortuna-merge-sim (main+merge sim).
Target: CARGO_TARGET_DIR=/tmp/fortuna-gate-target. Disk held (6.1Gi free at exit).

## Criteria (fixed before reading the diff)

- A. CLIENT-ID FIX (spec 5.15; c25b368 adjudication): PASS — the kinetics test
  derives its expectation THROUGH the path. The wire client_order_id
  (adapter.rs:132 = order.client_order_id()) is carried verbatim from the
  candidate via GatedPerpOrder::assemble (perp.rs:145), NOT re-derived from the
  IdGen-sequenced IntentId. Zero `intent`/`from_intent` references in the
  adapter or DTO — the IntentId (sole IdGen consumer) never enters the wire body.
  The test injects req["client_order_id"] from the recorded fixture and asserts
  the wire body == req, so it is structurally tree-state-independent.
  DECISIVE EMPIRICAL PROOF: merged track-c into a detached worktree at main tip
  (caefc14) — clean merge, no conflicts — and ran the test on that exact
  post-merge tree: GREEN. This is the literal condition that broke pre-fix.
  Upgrade-safety pin present + green on track-c
  (crash_recovery_adopts_a_resting_order_via_its_persisted_client_order_id,
  manager.rs:243) — recovery reads the persisted id, never re-derives.
- B. 2x LEVERAGE CAP (operator item 4): PASS — config max_leverage_x10:i64
  (integer fixed-point, venue Option + asset; config.rs:114/121), enforced as
  min(venue,asset) (perp.rs:725-727), comparison integer-exact in i128
  (lhs=notional*10 > rhs=equity*cap_x10; perp.rs:731-733). NO f64 in money/config
  path. Boundary test operator_venue_leverage_ceiling_binds_at_two_x
  (perp_gates.rs:1013): 1.99x(qty3184)/2.00x(3200) pass, 2.01x(3216) refused —
  GREEN. MUTATION-PROVEN both directions: (A) dropping the config min => 2.01x
  slips through, test RED; (B) `>`→`>=` => exact-2.0x wrongly refused, test RED.
  ASSUMPTIONS I7-review-on-loosening note present (ASSUMPTIONS.md:91-97).
- C. RED SEED (spec 5.15 fail-closed): PASS — in-harness corpus at
  crates/fortuna-core/dst-corpus/perp-curve-exceeded-11819682492387934495.seed,
  tagged `# harness: perp-dst` `# expect-arm: curve_exceeded`. Loaded by
  perp_dst.rs load_perp_corpus + asserted by regression_corpus_replays_green
  (GREEN) and replayed by run-dst stage 4. The curve_exceeded arm
  (perp_dst.rs:516-538) asserts check_liquidation MUST refuse when notional >
  last tier (is_ok() => test failure), then halts Global per spec 5.15 and
  records the designed refusal as SUCCESS.
- D. THE PLANE (re-verify, re-entering main): PASS — perp types integer-exact
  (PerpPrice(i64)/PerpValue(i64), Decimal only at decimal_to_i64 boundary). Five
  gate checks fail-closed (MarginHeadroom Err when available<required with
  funding-drag subtracted; LiquidationDistance Err below floor + on unusable
  mark; LeverageCap; PerpNotionalCap; integer-exact). MarginSim recorded-curve
  converter reads fixtures, f64 confined to one venue-payload boundary
  (margin_sim.rs:120, output integer bps, ceil() rounds against us, fail-closed
  on empty/non-finite/<1.0). Kinetics adapter fixtures-gated (Arc<dyn
  KalshiTransport>, MockKalshiTransport in tests, no live dial; fixtures record
  no key material). B2-B6 ticked [x]; B7 (strategies) + B8 (ops perps flatten)
  queued [ ]. Invariant ADDITIONS are PURE: ONLY modified protected file is
  src/lib.rs (+25 doctest lines, no existing assertion changed); perp_i1/i2/i3
  test files are status A (added). Zero existing-assertion changes — matches the
  operator's "628 insertions / 0 deletions" signed finding.
- E. FULL BATTERY: PASS — fmt 0; clippy --workspace --all-targets -D warnings 0;
  cargo test --workspace 0 (122 ok-result lines, 0 FAILED); invariants per-test
  all green incl. perp_i1(1)/perp_i2(3)/perp_i3(3) + 6 doctests; run-dst.sh
  10000 exit 0 — all 5 stages green, perp arms hit
  {funding_tick:134178, margin_reject:57301, ack_delay_fill:22691,
  demo_divergence:13463, api_error_retry:11377, liquidation:3927,
  curve_exceeded:3}, zero invariant violations. Test-weakening sweep diff-wide:
  no #[ignore], no deleted assertions, no proptest case reductions, no loosened
  tolerances. Mechanical sweep clean (no unwrap/expect/panic in perp money path;
  no SystemTime/Instant/Utc::now; no HashMap/HashSet nondeterminism; no secrets).
- F. RE-MERGE READINESS: PASS — see statement below.

## Findings

(No Critical / Major / Minor findings against the package.)

- [Informational] B7/B8 remain queued (perp strategies + ops kill-switch perps
  flatten). This is the documented scope boundary of the package, not a defect;
  the plane through B6 is complete and self-consistent. Ledger only.
- [Informational] Kinetics fixtures recorded against a degraded demo account
  (margin not enabled). Order-write fixtures are the venue's blocked/error
  responses; the adapter is tested against mock transport with the recorded
  shapes. Operator action to re-record on an enabled account is ledgered in
  SESSION-NOTES.md. Does not gate re-merge (no live path is exercised).

## RE-MERGE READINESS STATEMENT

The orchestrator MAY now `git merge track-c` into main. The merge was simulated
in /tmp/fortuna-merge-sim against the current main tip (caefc14) and applied
CLEANLY with no conflicts. Standing operator signatures
(operator-decisions-2026-06-12.md: waive batch 5 + F1 + item 4) cover this
re-entry in full; NO new signature is required.

Load-bearing POST-MERGE INTEGRATION CHECK (must run on merged main, must be
green before main is considered re-stabilized):
  cargo test -p fortuna-venues --test kinetics_adapter \
    place_maps_gated_order_to_the_recorded_create_request
This verifier already executed that exact check on the merged tree and it passed
— but re-run it on the actual post-merge HEAD as the gating integration check,
since the value of the fix is precisely that it survives the merged tree's
IdGen-stream shift. Also run the full run-dst.sh 10000 once on merged main to
confirm the perp stage and the curve_exceeded corpus seed replay green in the
integrated tree.

CALL: MERGE.

## Commands run (verbatim results, trimmed to verdict lines)

# fmt
$ cargo fmt --check        => FMT EXIT: 0 (no output)
# clippy
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile ... in 1m 09s   => CLIPPY EXIT: 0
# Rubric A — fix test on track-c
test place_maps_gated_order_to_the_recorded_create_request ... ok (1 passed)
# Rubric A — upgrade-safety pin on track-c
test crash_recovery_adopts_a_resting_order_via_its_persisted_client_order_id ... ok
# Rubric A — DECISIVE: fix test on MERGED-INTO-MAIN tree (/tmp/fortuna-merge-sim)
$ git merge --no-edit track-c  => MERGE EXIT: 0 (clean, no conflicts)
test place_maps_gated_order_to_the_recorded_create_request ... ok (1 passed)
# Rubric B — boundary test
test operator_venue_leverage_ceiling_binds_at_two_x ... ok (1 passed)
#   mutation A (drop config min): FAILED — "qty 3216 must be refused" left None
#   mutation B (`>`→`>=`):        FAILED — exact-2.0x wrongly refused
#   (perp.rs restored; git status clean)
# Rubric C — red seed corpus replay
test regression_corpus_replays_green ... ok (1 passed)
# Rubric E — invariants per-test
i1..i7 all ok; perp_i1(1) perp_i2(3) perp_i3(3) ok; 6 doctests ok
# Rubric E — full workspace
$ cargo test --workspace  => WORKSPACE TEST EXIT: 0 (122 ok-result lines, 0 FAILED)
# Rubric E — DST corpus 10000
$ bash scripts/run-dst.sh 10000  => RUN-DST EXIT: 0
[dst] OK: 4 corpus + 10000 random seeds, zero invariant violations
[synthesis-dst] ok (92.60s)
[settlement-dst] ok (16.36s)
[perp-dst] master 1781319711493 -> 10000 scenario(s); arms incl curve_exceeded:3,
           liquidation:3927; perp_plane_survives_seeded_chaos ok; corpus ok (2 passed)
[daemon_smoke] 15 passed
