# Review: latency-batch-gate — 2026-06-10
Base: cc3bde0  Head: d1cafe1  Verdict: BLOCK
Protected crate touched: no (empty diff for crates/fortuna-invariants/; last touch 1d1c033, ancestor of 1e3e5e7)

Scope: 5a1581a (concurrent leg submission) + d1cafe1 (MarketStream layer, Kalshi WS
message layer, paper replay seam, Polymarket research). Code: fortuna-exec,
fortuna-runner, fortuna-venues, fortuna-paper, ledgers. Only dep added: `futures`
(workspace); no tungstenite/socket dep anywhere in the lock diff.

## Criteria (fixed before reading the diff)

- C1 journal BEFORE network; outcomes in leg order; pre-network refusals never touch
  venue (spec 5.4 "persisted ... BEFORE any network call"): PASS — evidence:
  submit_group_concurrent phase 0 appends Created+SubmitAttempted per leg before the
  phase-1 `join_all`; phase 2 zips `staged` (input order) with placements (join_all
  preserves input order). `LegOutcome::NotSubmitted` is only constructible in phase 0,
  and the refusal branches `continue` before `staged.push` — refused legs structurally
  cannot reach the venue. Test `working_order_collisions_refuse_the_leg_without_
  touching_the_venue` asserts `venue.max_seen() == 1`. 24/24 manager tests pass.
- C2 OverlapVenue proves max in-flight == 3 on a single-threaded executor: PASS —
  `group_legs_place_concurrently_and_outcomes_keep_leg_order` runs under
  `futures::executor::block_on`, venue `place` yields twice (every sibling polled
  before any completes), asserts `max_seen() == 3`. Executed: ok.
- C3 runner all-or-nothing upgrade; single-leg parity; reservation discipline; one
  submit timestamp per group: PASS (one Minor coverage gap) — any gate rejection on a
  multi-leg group sets `group_rejected`, releases every staged reservation, returns
  before phase B (nothing submitted). Phase-C outcome arms are line-for-line parity
  with the old match: release on venue `Rejected` and on `WorkingOrderExists`, keep on
  `Unknown` (reconciliation resolves), `submitted_at_ms` taken once before
  `submit_group_concurrent` and shared by all legs (ack-latency + submit_times sane).
  Single-leg parity evidenced empirically: 647-test workspace suite,
  `same_seed_same_script_byte_identical_recording`, and both DSTs green over the
  rewired path. Minor: no test drives the mid-group abort with `staged` non-empty
  (see findings).
- C4 two same-(strategy,market,side) legs in ONE group caught pre-network: PASS —
  `IntentEvent::SubmitAttempted` folds status to `Submitted` (manager.rs:948);
  `is_working()` covers `Submitted` (manager.rs:89-94); `append` folds into
  `self.intents` BEFORE the journal write and phase 0 is sequential, so leg 1 is
  visible-as-working to leg 2's `working_order()` precheck inside the same phase 0.
  Proven by committed test (NotSubmitted(WorkingOrderExists), venue untouched).
  No I3 bypass by leg multiplication: rate tokens are consumed per leg at gate time
  (phase A), before any venue IO.
- C5 I1 — only GatedOrder reaches place(): PASS — grep: every `place` signature and
  call site (venues trait, kalshi adapter, sim, polymarket stub, paper, exec x2) takes
  `GatedOrder`; no GatedOrder constructor/From/Into outside fortuna-gates (exec's
  `From<&GatedOrder> for OrderSnapshot` is read-direction).
- C6 DST at 300 + byte-identical replay over the concurrent path: PASS —
  settlement_dst 300/300 ok (master seed 1781145237489), synthesis_dst 300/300 ok
  (master seed 1781145344826), full corpus `scripts/run-dst.sh` (set -euo pipefail,
  N=2000) ran to completion: settlement at 2000 ok (seed 1781146374749).
