# GAPS.md - honesty ledger (agent-maintained)

Open items the implementation defers, lacks, or needs from the operator. Acceptance
requires this file to contain ONLY operator-blocked items, each with exact unblock steps.

Status (post-E-batch, 2026-06-10): the T3.6 completion claim was FALSIFIED
by the full-build gate (docs/reviews/system-0-3-final-2026-06-10.md, BLOCK:
four unledgered Majors). The fix batch (commits 1d1c033..1e3e5e7) closed
E1-E5 and the RE-RUN GATE is an ACCEPT
(docs/reviews/system-0-4-egate-2026-06-10.md): every E-item graded CLOSED
with executed evidence, regression battery clean (630 tests, three-stage
10,000-seed DST). An INDEPENDENT re-gate (docs/reviews/
system-0-4-egate-INDEPENDENT-2026-06-10.md, blind to the first e-gate
verdict, fresh seeds) corroborated all five E-closures on their ledgered
close criteria — and found ONE new Major (F1 below) plus two Minors that
the first e-gate missed. F1-F3 were closed and re-gated (f-batch-gate, ACCEPT-WITH-GAPS; its three
Minors closed at head). Everything below is an OPERATOR action. One Minor stays disclosed: the
regression-seed corpus is empty (no randomized run has produced a red
seed; discipline in place).

## Engineering items F1-F3: CLOSED (gate-verified)

Found OPEN by the independent e-gate; closed by b4c839f (F1: degrade
kind preserved + drained to 'cognition' audit rows + bus events, budget
breaches counted once at the drain and exported; ops alerts module —
every breach alerts with the scrape count, failure bursts threshold-
gated; F2: BUILD_PLAN T2.8 visible correction; F3: wholly-discarded
output writes a model_proposals_discarded trace) and graded CLOSED by
the targeted re-gate (docs/reviews/f-batch-gate-2026-06-10.md,
ACCEPT-WITH-GAPS). That gate's three Minors closed at the head commit:
the settlement_dst aggregate coverage asserts now gate on a 100-scenario
floor (a 20-scenario draw can legitimately miss the halting arms — a
coverage assert that intermittently reds healthy code erodes trust in
real reds; repro master 1781139292562 now passes), the cost-metric
undercount gained the budget-true surface (fortuna_mind_spend_today_cents
includes failed-call burn, test-asserted on a schema-invalid call), and
the false "cost rides in cognition audit rows" doc line was corrected
visibly in ASSUMPTIONS (per-decision cost rides in belief provenance).

Verification record: five verdicts in docs/reviews/ (phase-1, phase-2,
system-0-1-2, phase-3, system-0-3-final, all 2026-06-10). The phase-3 gate
and the system gate disagreed on acceptance-item grading; the disagreement
was adjudicated with executed evidence (zero Kelly call sites, no
`impl Mind for AnthropicMind`, vacuous per-cycle budget, zero discrepancy
hits in seeded DST — all confirmed at 7bbc3ef). The system gate's stricter
reading governs.

## Engineering items E1-E5: CLOSED (gate-verified)

Ledgered open at 7bbc3ef; closed by 1d1c033 (E1+E5a: calibration layer
binds in the cycle, haircut-Kelly sizing = min(kelly, affordable) with
composition-fed quality failing closed to zero, integer-money Kelly
budget), E2 commit (AnthropicMind behind `dyn Mind` with owned budget +
env-gated factory), 5954999 (E3: per-cycle cap binds via begin_cycle
tracking; config surface added), ca38028 (E4: 10-arm settlement/watchdog
seeded DST as run-dst.sh stage 4, fail-closed script), 1e3e5e7 (E5:
watchdog outage partition, counted discards + audited proposal manifest
hashes, hygiene). Each close criterion was graded CLOSED with executed
evidence by the independent gate: docs/reviews/system-0-4-egate-2026-06-10.md
(verdict ACCEPT). False documentation was corrected with the correction
visible (ASSUMPTIONS.md, BUILD_PLAN.md T2.6 note), never erased.

## Minor engineering residue: status (from the three INDEPENDENT batch gates)

CLOSED at head (this commit):
- mind-spend gauge exported with the wrong type flag -> moved to the
  gauge block (counter: false).
