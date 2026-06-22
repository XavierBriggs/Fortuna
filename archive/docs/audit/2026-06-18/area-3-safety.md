# Area 3 — Safety & Invariant Integrity

## Summary

All seven FORTUNA invariants (I1–I7) are enforced by code and tests, not convention, on
the `feature/paper-on-live-data` working tree. The `GatedOrder` sealed constructor,
the append-only DB triggers, the kill-switch dependency graph, and the paper-on-live
`GuardedKalshiTransport` gate are all intact and all invariant tests pass (`SQLX_OFFLINE=true
cargo test -p fortuna-invariants` — 34 tests pass, 0 failures). The biggest live risk is not
a bypass but a **gap in the re-arm CLI surface** (noted as boundary-blocked in GAPS.md:
`gates.rearm()` exists and is correct, but the operator-facing `fortuna-cli rearm` command is
unbuilt, so a real drawdown halt or kill-switch revocation requires a Rust-internal re-arm until
T4.4 lands). For the paper demo target no live capital is at risk — but an operator cannot easily
unblock a halted demo run without code assistance.

---

## Findings

| Severity | Readiness | Finding | Evidence (path:line) | Why it matters | Root cause | Recommended fix | Suggested test |
|---|---|---|---|---|---|---|---|
| P2 | BLOCKS | Operator re-arm CLI unbuilt; halt can only be cleared via code or restart | `GAPS.md:3690-3700`; `fortuna-gates/src/halt.rs:87` (`gates.rearm()` correct but unexposed); `run_loop.rs:165-176` (re-arm is "restart-gated") | A drawdown halt, rate-limit halt, or kill-switch revocation during a demo run requires either restarting the process (loses in-flight paper fills) or writing code; operators cannot self-serve unblock | `fortuna-cli` (T4.4) is on Track-B and has not landed; the loop-doc boundary prevents Track-A from touching it | Land the `fortuna-cli rearm <scope>` sub-command exposing `gates.rearm()` and `clear_revocation()`; wire "pending restart" status to the ROTA health panel | CLI integration test: `fortuna-cli rearm global` calls `gates.rearm(Global)` and emits a confirmation row; a halted pipeline accepts an order after the CLI command completes |
| P2 | PARK | `is_revoked()` treats unreadable parent directory as "not revoked" | `crates/fortuna-killswitch/src/lib.rs:258` — `path.exists()` returns `false` on FS error; lib.rs comment: "an unreadable parent dir reports 'not revoked', but the poller is layered OVER the durable PgHaltPoller" | If the filesystem is misconfigured, the revocation sentinel might not be detected, leaving the gate open to order placement | `std::path::Path::exists()` returns `false` on permission error; the sentinel was chosen as defense-in-depth over `PgHaltPoller` | Change `is_revoked` to return `true` (fail-closed) on any `metadata()` error that is not `NotFound`; or add a filesystem health assertion at boot | Unit test: call `is_revoked` on a path in a directory with mode 000; assert it returns `true` (fail-closed) |
| P3 | PARK | `exec_policy_for_runtime` passes `Ok` (no policy restriction) for `ProductionOrders` mode with `orders_enabled=true` but there is no compile-time barrier | `crates/fortuna-live/src/daemon.rs:783-793` | `ProductionOrders` mode is structurally reachable in this codebase; a config error could boot with live orders enabled on the paper demo venue; boot validation guards it but config TOML is the only gate | `ExecutionMode::ProductionOrders` is a valid enum variant with no static lock-out | Add a boot-time check that refuses `ProductionOrders` unless a `production_unlock = true` flag is present (already in `RuntimeSection`); confirm `validate_bootable` enforces this cross-field constraint end-to-end | Boot test: a TOML with `execution_mode = "production_orders"` and `production_unlock = false` must return `BootError` before any transport is constructed |
| P3 | PARK | I2 re-arm wiring comment in `run_loop.rs` says "only on restart" but tests call `gates.rearm()` directly; divergence could mislead future contributors | `crates/fortuna-live/src/run_loop.rs:165-176` vs `tests/i2_drawdown_human_rearm.rs:222` | Maintenance confusion: tests prove `gates.rearm()` works in process; production requires restart; the distinction is correct but undocumented at the call site | The operator-facing restart requirement is a deliberate design choice (GAPS.md adjudication 2026-06-12) but not annotated inline in run_loop | Add an inline comment in `run_loop.rs:165-176` explicitly cross-referencing the GAPS adjudication and the forthcoming CLI | No new test needed; the existing invariant test already asserts the behavior |

