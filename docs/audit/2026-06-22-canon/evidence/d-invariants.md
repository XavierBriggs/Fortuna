# Evidence d — Invariants Audit (FORTUNA)

Scope: enumerate the intended invariants (spec §3 + CLAUDE.md), map them to their
enforcement mechanism in production code and their covering test(s) in
`crates/fortuna-invariants/`, grade coverage, and audit the governance gate
(`scripts/check-protected-invariants.sh` + `.github/workflows/invariants-dst.yml`).

All citations verified by reading the cited file:line. Test bodies were read in full;
test names were NOT trusted alone.

---

## 0. Sources of the invariant list

- **Spec §3** `docs/spec.md:40-46` — the canonical numbered I1–I7 (verbatim wording captured below).
- **CLAUDE.md** restates I1–I7 with the C1 scoped exception on I5 (belief scoring columns set once).
- **Protected crate** `crates/fortuna-invariants/` — 16 test files + `src/lib.rs` doc-tests. ADDITIONS-ONLY.

Canonical wording (spec `docs/spec.md`):
- I1 (`:40`) Universal gate. Every order, any origin, passes the same deterministic pre-trade gate pipeline; model cannot bypass/modify/disable/be consulted by gates; gates config-driven, hot-reloadable only by operator.
- I2 (`:41`) Drawdown halts with human re-arm. Per-strategy + global thresholds; breach sets a halt only a human clears out-of-band; no automatic resumption.
- I3 (`:42`) Runaway detection. Dual token-bucket (burst + sustained) per venue and per market; breach is a halt not a throttle; duplicate-order detection via client-order-id idempotency.
- I4 (`:43`) Out-of-band kill switch. Slack + local CLI; flatten-or-freeze AND revoke order-placing capability; must not depend on cognition runtime, event loop, or any LLM provider; tested monthly.
- I5 (`:44`) Append-only audit log. Every model call/belief/proposal/gate decision/order/fill/config change; replayable; never deleted, never updated in place. C1 scoped exception: a belief's four post-resolution SCORING columns (status, outcome, brier, clv_bps) filled exactly once by the scoring job; decision CONTENT immutable; all other stores strictly append-only. DB-enforced via `fortuna_beliefs_guard` + `fortuna_refuse_mutation`.
- I6 (`:45`) Propose-only model interface. Model emits structured proposals/beliefs into a queue; sizing/timing/order-type/execution belong to the harness; model has zero state-mutating tools.
- I7 (`:46`) Promotion gates. No strategy touches live capital without passing forward validation; no model swap into live flow without shadow comparison; no scale-up without continued forward performance.

---

## 1. Invariant → enforcement → test → status table