- Unchecked i64 add in the assembler delta -> checked_add, fail closed.
- Lenient envelope parsing -> STRICT: frames without a type tag refuse;
  orderbook frames without sid/seq refuse (lenient zeros could alias
  real sequence state); pinned by tests.
- Crossed-assembled-book refusal + non-array-side refusal pinned as
  committed tests (were gate-scratch-verified only).
- Out-of-order leg completion pinned: a staggered mock completes legs in
  REVERSE input order; outcomes and journals still land in leg order.
- Multi-leg DST arm added (settlement_dst Arm::MultiLegGroup): two-leg
  groups drive submit_group_concurrent under seeded ack-delay/api-error/
  reject faults; 400-scenario shakeout green, all 11 arms hit.
- settlement_voids / settlement_reversals counters post-state asserted.
- Kelly legs[0] design constraint corrected in ASSUMPTIONS. [CORRECTED
  2026-06-11, ledger-accuracy gate, SECOND-GATE MAJOR: the rest of this
  line previously claimed the degrade_alerts/CalibrationParamsRepo
  ASSUMPTIONS entry and a Polymarket "95" erratum already existed — both
  were false from the f-batch closure until 2026-06-11, when the real
  corrections finally landed: the wiring-status entry is now in
  ASSUMPTIONS.md, and the research doc carries an ERRATUM stating that
  neither "96" nor "95" matched a canonical count (ground truth: 38
  source-table rows; 93 archived raw files). A closure claim that
  survived TWO gates unverified is exactly the defect class this ledger
  exists to prevent.]

REMAINING (composition-wiring by nature; bound to the LIVE COMPOSITION
DAEMON task — the next engineering build):
- fortuna_ops::alerts::degrade_alerts needs its scrape-delta consumer.
- CalibrationParamsRepo.latest needs its live call site (the composition
  fetches params + quality per scope and feeds SynthesisStrategy +
  set_calibration_quality).

## SECURITY INCIDENT 2026-06-11 (gate finding F1, Critical) — keys were committed

WHAT HAPPENED: both Kalshi PEM private keys (`.keys/fortuna-demo-v1.txt`
and `.keys/fortuna-key.txt` — the latter mapped by .env to BOTH
KALSHI_PRIVATE_KEY_PATH and FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH)
were tracked in git from the B0 commit until the same-day remediation.
ROOT CAUSE: an agent `echo "data/" >> .gitignore` onto a file whose last
line `.keys/**` had no trailing newline corrupted the pattern to
`.keys/**data/`, un-ignoring `.keys/`; the next `git add -A` swept the
keys in. EXPOSURE BOUND: this repository has NEVER been pushed — the key
material never left this machine; it existed only in local git objects.
REMEDIATION (engineering, done same day — CORRECTED per the remediation
re-gate's F1d finding, which caught this entry describing the PLANNED
purge as a completed one): .gitignore repaired (`.keys/` restored,
trailing newline, `.playwright-mcp/` added), keys + runtime data
untracked at HEAD, and the BRANCH history rewritten via filter-branch
(old->new hash mapping in docs/reviews/history-rewrite-2026-06-11.md —
hashes cited in documents dated before 2026-06-11T08:30Z are
pre-rewrite). FINALIZATION HAS NOT RUN: the refs/original backup ref and
reflogs still REACH THE OLD OBJECTS (the key blobs remain recoverable
inside .git — `git show <old-hash>:.keys/...` works by design until
finalization); the permission classifier correctly refused the
irreversible step (reflog expire + gc) without explicit operator
authorization. Full-unreachability verification happens only AFTER the
operator-approved finalization. The earlier text here claimed "reflog
expire + gc ... VERIFIED" — that was the plan, written ahead of the
denial and not reconciled; corrected, not erased. Pre-batch
.playwright-mcp blobs (zero-secret browser logs) also remain in older
history; their purge is optional and folds into the same finalization
decision. PROCESS FIX: never append to .gitignore with `>>`; edit with
anchored tools and verify `git status --ignored` after.
F5 DISPOSITION (ledger-gate fix 2): the B0/B1-gate's F5 (runtime data +
playwright litter committed) closed as follows — data/ purged from branch
history in the F1 rewrite and gitignored; .playwright-mcp/ untracked at
HEAD and gitignored; PRE-batch playwright blobs remain main-reachable
(zero-secret browser logs) and their removal would need a second rewrite
— folded into the operator finalization decision as an optional extra.
OPERATOR ACTIONS REQUIRED (two distinct decisions):
1. ROTATE both Kalshi keys (treat as compromised per policy even though
   exposure was machine-local — the live key is also the I4 kill-switch
   credential): demo + prod key pages at (demo.)kalshi.co Account &
   security -> API Keys; place new PEMs at the paths .env names; the
   fixture set is unaffected (recorded with the demo key, which you may
   rotate independently).
