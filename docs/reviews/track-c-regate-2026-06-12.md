# Review: track-c-regate (BLOCK remediation) — 2026-06-12

Base: 87b55e0  Head: 3c8b126 (substantive fix e0d4ae2)  Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no (`git diff 87b55e0 3c8b126 --stat -- crates/fortuna-invariants/` = empty)

Scope of code delta base->head: GAPS.md, crates/fortuna-state/src/margin_sim.rs (+2/-1,
error-message reformat only), crates/fortuna-state/tests/perp_dst.rs (+92/-11),
fixtures/kinetics-perps/SESSION-NOTES.md (+14), docs. No other production code moved.

## Criteria (fixed before reading the diff: GATE-FINDINGS TRACK-C FINAL GATE items 1-4 + re-gate rubric)

- C1 The fix — designed-refusal arm, spec-5.15 fail-closed asserted as SUCCESS,
  harness-local (fix item 1; spec 5.15): **PASS** — evidence:
  - Option chosen: designed-refusal arm (curve coverage unchanged: tiers still
    [(2,000,000, 500), (10,000,000, 800)], now via CURVE_TOP_CENTS const).
  - The refusal is PINNED, not bypassed: when over_tier, the harness FAILS the scenario
    if `sim.check_liquidation(...)` returns Ok ("notional beyond the last tier did NOT
    refuse"). Production fail-closed must fire for the scenario to pass.
  - Spec-5.15 halt applied and ENFORCED by pre-existing invariant-4 assertions: after
    `set_halt(Global, ...)`, any gate-pass => scenario failure; any rejection not at
    GateCheck::Halts => scenario failure. Spec quotes: "REFUSE any order whose worst
    case the approximation cannot bound"; "trading on a wrong margin model is not
    allowed to continue silently"; "the worse-for-us number governs halt math".
  - Boundary semantics EXACTLY aligned with production: harness `notional_at(max(venue,
    cons)) > 10_000_000` <=> `RiskCurve::mm_bps` returns None (tier coverage is
    `notional <= threshold`, margin_sim.rs:76-81) <=> check_liquidation Err
    (margin_sim.rs:430-438); same `worse_notional_mark` = max(conservative, venue)
    (margin_sim.rs:539-544); same ceiled `notional_at` (fortuna-core perp.rs:310-317).
    Equality-at-tier-top is bounded on both sides (no false designed-arm, no false red).
  - Conservation check still runs on the refusal step; balance untouched (production
    Err returns before any mutation, margin_sim.rs:449+).
  - Blast radius proven surgical: arm counts at the exact gate master are IDENTICAL
    pre-fix vs post-fix except curve_exceeded 0->1 and funding_tick 134496->134499
    (the formerly-aborted scenario completing its post-halt steps).
  - Production-code change graded with suspicion: margin_sim.rs delta is a pure
    `format!` string-literal whitespace reformat of the funding-cap error message
    (a prior-gate Minor). No logic change; grep confirms zero test/code couples to
    that message text outside margin_sim.rs.
- C2 The seed — regression pin, failure-mode comment, replayed by run-dst.sh, corpus
  count 4 (fix item 2): **PASS on substance, letter deviation (Finding 1)** — evidence:
  - Seed 11819682492387934495 committed in `REGRESSION_SEEDS` in
    crates/fortuna-state/tests/perp_dst.rs:652-662 with the full failure-mode story
    (gate id, mechanism, $100,068.43 > $100k, "Never delete entries.").
  - REAL regression pin, reproduction EXECUTED: pre-fix harness (blob 8e3f1eb at
    87b55e0) with `DST_MASTER_SEED=1781262032255 PERP_DST_SCENARIOS=10000` => exit 101,
    exactly seed 11819682492387934495, "notional $100068.43 exceeds the last risk-curve
    tier" — byte-matches the gate's finding.
  - Replayed every battery: run-dst.sh stage 5 runs the whole perp_dst binary;
    `regression_seeds_replay_green ... ok` observed in the 10000 battery AND in the
    plain workspace suite (no env vars needed). Asserts the curve_exceeded arm fires
    AND digest determinism on the seed.
  - Letter deviation: crates/fortuna-core/dst-corpus/ still holds 3 anchor seeds, not
    4. JUSTIFIED: that corpus feeds fortuna-core's dst harness (load_corpus -> run_one,
    crates/fortuna-core/tests/dst.rs:91-94); a perp_dst scenario seed there would be
    replayed by the WRONG generator as a meaningless core scenario. The in-harness set
    is the semantically correct realization of "never delete, replay every run".
    Deviation rationale not ledgered anywhere — Finding 1 (Minor).