- C7 ws.rs shapes trace to research §11 / raw archive; no live dial: FAIL (Major) —
  happy-path shapes are verbatim-traced: snapshot/delta (research.md:705-721), trade
  (raw/pages/websockets_public-trades.md fields market_ticker/yes_price_dollars/
  count_fp/taker_side + verbatim example, also asyncapi trade example), error envelope
  `{type:"error", msg:{code,msg}}` and subscribed `{msg:{channel,sid}}`
  (research.md:692-697). No socket dep added; dial explicitly fixture-gated. BUT the
  archived AsyncAPI documents `yes_dollars_fp`/`no_dollars_fp` as OPTIONAL on
  `orderbook_snapshot` ("This key will not exist if there are no Yes offers",
  asyncapi.yaml ~2630-2655, schema `const: orderbook_snapshot`); `parse_levels`
  hard-errors on the absent key. Executed repro: one-sided snapshot frame ->
  `ERR: invalid: book side is not an array`. A documented, routine frame (one-sided
  book — normal at price extremes) is rejected by the layer this commit delivers.
- C8 stream/paper tests; torn-state discipline: PASS (Minors) — 6/6 stream, 12/12
  paper tests pass. Delta-before-snapshot errors (tested); overdrawn level errors
  (tested); SeqGap does NOT advance baseline — code-verified (insert only on the
  in-sequence branch) and execution-verified (scratch: delta seq 13 after gap at 12
  -> SeqGap expected=11 got=13), but NOT pinned by any committed test (Minor).