---

## Trace / narrative

### I1 — Universal gate (sealed `GatedOrder`)

`GatedOrder` in `crates/fortuna-gates/src/order.rs:19-86` has private fields and a single
`pub(crate) fn assemble(...)` constructor callable only from `pipeline.rs`. It derives
`Serialize` but deliberately omits `Deserialize` (deserialization would be a constructor
bypass — documented at line 8-10). Two compile-fail doc-tests in
`crates/fortuna-invariants/src/lib.rs:20-56` confirm that `GatedOrder {}` and
`requires_deserialize::<GatedOrder>()` are compile errors. The same seal is replicated for
`GatedPerpOrder` (perp arm, spec 5.15, lines 33-56 of `lib.rs`).

The `Venue` trait in `crates/fortuna-venues/src/lib.rs:112` binds `place` to accept only
`GatedOrder`:

```
async fn place(&self, order: GatedOrder) -> Result<VenueOrderId, VenueError>;
```

The Kalshi adapter at `crates/fortuna-venues/src/kalshi/adapter.rs:571` implements this
signature with no way to bypass it.

The runtime invariant test at `tests/i1_universal_gate.rs:167-208` verifies that every
venue-acceptable order carries a complete 10-check pass trail, and a property test
(`tests/i1_universal_gate.rs:210-240`) sweeps over arbitrary candidates. Both pass.

Perp path pinned at `tests/perp_i1_sealed_order.rs` — all 14 PERP_ALL checks verified
with property tests.

**SESSION EVIDENCE VERIFIED:** Sealed `GatedOrder` constructor confirmed private to
`crates/fortuna-gates/`. Venues confirmed to accept only `GatedOrder`. Compile-fail
tests confirmed present and passing.

### I1 (paper-on-live) — No real execution path from paper mode

`tests/i_paper_live_no_real_order.rs:213-214` implements `GuardedKalshiTransport` which
panics on any non-GET call or any call containing `/portfolio/order`. The test at line
245 drives 50 gated place calls and 5 cancel calls through `PaperLiveVenue` and asserts:
- All recorded calls are GET-only (`line 334`)
- No call touches a Kalshi order endpoint (`line 335-338`)

The `KalshiReadClient` in `crates/fortuna-venues/src/kalshi/read_client.rs:6` confirms:
"a `GatedOrder` has no method on this type that can send it to Kalshi."

The daemon composes a `PaperLiveVenue` (not `KalshiVenue`) for the paper-on-live path
at `daemon.rs:726-737`. The `PaperLiveVenue` routes all `place` and `cancel` calls to
its internal paper book, never to the live transport.

**SESSION EVIDENCE VERIFIED:** Invariant test at `~i_paper_live_no_real_order.rs` confirmed
at lines 213 and 245 (within `GuardedKalshiTransport::request`) and the full assertions
at lines 333-339. Test passes.

### I2 — Drawdown halt with human re-arm

`DrawdownMonitor.check()` returns a `DrawdownVerdict::Breach` when daily loss reaches the
limit; the `i2_drawdown_human_rearm` test at line 135 verifies:
1. Breach fires at exactly the daily loss limit (`line 150-155`)
2. The breach is wired to `gates.set_halt(HaltScope::Global, ...)` (`line 159`)
3. No subsequent order passes (`line 162-165`)
4. Equity recovery does NOT clear it (`line 169-178`)
5. Time passing into the next UTC day does NOT clear it (`line 183-188`)
6. A config reload does NOT clear it (`line 192-219`)
7. Only `gates.rearm(HaltScope::Global)` restores flow (`line 222-223`)

The property test at line 226 sweeps randomized equity paths confirming breach-always-locks.

`HaltFlags::rearm()` in `crates/fortuna-gates/src/halt.rs:87-100` is the only code path
that clears a halt. The `run_loop.rs` comment at line 165-176 confirms that in the daemon,
re-arm only takes effect on restart (the running daemon never auto-clears the gate halt).

**GAP FOUND (P2):** The CLI command `fortuna-cli rearm` (T4.4) is unbuilt and
boundary-blocked (GAPS.md:3690-3700). The `gates.rearm()` method exists and works; it
is exposed in tests but inaccessible to the operator without code.

### I3 — Runaway detection (dual token-bucket halt)

`i3_runaway_halt` at line 101 verifies:
1. Exceeding the venue-bucket burst (3rd order with burst=2) fires `GateCheck::RateLimits`
   and sets `p.halts().venue_halted("sim")` (lines 133-135)