- C3 Green at N — full run-dst.sh 10000 incl. B6 + exact original master green where it
  was red, byte-deterministic (fix item 3): **PASS** — evidence:
  - `bash scripts/run-dst.sh 10000` => DST-EXIT=0 end-to-end: core "[dst] OK: 3 corpus
    + 10000 random seeds, zero invariant violations"; synthesis ok (92.98s); settlement
    ok (16.32s); perp_dst 10000 ok (20.33s, fresh master 1781271809660 fired
    curve_exceeded 4x ORGANICALLY); daemon_smoke ok.
  - Exact original master: `DST_MASTER_SEED=1781262032255 PERP_DST_SCENARIOS=10000`
    => exit 0, "2 passed; 0 failed", arms {..., "curve_exceeded": 1, ...}.
  - Byte-determinism: second run of the same command, `diff` over all [perp-dst] output
    lines => identical ("BYTE-IDENTICAL perp-dst output").
- C4 Stale-wording annotation (fix item 4 / gate Finding 4): **PASS** — GAPS.md B5
  done-entry now carries "[ANNOTATION 2026-06-12 per final-gate Minor 4, never erased:
  at e8fe069 'recorded risk curves' overstated the data source (config curves only);
  the recorded-curve path landed later via RiskCurve::from_leverage_estimates ...]".
- C5 Battery + test-weakening sweep: **PASS** — evidence:
  - `cargo fmt --check` => clean.
  - `cargo clippy --workspace --all-targets -- -D warnings` => exit 0 (after an
    ENVIRONMENTAL false failure, Finding 4/Info: my fresh CARGO_TARGET_DIR held a
    corrupted cached fortuna-core rmeta from the first cold parallel build — E0432
    "could not find `perp`" + cascading E0282 in fortuna-venues, twice reproducible;
    `cargo clean -p fortuna-core -p fortuna-gates -p fortuna-venues` + rebuild of the
    IDENTICAL sources => exit 0; per-package clippy was green throughout; none of the
    affected crates are in the diff).
  - `cargo test --workspace` => exit 0, 940 passed / 0 failed / 0 ignored.
  - fortuna-invariants per-test: i1 2/0, i2 2/0, i3 1/0, i4 1/0, i5 1/0, i6 3/0,
    i7 3/0, perp_i1 1/0, perp_i2 3/0, (+ cross-domain) — all ok.
  - Weakening sweep over `git diff 87b55e0 3c8b126 -- crates/`: zero deleted asserts,
    zero #[ignore], zero tolerance/case-count changes; only added unwrap/panic are
    failure reporters inside the new regression TEST. Per-arm enumeration before/after:
    invariants 1-7 of the B6 header all still asserted (idempotency, gate-pass=>no
    instant liq, liquidation-never-silent + halt-at-check-1, conservation every step
    incl. the refusal step, demo-divergence fail-closed, determinism digest EXTENDED
    with curve_exceeded — strictly more sensitive). Coverage floor unchanged (all six
    original arms still enforced at >=100 scenarios); curve_exceeded coverage rides on
    the deterministic regression seed with in-code rationale (needs ~10k scenarios to
    arise by chance — corroborated: 1x at the gate master, 4x at a fresh 10000 master).

## Findings