- C9 use_yes_price risk; sub-cent on all paths; render ordering; validate on
  assembled books: PASS (Minors) — misread risk confirmed: parser maps no_dollars_fp
  prices as-is to yes_asks; under no-leg pricing every no-side price would be wrong by
  100-p with no runtime defense; ledgered (research fixture items #20/#24, GAPS.md:146
  — which mis-cites #20; use_yes_price is item #24). Sub-cent: snapshot level and
  delta price tested; TRADE price path verified by execution (scratch: "0.365" ->
  rejected with sub-cent remainder error) but untested in the committed suite (Minor).
  Render: bids descending via `.rev()` on BTreeMap, asks ascending — tested
  (`the_assembler_folds_snapshot_and_deltas_into_a_canonical_book`).
  OrderBook::validate is NOT called by BookAssembler::render, but IS called at every
  wired sink (paper apply_book lib.rs:171, sim.rs:275, kalshi adapter.rs:409) — the
  stream->paper seam fails closed on invalid books.
- C10 Polymarket research + GAPS honesty: PASS — research.md (829 lines) +
  raw/ archive (asyncapi-style schemas, pages, web-sources.md) exist on disk. GAPS.md
  rewrite is honest: OPERATOR DECISION REQUIRED, retail API has no client order id
  (breaks crash-resubmission idempotency), sub-cent ticks LIVE vs integer-cents core,
  no retail sandbox, shelving argued. No decision simulated.
- C11 `cargo fmt --check`: PASS (exit 0).
- C12 `cargo clippy --workspace --all-targets -- -D warnings`: PASS (exit 0).
- C13 full workspace suite: PASS — 647 passed, 0 failed, 0 ignored.
- C14 invariants crate untouched: PASS — `git diff cc3bde0...d1cafe1 --
  crates/fortuna-invariants/` empty.

## Findings

- [Major] Kalshi WS parser rejects documented one-sided `orderbook_snapshot` frames:
  `parse_levels` errors when `yes_dollars_fp`/`no_dollars_fp` is ABSENT, which the
  archived AsyncAPI explicitly documents for empty sides (asyncapi.yaml ~2630-2655,
  "This key will not exist if there are no Yes offers"). Reproduction (executed,
  scratch /tmp/fortuna-scratch-review): snapshot with only `yes_dollars_fp` ->
  `ERR: invalid: book side is not an array`. Fail direction is CLOSED (no book, no
  trading) but stream ingestion functionally breaks on any one-sided market — the
  habitat of mech_extremes — and the divergence is unledgered while the module claims
  "Nothing here invents wire behavior". Fix shape: absent key == empty side, + test.
- [Minor] BookAssembler overdraw error corrupts state: `entry(price).or_insert(0)`
  inserts a phantom 0-qty level BEFORE the overdraw check; after the error the next
  successful apply renders it as a top-of-book level. Reproduction (executed):
  overdraw at 99 -> Err; next delta -> rendered best bid `price=$0.99 qty=0`.
  Mitigated: every wired consumer validates (paper apply_book rejects "non-positive
  quantity") and the mandated snapshot resync replaces state — fail-closed today.
  Must be ledgered; recommend purging the entry on the error path or making apply
  errors sticky per market.
- [Minor] SeqGap persistence (a second delta after a gap also reports the gap) is
  correct by code and by executed scratch check, but no committed test pins it — the
  doc comment is the only guard against regression.
- [Minor] Sub-cent TRADE price rejection is correct by executed scratch check but
  untested in the committed suite (snapshot/delta paths are tested; trade is not).
  Non-positive trade count rejection is likewise untested.
- [Minor] Negative snapshot-level quantities parse successfully
  (`parse_count_integral` rejects fractional but not negative; executed: "-300.00" ->
  Contracts(-300)) and the assembler silently drops them (`qty > 0` filter) instead
  of refusing a malformed frame — silent swallow vs the fail-loud doctrine.
- [Minor] Runner mid-group abort with staged reservations is untested: the only gate-
  rejection test (`thin_edge_below_gate_floor_is_rejected_by_the_gates`) rejects on
  the FIRST leg, so the `group_rejected` release loop never runs with `staged`
  non-empty. The new all-or-nothing branch that releases already-reserved earlier
  legs has zero coverage.
- [Minor] GAPS.md:146 cites "fixture #20" for use_yes_price confirmation; the
  research checklist numbers that item #24 (#20 is the REST `no_dollars` leg
  question). Substantively ledgered, citation off.

## Commands run (verbatim results, trimmed)

- `cargo fmt --check` -> exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` -> exit 0
- `cargo test --workspace` -> TOTAL PASSED: 647; no failing result lines
- `cargo test -p fortuna-exec --test manager` -> ok. 24 passed; 0 failed
  (incl. concurrent::working_order_collisions_refuse_the_leg_without_touching_the_venue,
  concurrent::a_rejected_leg_keeps_its_slot_and_its_siblings_ack,
  concurrent::group_legs_place_concurrently_and_outcomes_keep_leg_order)
- `cargo test -p fortuna-venues --test stream` -> ok. 6 passed; 0 failed
- `cargo test -p fortuna-paper --test paper` -> ok. 12 passed; 0 failed
- `SETTLE_DST_SCENARIOS=300 cargo test -p fortuna-runner --test settlement_dst`
  -> master seed 1781145237489 -> 300 scenario(s) ... ok. 1 passed
- `SYNTH_DST_SCENARIOS=300 cargo test -p fortuna-runner --test synthesis_dst`
  -> master seed 1781145344826 -> 300 scenario(s); 831 orders, 1295 proposals ... ok
- `bash scripts/run-dst.sh` (N=2000, set -euo pipefail) -> completed; final stage
  settlement_dst master seed 1781146374749 -> 2000 scenario(s) ... ok. 1 passed
- Scratch repros (/tmp/fortuna-scratch-review, executed): REPRO1 one-sided snapshot
  ERR; REPRO2 phantom best bid $0.99 qty=0 after overdraw; REPRO3 second delta after
  gap -> SeqGap expected=11 got=13; REPRO4 trade price "0.365" rejected; REPRO5
  snapshot qty "-300.00" parsed to -300 then silently dropped.
- Sweeps: no unwrap/expect/panic/todo in new non-test code; no
  SystemTime/Instant/Utc::now in the diff; no deleted asserts, #[ignore], or proptest
  reductions diff-wide; `unreachable!("handled above")` at manager.rs:943 predates the
  batch (introduced ee96f14, ancestor of base); BTreeMap-only in new state paths; no
  secrets in new code/config.

## Verdict rationale

Every executed regression criterion is green and the concurrent-legs work survives
its highest-risk adversarial probe (same-key legs in one group) with committed-test
proof. The single Major is in the d1cafe1 deliverable itself: a reproduced divergence
between the WS parser and its own cited archive on a routine frame shape, unledgered.
Per the severity taxonomy (Major = BLOCK unless operator waives), the verdict is
BLOCK. The fix is small (absent fp key == empty side, plus a test) and nothing else
in the batch needs to move; alternatively the operator may waive until the fixture
recording session, in which case the remaining items land as ACCEPT-WITH-GAPS with
the six Minors ledgered in GAPS.md.