2. A full hour of token refill does NOT clear it — the halt persists at `later` (`line 138-140`)
3. Only `p.rearm(HaltScope::Venue("sim"))` clears it (`line 143-147`)
4. Market-bucket breach halts ALL subsequent orders on different markets too (lines 176-181)
5. Duplicate client order IDs are idempotency-rejected at `GateCheck::Idempotency` (lines 200-206)
6. At the venue layer, duplicate coids return `VenueError::AlreadyExists` and leave exactly
   one resting order (lines 248-255)

**SESSION EVIDENCE VERIFIED:** Dual token-bucket halt (not throttle) confirmed.

### I4 — Kill switch independence

Three-layer verification in `i4_killswitch_independence.rs`:

**Layer 1 (structural):** `cargo metadata` walk at lines 96-145 traverses the full normal
dependency graph of `fortuna-killswitch` and asserts none of `[sqlx, tokio-postgres,
postgres, fortuna-ledger, fortuna-cognition]` appear. The `Cargo.toml` of
`fortuna-killswitch` uses only `fortuna-core`, `fortuna-venues`, `fortuna-gates` (and
standard/async crates). This is verified fresh on each test run.

**Layer 2 (operational):** The binary self-test at lines 148-174 runs the kill-switch
binary as a subprocess with `DATABASE_URL` scrubbed from the environment, confirming it
completes successfully and writes `"freeze_and_cancel_started"` and `"orders_cancelled":2`
to a journal file.

**Layer 3 (behavioral):** `freeze_and_cancel` clears all resting orders even under
`cancel_timeout_not_cancelled_pm: 300` fault injection (lines 176-206).

**Revocation sentinel (I4 completeness, C2):** `write_revocation`, `is_revoked`,
`clear_revocation` in `src/lib.rs:213-259` use `std::fs` only (no Postgres). The
`RevocationHaltPoller` in `run_loop.rs:85-105` wraps the durable `PgHaltPoller` and
checks the sentinel first — before any tick. Behavioral test at
`tests/i4_killswitch_revocation.rs` verifies all four properties: write → poll halts,
halt blocks gate, survives restart, clear → poll delegates.

**MINOR GAP (P2):** `is_revoked()` at `src/lib.rs:258` uses `path.exists()` which
returns `false` on filesystem permission errors (not just `NotFound`). This means a
misconfigured filesystem could allow placement when the sentinel exists but is
unreadable. Mitigated by layering: the durable `PgHaltPoller` still holds the halt if
a revocation was DB-written; the sentinel is defense-in-depth.

**SESSION EVIDENCE VERIFIED:** Kill switch confirmed standalone at
`crates/fortuna-killswitch/`. Dependency walk passes; no Postgres/ledger/cognition in
the dep graph.

### I5 — Append-only audit log

Database-level enforcement verified at `crates/fortuna-ledger/migrations/20260609000001_initial.sql`:
- `fortuna_refuse_mutation()` trigger function (line 14-17): raises exception "table %
  is append-only (I5): % refused"
- Applied to: `audit`, `signals`, `intent_events`, `fills`, `market_snapshots`,
  `price_snapshots`, `settlement_entries`, `discrepancies`, `discrepancy_resolutions`,
  `journal`, `lessons`, `calibration_params`, `reservation_events`, `halt_events`,
  `market_event_edges` — 15 tables with `BEFORE UPDATE OR DELETE` triggers
- `fortuna_beliefs_guard()` (lines 79-99) provides a scoped exception for the four
  belief scoring columns (status/outcome/brier/clv_bps, set-once post-resolution by
  the scoring job; CLAUDE.md C1 exception; decision content stays immutable)
- New migration `20260617000001_event_source_evidence.sql` adds `event_source_evidence`
  table with its own `event_source_evidence_append_only` trigger (line 26)

The `i5_audit_append_only` test at `tests/i5_audit_append_only.rs:129` uses
`#[sqlx::test]` to provision an isolated migrated DB and verifies:
1. `UPDATE audit SET payload = '{}'` returns an error containing "append-only" (line 148)
2. `DELETE FROM audit` returns an error containing "append-only" (line 151-152)
3. Two sequential reads produce byte-identical streams (lines 156-165)
4. Pool close → `writer.append()` error → `gates.set_halt(Global)` → all orders halt
   (lines 174-184)
5. Only `gates.rearm(Global)` restores flow (lines 186-188)

