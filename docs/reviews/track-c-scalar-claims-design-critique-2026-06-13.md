# Track C — scalar-claims + perp-strategy design: adversarial DESIGN critique

Date: 2026-06-13. Target: `docs/design/perp-strategies-and-scalar-claims.md`
(295 lines) on track-c @ 41e94be. Design-first (doc is the artifact, no new code
in this commit). Read-only critique; rubric fixed before reading. This is the
FOUNDATIONAL prob_claims/v1 design — it unblocks B7 (funding_forecast), Aeolus
weather quantiles, and track-E personas, so flaws are expensive.

## VERDICT: ACCEPT-WITH-CONDITIONS — strong, code-grounded; ONE must-fix before build

Every structural claim that could be checked against real source held up, and
several choices are stronger than the spec requires. There is one concrete
must-fix (a mis-named egress seam) + watch-items; none is a structural flaw
needing redesign. The operator may approve to build after the egress-seam
correction is folded in. (Operator approval is a DESIGN-GATE STOP — see F.)

## The one MUST-FIX: the scalar-belief egress seam (finding A3)

The doc (line 150) says funding_forecast "emits a scalar PredictiveDistribution
via `drain_beliefs()`." But `drain_beliefs()` returns `Vec<BeliefDraft>`, and
`BeliefDraft` (fortuna-cognition/src/beliefs.rs:51-85) is `deny_unknown_fields`
with a REQUIRED `p: f64` validated strictly in (0,1) — it is BINARY-ONLY. A
scalar `PredictiveDistribution` cannot flow through it. The design's own
constraint (binary path untouched, no track-A collision) therefore FORCES a NEW
additive seam: a new Strategy-trait method (e.g. `drain_scalar_beliefs() ->
Vec<ScalarBeliefDraft>`) + a parallel runner buffer + a parallel daemon persist
into `scalar_beliefs`/`belief_scores`. Achievable additively, but the doc
mis-names the load-bearing §1 mechanism; fix it before slice 1 so the build does
not either collide with track A's `BeliefDraft` or stall. (Consequence noted in E:
this new method is a SECOND shared touch on fortuna-runner/src/lib.rs's Strategy
trait, beyond the daemon registration — coordinate with track A on both.)

## Rubric (A–F), evidence before verdict

**A. Code-reality grounding — CONFIRMED (one mis-named seam).** (1) `EventPayload`
genuinely invites a typed `PerpTick` (bus.rs:64-85, the verbatim "typed variants
added by the tasks that own them"). (2) Strategy reads events via `on_event`
(fortuna-runner/src/lib.rs:185-189, invoked runner.rs:695) — no CoreHandle
surgery. (3) `drain_beliefs()` exists (lib.rs:199) BUT is binary-only → the A3
must-fix. (4) BINARY PATH UNTOUCHED — `BeliefRow`/`BeliefsRepo` (repos.rs:910-1157)
are entirely binary; the parallel `scalar_beliefs`+`belief_scores` touch none of
it; no breaking change to track A. (5) the append-only INSERT-only migration
pattern (20260609000001_initial.sql:79-99) supports the new tables.
**B. Scalar type + scoring soundness — CONFIRMED (genuinely strong).**
`PredictiveDistribution{Binary|Categorical|Scalar{quantiles,unit}}` + dual
`RealizedOutcome` is replay-stable. CrpsPinballRule math is CORRECT: pinball
`q(y−v) if y≥v else (1−q)(v−y)` is the proper check loss (minimized at the true
q-quantile); the mean over levels is the textbook discrete CRPS (∫₀¹2·pinball_q dq).
Validation rules (q strictly incr in (0,1); v non-decreasing = no crossing; ≥2
quantiles; finiteness; bin-sum tol) are exactly a coherent quantile function.
The `(belief_id,rule_id)→score` separation over immutable facts is I5-safe and
STRONGER than the binary path's inline `brier`. Minor: the "1-quantile orders like
Brier" wording is loose (it's scaled absolute error around the point, not squared
error) — cosmetic.
**C. Invariant fit — CONFIRMED STRUCTURAL.** I6: `ProposedLeg` (lib.rs:142-152)
carries NO qty/size; harness sizes (runner.rs:807-882); funding_forecast proposes
nothing, perp_event_basis proposes unsized bracket legs. I7: runner rejects
non-Sim (RunnerError::StageViolation, runner.rs:369-377); the fee-trap gate on GO
lands as a test obligation (the right place). I5/I1: append-only audited scalar
writes; scalar-informed bracket orders take the normal Proposal→gates path
(runner.rs:907-978), no bypass.
**D. Fixture grounding + never-invent — CONFIRMED.** PerpTick fields verbatim in
fixtures/kinetics-perps/ws__public_orderbook_ticker.jsonl + funding__rates_estimate
(KXBTCPERP1); funding input = recorded estimate authoritative + mark−reference
proxy LABELED secondary (matches the FundingWindow kernel perp.rs:295-377 + the
unpublished-formula discipline); perp_event_basis e2e stays fixture-gated on a
SAMPLED paired-cycle, ships kernel-unit-tested-only until then, never invents the
(uncommitted) KXBTC15M bracket surface. Faithfully incorporates the verifier's
prior fixture adjudication.
**E. Cross-cutting ownership — CONFIRMED with RISK.** Operator-granted expanded
scope (cognition+ledger+runner+core) answering the GAPS RALPH STOP (GAPS.md:21-160).
Additive (new files), file-level coordination, 4-step sequence each gate-clean.
RISK/WATCH: the daemon-composition registration AND the new scalar drain method
(A3) are TWO shared touch-points on track-A-adjacent files (fortuna-live daemon +
fortuna-runner Strategy trait) — respect "do not rewrite track A's runner files"
under concurrent edits.
**F. Status integrity — FLAGGED for operator.** The header "OPERATOR-APPROVED,
Build authorized" is partially self-evidencing (cites the operator's design
directive, supersedes signal-contract.md §2/§5, the GAPS RALPH STOP asked the
operator to grant exactly this scope) but NOT fully substantiated: no linkable
approval artifact, and BUILD_PLAN T5.B7 is still UNCHECKED with only the kernel
(507b1ad) done. Like track E pre-b4eaae3, this is at the DESIGN-GATE STOP —
reconcile the build-authorization with the operator before building slices 2-4.

## Watch during build
- The A3 scalar drain method = a 2nd shared touch on fortuna-runner Strategy trait
  (beyond daemon registration); coordinate with track A on both.
- New scalar_beliefs/belief_scores MUST carry the append-only trigger + exactly-once
  scalar resolution (mirror resolve_and_score repos.rs:1056) — the I5 detail.
- perp_event_basis e2e stays unit-tested-only until the operator/recorder samples a
  KXBTC15M paired-cycle fixture; never claim e2e validation on synthetic alone.
- Fix the "orders like Brier" wording.

## Stronger than required (honest credit)
- The durable-facts / derived-score separation is more replay-robust + auditable
  than the existing inline-brier binary path, and makes scorers backtestable side
  by side — a real I5 upgrade, not parity.
- Quantile/pinball CRPS over a Gaussian assumption is the correct distribution-free
  choice and a proper scoring rule as written.
- The never-invent discipline (estimate authoritative, proxy labeled, KXBTC15M never
  invented) faithfully extends the kernel + venue-behavior rules.

Protected crate untouched; no cargo build run (read-only mandate honored).