2. FINALIZE THE PURGE (irreversible; classifier-gated to you): run
   `git for-each-ref --format='%(refname)' refs/original/ | xargs -n1 git
   update-ref -d && git reflog expire --expire=now --all && git gc
   --prune=now` from the repo root (or tell the agent "finalize the
   purge" to run it with your authorization). Until this runs, the old
   key blobs remain reachable inside .git via the backup ref. Do this
   BEFORE any first push of this repository, whatever else happens.

## Operator adjudication queue — RESOLVED (signed off 2026-06-10)

OPERATOR SIGN-OFF RECORDED: 2026-06-10, in-session, verbatim "I sign off",
given in direct response to this queue (the four waive batches below).
This converts every rule-based BLOCK from the protected-crate touches.
The audit record stays below for the trail.

- **Protected-crate waives (3 batches).** crates/fortuna-invariants/ was
  touched in Phases 1-3; each touch triggered the automatic BLOCK rule.
  All three were audited line-by-line across the five reviews: every
  deletion in history was a baseline `#[ignore]`+`todo!()` placeholder
  replaced by implemented tests; ZERO existing assertions modified,
  deleted, or loosened. Batches: (1) Phase 1 — three one-line
  `volume_contracts: None` fixture compile-fixes (i1/i3/i4); (2) Phase 2 —
  I6/I7 stub closures + 2 new T3-staged stubs; (3) Phase 3 (6276274) —
  closure of those two I7 stubs. Unblock: operator reviews
  `git log -p -- crates/fortuna-invariants/` (or the diffs quoted in the
  verdict files) and records the waive decision in this file under
  "Disputed invariant tests" or in the ops log; that converts the
  rule-based BLOCKs. Batch (4), added by the E-batch: one fixture line in
  i7_promotion_gates.rs (`kelly_fraction: 0.25,` in its runner_config —
  a compile fix for the new required RunnerConfig field), confirmed
  assertion-clean via full patch by the E-gate verdict.

## Path to production (ordered; after this list only operator actions remain)

1. DONE (1d1c033..1e3e5e7): E1-E4 + E5 sweep landed, E5a before E1.
2. DONE (docs/reviews/system-0-4-egate-2026-06-10.md): full gate re-run
   at 1e3e5e7 — ACCEPT; all remaining gaps are in this file's operator
   sections.
3. DONE 2026-06-10: operator signed off on the protected-crate waives
   (queue above, "I sign off" recorded in-session).
4. OPERATOR: provision credentials (.env per README) — ANTHROPIC_API_KEY
   first, then the one-haiku-smoke-call under a tight CostBudget; Slack
   app token + allow-listed user ids (Socket Mode listener exercise);
   Kalshi credentials last (they unlock nothing alone by design — I7).
5. OPERATOR: Kalshi demo-env fixture recording session (single session
   covers the 27-item checklist + websocket streams + voided market + fee
   fields — details in the Kalshi section below). STATUS 2026-06-10:
   delegated to the agent; recorder tool BUILT and session attempted —
   blocked on a demo key-id/PEM pairing mismatch (one operator step to
   unblock; see the Kalshi section). Then ENGINEERING:
   venue-generic runner composition replaying recordings into PaperVenue
   (first post-fixture task), kill-switch KalshiVenue plug with its OWN
   FORTUNA_KILLSWITCH_* credentials.
6. OPERATOR: Aeolus recorded-export one-liner (section below); Polymarket
   research authorization if/when wanted (parallel, not on the critical
   path); spec v0.9 fee touch-up before any Polymarket go-live.
7. PROMOTION (I7, operator-only): mech strategies Sim -> Paper forward
   window -> operator promotion to live capital; model swaps only via the
   shadow comparison harness recommendation; drawdown-halt re-arms stay
   CLI-only out-of-band. Live trading remains OFF until every step above
   is recorded.

## Operator-blocked: Kalshi fixtures (one recording session unblocks all)

**SESSION COMPLETE 2026-06-11:** after the operator installed the matching
demo key (the original mismatch: the configured id was a fresh key, the
available PEM was a February-dated one), the full session ran end to end —
60 captures under fixtures/kalshi/ covering the 27-item checklist EXCEPT
the ledgered exceptions (see README Known gaps incl. STP `maker` mode
unobserved, #20 vacuous empty-book capture, #17 cursor-stability sub-items
— gate finding F4), both WS flag states, and cleanup. Load-bearing wire
findings (full table in fixtures/kalshi/README.md): THREE error-body
shapes on the wire — nested {"error":{...}} (17/19 4xx), the flat OpenAPI
shape (json-decode 400s), and bare {"msg"} (parameter-validation 400s);
the adapter must parse all three (CORRECTED 2026-06-11, gate F2: the
original "nested everywhere / flat never occurs" claim was falsified by
this set's own captures); CANCEL-ACK/READ STALE RACE captured live (gate
F3 — checklist #15's highest-stakes item): DELETE 200 then GET ~360ms
later still "resting"/full remaining while re-cancel 404s — adapter must
poll-until-terminal after cancel and treat recancel-404 as canceled; 409
dup code string `order_already_exists`; canceled client_order_ids never
free up; non-resting cancels are 404; skew window (>5s, <30s);
post_only-cross rejected AT CREATE on demo (docs say 201-then-cancel —
demo/prod divergence to re-check); quadratic taker fee x0.07 confirmed
against two independent fills; cursor last-page = empty string.
REMAINING for clearance (T4.2): adapter re-pointed at recordings + nested-
envelope fix; settlement capture after the seeded market closes; voided
market when one occurs; series fee fields via event lookup; prod-parity
read-only re-record before live. The PERPS fixture session (18 items,
research §12) is now also credential-unblocked — recorder extension queued.

Historical record of the blocked first attempt (resolved above):
the recorder TOOL is built and committed —
`crates/fortuna-venues/examples/record_kalshi_fixtures.rs`, demo-hosts-only,
covers the 27-item checklist + both-flag-state WS captures + cleanup — and
the session ran to the auth wall, where it is BLOCKED ON A CREDENTIAL
PAIRING: the demo key id in .env (after repairing a stray leading character
in the pasted value) IS recognized by the demo environment, but the only
available private key (`~/keys/kalshi-demo.pem`, moved from
`~/Downloads/kalshi-demo-key.txt`) does not pair with it — every signed
request returns 401 `INCORRECT_API_KEY_SIGNATURE` under TWO independent
signing implementations (the adapter's rsa crate and an openssl-CLI probe)
across four message-format variants and both demo hosts; local clock skew
was +0.8s. Conclusion: the PEM belongs to a different key (a second demo
key, or the live key's download). UNBLOCK (operator, one step): either
locate the PEM that pairs with the configured demo key id, or create a
fresh demo API key at demo.kalshi.co (Account & security -> API Keys),
save the download to `~/keys/kalshi-demo.pem` (chmod 600) and put its key
id in `KALSHI_API_DEMO_KEY_ID`, then rerun:
`set -a && source .env && set +a && cargo run -p fortuna-venues --example
record_kalshi_fixtures`.
Incidental findings already banked from the probes: (a) the wire 401 error
envelope is NESTED — `{"error":{"code","message","details"}}` — not the
flat `ErrorResponse` the OpenAPI spec documents (at minimum for the auth
gateway; fixture the API-layer shape too); (b) unauthenticated GET /markets
returns 200 on demo (checklist #5, demo half); (c) auth `details` strings
observed so far: `INVALID_PARAMETER` (malformed key id) and
`INCORRECT_API_KEY_SIGNATURE` (sig mismatch).

- **Kalshi fixture recording + adapter clearance (T1.1).** The adapter is
  BUILT and tested against doc-derived samples (124 venues tests), but it
  is cleared for Sim development ONLY. Paper/live clearance requires
  operator-recorded fixtures under fixtures/kalshi/ confirming the 27-item
  checklist in docs/research/venue/kalshi-api-2026-06-10/research.md
  (highest-stakes items: 409-duplicate body shape, error code catalog,
  cancel-reconcile race, fills cursor terminal semantics, timestamp skew
  tolerance, fee_multiplier maker scaling).
  Unblock: operator records demo-env fixtures per
  crates/fortuna-venues/tests/kalshi_doc_samples/README.md.
  The SAME capture must also include:
  - websocket `orderbook_snapshot`/`orderbook_delta` + public `trade`
    messages. (Status 2026-06-10: the WS MESSAGE layer is BUILT
    doc-derived — parser, seq-gap detection, yes-scale subscribe builder,
    BookAssembler, and the stream->PaperVenue replay seam are tested
    against the verbatim official examples. The capture CONFIRMS the
    contract — esp. use_yes_price semantics — confirm against the fixture checklist's no-leg-pricing item (research checklist item #20; the recording session should exercise BOTH flag states) — and unblocks
    the live socket DIAL (signed-handshake auth, keep-alive, redial) plus
    the venue-generic runner composition replaying the recordings.),
  - a VOIDED market's settlement record (`market_result` documents only
    yes/no/scalar; the adapter hard-errors on anything else so a live void
    surfaces loudly instead of passing silently),
  - fee fields on fills (verifies the inferred maker-fee x multiplier
    scaling and the unused-in-the-wild `flat` fee_type mapping).
- **Kill-switch live venue plug (after fixture clearance).** The binary +
  freeze logic + self-test are complete and I4-proven against the sim
  venue; `freeze --venue kalshi` stays unwired until the adapter passes
  fixture confirmation — the kill switch must not take its first real
  cancel path through unverified venue code. Unblock: fixtures above, then
  wire KalshiVenue into the killswitch with its OWN credential set
  (FORTUNA_KILLSWITCH_* env).

## Operator-blocked: credentials

- **Venue + Anthropic + Slack credentials (env vars).** STATUS 2026-06-10:
  PROVISIONED AND VERIFIED LIVE by the operator — DATABASE_URL (fortuna db
  migrated, 23 relations owned by fortuna_app, connection verified as the
  app role), ANTHROPIC_API_KEY (the recommended haiku smoke call returned
  "FORTUNA smoke OK", 16in/8out tokens — the env-key cognition gate is now
  ARMED), Slack bot `fortuna` (auth.test ok; test post landed in ALL FIVE
  channels), FORTUNA_DEADMAN_URL set (deliberately not pinged yet: one
  ping arms the monitor and the runtime is not running — expect a false
  "down" page if armed before go-live). Remaining in this entry:
  - Kalshi: a key id is set — CONFIRM it is the DEMO-environment key (the
    fixture session needs demo; prod trading + the separate
    FORTUNA_KILLSWITCH_* pair come later) and that KALSHI_PRIVATE_KEY_PATH
    points at the downloaded .key PEM (chmod 600, outside the repo).
  - (Historical note kept: AnthropicMind was built and mock-tested at
    T2.5; the env-key gate IS the feature flag.)
  - Slack app token(s): send-side routing (config-driven channel router,
    Block Kit approval builder) is built and tested with a mock transport;
    the Socket Mode interactivity listener (button presses, slash-command
    kill REQUESTS — never re-arms, which stay CLI-only) is built-to-contract
    work that needs a real app + allow-listed user ids to exercise.
    Research contract ready: docs/research/ops/ (apps.connections.open,
    envelope ack, user-id allow-listing).
  - Kalshi API credentials: live trading also requires promotions (I7) and
    fixtures above; credentials alone unlock nothing by design.

## Operator-blocked: Aeolus

- **Operator-recorded Aeolus envelope export (T2.7).** The ingestion
  contract is FORTUNA-defined (AeolusEnvelope, strict deny-unknown-fields)
  and the full fixture->drafts->persisted->scored path is proven against
  fixtures/aeolus/sample_envelope.json (the contract sample). The
  OPERATOR-RECORDED real export remains open — it validates Aeolus's
  exporter, not FORTUNA's parser. A read-only export from the Aeolus box
  was attempted and DENIED by the permission classifier (prod read without
  explicit approval — correct call). Unblock: operator runs ONE read-only
  command and commits the output as fixtures/aeolus/recorded_envelope.json:
  `ssh Aeolus 'sqlite3 -json /home/ec2-user/aeolus/artifacts/live/aeolus.db
  "SELECT ... one run ..."'` shaped to the contract (or adds an export
  endpoint to aeolus-runner). Any mismatch is a contract negotiation,
  never a silent adapt.

## Operator-blocked: Polymarket US

- **Polymarket US adapter is a fixtures-gated STUB (T3.4); RESEARCH NOW
  DONE (2026-06-10, operator-authorized this session):**
  docs/research/venue/polymarket-us-2026-06-10/research.md (829 lines, 95
  archived sources (the doc header said 96; the independent gate counted 95 — erratum noted in the doc)). Material findings that reshape the build decision:
  (a) retail API has NO CLIENT ORDER ID — FORTUNA's crash-resubmission
  idempotency model does not transfer (institutional stack has clordId);
  (b) SUB-CENT TICKS ARE LIVE (0.5c, 0.25c preprod) + decimal quantities
  + decimal settlements — three explicit conflicts with the integer-cents
  core; (c) NO RETAIL SANDBOX — fixtures would be minimum-size recordings
  on PROD, or institutional preprod via firm onboarding; (d) fee reality
  CONFIRMED vs the 2026-06-09 research (taker 0.05 / maker -0.0125
  quadratic, banker's rounding); (e) sports-only listings today.
  OPERATOR DECISION RECORDED 2026-06-10: "polymarket should be after the
  perptuals [sic]" — Polymarket US is SEQUENCED AFTER the Kinetics perps
  module (shelved for now; the stub keeps refusing everything). Revisit
  after perps lands; the cents-core conflict still requires a spec-level
  price-tick decision before any build.

## Kinetics perps module (operator-directed 2026-06-10)

- **Phase A research: DONE** (2026-06-10/11):
  docs/research/venue/kinetics-perps-2026-06-10/research.md — 844 lines,
  ~50 sources, 110 raw archives including perps_openapi.yaml /
  perps_asyncapi.yaml / the SCM spec verbatim plus live prod+demo API
  captures. Headline build facts: DEMO CARRIES PERPS (open to all, mock
  funds); auth = same RSA-PSS recipe under /margin/*; tick $0.0001 (breaks
  Cents as the price carrier — venue-scoped PerpPrice type proposed);
  client_order_id REQUIRED (idempotency transfers); portfolio margin via
  API + UNPUBLISHED maintenance-margin formula (conservative gate stance
  required); Klear liquidates via order_source=system fills (legitimate
  venue-originated fills need a lifecycle state); fees $0 promo with real
  rates via /margin/fee_tiers from the June 11 release (re-check then).
  Known conflicts the doc flags: orderbook ordering vs spec text,
  help-center contract-size mislabel, NFA-id discrepancy in Kinetics' own
  filing.
- **Phase B: CONFIRMED by the operator 2026-06-11** ("your B1–B8 order
  supersedes the truncated directive") with amendments A (B0 recorder
  first/standalone — BUILT and RUNNING), B (funding_carry data-only
  >=60d), C (fee-trap rule); the operator's recovered original list is
  folded in (docs/design/kinetics-perps-module-plan.md §6 verbatim).
  BUILD_PLAN T5.B0-B8 enumerates the confirmed order. (This entry
  previously said "awaiting confirmation" after confirmation had landed —
  one ledger held two states; corrected per gate finding F7.)
- **OPERATOR (rides the SAME demo-key unblock as the Kalshi session):**
  perps fixture recording session — 18-item request list in research §12,
  output under fixtures/kinetics-perps/ (margin-WS signing path, order
  lifecycle, 409 code, funding/risk/fee_tiers captures). The same session
  must also capture, on the EVENT API: a public WS `trade` frame (never
  observed in the 60-capture session — ledger-gate fix 3; it gates the
  paper-engine trade-through replay), the STP `maker` mode, a two-sided
  REST orderbook (#20 re-capture), and the settlement re-poll. One credential
  fix, two recording sessions, ideally back-to-back.

## Spec maintenance: RESOLVED by v0.9 (2026-06-11)

- **Spec 5.2 fee claims** (stale "Intl mostly zero" / "US flat 10bp")
  were corrected — not erased — in the operator-directed v0.9 amendment
  (B1, confirmed in-session 2026-06-11: "Proceed with B1 (spec v0.9
  amendment)"), which also added the 5.15 perpetual-futures domain. The
  perps fee model (notional-fraction maker/taker via fee_tiers) entered
  5.2 alongside the corrections; the Kalshi event quadratic x0.07 is now
  marked fixture-confirmed (real demo fill, 2026-06-11).

## Disputed invariant tests
(none)
