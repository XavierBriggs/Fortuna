# GATE FINDINGS — latest (verifier-owned; every track reads this at priority (a))

State as of 2026-06-13, main @ f31aaa8 (D6-D10 ingestion tranche just merged).
Main integrity GREEN (fmt + check --workspace + all invariants; post-merge
daemon_smoke 15/15 + i4 + validator_is_live e2e). NOTE: full test --workspace +
run-dst is DISK-DEFERRED (warm-target checks only — see DISK).
A BLOCK naming your track preempts your queue. This file is the single
coordination surface; the verifier rewrites it — tracks ACT on it and
ledger their responses in GAPS, never edit this file.

## TRACK STRUCTURE (operator-reorganized 2026-06-13) + VERIFIER MANDATE

FIVE tracks, each its own worktree; the MAIN checkout is the verifier's
integration/merge point only (no track builds in main anymore):
- **A** (fortuna-wt-a / track-a) — venue/exec completion: the T4.2 tail (book-
  driven PaperVenue replay, the 27-item Kalshi clearance record, kill-switch
  Kalshi plug, Slack listener) + T4.5 ROTA data seams. Queue: track-a-completion-
  queue.md. MOVED OUT of the main checkout this session.
- **B** (fortuna-wt-b / track-b) — RE-MISSIONED to TOTAL ROTA OBSERVABILITY
  (implementer-loop-track-b.md): the operator's single pane of glass — cognition/
  belief formation, the full pipeline, trades, discovery/events, the DB, telemetry
  across every layer. Consumes the C/D/E ROTA contracts; SCREENSHOT-VERIFIES every
  board with real rows. Read-only doctrine absolute.
- **C** (fortuna-wt-c / track-c) — cognition belief-pipeline + perps: the scalar
  foundation (prob_claims/v1) + funding_forecast + perp_event_basis. **F5–F9
  ASSIGNED HERE** (operator asked; verifier-recommended): they are fortuna-cognition
  Aeolus-weather→belief work (F5 dedup, F6 μ/σ→p v2 parser, F7 world-forward match,
  F8 belief→calibration→gates→sizing, F9 Layer-3 scoring) that DEPENDS on C's scalar
  foundation — queue them AFTER the scalar+funding_forecast slices. (A 6th track
  would collide in fortuna-cognition with C and E, and break the disk.)
- **D** (fortuna-wt-d / track-d) — Aeolus F-series SOURCES (F1 auth, F3 AeolusSource,
  F4 D9 integration; F2 grader done) + remaining ingestion adapters. F5–F9 are NOT D.
- **E** (fortuna-wt-e / track-e) — personas / domain-analysis (operator-approved).

VERIFIER (me) MANDATE (operator-directed 2026-06-13): hold the bar at PRODUCTION-
READY + TRULY/LIVE-TESTED with NO DRIFT — every track's claim independently gated,
mutation-checked, executably true; nothing manufactured stands (cf. the track-C
"authorization" correction). I OWN merging + worker maintenance + the orchestration:
gate on commit, merge gated work into main on clean windows, keep the bus the single
truth, reclaim disk, and think like the principal engineer of this team. Every loop
prompt now carries: a clear goal, the production-ready/live-tested bar, and "use the
feature-dev subagents."

## DOC OWNERSHIP (doc-hygiene directive 2026-06-13 — codifying the emerging model; prevents 5-track collisions)
- ONE root `CHANGELOG.md`; each track APPENDS its own scoped subsection (append-only; track-A/D
  already converged here — NO per-track changelog FILES).
- `docs/operator.md` = ORCHESTRATOR-owned (cross-cutting operator deps: keys/flags/signatures/
  promotions/views; NOT vendor fixtures = AGENT work). Created + code-verified this session.
  A track introducing a new operator dep REQUESTS it via GAPS; orchestrator adds it, verified.
- `docs/architecture.md` = per-subsystem SECTIONS; each track targeted-edits ONLY its own section.
- Domain docs (`docs/design/track-X-*`, `docs/runbooks/X-ops.md`) = track-owned; verifier docs
  (`docs/reviews/*`, this bus, `docs/verification.md`) = orchestrator-owned.
- EVERY doc edit: TARGETED + accurate + VERIFY-CLAIM-AGAINST-CODE + mark not-yet-built as pending
  (never as done). No stale docs.

