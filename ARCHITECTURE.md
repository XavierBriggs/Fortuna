# FORTUNA ARCHITECTURE

> Purpose: show how the system fits together as built.
> Holds: the crate map (single source of truth for the count and layering), the layer model, live and backtest data flow, boundary contracts, ramp topology, maker-checker placement, and the codename resolution.
> Excludes: deep module internals and rationale (decisions/), what must be true (CONSTITUTION.md), and code-writing rules (STANDARDS.md). For live ramp status of each module, see STATE.md.

## Layer model

```
        L3 OPERATIONS  (fortuna-ops, fortuna-cli, fortuna-killswitch)
        metrics, Slack, dashboard, dead-man, accounting export, CLI, kill switch
                 |                                   |
   L2 COGNITION (model-agnostic)         L0 DETERMINISTIC CORE (Rust)
   fortuna-cognition, fortuna-sources    fortuna-core (bus, clock, ids, money)
   context assembler, Mind trait,        fortuna-gates (pipeline, sealed GatedOrder)
   Source trait, loops, comparator,      fortuna-exec (order mgr, intent journal)
   Kelly sizing, calibration             fortuna-state (positions, marks, reservations)
                 |                        fortuna-venues (Venue trait, fees, kalshi/pm/sim)
   L1 BELIEF + MEMORY (Postgres)                        |
   fortuna-ledger (beliefs, events,            market data, fills
   audit, journal, intents, settlements,
   validation_runs, ...)                       VALIDATION (offline, read-only)
   fortuna-scoring (Brier/CLV/calibration       fortuna-backtest (point-in-time replay,
   math, deflation)                             overfitting-deflated GO/NO-GO)
```

Composition: `fortuna-runner` wires strategies to gates; `fortuna-paper` is a paper `Venue`; `fortuna-live` is the daemon composition root; `fortuna-recorder` is a read-only capture tool; `fortuna-invariants` is the protected test crate.

## Crate map (18 crates; this is the source of truth)

| Crate | Layer | Owns | Key types / seams |
|---|---|---|---|
| fortuna-core | L0 | event bus, `Clock`, ULIDs, `Cents`, `PerpPrice`, `BusEvent`, replay, `MarketView` | `Clock`, `Cents`, `PerpPrice`, `BusEvent` |
| fortuna-gates | L0 | the 11-check gate pipeline, halt flags, sealed orders | `GatedOrder` (sealed), `GatedPerpOrder`, `GateCheck`, `HaltFlags` |
| fortuna-exec | L0 | order manager, intent journal + crash recovery, fee interpreter, flatten planner | `OrderManager` (consumes `GatedOrder`) |
| fortuna-state | L0 | positions, account views, conservative marks, reservation ledger | position/reservation repos |
| fortuna-venues | L0 | `Venue` trait, fee-model interpreter, sim venue (fault injection), kalshi/, polymarket/ | `Venue`, `FeeModel` |
| fortuna-ledger | L1 | all Postgres: beliefs, events, market_event_edges, audit, journal, intents, settlements, discrepancies, price_snapshots, source_registry, reservations, calibration_params, validation_runs, scorecards, trade_scores | typed INSERT-only repos |
| fortuna-scoring | L1 | pure scoring math: Brier, CLV, calibration (PAV/Platt), deflation (DSR/PBO/CSCV/SPA/MinTRL) | pure fns, no IO |
| fortuna-cognition | L2 | `Mind` trait, context assembler + manifest, `Source` trait + normalizer, cognition loops, comparator, Kelly sizing | `Mind`, `Source`, `AssembledContext`, `MindOutput` |
| fortuna-sources | L2 | concrete ingest adapters (AeolusSource, news, calendar) behind `Source` | impls `Source` |
| fortuna-ops | L3 | config, Slack (one client, channel-routed), metrics (Prometheus text), dashboard, dead-man pinger, accounting export | config, slack, metrics |
| fortuna-runner | orchestration | `Strategy` trait + impls, deterministic sizing, gate-submission wiring | `Strategy`, `Stage`, `OrderIntent` |
| fortuna-paper | venue | paper `Venue` impl with trade-through fill realism | `Venue` impl |
| fortuna-live | binary | daemon composition root, loop wiring, belief scoring writer, audit bridge | `fortuna-live` bin |
| fortuna-cli | binary | operator CLI: lifecycle, halt, re-arm, backtest, validate, paper-demo | `fortuna` bin |
| fortuna-killswitch | binary | standalone kill switch, own credentials, no Postgres/cognition | `fortuna-killswitch` bin |
| fortuna-recorder | tool | read-only HTTP-to-JSONL orderbook capture | `fortuna-recorder` bin |
| fortuna-backtest | validation | generic point-in-time, overfitting-deflated backtest/validation over any historical source | `HistoricalSource`, `EdgeProvider` |
| fortuna-invariants | test (protected) | executable invariant tests I1-I7 + perp + structural | additions-only |

The dependency graph is an acyclic DAG. `fortuna-killswitch` is dependency-locked to exclude ledger/sqlx/Postgres/cognition (I4). `fortuna-live` is the composition root. `fortuna-backtest` is consumed only by `fortuna-cli` and depends on core/scoring/ledger only; nothing in the live runtime depends on it.