| Inv | Statement (short) | Enforcement site (mechanism) | Covering test (fn @ path:line) | Status |
|---|---|---|---|---|
| **I1** | Universal gate; venue-acceptable order constructible only by the pipeline | Sealed type `GatedOrder` — private fields, only ctor `pub(crate) assemble` `crates/fortuna-gates/src/order.rs:20,37`; `Serialize` only, **no `Deserialize`** `order.rs:19,9-10`. Venue trait `place` accepts only `GatedOrder` `crates/fortuna-venues/src/lib.rs:115`. Pipeline runs all 11 checks in order `crates/fortuna-gates/src/pipeline.rs:83-94,228-229` | compile-fail doc-tests (forge struct / require Deserialize) `crates/fortuna-invariants/src/lib.rs:11-31` + runtime `i1_universal_gate` `tests/i1_universal_gate.rs:167` + property `i1_prop_all_orders_carry_gate_verdicts` `tests/i1_universal_gate.rs:216` | **enforced** |
| **I1 (perp)** | Perp orders ride the same gate; sealed `GatedPerpOrder` | Sealed type `GatedPerpOrder` — private fields, only ctor `pub(crate) assemble`, Serialize-only `crates/fortuna-gates/src/perp.rs:120,137,117-119`; 15-check `PERP_ALL` trail; `evaluate_perp` `perp.rs:217` | compile-fail doc-tests `src/lib.rs:42-56` + property `perp_i1_every_outcome_carries_a_coherent_trail` `tests/perp_i1_sealed_order.rs:95` | **enforced** |
| **I2** | Drawdown breach → halt; cleared only by human re-arm; no auto-resume | `DrawdownMonitor` computes sticky breach (`fortuna-state`); halt flag in `HaltFlags` set/get `crates/fortuna-gates/src/pipeline.rs:205,200`; **`rearm` is the only clear path** `crates/fortuna-gates/src/halt.rs:87-100` (returns err if not halted — never silent); gate check 1 = `Halts` `pipeline.rs:84` | `i2_drawdown_human_rearm` `tests/i2_drawdown_human_rearm.rs:135` (asserts breach→halt; recovery, day-roll, config-reload all fail to clear; only `rearm` restores) + property `i2_prop_breach_always_locks_until_rearm` `tests/i2_drawdown_human_rearm.rs:231` | **enforced** |
| **I2 (perp)** | Perp mark-loss / funding-paid is drawdown; worse-for-us mark governs | `equity_with_margin` + `MarginAccountView::compute` (conservative mark) in `fortuna-state`/`fortuna-core::perp` | `perp_i2_mark_loss_breaches_and_locks_until_rearm` `tests/perp_i2_drawdown_extension.rs:154`; `perp_i2_funding_paid_breaches` `:205`; `perp_i2_worse_for_us_mark_governs_breach` `:232` | **enforced** |
| **I3** | Dual token-bucket per venue + per market; breach is a HALT not throttle; coid idempotency | Rate buckets `crates/fortuna-gates/src/rate.rs`; breach sets venue halt (check 7 `RateLimits` `pipeline.rs:90`); idempotency check 8 `Idempotency` `pipeline.rs:91`; venue-side dup refusal in `SimVenue` | `i3_runaway_halt` `tests/i3_runaway_halt.rs:101` (venue-bucket breach halts; 1h refill does NOT clear → `Halts`; market-bucket breach halts venue; coid rejected at check 8; venue resubmit yields exactly one resting order) | **enforced** |
| **I3 (perp)** | Halt is venue-scoped across BOTH arms (perp + event-contract share buckets) | Single venue bucket + halt keyed by `VenueId` (no per-arm split); `evaluate_perp` + `evaluate` consult same `HaltFlags` | `perp_i3_perp_breach_halts_event_orders_too` `tests/perp_i3_cross_domain_halt.rs:174`; `perp_i3_event_breach_halts_perp_orders_too` `:188`; `perp_i3_shared_bucket_no_per_arm_split` `:198` | **enforced** |
| **I4** | Out-of-band kill switch; independent of Postgres/cognition/runtime; flatten/freeze AND revoke | (a) STRUCTURAL: `fortuna-killswitch` dep graph excludes sqlx/postgres/ledger/cognition (asserted from `cargo metadata`); (b) OPERATIONAL: binary self-test with `DATABASE_URL` removed; (c) BEHAVIORAL: `freeze_and_cancel` clears all orders under fault, touches 0 positions; REVOCATION: durable `KILLSWITCH_REVOKED` sentinel (`write_revocation`/`is_revoked`/`clear_revocation`) consumed by `RevocationHaltPoller` (`fortuna-live`) | `i4_killswitch_independence` `tests/i4_killswitch_independence.rs:94` (3 layers); `killswitch_revocation_halts_then_clears_and_survives_restart` `tests/i4_killswitch_revocation.rs:164`; `write_revocation_is_idempotent` `:251`; perp-flatten seal+dep-graph `tests/perp_i4_flatten_seal.rs:131,164,186,205,222` | **enforced** |
| **I5** | Append-only audit; never updated/deleted; replayable; belief scoring set-once (C1) | DB triggers: `fortuna_refuse_mutation` rejects UPDATE/DELETE on audit + ~20 stores `crates/fortuna-ledger/migrations/20260609000001_initial.sql:14-16,117-118` + all `*_append_only` triggers; `fortuna_beliefs_guard` blocks DELETE + content mutation, allows only scoring columns `initial.sql:79-99`. Runtime: audit-write failure → global halt `crates/fortuna-runner/src/runner.rs:879-881` ("no audit, no trading") | `i5_audit_append_only` (`#[sqlx::test]`) `tests/i5_audit_append_only.rs:129` (UPDATE+DELETE refused by trigger; replay byte-identical; dead store → halt → only re-arm clears). DST: `Arm::AuditDeath` in `crates/fortuna-runner/tests/settlement_dst.rs:301,351,508-510` | **enforced** |
| **I6** | Propose-only; model output is data; no sizing/exec fields; no venue/state handle | (1) `ProposalDraft`/`MindOutput` use `deny_unknown_fields` (smuggled sizing rejected) in `fortuna-cognition::mind`; (2) STRUCTURAL: `fortuna-cognition` does NOT depend on venues/exec/state/runner (manifest assert) | `i6_sizing_fields_in_proposals_are_schema_rejected` `tests/i6_propose_only_mind.rs:40`; `i6_mind_output_carries_no_executable_side_effects` `:66` (exact key-set pin = spec 5.9); `i6_mind_crate_cannot_name_a_venue_or_mutate_state` `:131`. Persona facet: `i6_persona_outcome_surface_is_order_free_and_data_only` `tests/i6_persona_propose_only.rs:51`; `i6_domain_analyses_table_carries_no_order_or_size_column` `:107` | **enforced** |
| **I7** | Promotion gates; no live capital without forward-validation; model swap needs shadow | Sim runner refuses any strategy staged above its allowlist at CONSTRUCTION (`RunnerError::StageViolation`); `Stage` is a strict total order (Sim<Paper<LiveMin<Scaled); no programmatic promote path — `effective_stage` walks operator records, "system"/blank cannot promote, declared stage is a cap, demotion auto; `evaluate_model_swap` returns {PromoteRecommended, Hold} only (no mutation) | `i7_promotion_gates` `tests/i7_promotion_gates.rs:152`; `i7_stage_promotion_requires_operator_action_record` `:194`; `i7_model_swap_requires_shadow_comparison_record` `:244`; `i7_sim_runner_new_still_refuses_paper_staged_strategies` `:296`; `i7_new_with_venue_accepts_paper_when_allowlist_admits_it` `:345`; `i7_new_with_venue_refuses_live_stages_even_with_paper_allowlist` `:376` | **enforced** |