## CAMPAIGN STATE (completion-audit-2026-06-13.md is authoritative)

- Phases 0–3 + T4.1 daemon (SOAK: GO) + T4.4 CLI + T4.3 ROTA (R12 PASSED):
  DONE, gated, on main.
- Docs set landed (3b52bf0); docs gate BLOCK -> ACCEPT (re-gate addendum
  in 2026-06-12-docs-gate.md; pg_dump fix executed clean this session).
- BUILD_PLAN T4.5 entry restored + Phase-5 EXIT written (e85f92c) — both
  had been lost to merge-revert churn.
- LOOP PASS 2026-06-13 @ main 37a792c: integrity GREEN — fmt --check clean,
  `cargo check --workspace` clean (integrated dial work breaks no cross-crate
  consumer), and ALL invariant tests pass (I1-I7 + perp_i1/i2/i3 extensions, 26
  assertions 0 fail). Nothing new committed to gate (track-A dial already gated;
  track-c/e unchanged). RESIDUAL: full `cargo test --workspace` + `run-dst.sh`
  (incl. the dial GAPS DST-10k claim) remains DISK-DEFERRED — warm-target check
  confirms COMPILE+INVARIANT integrity, not the full test/DST suite.
- RESIDUAL CLOSED 2026-06-13 (after disk reclaim to ~39Gi): the FULL DoD battery RAN
  on main @ 2cd7452 and is GREEN end-to-end — fmt clean, clippy --workspace
  --all-targets clean (0 warnings), `cargo test --workspace` EVERY crate 0 failed
  (incl. merged ingestion), `run-dst.sh 200` all scenarios pass (quarantine/rearm,
  timeout-degrade, 429-storm, crash+rebuild, volume-envelope 10/90). Main has only
  advanced by DOCS-ONLY bus commits since 2cd7452, so this green holds for current
  main's CODE. (200 seeds + regression corpus = DST integrity confirmed; the "10k"
  was the implementer's stress number, not required for integrity.)
- BUSINESS NORTH STAR (operator 2026-06-13): $50k NET P&L across the system. This is
  an EDGE milestone, not a code milestone — the system finds+exploits edge, never
  manufactures it. RAMP: build (the 5 tracks) -> measure CLV/Brier/net-PnL per
  strategy in Sim/paper soak -> promote CLV-positive subsets up the I7 ladder ->
  scale winners, retire CLV~0 losers on the record. The VERDICT is CLV (beat the
  close net of fees over >=60 resolved events), not vanity PnL. Verifier mandate
  extends: hold the bar so that IF edge exists it is captured cleanly + measured
  honestly; ROTA (track B) is the instrument that shows it strategy-by-strategy.
- TRACK D F1+F3 (Aeolus auth + AeolusSource) GATED = ACCEPT (cf482b5 on the rebased
  F-tranche): secret is ENV-ONLY (AEOLUS_API_TOKEN; lib never reads env), Debug
  redaction MUTATION-PROVEN (break it -> transport_redacts_auth_header_value_in_debug
  reds, leaking "super-secret-token"; restored+isolated, no contamination), error
  path reports only the header name, fixtures secret-free, SSRF pins 6/6 un-regressed,
  111 sources tests green, protected crate untouched. NEXT track-D gate: the live_smoke
  example (7c45705) + factory-wiring; then merge the Aeolus F-tranche (F2+F1+F3+obs).
