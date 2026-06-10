---
name: fortuna
description: Project knowledge for building FORTUNA, the model-driven autonomous trading system. Use in every session in this repository - covers crate map, house conventions, DST workflow, schema and migration patterns, and where each spec section lands in code.
---

# FORTUNA project skill

Authority chain: docs/spec.md (v0.8) > CLAUDE.md > this skill. PROMPT.md is the mission;
BUILD_PLAN.md is the task list.

## Crate map (what lives where)

- fortuna-core: Clock trait, Cents, ULIDs, BusEvent, deterministic bus, replay. No IO.
- fortuna-gates: gate checks 1-10 (spec 5.3), GatedOrder sealed type, halt flags.
- fortuna-exec: intent journal, IntentGroup, execution policy, flatten planner.
- fortuna-state: positions, account views, marks, reservations (spec 5.14).
- fortuna-venues: Venue trait, FeeModel interpreter, sim venue (fault injection), kalshi/, polymarket/.
- fortuna-ledger: ALL Postgres (sqlx + migrations): beliefs, events, edges, audit, journal, lessons, intents, settlements, discrepancies, price_snapshots, source_registry, reservations, calibration_params.
- fortuna-cognition: Source trait + ingestion, trigger engine, context assembler, Mind trait (StubMind, AnthropicMind), loops, comparator, Kelly sizing lib, calibration.
- fortuna-ops: config, Slack, CLI, dead-man pinger, metrics, dashboard, accounting export, and the STANDALONE kill-switch binary (no Postgres dependency).
- fortuna-invariants: PROTECTED. Executable invariant tests I1-I7. Add only; never weaken.

## DST workflow

- scripts/run-dst.sh [N_SEEDS] runs the randomized corpus; regression seeds live in
  crates/fortuna-core/dst-corpus/ as one file per seed with a comment naming the failure mode.
- On a red seed: re-run with the printed seed to reproduce; minimize by disabling fault
  classes; fix; commit the seed file. Never delete a regression seed.
- Sim venue faults are seeded and configurable: ack_delay, drop_fill, dup_fill, crash_at(state), book mutation.

## Patterns

- Sealed GatedOrder: private field + constructor only in fortuna-gates; venues accept GatedOrder only (type-level I1).
- Money: Cents(i64), checked ops, fee rounding ALWAYS against us. Decimal only at venue payload boundaries.
- Append-only tables: INSERT-only repos; "updates" are superseding rows (supersedes/superseded_by columns).
- State machines as enums with explicit transition fns returning Result; illegal transition = error + audit row, never a silent coerce.
- All loops take &dyn Clock and a CancellationToken; nothing sleeps on wall time directly.
- sqlx: offline mode in CI (cargo sqlx prepare); one migration per BUILD_PLAN task touching schema.
- Slack: one client in fortuna-ops; route via config table channel->types; every outbound message also writes an audit row.

## Test taxonomy

- Unit (per crate), property (proptest, money/gates/sizing), DST scenarios (cross-crate, seeded), invariant tests (fortuna-invariants, spec Section 3 phrased as executable assertions), fixtures tests (venue adapters vs fixtures/).
- Paper realism tests live with the paper engine: a fill at touch must FAIL the suite.