---

## 2. Test → invariant map (every fn in fortuna-invariants/)

Every test function maps cleanly to a numbered invariant. None is orphaned.

| Test file | fn | Maps to |
|---|---|---|
| `src/lib.rs` (doc-tests) | path-witness + 2 compile_fail (`GatedOrder`) + perp path-witness + 2 compile_fail (`GatedPerpOrder`) | I1 |
| `i1_universal_gate.rs` | `i1_universal_gate`, `i1_prop_all_orders_carry_gate_verdicts` | I1 |
| `i2_drawdown_human_rearm.rs` | `i2_drawdown_human_rearm`, `i2_prop_breach_always_locks_until_rearm` | I2 |
| `i3_runaway_halt.rs` | `i3_runaway_halt` | I3 |
| `i4_killswitch_independence.rs` | `i4_killswitch_independence` | I4 |
| `i4_killswitch_revocation.rs` | `killswitch_revocation_halts_then_clears_and_survives_restart`, `write_revocation_is_idempotent` | I4 (revocation half / C2) |
| `i5_audit_append_only.rs` | `i5_audit_append_only` | I5 |
| `i6_propose_only_mind.rs` | `i6_sizing_fields_in_proposals_are_schema_rejected`, `i6_mind_output_carries_no_executable_side_effects`, `i6_mind_crate_cannot_name_a_venue_or_mutate_state` | I6 |
| `i6_persona_propose_only.rs` | `i6_persona_outcome_surface_is_order_free_and_data_only`, `i6_domain_analyses_table_carries_no_order_or_size_column` | I6 (persona facet) |
| `i7_promotion_gates.rs` | `i7_promotion_gates`, `i7_stage_promotion_requires_operator_action_record`, `i7_model_swap_requires_shadow_comparison_record`, `i7_sim_runner_new_still_refuses_paper_staged_strategies`, `i7_new_with_venue_accepts_paper_when_allowlist_admits_it`, `i7_new_with_venue_refuses_live_stages_even_with_paper_allowlist` | I7 |
| `perp_i1_sealed_order.rs` | `perp_i1_every_outcome_carries_a_coherent_trail` | I1 (perp) |
| `perp_i2_drawdown_extension.rs` | `perp_i2_mark_loss_breaches_and_locks_until_rearm`, `perp_i2_funding_paid_breaches`, `perp_i2_worse_for_us_mark_governs_breach` | I2 (perp) |
| `perp_i3_cross_domain_halt.rs` | `perp_i3_perp_breach_halts_event_orders_too`, `perp_i3_event_breach_halts_perp_orders_too`, `perp_i3_shared_bucket_no_per_arm_split` | I3 (perp) |
| `perp_i4_flatten_seal.rs` | `a_valid_reduce_only_close_seals_with_a_full_pass_trail`, `a_same_direction_reduce_only_rejects_at_margin_headroom_and_never_seals`, `an_oversized_reduce_only_would_flip_and_never_seals`, `a_reduce_only_with_no_position_never_seals`, `killswitch_dep_graph_keeps_i4_forbidden_absent_and_now_includes_the_gate_seal` | I1 + I4 (perp flatten) |
| `i_decoupling_spine.rs` | `spine_gates_has_zero_domain_literals`, `spine_exec_has_zero_domain_literals`, `spine_state_has_zero_domain_literals`, `fortuna_live_has_no_kalshi_type_leak` | **Principle 10** (venue-agnostic spine) — NOT a numbered I1–I7 |
| `i_paper_live_no_real_order.rs` | `paper_on_live_cannot_place_or_cancel_real_orders` | **I7-adjacent** (Paper stage must never hit live exec) — NOT a numbered I1–I7 |