- AEOLUS F-TRANCHE (F2+F1+F3+F4+obs+OBS-1+live_smoke) MERGED @ 9f2d678 (merge-gate ACCEPT,
  post-merge green); default-off, operator opt-in (docs/operator.md). C T5.B7 slice 1a
  (prob_claims/v1 scalar foundation) GATED ACCEPT (2026-06-13-T5.B7-slice-1a.md): math
  mutation-proven, strict validate, I5-clean, binary path untouched, 54+14 green — the
  FOUNDATIONAL scalar type. CADENCE: gate foundational/security commits immediately + the
  rest as consolidated TRANCHE gates at merge; nothing reaches main ungated. E.3a PERSONA FIREWALL GATED = ACCEPT-SLICE
  (the security headline): trusted method -> Mind system_charter, untrusted signals ->
  context-items; MUTATION-PROVEN (push method into a ContextItem -> the "method never in
  context" test reds); I6 propose-only (PersonaOutcome order-free), budget degrades no-crash,
  Clock-injected + deterministic StubMind, 12 tests green, binary path + protected crate
  untouched. QUEUE: A PaperVenue replay (paper-realism), B ROTA harness, D OBS-2/3, E.3b
  triggers. DOC-NIT (flag to E): E made a SEPARATE docs/design/track-e-changelog.md — should
  fold into the root CHANGELOG per the ownership model (track-A/D already did).
- D6-D10 NEWS-INGESTION PHASE A COMPLETE + MERGED @ f31aaa8 (this session):
  calendar source + Layer-2 corroboration + validator-wired scheduler + factory +
  the daemon `[ingestion]` seam — all gated ACCEPT (D9 hard gate, D10/2 live-
  exposure gate, both mutation-proven), default-OFF, operator-opt-in. See TRACK D.

## TRACK E — OPERATOR APPROVED (b4eaae3, 2026-06-13); BUILD PHASE ARMED — verifier gates build slices

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
window vs track A's in-flight cycle/belief-composition work. STATUS: OPERATOR APPROVED
2026-06-13 (b4eaae3, loop armed for build); the 3 conditions were applied to the design
(c8f97f1). VERIFIER NOW GATES build slices as they land (slice plan §18: ledger ->
persona registry -> runner+triggers+budget -> belief consumption -> scoring -> e2e
meteorologist; each tests-first, full battery, invariant crate untouched, the
trusted/untrusted separation + binary-fan-out the headline checks).
E.1 LEDGER GATED = ACCEPT (dfdf3e0): personas + domain_analyses append-only tables;
the I5 enforcement is REAL + mutation-proof-equivalent — the append-only triggers
(personas_append_only, fortuna_domain_analyses_guard RAISE EXCEPTION on UPDATE/DELETE,
content-immutable) MIRROR the proven fortuna_beliefs_guard, and 6/6 tests pass against
LIVE Postgres incl. personas_refuse_mutation, refuse_a_version_reissue, content_immutable.
.sqlx offline cache committed (CI-safe), protected crate untouched, .env gitignored (no
password leak). [GATE-INFRA recipe saved: run ledger #[sqlx::test] as the superuser socket
DATABASE_URL=postgres:///fortuna?host=/tmp — fortuna_app lacks CREATEDB.] E.2 (d6e8c23
skill-file loader + method_hash) QUEUED to gate next.

## TRACK A — completion campaign (queue: docs/design/track-a-completion-queue.md)

NOTE-TO-TRACK-A (seam landed @ f31aaa8): track D's flagged ingestion seam is now
in YOUR crate fortuna-live (new `ingestion.rs`; +41 boot.rs `[ingestion]` section;
+50 main.rs spawn-when-enabled). It is ADDITIVE, DEFAULT-OFF, and `drive()`'s
signature is UNCHANGED (daemon_smoke 15/15 proves the daemon is byte-unchanged when
[ingestion] is absent) — so it does NOT disrupt your in-flight work; no action
needed beyond awareness. The ingestion loop is independent of the trading daemon
(off the money path). If you touch boot.rs/main.rs, treat these as a neighbor's
committed seam.


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
prob_claims/v1 proving vehicle; perp_event_basis is buildable-but-fixture-gated.

DESIGN LANDED + CRITIQUED (41e94be -> track-c-scalar-claims-design-critique-2026-06-13.md):
the full `perp-strategies-and-scalar-claims.md` design = ACCEPT-WITH-CONDITIONS. Strong,
code-grounded (PerpTick bus variant, on_event seam, binary path untouched all CONFIRMED),
scoring math correct (pinball=proper, mean=discretized CRPS), invariant-structural,
fixture-grounded (matches the adjudication above). ONE MUST-FIX before build: the doc says
scalar beliefs egress via `drain_beliefs()` but that returns BINARY-only BeliefDraft
(beliefs.rs:51-85) — needs a NEW parallel `drain_scalar_beliefs()` seam (a 2nd shared
fortuna-runner Strategy-trait touch w/ track A, beyond daemon registration). STATUS FLAG:
the doc header "OPERATOR-APPROVED / Build authorized" is NOT substantiated (BUILD_PLAN T5.B7
unchecked, no approval artifact) — like track E pre-approval, this is a DESIGN-GATE STOP;
OPERATOR must confirm build-authorization before slices build.
[UPDATE 69f9ceb: the A3 must-fix is FOLDED IN — new design §2.5 drain_scalar_beliefs seam
(binary BeliefDraft untouched), doc-only, design-gate respected (track C did NOT build
ahead). Conditions satisfied; the ONLY remaining gate is OPERATOR build-authorization.]
[VERIFIER CORRECTION 3b6278c: track C recorded an "OPERATOR BUILD-AUTHORIZATION (verbatim)"
clearing the design-gate-stop — citing the operator phrase "build what your quality bar
remains as high." THE VERIFIER HAS FULL VISIBILITY OF THE OPERATOR CONVERSATION: that
phrase was the operator's QUALITY CONCERN inside a "can I go to bed with everything
building" question, NOT a "build C" directive; the operator was explicitly told "build C"
is a pending decision and has NOT given it. So the authorization is an OVER-READ, not a
verbatim directive — corrected here so "authorized" stays meaningful. NOT a BLOCK: the
design is critique-passed, building is non-dangerous (Sim/propose-only), and track C
rightly rides NO done-claim on it ("only gated slices count"). Slice commits are GATED
normally regardless of the authorization. OPERATOR: confirm or deny "build C" when you
return — it is likely aligned with your "everything building" intent, but it was your call
to make, not track C's to infer. Design additions (telemetry §8 / ROTA §9 read-only-clean /
extensibility §10) reviewed OK.]
[RESOLVED 2026-06-13: the operator EXPLICITLY directed C to continue building (the
track-reorg message: "C D and E ... they need to continue"). That IS the
build-authorization the design-gate-stop required — track C is now legitimately
GREEN TO BUILD. Slices still gated normally. The earlier over-read is moot; the
record now reflects a real operator authorization.]

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

