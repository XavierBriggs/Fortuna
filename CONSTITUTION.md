# FORTUNA CONSTITUTION

> Purpose: state what must always be true, as enforceable invariants, each mapped 1:1 to a named test.
> Holds: the non-negotiable invariants (I1-I7), their perps extensions, the derived structural invariants, the maker-checker rule, the promotion/ramp gates, and the machine-readable invariant-to-test map.
> Excludes: how the system is built (ARCHITECTURE.md), why decisions were made (decisions/), and how to write code (STANDARDS.md). This document is amend-only: invariants are added or tightened, never weakened, and every amendment is an ADR plus a version bump.

Provenance: the seven invariants are spec section 3 (`docs/spec.md:36-46`), modeled on SEC 15c3-5 / MiFID II Art. 17 post-Knight-Capital control requirements, applied voluntarily. This document is now their source of truth; `docs/spec.md` is design-rationale reference.

Authority: when this file and any other document disagree about an invariant, this file wins. The protected crate `crates/fortuna-invariants/` encodes these as executable tests. Per the repository constitution rule, those tests are additions-only: never weaken, delete, rename, or modify the assertion logic of an existing test. If a test seems wrong, stop and record it in `GAPS.md` under "Disputed invariant tests"; leave the test untouched.

---

## The seven non-negotiable invariants

- **I1. Universal gate.** Every order, regardless of origin (model proposal, mechanical strategy, manual CLI), passes the same deterministic pre-trade gate pipeline. The model cannot bypass, modify, disable, or be consulted by the gates. Enforcement is type-level: `GatedOrder` has private fields and a single `pub(crate)` constructor in `fortuna-gates`, implements `Serialize` only (no `Deserialize`), and `Venue::place` accepts only a `GatedOrder`. Gates are config-driven (TOML), hot-reloadable only by the operator.
- **I2. Drawdown halts with human re-arm.** Per-strategy and global max-drawdown thresholds. Breach flattens or freezes per policy and sets a halt flag that only a human can clear, out-of-band, via CLI. No automatic resumption. Halt math uses conservative-side marks.
- **I3. Runaway detection.** Dual token-bucket rate limits (burst plus sustained) per venue and per market on order submissions. Breach is a halt, not a throttle. Duplicate-order detection via client-order-id idempotency.
- **I4. Out-of-band kill switch.** A kill path (Slack command plus local CLI) that flattens or freezes all positions and revokes order-placing capability. It must not depend on the cognition runtime, the event loop, Postgres, or any LLM provider being healthy. Re-arm is refused while a kill sentinel is present or unverifiable. Tested monthly.
- **I5. Append-only audit log.** Every model call, belief, proposal, gate decision (pass/modify/reject plus reason), order, fill, and config change is recorded, never deleted, never updated in place; sufficient to replay any decision after the fact. DB-enforced by `fortuna_refuse_mutation` (all append-only stores) and `fortuna_beliefs_guard`. Scoped exception (C1, 2026-06-14): a belief's four post-resolution scoring columns (status, outcome, brier, clv_bps) are set exactly once by the scoring job; decision content and every audit row stay immutable.
- **I6. Propose-only model interface.** The model emits structured, schema-validated proposals and beliefs into a queue. Sizing, timing, order type, and execution belong to the harness. The model has zero tools that mutate external state. Schema-invalid output is rejected and logged, never repaired silently. The cognition crate cannot name a venue or depend on execution/state.
- **I7. Promotion gates.** No strategy touches live capital without passing its forward validation gate (see Ramp gates). No model version replaces another in live decision flow without a shadow-mode comparison period. No capital scale-up without continued forward performance. Promotion is a human action; demotion is automatic on breach.

## Perps extensions (Kinetics domain; same invariant middle)

Every perp order is the same sealed-type discipline through the same pipeline: `GatedPerpOrder` mirrors `GatedOrder`, `PerpPrice` is type-separated from `Cents`, and the kill switch gains perps coverage with its own credential pair while staying Postgres/cognition-independent.

- **I1.P / I4.P. Perp seal and flatten seal.** A perp order (including a kill-switch reduce-only close) is a `GatedPerpOrder` only if the perp gate builds it; the killswitch dependency graph keeps I4-forbidden crates absent.
- **I2.P. Drawdown extension.** I2 halt math includes funding paid/received and margin unrealized PnL, marked at the worse-for-us of the venue mark and the conservative mark.
- **I3.P. Cross-domain halt.** A perp halt also halts event-contract orders.

## Derived structural invariants (code-enforced, test-backed)

These are not in the spec's numbered seven but are real guarantees the code enforces and tests pin. They are promoted here so they cannot silently regress.