The `PgAuditSink` in `crates/fortuna-live/src/audit_bridge.rs` provides fail-synchronous
semantics: every `append()` call blocks on a channel round-trip to a dedicated writer
thread; a `RunnerError::AuditFailed` bubbles back in the SAME call. The runner at
`crates/fortuna-runner/src/runner.rs:771-779` catches this and calls
`gates.set_halt(Global, ...)` immediately.

**SESSION EVIDENCE VERIFIED:** `fortuna_refuse_mutation` and `fortuna_beliefs_guard`
triggers confirmed in migrations. Audit failure → halt wiring confirmed in runner.

### I6 — Propose-only model interface

Two complementary invariant files:

`tests/i6_propose_only_mind.rs`:
1. `ProposalDraft` schema with `deny_unknown_fields` rejects 6 smuggled sizing/execution
   fields: `contracts`, `size`, `notional_cents`, `quantity`, `order_type`,
   `time_in_force` (lines 48-63)
2. `MindOutput` rejects `orders`, `tool_calls`, `commands` at top level (lines 73-86)
3. `ProposalDraft` surface is exactly `[belief_ref, market, max_price_cents, side,
   thesis, urgency]` — 6 fields pinned (lines 90-110)
4. `MindOutput` surface is exactly `[beliefs, cost_cents, journal, proposals]` — 4
   fields pinned (lines 113-128)
5. `fortuna-cognition`'s `Cargo.toml` must not list `fortuna-venues`, `fortuna-exec`,
   `fortuna-state`, `fortuna-runner` (lines 131-162)

`tests/i6_persona_propose_only.rs`:
1. `PersonaOutcome` serialized surface pinned to 13 data-only fields (lines 57-82)
2. Defense-in-depth: explicitly asserts that none of `contracts`, `size`,
   `notional_cents`, `quantity`, `order_type`, `time_in_force`, `max_price_cents`,
   `side`, `price`, `urgency`, `market` appear (lines 86-103)
3. The `domain_analyses` migration is scanned for execution columns (lines 107-141)

**SESSION EVIDENCE VERIFIED:** Both I6 tests pass. ProposalDraft and MindOutput field
sets confirmed pinned.

### I7 — Promotion gates

`tests/i7_promotion_gates.rs` verifies:
1. Stage ladder total order: `Sim < Paper < LiveMin < Scaled` (line 155-157)
2. `SimRunner::new()` refuses Paper/LiveMin/Scaled strategies at construction —
   before any input can reach the strategy (lines 162-180)
3. `effective_stage()` requires a contiguous chain of HUMAN operator records from Sim;
   "system" actor cannot promote; blank actor cannot promote; a gap in the chain stops
   the walk; the declared stage is a CAP (lines 194-241)
4. `evaluate_model_swap()` requires ≥30 paired resolutions per category, returns only
   `PromoteRecommended` (never mutates the live model) (lines 243-284)
5. A new test `i7_sim_runner_new_still_refuses_paper_staged_strategies` (lines 295-319)
   pins that the `SimRunner::new()` path still uses `&[Stage::Sim]` after the
   `new_with_venue` split — a Paper strategy is still refused by the default constructor
6. `i7_new_with_venue_accepts_paper_when_allowlist_admits_it` (lines 344-370) confirms
   the demo path via `new_with_venue(..., &[Stage::Sim, Stage::Paper])` is deliberate
7. `i7_new_with_venue_refuses_live_stages_even_with_paper_allowlist` (lines 374-410)
   confirms `[Stage::Sim, Stage::Paper]` still refuses LiveMin and Scaled

The daemon composes the paper-on-live runner via `compose_kalshi_family_runner_with_venue`
which calls `SimRunner::new_with_venue(..., &[Stage::Sim, Stage::Paper])` at
`daemon.rs:1016`, precisely the seam the I7 test probes.

**SESSION EVIDENCE VERIFIED:** I7 tests all pass including the new `new_with_venue`
allowlist tests added for the demo flip.

### `execution_mode` runtime enforcement

`daemon.rs:772-793` (`exec_policy_for_runtime`) enforces:
- `orders_enabled = false` → `with_order_mutation_disabled()`
- `execution_mode = LiveDataOnly` or `DryRun` → `with_order_mutation_disabled()` regardless of `orders_enabled`
- `PaperLedger`, `DemoOrders`, `ProductionOrders` → policy passes through (ExecPolicy default)