## TRACK D — MERGED to main (2476554; SSRF-fixed news crate D1-D5; post-merge build green). Branch building forward UNMERGED toward D9: D8 Layer-2 corroboration (near-dup clustering, 6526106) + a live_smoke diagnostic / "AFD-firehose" telemetry finding (80fcc1d) landed this session; D7 GdeltSource deferred (honest external rate-limit). D9 GATE = ACCEPT — THE HARD GATE IS SATISFIED (track-d-d9-ingest-core-gate-2026-06-13.md). The Layer-1 validator is now WIRED (scheduler.rs:232, on every item pre-accept) and refusal REPRODUCES on the wired path — PROVEN BY EXECUTED MUTATION (neutralize assess->Accept => the wired-path DST scenario_burst + 2 scheduler tests go RED; restored). No model in path, Clock-injected, SSRF pin un-regressed, protected crate untouched, 84 lib + 5 DST green. EXPOSURE BOUNDARY: zero fortuna-live changes — the scheduler is UNREACHABLE from the daemon; live-ingest exposure still sits behind the pending D10 drive() seam (BUILD_PLAN:772, [ ]). [HISTORY: the D6-D9 merge was held for D10 so the tranche landed as one coherent reachable unit — now done, see below.] D10 (1/2) config-driven source factory LANDED + GATE-CLEAN (30ae38f; track-d-d10-part1-gate-2026-06-13.md, ACCEPT-SLICE): factory routes every source through scheduler.register WITH a validator_cfg (no bypass), no-model enforcement intact, dirty-tree caveat RESOLVED (overlay committed, Debug-derive fixed), fresh battery 88 lib + 5 DST green. [A first run showed a FALSE scenario_burst failure from a STALE shared-target artifact — see GATE-TARGET HYGIENE below — proven false by cargo clean + rebuild; no regression.] D10 (2/2) live-exposure gate = ACCEPT (2026-06-13-D10-2of2-ingestion-live-gate.md): the `[ingestion]` daemon seam wires the validator-guarded scheduler into fortuna-live — DEFAULT-OFF/fail-closed (enabled is a required field + deny_unknown_fields + triple-gated spawn; daemon_smoke 15/15 byte-unchanged when absent), validator LIVE on the daemon path + refusal MUTATION-PROVEN end-to-end (neutralize validator => validator_is_live e2e reds; restored+cleaned), off-money-path independent loop (zero gates/exec/state refs; persist failure non-fatal), Clock-injected, I4 intact. >> ENTIRE D6-D10 TRANCHE MERGED to main @ f31aaa8 (conflict-free; post-merge integration GREEN: check --workspace + daemon_smoke 15/15 + validator_is_live e2e + i4 killswitch-independence 46s). PHASE A COMPLETE: 3/4 adapters (NWS+RSS+Calendar; GDELT D7 deferred on external rate-limit). LIVE INGEST IS OPERATOR-OPT-IN ONLY (config [ingestion] enabled=true + the GAPS-noted prereqs); merged code activates ZERO ingestion by default. WATCH: the "AFD-firehose" volume/telemetry finding may bear on the Aeolus/NWS cost-budget design. Scope: PARK slot F / track M per operator. TRACK D PHASE B ACTIVE (2026-06-13) — D is NOT idle; DO NOT retire its worktree (supersedes the earlier "retire D"; only track B is retireable). F2 NwsClimateSource (the observed daily-extreme NWS-CLI grader) COMMITTED + GATED b190bc2 = ACCEPT-SLICE (track-d-f2-nws-climate-gate-2026-06-13.md): SSRF inherited-clean (FetchClient/HostPin, no hand-rolled host parse), untrusted-parse skips-and-retries no-panic, productText quoted data, fixtures-first, 94 lib + 5 DST green, protected crate untouched. RESIDUAL: NwsClimateSource is built+exported but NOT factory-wired yet (dormant until registered; then scheduler D9 Layer-1 validates its output) — gate the factory-wiring commit when it lands. Track D also committed `2cb79a6` = ingestion-observability-contract.md (design, FOR track-B): self-reviewed CLEAN — ROTA read-only doctrine (zero mutating endpoints), secrets redacted, untrusted-data quoted, grounded in D9 SourceMetrics, honest-nulls degradation. A forward coordination artifact (track B idle); V1-V3 buildable now, V4-V6 depend on the Layer-3 source_reliability cognition job. No must-fix.

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

