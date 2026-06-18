# Review: perps-b0-b1-fixtures-gate — 2026-06-11

Base: 825d144  Head: 3e0d34f (9 commits; brief said 10 — `git rev-list --count` = 9)
Verdict: **BLOCK**
Protected crate touched: **no** (`git diff 825d144..3e0d34f -- crates/fortuna-invariants/` = 0 bytes)

Reviewer: FORTUNA verifier (independent context; implementer rationale not read).
Rubric fixed before opening the diff: task brief claims 1–5, docs/design/kinetics-perps-module-plan.md
(header + §6 verbatim + amendments A/B/C), BUILD_PLAN T5.B0/T5.B1, research.md §Uncertainties (27 items),
CLAUDE.md conventions, fortuna-review mechanical checklist.

## Criteria (fixed before reading the diff)

### C1 — Kalshi fixture session (bab1437)
- C1a 60 captures / 27-item coverage: **PASS with Minor F4** — session__manifest.meta.json has exactly 60
  result rows; 59 metas + 57 bodies + 2 ws jsonl on disk; items #26/#27 honestly deferred (README Known
  gaps + GAPS); #11 (STP `maker` mode) never exercised, #20 capture vacuous (empty book), unledgered (F4).
- C1b README load-bearing findings vs fixture bodies+metas:
  - Nested error envelope ("every 4xx", "flat does not occur"): **FAIL** — F2. 17/19 4xx nested; 2 flat
    counterexamples in-set.
  - 409 `order_already_exists`: PASS — orders__duplicate_client_order_id.json body code string + meta 409.
  - Reuse-after-cancel 409: PASS — same client_order_id 4a75a66e… chain: create 201 (manifest row 29,
    order 2597b999) → cancel 200 (row 32) → re-cancel 404 (row 34, proves canceled) → reuse 409 (row 35).
  - Cancel-nonresting 404: PASS — all three bodies `{"error":{"code":"not_found",…}}`, metas 404.
  - Skew window ">5s and <30s": PASS — metas: −5s/+5s → 200; −30s/−5min/+5min → 401
    `header_timestamp_expired`. Exactly the five skews checklist #2 demanded.
  - Taker fee quadratic ×0.07 ceil-against-us: PASS — fills__after_taker: price 0.5200 fee_cost "0.017500"
    = ceil₄(0.07·0.52·0.48 = 0.017472); second independent fill: 0.9900 → "0.000700" = ceil₄(0.000693).
  - Cursor empty-string-on-last-page: PASS — fills__after_taker and markets__single_filter_lastpage both
    carry `"cursor":""` (key present, empty).
  - Numeric-fields 400: PASS on status (meta 400, request_body shows numeric count/price) — but the body
    is one of F2's flat counterexamples.
  - post_only 400-at-create: PASS — 400 `invalid_order` / details "post only cross"; docs-divergence
    (201-then-cancel) honestly noted in README #6 and GAPS.
  - WS claims (#13): PASS — both metas: 101, use_yes_price true/false, duration_secs 90, pings_observed 8.
  - STP self-cross (#12): PASS — fill_count "0.00" + remaining "0.00"; resting order status "resting" after.
  - Units (#10): PASS — balance 9801 int cents + "98.0186" 4dp string; fee_cost 6dp string; count_fp strings.
  - Garbage cursor silent-200 / limit 1001 hard-400 (#8): PASS — bodies verified (5 markets, no error key;
    400 `{"msg":…}`).
  - Cancel-reconcile (#15): **FAIL** — F3: the captured evidence contradicts the session narrative and is
    nowhere surfaced.
- C1c recorder tool (crates/fortuna-venues/examples/record_kalshi_fixtures.rs — predates this batch; reviewed
  per brief): **PASS** — demo hosts hardcoded (lines 34–36: external-api.demo.kalshi.co, demo-api.kalshi.co,
  external-api-ws.demo.kalshi.co); reads only KALSHI_API_DEMO_KEY_ID / KALSHI_DEMO_PRIVATE_KEY_PATH (lines
  333–335); meta schema is {auth, environment, host, method, note, path, recorded_at_epoch_ms, request_body,
  status} — no headers, no key material (verified across all metas read).
- C1d gaps ledgered: **PARTIAL** — settlement/voided/series-fee/prod-parity/maintenance-window ledgered in
  README Known gaps + GAPS; F3 and F4 items not ledgered.
- C1e secrets in fixtures/: **PASS** — sweep clean (only benign "token cost"/"signature path" notes).

### C2 — B0 perishable recorder (7b00ce6, T5.B0)
- C2a tests pin load-bearing behavior: **PASS** — 8/8 unit tests executed green this session; ordering
  independence pinned by the verbatim live capture (asks descend, bids ascend — best levels at array TAILS,
  defeating positional parsing); parser refusals (negatives, 5dp, garbage, malformed levels); zero-qty levels
  excluded from best; one-sided/empty book → None; day_dir exact UTC-midnight rollover (independently
  recomputed: 1781136000000 ms = 2026-06-11T00:00:00Z).
- C2b no f64 price math: **PASS** — i64 ten-thousandths with checked ops; only f64 mention is the doc comment.
- C2c amendment A scope: **PASS** — perp books + derived spreads, bracket quotes paired by shared cycle_id
  per sweep (pairing confirmed in live data: same cycle_id 1781160747296 across streams), per-ticker intraday
  funding estimates (+ marks, + risk_parameters hourly), standalone crate, JSONL day files.
- C2d running + data hygiene at HEAD: **PASS** — pid 79813 (`--interval-secs 30 --bracket-series
  KXBTC15M,KXBTC,KXBTCD`), output files current to the minute; `.gitignore` line 8 `data/`;
  `git ls-files data/` = 0. History transit in ad89942 is F5.
- C2e conventions: **PASS with Minor F6** — no unwrap/expect/panic outside tests; wall-clock usage
  documented in-code but not in ASSUMPTIONS.md; unused chrono dep.

### C3 — B1 spec v0.9 (eb189cc; this verdict gates the T5.B1 tick)
- C3a version line 0.9 / June 11 / change summary: **PASS**.
- C3b 5.15 fidelity to operator directives (plan §6 verbatim + amendments): **PASS** — all items present and
  faithful: InstrumentKind {BinaryEvent, Perp} threaded through Market/positions/gates with dispatch-on-kind
  rule; PerpPrice integer TEN-THOUSANDTHS venue-scoped, checked arithmetic, Decimal at payload boundaries,
  Cents-conversion only at notional/PnL/fee boundaries rounding against us, "type-level separation, not
  convention"; perps carry NO settlement lifecycle (5.13 excluded) — margin/maintenance state + mark feed +
  funding accruals instead; margin ACCOUNT as exposure unit, DEDICATED 5.14 envelope never fungible
  intra-month, worst case = LIQUIDATION loss never premium; unpublished MM formula → recorded-risk-curve
  approximation + safety multiplier + REFUSE unboundable (fail closed); liquidation-distance floor +
  per-asset leverage caps + funding drag + notional caps, "none consults the model (I1/I6)";
  order_source=system liquidation fills → dedicated lifecycle state + mandatory alert + halt evaluation,
  never silently absorbed; kill-switch perps flatten with OWN credential pair, reduce_only IOC + cancel-all,
  no Postgres/cognition dependency (I4); fee-trap rule with 5–12 bps schema examples, Sim re-run on fee
  activation, promo-$0 never justifies promotion (amendment C); funding_carry data-only ≥60d (amendment B);
  demo/prod divergence discipline incl. target-environment runtime reads; I2 drawdown extended to funding +
  margin unrealized PnL at conservative settlement mark, worse-for-us governs.
- C3c 5.2 fee corrections: **PASS** — cites research ("researched 2026-06-09/11, docs/research/venue/");
  corrects-not-erases ("Superseded 0.8 text claimed 'Intl mostly zero' and 'US flat 10bp taker' — corrected,
  not erased"); Kalshi quadratic marked fixture-confirmed (matches C1b fee evidence); perps notional-fraction
  fees added with fee-trap cross-reference.
- C3d zero changes to the invariant middle: **PASS** — spec diff is exactly 3 hunks (+24/−2): version line,
  5.2 paragraph, 5.15 insertion. No Section 3 content in the diff; protected crate diff 0 bytes.
- C3e T5.B1 unticked pending this gate: **PASS** (BUILD_PLAN line 333 `[ ]`).

### C4 — Env-leak fix (ad89942)
- C4a `[env] DATABASE_URL` dev default, force=false documented: **PASS** — .cargo/config.toml explains the
  dotenvy fallback, the 42501 discovery, and the force=false override semantics ("fails closed at boot
  anyway, because its OTHER secrets are absent").
- C4b protected crate untouched: **PASS** (0-byte diff across whole batch).
- C4c environmental-only: **PASS** for the fix (no code/test changes) — commit also accidentally carried
  data/ runtime files (F5).
- C4d T4.1-kickoff daemon corollary: **PASS** (kickoff lines 106–112).
- C4e proven by execution: **PASS** — this review shell had DATABASE_URL unset (env | grep -c = 0) and
  i5_audit_append_only passed, so the cargo [env] default was the operative source.

### C5 — Bookkeeping
- Operator sign-off verbatim: PASS (GAPS line 93: `verbatim "I sign off"`).
- Polymarket-after-perps decision: PASS (GAPS lines 282–284, quoted with [sic]).
- Perps Phase A DONE: PASS (GAPS line 290). Spec-maintenance RESOLVED: PASS (GAPS lines 316–324, with the
  in-session "Proceed with B1" quote). BUILD_PLAN T5.B0–B8 + amendments A/B/C: PASS (lines 304–355).
- signal-contract.md design-only: PASS (135-line doc; `grep -rn prob_claims crates/` = no hits).
- GAPS internal consistency: **PARTIAL** — F7 staleness.

### C6 — Mechanical (all executed this session)
- fmt --check: PASS (EXIT=0). clippy --workspace --all-targets -D warnings: PASS (EXIT=0, 4m16s).
- cargo test --workspace: PASS — 91 suites, 665 passed, 0 failed, 0 ignored, EXIT=0. (Brief expected ~712;
  did not reproduce; zero failures regardless.)
- scripts/run-dst.sh 200: PASS — core: "0 corpus + 200 random seeds, zero invariant violations" (corpus
  empty since T0.4; batch diff on dst-corpus/ = 0 files — pre-existing, not weakening); synthesis_dst 200
  scenarios ok (593 orders, 930 proposals, 2889 cognition failures); settlement_dst 200 scenarios ok
  (11 arms, 37 discrepancies, 49 watchdog rows, 31 halts); EXIT=0 under set -euo pipefail.
- Test-weakening sweep: PASS — no modified test files in diff; no `#[ignore]`/deleted asserts/case
  reductions/loosened tolerances (diff-wide grep clean).
- place()/GatedOrder sweep: PASS — batch adds no venue/exec code; recorder is GET-only.
- Secrets sweep: **FAIL — F1 (Critical)**.

## Findings

1. **[Critical] Two PEM RSA private keys committed to the repository; one is the production AND kill-switch
   Kalshi credential.** `.keys/fortuna-demo-v1.txt` and `.keys/fortuna-key.txt` (both `file`-identified as
   "PEM RSA private key") were added in 7b00ce6 (B0 commit) and are tracked at HEAD (`git ls-files .keys/`).
   The operator's .env maps `KALSHI_PRIVATE_KEY_PATH` AND `FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH` to
   `…/fortuna-key.txt`, and `KALSHI_DEMO_PRIVATE_KEY_PATH` to `…/fortuna-demo-v1.txt`. Root cause, evidenced
   by `git log -p -- .gitignore`: 7b00ce6 appended `data/` to a `.gitignore` whose last line `.keys/**` had
   no trailing newline, producing the corrupt pattern `.keys/**data/` — which ignores neither `.keys/` nor
   `data/`. The pattern is STILL corrupt at HEAD (line 7); 3e0d34f added `data/` as line 8 but never restored
   `.keys/**`. Violates CLAUDE.md ("secrets only via env vars, never in the repo") and undermines I4
   credential separation (kill-switch key in git history). Remediation is operator-scope: rotate BOTH keys
   (treat as compromised), purge `.keys/` from history, restore `.keys/**` with trailing newline.
   — reproduction: `git ls-files .keys/`; `file .keys/*`; `git log -p 825d144..3e0d34f -- .gitignore`;
   `grep KEY_PATH .env` (paths only).

2. **[Major] README load-bearing finding #1 (and GAPS.md) contradicted by its own fixture set.** Claim:
   nested envelope in "every 4xx capture"; "the OpenAPI's FLAT ErrorResponse does not occur on the wire";
   GAPS repeats "everywhere"/"never occurs". Evidence: envelope scan of all 19 4xx captures → 17 nested,
   2 flat: `orders__numeric_field_types.json` = `{"code":"bad_request","message":"bad request","details":
   "json: cannot unmarshal number into Go struct field …"}` (the flat OpenAPI shape, on the wire) and
   `markets__limit_over_max.json` = `{"msg":"Parameter validation failed …"}` (a third shape). An adapter
   built to the README's universal claim mis-handles two error classes it will certainly hit (JSON-decode
   400s, parameter-validation 400s). Fix is textual: scope the finding and enumerate all three observed
   shapes. — reproduction: python envelope scan over `fixtures/kalshi/*.meta.json` status>=400 (output in
   review transcript); fixture bodies quoted above.

3. **[Major] Unledgered load-bearing wire divergence: order GET returns stale state after cancel-ack.**
   Chain (same order 2597b999): DELETE at epoch-ms 1781159364059 → 200 `{"reduced_by":"1.00","ts_ms":
   1781159364112}`; GET at 1781159364471 (~360 ms after ack) → `"status":"resting"`, `"remaining_count_fp":
   "1.00"`, last_update_time unchanged from creation; second DELETE → 404 not_found (the cancel surface
   knows the order is dead while the read surface says resting). This is precisely checklist #15's
   "cancel-reconcile race", which GAPS itself lists among "highest-stakes items" — the session CAPTURED the
   divergence and then surfaced it nowhere: not in README findings, not in Known gaps, not in GAPS. An
   adapter reconciling via GET immediately after cancel-ack will mis-model order state. — reproduction:
   `orders__cancel_v2.{json,meta.json}`, `orders__get_after_cancel.{json,meta.json}`,
   `orders__cancel_already_canceled.meta.json` (timestamps and bodies quoted).

4. **[Minor] Checklist-coverage overstatements, unledgered.** (a) #11 requires BOTH
   `self_trade_prevention_type` modes (enum `taker_at_cross` | `maker`, research.md line 374); every order
   in the session used `taker_at_cross` — `maker` mode unobserved. (b) #20 (no-leg pricing of REST orderbook
   levels): `orderbook__base.json` is `{"orderbook":{}}` — an empty book confirms nothing. (c) #17 sub-items
   (cursor stability across inserts; expired cursor) uncaptured. None of (a)–(c) appear in README Known gaps
   or GAPS. "27-item checklist covered" should read "covered except…".

5. **[Minor] Repo hygiene.** ad89942 committed ~2.4k lines of runtime capture (data/*.jsonl,
   data/recorder.log) — consequence of the F1 .gitignore corruption; self-corrected (576d826 untracks,
   3e0d34f ignores), clean at HEAD, public-endpoint bodies only (no secrets). `.playwright-mcp/console-…log`
   (14 KB browser console, zero secret-pattern hits) committed as litter and still tracked.

6. **[Minor] Recorder wall-clock deviation not ledgered.** fortuna-recorder/src/main.rs calls
   `SystemTime::now()` (lines 38–43) and `Instant::now()` (line 177) against CLAUDE.md's blanket "all time
   comes from the injected Clock; SystemTime::now() anywhere outside the Clock impls is a defect". The
   deviation is well-justified in-code ("live capture timestamps ARE the data"; lib half is deterministic
   and tested) but Definition-of-done #4 required an ASSUMPTIONS.md entry — none exists (grep clean). Also:
   `chrono` is an unused dependency in the crate manifest (lib.rs hand-rolls civil-from-days instead).

7. **[Minor] GAPS.md internal staleness.** The Kinetics section still records "Phase B: PROPOSED, awaiting
   operator confirmation … Nothing builds before confirmation" while the plan doc header and BUILD_PLAN
   record Phase B CONFIRMED 2026-06-11 (and B0 is built and running). One ledger, two states.

## What survives this BLOCK (for the re-submission)

- eb189cc (spec v0.9) passed every fidelity criterion (C3a–C3e) — the amendment itself needs no rework;
  the T5.B1 tick is gated only by batch-level F1–F3 remediation.
- The B0 crate content (C2a–C2c) and the env fix (C4) passed on their own merits.
- The fixture BODIES are honest verbatim wire truth throughout — F2/F3 are defects of the summary layer
  (README/GAPS text), not of the recordings; no re-recording is required for them.

## Commands run (verbatim results, trimmed to verdict lines)

```
cargo fmt --check                                          → EXIT=0
cargo clippy --workspace --all-targets -- -D warnings      → Finished `dev` profile … in 4m 16s; EXIT=0
cargo test --workspace                                     → 91 suites: 665 passed; 0 failed; 0 ignored; EXIT=0
  (incl. test i5_audit_append_only ... ok — with DATABASE_URL unset in the review shell)
  (incl. fortuna_recorder unittests: running 8 tests … 8 ok)
scripts/run-dst.sh 200                                     → EXIT=0
  [dst] regression corpus: 0 seed(s)
  [dst] OK: 0 corpus + 200 random seeds, zero invariant violations
  [synthesis-dst] master seed 1781166041612 -> 200 scenario(s) … test result: ok. 1 passed
  [settlement-dst] master seed 1781166049371 -> 200 scenario(s) … test result: ok. 1 passed
git diff 825d144..3e0d34f -- crates/fortuna-invariants/    → empty (0 bytes)
git ls-files .keys/                                        → fortuna-demo-v1.txt, fortuna-key.txt
file .keys/*                                               → PEM RSA private key (both)
ps aux | grep fortuna-recorder                             → pid 79813 RUNNING (interval 30s, 3 series)
git ls-files data/ | wc -l                                 → 0
```