Boot validation in `boot.rs:580-742` cross-validates:
- `LiveDataOnly + orders_enabled=true` → `BootError` (line 582)
- `DryRun + orders_enabled=true` → `BootError` (line 714)
- `DemoOrders` requires `data_source = "kalshi"` and `orders_enabled=true` (lines 720-737)
- `ProductionOrders` requires `production_unlock = true` (line 742)

---

## Self-adversarial pass

**Is the I1 compile-fail truly enforcement, or could it be vacuous?**
The `lib.rs:11` path-witness doc-test (`fn _witness(_: &GatedOrder) {}`) must compile for
the module to load; if `GatedOrder` were renamed, the path-witness would also fail to
compile and catch the rename. The two compile-fail tests are NOT vacuous.

**Could `Deserialize` be added to `GatedOrder` without triggering invariant tests?**
The `i1_universal_gate` test does not explicitly test for `Deserialize`. However, the
compile-fail test `requires_deserialize::<GatedOrder>()` in `lib.rs:28-30` fails at
compile time if `Deserialize` is derived — so adding it WILL break the test suite
(correctly). This is genuine enforcement.

**Is the `is_revoked` fail-open concern overblown?**
`Path::exists()` is documented to return false on any error, including permission
errors. However: (1) the sentinel file is owned by the same process that writes it;
permission errors are operationally rare; (2) the `RevocationHaltPoller` is layered
OVER the `PgHaltPoller`, so a DB-recorded halt is still active; (3) the sentinel is
defense-in-depth for the case when Postgres is dead. The risk is real but low-severity
in practice. Rated P2/PARK (not P1) because it requires a real FS misconfiguration.

**Did I miss any bypass route in the paper-on-live path?**
The `PaperLiveVenue` exposes a `Venue` impl. The runner calls `venue.place(gated_order)`.
`PaperLiveVenue::place` routes to its internal paper book without touching the embedded
`KalshiReadClient`. The read client has `async fn request(...)` but no `place` method
and no `GatedOrder` parameter. The only way to reach a Kalshi execution endpoint from
this path is through the transport, which only handles `KalshiReadClient` GET requests.
The `GuardedKalshiTransport` test confirms this mechanically. No bypass found.

**Is the I5 wiring gap (audit → halt in the composited runner) actually closed?**
`i5_audit_append_only` tests the LIBRARY contract (audit error → `gates.set_halt`)
but delegates the "no-audit-no-trading wiring in the DST" to T0.10 (per the test's
comment at line 12-14). The `PgAuditSink` provides fail-synchronous semantics in the
real daemon, and the runner's `audit()` method at `runner.rs:767-780` wires the halt
immediately. The DST injection extension is not yet wired (GAPS), but the runtime path
through `PgAuditSink` → `RunnerError::AuditFailed` → `gates.set_halt` IS implemented
and wired. Rated P2 (test gap, not behavior gap).

**Is the I7 demo-flip `new_with_venue` a weakening of the stage gate?**
The `new_with_venue` allowlist is explicit: `&[Stage::Sim, Stage::Paper]`. The test
`i7_new_with_venue_refuses_live_stages_even_with_paper_allowlist` directly confirms
`LiveMin` and `Scaled` are still refused. Opening Paper for the demo is deliberate and
gated; it does not open live stages. Not a weakening.

---

## Open questions for the Lead

1. **CLI re-arm (P2, M3):** The operator cannot self-serve unblock a halt during a demo.
   Is the paper demo acceptable without this? The GAPS.md boundary-block is current —
   does the Lead want to waive the track-B boundary for M3 before the demo?

2. **`is_revoked` fail-open on FS error (P2):** Elevate to P1 if the demo environment
   is known to have restrictive filesystem permissions on the sentinel path?

3. **I5 DST injection (T0.10 deferred):** The runner's `PgAuditSink` fail-synchronous
   wiring is implemented but DST-level injection of audit failures has not landed. Is
   this in scope for the demo gate or a post-demo item?

4. **`event_source_evidence` migration (20260617):** This is an untracked migration file
   in the working tree. Does it have a corresponding `PgAuditSink`/trigger test, or
   should one be added to the ledger test suite?

5. **`StubVetoMind::allow_all()` in production paths:** The daemon composes `veto_mind
   = Some(Arc::new(StubVetoMind::allow_all()))` for `mech_extremes` when the real
   veto mind is absent. Is this acknowledged, and is it safe for demo? (The daemon.rs
   comment at line 1050 says "HONESTLY NOT HERE YET".)