- [Minor] Fix-spec item 2 letter deviation: seed pinned in in-harness REGRESSION_SEEDS,
  not crates/fortuna-core/dst-corpus/ (corpus count still 3, not 4). Substance verified
  superior (the literal placement would replay the wrong harness), but the deviation is
  ledgered nowhere — one GAPS line owed. — reproduction: ls dst-corpus (3 .seed files);
  crates/fortuna-core/tests/dst.rs:91-94 corpus consumption.
- [Minor] The designed-arm pin asserts only that `check_liquidation` returns SOME error
  when over_tier (`.is_ok()` negation, perp_dst.rs:527), not the curve-exceeded reason
  specifically; a coincident unrelated fail-closed error would be misclassified as the
  designed refusal. Tighten by matching "exceeds the last risk-curve tier". Fail-closed
  direction either way; non-blocking.
- [Minor] Latent false-red of the SAME class remains at the invariant-3 call site
  (instant-liquidation check after an immediate gated fill, perp_dst.rs:~487): a fill
  gated at conservative/limit marks can in principle land with worse-mark notional past
  the top tier => check_liquidation Err propagates as scenario failure, not the designed
  arm. Empirically unhit in 2x10000 verifier runs + battery; fail-noisy, not fail-open.
  Recommend the same designed-refusal classification there, or a comment + GAPS line.
- [Info] Environmental, not attributable to the diff: first cold workspace clippy in a
  fresh target dir failed reproducibly on corrupted cached rmeta (E0432/E0282 in
  fortuna-venues, crates untouched by the diff); targeted clean of identical sources =>
  green. Recorded so the battery claim is not doubted later.
- [Info] B6 harness header (per-seed invariants doc, perp_dst.rs:9-37) not updated to
  name the curve_exceeded designed arm; the inline comment at the site is thorough.

## Commands run (verbatim verdict lines)

```
# Reproduction (pre-fix, worktree at 87b55e0, blob 8e3f1eb):
DST_MASTER_SEED=1781262032255 PERP_DST_SCENARIOS=10000 cargo test -p fortuna-state --test perp_dst -- --nocapture
=> EXIT=101; [perp-dst] master 1781262032255: 1 failing seed(s): [(11819682492387934495,
   "seed 11819682492387934495: liq check: margin sim (KXBTCPERP): notional $100068.43
   exceeds the last risk-curve tier: liquidation cannot be modeled (under-modeled =
   failure, not surprise)")]

# Post-fix (worktree at 3c8b126, blob f435e97):
cargo fmt --check                                   => clean
cargo clippy --workspace --all-targets -- -D warnings => exit 0 (after env-artifact clean)
cargo test --workspace                              => exit 0; 940 passed; 0 failed; 0 ignored
DST_MASTER_SEED=1781262032255 PERP_DST_SCENARIOS=10000 cargo test -p fortuna-state --test perp_dst -- --nocapture
=> EXIT=0; regression_seeds_replay_green ok; arms {"ack_delay_fill": 22574,
   "api_error_retry": 11503, "curve_exceeded": 1, "demo_divergence": 13526,
   "funding_tick": 134499, "liquidation": 3947, "margin_reject": 56888}; 2 passed
(re-run of the same command: BYTE-IDENTICAL perp-dst output)
bash scripts/run-dst.sh 10000                       => DST-EXIT=0
   [dst] OK: 3 corpus + 10000 random seeds, zero invariant violations
   synthesis ok 92.98s; settlement ok 16.32s
   [perp-dst] master 1781271809660 -> 10000: curve_exceeded 4x organic; 2 passed
   daemon_smoke ok
```

Arm-count delta at the exact gate master (pre-fix red run vs post-fix green run):
identical everywhere except curve_exceeded 0->1, funding_tick 134496->134499. The fix's
blast radius is exactly the one formerly-red scenario.

Merge recommendation: the BLOCK is cleared with executed evidence in both directions.
Merge may proceed under the prior verdict's unchanged operator conditions (waive
batch 5 protected-crate additions; F1 disposition) — nothing in this re-gate alters
either. The three Minors are GAPS-ledger items for the implementer, not merge blockers.