### Tests NOT mapping to a numbered I1–I7 (flagged)
- `i_decoupling_spine.rs` (4 fns) — encodes spec **Principle 10** (venue-agnostic-by-contract spine purity), not §3. Legitimate guard; it is an architectural invariant, not one of the seven. If a future CONSTITUTION promotes Principle 10 to a numbered invariant, this is its test.
- `i_paper_live_no_real_order.rs` (1 fn) — encodes "Paper-on-live execution stays inside `PaperVenue`; never a real Kalshi order endpoint." Closest to I7 (stage discipline) but really a distinct safety invariant ("no real order before live promotion"). Worth numbering.

---

## 3. Governance gate

### `scripts/check-protected-invariants.sh`
Asserts the invariants crate is **additions-only**. Mechanism (quoted, `:31`):
```
removed="$(git diff "$BASE" -- "$DIR" | grep -E '^-' | grep -vE '^---' || true)"
if [ -n "$removed" ]; then ... exit 1
```
Any removed/changed line (a `-` line) under `crates/fortuna-invariants/tests/` vs base (default `main`) fails. New files / pure appends are all `+` lines → pass. `DIR` is hardcoded to `crates/fortuna-invariants/tests` (`:18`).

**Limitation (as-built):** the guard only covers `tests/` — it does NOT diff `crates/fortuna-invariants/src/lib.rs`, which holds the I1/perp-I1 compile-fail doc-tests. Weakening or deleting a `compile_fail` doc-test in `src/lib.rs` (e.g. removing the `requires_deserialize` block) would NOT be caught by this script. See Gaps.

### `.github/workflows/invariants-dst.yml`
Runs on PR + push-to-main. Steps, in order:
1. **Protected-invariant guard** `bash scripts/check-protected-invariants.sh "origin/${BASE_REF}"` (load-bearing governance gate; `BASE_REF` passed via env, never inlined).
2. `cargo fmt --check`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. pre-build killswitch (`cargo build -p fortuna-killswitch`) — so the i4 nested `cargo run` doesn't wedge.
5. **invariants**: `cargo test -p fortuna-invariants` (I1–I7 + perp_i1-4 + i4 revocation/independence).
6. **DST corpus**: `bash scripts/run-dst.sh` — includes `Arm::AuditDeath` (I5 halt) and perp margin/funding/liquidation DST.
Provisions a Postgres 16 service (for the `#[sqlx::test]` I5 test). Workflow header notes it "runs once a GitHub remote exists — the repo is local-only by policy until then" → **CI is currently latent (not executing)**; the guard is enforced by the local pre-commit discipline only. See Gaps.

---

## 4. Gaps (weak / missing / latent coverage)

1. **`src/lib.rs` doc-tests are unprotected by the additions-only script.** The I1 and perp-I1 type-level compile_fail assertions (the `GatedOrder`/`GatedPerpOrder` "cannot construct / cannot Deserialize" pins) live in `crates/fortuna-invariants/src/lib.rs`, but `check-protected-invariants.sh` only diffs `tests/`. A diff weakening those doc-tests passes the guard. **Recommendation:** extend `DIR`/the diff scan to include `src/lib.rs`. (This is the single most material gap — it is the protected-crate guard with a blind spot on real assertions.)

2. **CI workflow is latent.** `invariants-dst.yml` self-documents that it only runs "once a GitHub remote exists — the repo is local-only by policy." Today the entire governance pipeline (protected guard + invariants + DST) is enforced only when a human runs it locally per the CLAUDE.md "Definition of done." No machine gate currently blocks a weakening commit.

3. **I4 "Slack command" + "tested monthly" not covered by an invariant test.** Spec I4 (`docs/spec.md:43`) names a Slack kill path and a monthly test cadence. The invariant tests cover the local-CLI/binary + library + revocation-sentinel paths thoroughly, but there is no test asserting a Slack-triggered kill path exists, and no test/CI enforcing the monthly-test cadence (operational, expected — flag for the operator runbook, not code).

