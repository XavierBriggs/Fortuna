# Track C changelog — cognition belief-pipeline + perps

One entry per landed slice, newest first. The design doc
[perp-strategies-and-scalar-claims.md](perp-strategies-and-scalar-claims.md) holds the
"why"; this file is the "what shipped, when, how it gated, and which shared docs moved".
Commits are referenced by subject (track-c rebases onto main each iteration, so hashes
shift until merge — `git log` has the live hash).

---

## 2026-06-13 — T5.B7 slice 1b: scalar-belief storage (`fortuna-ledger`)

- **Shipped.** Migration `20260613000001_scalar_beliefs.sql` — two append-only tables:
  `scalar_beliefs` (the immutable `prob_claims/v1` claim: `producer` first-class,
  `event_key` free-form, `quantiles` JSONB, `unit`, `horizon`, `provenance`, plus
  one-time `realized_value`/`resolved_at`) and `belief_scores` (derived, rule-tagged
  `(belief_id, rule_id) → score`, `UNIQUE` per rule, FK to `scalar_beliefs`). Guards:
  a fine-grained `fortuna_scalar_beliefs_guard` (refuses DELETE + content mutation,
  allows resolution columns once-from-NULL) and the blunt `fortuna_refuse_mutation` on
  the fully-immutable `belief_scores`. `ScalarBeliefsRepo` + `BeliefScoresRepo` in
  `repos.rs` (insert/get/resolve-exactly-once/recent; insert/scores_for_belief/_for_rule).
- **Additive.** Binary `beliefs` path byte-unchanged; only added exports + new files.
- **Gate.** Full battery green (fmt, clippy `--workspace -D warnings`, `test --workspace`,
  `run-dst.sh`); 7 `#[sqlx::test]` live-PG tests (exactly-once resolve, guard refusals,
  unique-per-rule, FK orphan-refusal). `feature-dev:code-reviewer` ACCEPT; the FK +
  rewrite-assertion + no-op-UPDATE-doc must-fixes folded in.
- **Shared docs.** `architecture.md` §3 — `scalar_beliefs`/`belief_scores` added to the
  `fortuna-ledger` table list. (No new runbook yet — the sqlx migrate/prepare flow is
  noted in the loop doc; a perps ops runbook lands with slice 4.)

## 2026-06-13 — T5.B7 slice 1a: scalar belief type + swappable scoring (`fortuna-cognition`)

- **Shipped.** `scoring.rs` — `PredictiveDistribution {Binary,Categorical,Scalar}` +
  `RealizedOutcome`, the swappable `ScoringRule` trait, `BrierRule` + `CrpsPinballRule`
  (native CRPS = mean pinball), `ScoreError`, full `validate()`. 54 tests incl. a
  proper-scoring proptest.
- **Additive.** Binary `BeliefDraft`/`brier_score` path untouched (only `pub mod scoring;`).
- **Gate.** Battery green; `feature-dev:code-reviewer` ACCEPT (K=1-identity doc + boundary
  test fixes folded in).
- **Shared docs.** `architecture.md` §3 — swappable scoring layer added to the
  `fortuna-cognition` entry.

## 2026-06-13 — Design pass: perp strategies + `prob_claims/v1` scalar claims + basis model

- **Shipped.** `perp-strategies-and-scalar-claims.md` — §1 scalar beliefs/scoring, §2 the
  perp-strategy runtime seam (`PerpTick`, `drain_scalar_beliefs`), §3 the basis model,
  §8 telemetry, §9 ROTA view contracts (for track B), §10 extensibility.
- **Gate.** Adversarial DESIGN critique = ACCEPT-WITH-CONDITIONS; the A3 egress-seam
  must-fix + watch-items folded in. Operator-authorized build (bus 82d32c8); F5–F9
  (Aeolus weather→belief) added to track-C scope (orchestration reorg 7fa4115).

## 2026-06-13 — T5.B7 foundation: funding-forecast kernel (`fortuna-core`)

- **Shipped.** `perp.rs` — `FundingWindow` (running TWAP of recorded premiums,
  equal-weight mean, premium-as-input never re-derived) + `finalize_funding_rate`
  (±2 % clamp, 0.01 % zero threshold) + `FUNDING_*` constants. 13 spec-first tests.
- **Gate.** Battery green. The deterministic baseline `funding_forecast` (slice 2) wraps.
