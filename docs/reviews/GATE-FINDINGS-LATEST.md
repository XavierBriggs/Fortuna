# GATE FINDINGS — latest (verifier-owned; every track reads this at priority (a))

State as of 2026-06-13, main @ e85f92c. Main is GREEN (fmt/clippy/
workspace-tests/run-dst all clean per completion-audit-2026-06-13.md).
A BLOCK naming your track preempts your queue. This file is the single
coordination surface; the verifier rewrites it — tracks ACT on it and
ledger their responses in GAPS, never edit this file.

## CAMPAIGN STATE (completion-audit-2026-06-13.md is authoritative)

- Phases 0–3 + T4.1 daemon (SOAK: GO) + T4.4 CLI + T4.3 ROTA (R12 PASSED):
  DONE, gated, on main.
- Docs set landed (3b52bf0); docs gate BLOCK -> ACCEPT (re-gate addendum
  in 2026-06-12-docs-gate.md; pg_dump fix executed clean this session).
- BUILD_PLAN T4.5 entry restored + Phase-5 EXIT written (e85f92c) — both
  had been lost to merge-revert churn.

## TRACK E — DESIGN critique done: ACCEPT-WITH-CONDITIONS; AWAITING OPERATOR BUILD-APPROVAL

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
window vs track A's in-flight cycle/belief-composition work. OPERATOR DECISION: approve
to build (with the 3 conditions folded into the design) — per track-E brief §3 this is
the design-gate STOP; build does not start until you approve.

## TRACK A — completion campaign (queue: docs/design/track-a-completion-queue.md)

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
prob_claims/v1 proving vehicle; perp_event_basis is buildable-but-fixture-gated. The basis
MODEL (worker's Section 3) is still to be presented — gate it on arrival.

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

## TRACK D — MERGED to main (2476554; SSRF-fixed news crate D1-D5; post-merge build green). Branch building forward UNMERGED toward D9: D8 Layer-2 corroboration (near-dup clustering, 6526106) + a live_smoke diagnostic / "AFD-firehose" telemetry finding (80fcc1d) landed this session; D7 GdeltSource deferred (honest external rate-limit). D9 LANDED on track-d (6b52f98 "ingestion scheduler — the validator-wired ingest core") — THE HARD GATE, GATING NOW (pre-merge). The gate cannot pass without REPRODUCTION-OF-REFUSAL on the WIRED path: a shape-drifted/poisoned item from a pinned host must refuse-and-quarantine live (Layer-1 was BUILT-but-UNWIRED at the merge; D9 is where it gets call sites). Verify the scheduler actually invokes the validator on every ingested item + the drive() seam stays the single flagged fortuna-live commit. None of D6-D8 crossed the live-ingest line; D9 does. Phase A partial (2/4 adapters). WATCH: an "AFD-firehose" volume/telemetry finding may bear on the Aeolus/NWS cost-budget design — surface it as a GAPS/bus note if cross-track. Scope: PARK slot F / track M per operator.

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

## DISK — MACHINE CONSTRAINT (operator action; the re-gate hit 120Mi mid-run)
Five build trees (A/C/D/E targets ~9-13GB each) + gate caches + 5GB perishable
exceed sustainable headroom on this disk; gates have ENOSPC'd twice. The
verifier reclaims aggressively (done/idle track targets, gate caches) but this
is structural. Operator options: free machine-wide space; or reduce concurrent
tracks; or approve a periodic `cargo clean` of idle-track targets. Track-B and
idle-track-D targets reclaimed this session.

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