- **S1. Decoupling spine (Principle 10).** The invariant middle (fortuna-gates, fortuna-exec, fortuna-state) names no venue or domain literals; `fortuna-live` leaks no Kalshi type. Verified by a source-scan test.
- **S2. Money-type integrity.** Money is `Cents(i64)` and `PerpPrice(i64)` (venue ten-thousandths), checked arithmetic only, `Decimal` only at conversion boundaries, fee rounding always against us. `PerpPrice` and `Cents` cannot be cross-assigned. (Covering invariant test to be written; see gap list.)
- **S3. Deterministic core.** All time comes from the injected `Clock`; no `SystemTime::now`/`Instant::now`/`Utc::now` and no RNG in any gate/exec/state/cognition decision path. Backtest reproducibility depends on this. (Covering invariant test to be written; see gap list.)
- **S4. No real order before live.** A paper engine running against live market data cannot place or cancel a real order.

---

## Maker-checker rule

FORTUNA is maker-checker by construction. The maker is the model (and mechanical strategies): it proposes beliefs and order intents. The checker is the deterministic harness: the gate pipeline, the sealed `GatedOrder`, the reservation ledger, and the halt flags. The maker can never also be the checker: I1 and I6 are the type-level expression of this separation. Operator actions that arm risk (drawdown-halt re-arm, kill-switch reversal, promotion) are CLI-only and never available to the model or to Slack (Slack may request; the CLI confirms).

## Ramp gates (promotion ladder, I7)

Stages are `Sim -> Paper -> LiveMin -> Scaled` (the `Stage` enum). Promotion between stages is a human decision against these thresholds; demotion is automatic on breach. Process metrics only; realized PnL is never a promotion criterion.

| Stage | Gate to enter | Cap |
|---|---|---|
| Sim | 100% gate test coverage, replay determinism verified, zero invariant violations across the DST corpus | no capital |
| Paper | >=30 trading days (mechanical) or >=60 resolved beliefs (synthesis); positive CLV or positive expectancy net of modeled fees; Brier beating market baseline (synthesis); fee/PnL ratio < 0.35; zero invariant violations. Maker fills count only on trade-through, never touch. | no capital |
| LiveMin | >=30 days; paper metrics hold within tolerance live; reconciliation clean | $500 exposure per strategy |
| Scaled | rolling 30-day forward metrics hold | stepwise 2x increases; any drawdown halt resets the step |

Model swaps: a new model runs in shadow (full cycles, beliefs scored, no orders) for >=30 resolved beliefs per active category; promotion requires Brier/CLV at least matching the incumbent on paired contexts. System-level kill criterion: if after 90 live days no strategy sustains positive CLV, the synthesis pipeline is shelved and only mechanical strategies run.

---

## Invariant-to-test map

The block below is machine-readable. `tools/check-canon.sh` parses it: every `present` row's named test must exist in the cited file (else CI fails); every `todo` row is reported as a known coverage gap. Format: `ID | path | test_fn | status`.