## Boundary contracts (the swappable edges)

The invariant middle (gates, ledger, state, core, scoring) is fixed. Five injected seams surround it:

| Seam | Trait | Defined | Consumed | Contract |
|---|---|---|---|---|
| Venue | `Venue` | fortuna-venues | fortuna-exec | `place` accepts only `GatedOrder`; per-venue `FeeModel` |
| Mind (model) | `Mind` | fortuna-cognition | fortuna-runner/live | takes `AssembledContext`, returns `MindOutput` (beliefs + proposals); no side effects |
| Strategy | `Strategy` | fortuna-runner | runner/live | returns `OrderIntent`s; declares `Stage` (I7) |
| Source | `Source` | fortuna-cognition | fortuna-sources | poll/push, returns normalized envelopes; ingested text is untrusted data |
| Clock | `Clock` | fortuna-core | everywhere | the only sanctioned time source |
| Historical (backtest) | `HistoricalSource` / `EdgeProvider` | fortuna-backtest | fortuna-cli | read-only replay; supplies purge/embargo windows |

Known boundary notes (as-built): `InstrumentKind` is defined in fortuna-core but not yet threaded through markets/positions/gates (the binary/perp split is done by type separation instead; see STATE for perps status). The `Source` trait lives in cognition while concrete adapters live in fortuna-sources (a crate split that does not mirror the layer boundary, deliberately). `MarketView` lives in fortuna-core to avoid a venues-to-cognition edge.

## Live data flow (one cycle)

1. Signal ingest (`Source`) and venue market data enter and are persisted point-in-time (`signals`, `markets`).
2. The context assembler builds a budgeted context and emits a manifest into the audit log.
3. The `Mind` emits beliefs and proposals (schema-validated); beliefs persist with provenance.
4. The calibration layer adjusts probabilities.
5. The deterministic comparator compares calibrated beliefs to live prices and derives candidate orders.
6. The harness sizes deterministically (fractional Kelly via the reservation ledger).
7. The candidate enters the gate pipeline (11 ordered checks). A pass yields a sealed `GatedOrder`.
8. The order manager submits via `Venue::place`; the intent journal records the lifecycle.
9. Fills update positions and account views.
10. Settlement reconciles asynchronously; the scoring job sets belief outcome/Brier/CLV once (C1).

The decision/execution split is type-enforced: nothing reaches a venue except a `GatedOrder` constructed by the pipeline (CONSTITUTION I1). The kill-switch flatten is the only path that bypasses the main runtime, by design (I4), and its perp closes still pass the perp gate.

### Gate pipeline (11 ordered checks)

`Halts (1) -> CapitalThreshold (2) -> PositionCaps (3) -> PriceSanity (4) -> SizeSanity (5) -> EdgeFloor (6) -> RateLimits (7) -> Idempotency (8) -> SameEvent (9) -> InternalNetting (10) -> BookAge (11)`. Ordered, fail-closed; each emits an audit record with verdict and reason. The i1 invariant test asserts every check produces a verdict via `GateCheck::ALL.len()`, so adding a check cannot silently escape coverage.

## Backtest / validation data flow (offline, read-only)

Archive (opened read-only) -> `HistoricalSource` -> as-of join enforcing point-in-time strict `<` (G-PIT) -> recomputed beliefs and scores via the same scoring path as live (G-PARITY) -> engaged-set coverage and voided/NO guard (G-DEAD) -> parameter sweep with purged/embargoed CSCV/PBO, Hansen SPA_c, MinTRL/effective-N, and DSR deflated against the joint family trial count -> Brier-primary GO surface (G-TRUTH; CLV corroborates, cannot create a GO) -> append-only `validation_runs` row.

This path constructs no `GatedOrder` and imports no venue/exec/gate crate. It validates deterministic components only (NORTH_STAR forward-only rule). The `run_id` is a deterministic FNV-1a content hash, so runs are reproducible and idempotent.

## Ramp topology

Strategies declare a `Stage` (`Sim -> Paper -> LiveMin -> Scaled`). A `Sim` runner refuses non-`Sim` strategies (I7). Promotion is operator-driven against the CONSTITUTION ramp gates; the validation subsystem produces the GO/NO-GO evidence; live ramp status per strategy is the model inventory in STATE.md.

## Maker-checker placement

Maker = `Mind` + mechanical strategies (propose). Checker = gate pipeline + sealed `GatedOrder` + reservation ledger + halt flags (dispose). The two are separated at the type level (I1/I6). Risk-arming operator actions are CLI-only.

## Codename resolution (read once, never confuse again)

The names Iris, Mercury, Atlas, Artemis, Nemesis, Nike are prior Olympus lineage projects, harvested for documented failure modes and venue knowledge, not linked as code (`docs/spec.md:17`). They are not FORTUNA subsystems. Mercury is a Postgres-familiarity precedent; ITHACA is the deployment/backup host. The only live codenames in this system: Aeolus (an external signal source consumed via `Source`; the `aeolus_eval` zero-capital scoring strategy) and Kinetics (the perpetual-futures domain). The real subsystems are the 18 `fortuna-*` crates above.