4. **I7 model-swap and stage-promotion positive rails are unit-tested, not DST-exercised.** `evaluate_model_swap` and `effective_stage` are pinned by direct unit tests in `i7_promotion_gates.rs`; there is no end-to-end DST that drives a full promotion/shadow lifecycle. Adequate for the logic, but the live promotion flow has no replay coverage.

5. **Two unnumbered invariant tests** (`i_decoupling_spine`, `i_paper_live_no_real_order`) sit in the protected crate but don't map to I1–I7. Not a coverage gap per se, but a *taxonomy* gap: they enforce real guarantees (Principle 10; no-real-order-before-live) that the seven-invariant frame doesn't name. A CONSTITUTION should either number them or explicitly scope them as "supporting invariants."

6. **`i_decoupling_spine.rs` documents a KNOWN GAP in-file** (`:21-27`): `fortuna-ledger/src/repos.rs` is NOT domain-neutral (`open_aeolus_weather_due`, `provenance->>'model_id' = 'aeolus'` literal ~repos.rs:1349-1374). The ledger spine-purity assertion is deliberately omitted because it would fail. Tracked in GAPS.md. This is a Principle-10 partial, not an I1–I7 gap, but worth surfacing.

---

## 5. Unnumbered code-enforced invariants worth promoting

These guarantees are enforced in code and/or pinned by a test, but are NOT among the numbered I1–I7. Candidates for the CONSTITUTION:

- **Fee rounds against the trader.** `RoundingMode::Up` → `Cents::from_dollars_ceil` `crates/fortuna-venues/src/fees.rs:182-183`. Fee modeled conservatively (never in our favor). Cited in spec 5.2 as a fixture-confirmed Kalshi behavior. No invariant-crate test pins it (lives in venue tests).
- **Clock-only time.** `crates/fortuna-core/src/clock.rs:1` declares `SystemTime::now()/Utc::now()` outside the module a defect; `pub trait Clock` `:163` is injected everywhere. House rule in CLAUDE.md; not pinned by an invariant test (could add a workspace grep guard like the decoupling spine test).
- **`PerpPrice` / `Cents` type separation.** `PerpPrice(i64)` `crates/fortuna-core/src/perp.rs:78` is a distinct newtype from `Cents` — perp marks can't be silently mixed with event-contract cents. Enforced by the type system; exercised in perp tests.
- **Append-only at the DB layer below the app** (already C1/I5 but worth restating as its own constitutional line): ~20 stores carry `*_append_only` triggers calling `fortuna_refuse_mutation`; the app layer is INSERT-only by convention, but the DB is the actual fail-closed enforcer.
- **Venue-agnostic spine purity (Principle 10).** Enforced by `i_decoupling_spine.rs` (gates/exec/state carry zero domain literals; fortuna-live no Kalshi type leak). Already in the protected crate — promote to a numbered invariant.
- **No real order before live promotion.** `i_paper_live_no_real_order.rs` — paper-on-live execution physically cannot call a Kalshi order endpoint (transport panics on `/portfolio/order`). Promote / number.

---

## 6. As-built vs as-intended notes

- I1/I2/I3/I5/I6/I7 as-built **match** spec §3 wording and the CLAUDE.md restatement. The C1 I5 exception is faithfully implemented: `fortuna_beliefs_guard` blocks every content column (`belief_id, created_at, event_id, p, p_raw, horizon, evidence, provenance, supersedes`) and permits only the scoring columns — exactly the spec's "content immutable, scoring set-once" (`initial.sql:84-94`).
- I4 as-built is **broader** than a literal reading might suggest, and correctly so: it covers structural independence, operational (no-DB) independence, behavioral freeze-and-cancel, AND the revocation half (the "revoke order-placing capability" clause, audit C2) via a durable on-disk sentinel — a strength, not a gap.
- I2 `rearm` returning an error when not halted (`halt.rs:96-98`) is an intentional "operator actions never silently no-op" hardening beyond the bare spec text — good.
- The audit-failure→halt wiring is real production code (`runner.rs:879`), not only a contract asserted inside the test, AND is DST-covered (`settlement_dst.rs:Arm::AuditDeath`) — I5 is end-to-end, not stub-only.

---

Evidence file: `/Users/xavierbriggs/fortuna-wt-ws3/scratch/evidence/d-invariants.md`