<!-- INVARIANT-MAP-BEGIN -->
```
I1   | crates/fortuna-invariants/tests/i1_universal_gate.rs           | i1_universal_gate                                                      | present
I1   | crates/fortuna-invariants/tests/i1_universal_gate.rs           | i1_prop_all_orders_carry_gate_verdicts                                 | present
I1   | crates/fortuna-invariants/src/lib.rs                           | compile_fail:GatedOrder-forged-and-deserialize                         | present
I2   | crates/fortuna-invariants/tests/i2_drawdown_human_rearm.rs     | i2_drawdown_human_rearm                                                | present
I2   | crates/fortuna-invariants/tests/i2_drawdown_human_rearm.rs     | i2_prop_breach_always_locks_until_rearm                                | present
I3   | crates/fortuna-invariants/tests/i3_runaway_halt.rs             | i3_runaway_halt                                                        | present
I4   | crates/fortuna-invariants/tests/i4_killswitch_independence.rs  | i4_killswitch_independence                                             | present
I4   | crates/fortuna-invariants/tests/i4_killswitch_revocation.rs    | killswitch_revocation_halts_then_clears_and_survives_restart           | present
I4   | crates/fortuna-invariants/tests/i4_killswitch_revocation.rs    | write_revocation_is_idempotent                                         | present
I4   | crates/fortuna-invariants/tests/i4_killswitch_revocation.rs    | rearm_guard_refuses_while_kill_sentinel_present                        | present
I4   | crates/fortuna-invariants/tests/i4_killswitch_revocation.rs    | rearm_guard_refuses_when_sentinel_unverifiable                         | present
I5   | crates/fortuna-invariants/tests/i5_audit_append_only.rs        | i5_audit_append_only                                                   | present
I5   | crates/fortuna-invariants/tests/i5_audit_append_only.rs        | i5_all_append_only_tables_reject_mutation                              | todo
I6   | crates/fortuna-invariants/tests/i6_propose_only_mind.rs        | i6_sizing_fields_in_proposals_are_schema_rejected                      | present
I6   | crates/fortuna-invariants/tests/i6_propose_only_mind.rs        | i6_mind_output_carries_no_executable_side_effects                      | present
I6   | crates/fortuna-invariants/tests/i6_propose_only_mind.rs        | i6_mind_crate_cannot_name_a_venue_or_mutate_state                      | present
I6   | crates/fortuna-invariants/tests/i6_persona_propose_only.rs     | i6_persona_outcome_surface_is_order_free_and_data_only                 | present
I6   | crates/fortuna-invariants/tests/i6_persona_propose_only.rs     | i6_domain_analyses_table_carries_no_order_or_size_column               | present
I7   | crates/fortuna-invariants/tests/i7_promotion_gates.rs          | i7_promotion_gates                                                     | present
I7   | crates/fortuna-invariants/tests/i7_promotion_gates.rs          | i7_stage_promotion_requires_operator_action_record                     | present
I7   | crates/fortuna-invariants/tests/i7_promotion_gates.rs          | i7_model_swap_requires_shadow_comparison_record                        | present
I7   | crates/fortuna-invariants/tests/i7_promotion_gates.rs          | i7_sim_runner_new_still_refuses_paper_staged_strategies                | present
I1.P | crates/fortuna-invariants/tests/perp_i1_sealed_order.rs        | perp_i1_every_outcome_carries_a_coherent_trail                         | present
I2.P | crates/fortuna-invariants/tests/perp_i2_drawdown_extension.rs  | perp_i2_mark_loss_breaches_and_locks_until_rearm                       | present
I2.P | crates/fortuna-invariants/tests/perp_i2_drawdown_extension.rs  | perp_i2_funding_paid_breaches                                          | present
I2.P | crates/fortuna-invariants/tests/perp_i2_drawdown_extension.rs  | perp_i2_worse_for_us_mark_governs_breach                               | present
I3.P | crates/fortuna-invariants/tests/perp_i3_cross_domain_halt.rs   | perp_i3_perp_breach_halts_event_orders_too                             | present
I4.P | crates/fortuna-invariants/tests/perp_i4_flatten_seal.rs        | a_valid_reduce_only_close_seals_with_a_full_pass_trail                 | present
I4.P | crates/fortuna-invariants/tests/perp_i4_flatten_seal.rs        | a_same_direction_reduce_only_rejects_at_margin_headroom_and_never_seals| present
I4.P | crates/fortuna-invariants/tests/perp_i4_flatten_seal.rs        | an_oversized_reduce_only_would_flip_and_never_seals                    | present
I4.P | crates/fortuna-invariants/tests/perp_i4_flatten_seal.rs        | a_reduce_only_with_no_position_never_seals                             | present
I4.P | crates/fortuna-invariants/tests/perp_i4_flatten_seal.rs        | killswitch_dep_graph_keeps_i4_forbidden_absent_and_now_includes_the_gate_seal | present
S1   | crates/fortuna-invariants/tests/i_decoupling_spine.rs          | spine_gates_has_zero_domain_literals                                   | present
S1   | crates/fortuna-invariants/tests/i_decoupling_spine.rs          | spine_exec_has_zero_domain_literals                                    | present
S1   | crates/fortuna-invariants/tests/i_decoupling_spine.rs          | spine_state_has_zero_domain_literals                                   | present
S1   | crates/fortuna-invariants/tests/i_decoupling_spine.rs          | fortuna_live_has_no_kalshi_type_leak                                   | present
S2   | crates/fortuna-invariants/tests/s2_money_type_integrity.rs     | s2_perp_price_and_cents_cannot_cross_assign                            | todo
S3   | crates/fortuna-invariants/tests/s3_clock_only_determinism.rs   | s3_decision_crates_have_no_wallclock_or_rng                            | todo
S4   | crates/fortuna-invariants/tests/i_paper_live_no_real_order.rs  | paper_on_live_cannot_place_or_cancel_real_orders                       | present
```
<!-- INVARIANT-MAP-END -->

## Coverage gaps (flagged)

1. **S2, S3 have no dedicated invariant test (status `todo`).** Money-type integrity is covered indirectly by `perp_i1` and core unit tests; Clock-only determinism by the DST corpus. Neither is pinned by a dedicated `fortuna-invariants` test. Add `s2_money_type_integrity.rs` and `s3_clock_only_determinism.rs` (a source-scan like `i_decoupling_spine`).
2. **I5 parametric coverage (status `todo`).** `i5_audit_append_only` exercises the `audit` table. The new append-only tables (`validation_runs`, `scorecards`, `trade_scores`, `bus_recordings`) are pinned only by per-crate tests, not the protected harness. Add `i5_all_append_only_tables_reject_mutation`.
3. **Protected-invariant guard scope hole (not a map row; tooling gap).** `scripts/check-protected-invariants.sh` diffs only `crates/fortuna-invariants/tests/`, not `src/lib.rs`, where the I1 `compile_fail` doctests live. Those doctests are weakenable without tripping the guard. Extend the guard to scan `src/lib.rs`. Tracked for the operator.
4. **CI invariant gate is latent.** `.github/workflows/invariants-dst.yml` runs only once a remote exists; today enforcement is local discipline.
5. **I7 forward rails are unit-tested, not DST end-to-end.** Promotion-record and shadow-comparison are pinned at construction; the full forward ladder is not DST-driven.

Audit-replay completeness defects (missing `prompt_hash`, no audit-log replay tool, `actor=NULL` on daemon rows) do not weaken the append-only enforcement under I5, which is fully tested. They are tracked as defects in `GAPS.md`, not as amendments here.