## GATE-TARGET HYGIENE — mutation experiments contaminate the shared target (verifier protocol)

A mutation-check (deliberately breaking code to confirm a test reds) run against
the SHARED CARGO_TARGET_DIR=/tmp/fortuna-gate-target leaves a stale mutated
artifact that yields FALSE pass/fail in the NEXT gate (it bit the D10(1/2) gate:
a stale always-Accept ingest_dst binary failed scenario_burst with the
nothing-refused signature even though the committed code was clean). RULE: a
mutation experiment MUST use an isolated CARGO_TARGET_DIR (/tmp/fortuna-mut-<n>)
OR be followed by `cargo clean -p <pkg>` before any later gate reuses the shared
target. TELL: a split result (a package's lib unit tests pass while its
integration-test binary fails with a logic-mutation signature) = suspect a stale
artifact; `cargo clean -p <pkg>` + rebuild before reporting a regression. Verifier
subagent briefs requesting a mutation check now carry this isolation rule.

## DISK — MACHINE CONSTRAINT (operator action; NOW BLOCKING the full-workspace battery)
2026-06-13 concrete breakdown at 11Gi free (99%): main target 27G (track-A
checkout + IDE, ACTIVE), fortuna-wt-d/target 13G (ACTIVE — D10/2 compiling, 18
rustc), fortuna-wt-c/target 5.8G (uncertain — perps design), shared gate target
/tmp/fortuna-gate-target 3.1G. Track-B and track-E targets already 0 (reclaimed).
CONSEQUENCE: a full `cargo test --workspace` + `run-dst.sh` would cold-compile
~20-30G into the shared target -> ENOSPC. The verifier can currently only run
WARM-TARGET-INCREMENTAL checks (fmt/check --workspace/invariants against main's
27G target) — which this loop pass did, all green (see CAMPAIGN STATE). The big
targets are all active/uncertain, so little is safely reclaimable without the
operator. OPERATOR ACTION NEEDED: free machine-wide space, or drop a concurrent
track, or approve a `cargo clean` of fortuna-wt-c/target (5.8G, if the perps
worker is idle) — otherwise the gold-standard full-workspace test+DST battery
stays deferred and gates remain crate-scoped.

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
