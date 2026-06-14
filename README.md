# FORTUNA

FORTUNA is a model-driven autonomous trading system built on one inversion: the
model is not the system — it is a consumed, untrusted component behind a
deterministic harness. A frontier LLM (currently Claude, interchangeable by
design) performs synthesis and proposes structured beliefs and trades; a
deterministic Rust harness owns state, sizing, execution, risk, and
accountability. No code path exists by which model output reaches a venue
without passing the deterministic gate pipeline ([docs/spec.md](docs/spec.md)
§2 Principle 1). The thesis, from the foundation research: model capability is
a rising commodity; durable edge lives in the harness and in proprietary
signal inputs (spec §1).

- **Design authority:** [docs/spec.md](docs/spec.md) (v0.9)
- **Constitution (binding on every contributor, human or agent):** [CLAUDE.md](CLAUDE.md)
- **Agent front door:** [AGENTS.md](AGENTS.md)
- **Run it:** [docs/quickstart.md](docs/quickstart.md)

## The seven invariants

These are absolute. The normative text lives in [CLAUDE.md](CLAUDE.md) (quoted
from spec §3); the executable form lives in
[`crates/fortuna-invariants/`](crates/fortuna-invariants/), a protected crate
where existing tests may never be weakened — additions only.

| # | Invariant (summary — CLAUDE.md is authoritative) |
|---|---|
| I1 | Universal gate: every order, regardless of origin, passes the same deterministic pre-trade gate pipeline; the model cannot bypass, modify, disable, or be consulted by it. |
| I2 | Drawdown halts with human re-arm: a breach sets a halt flag only a human can clear, out-of-band. No automatic resumption. |
| I3 | Runaway detection: dual token-bucket rate limits per venue and per market. Breach is a halt, not a throttle. |
| I4 | Out-of-band kill switch: must not depend on the cognition runtime, the event loop, Postgres, or any LLM provider being healthy. |
| I5 | Append-only audit log: never deleted, never updated in place; sufficient to replay any decision after the fact. |
| I6 | Propose-only model interface: the model has zero tools that mutate external state; sizing, timing, order type, and execution belong to the harness. |
| I7 | Promotion gates: no strategy touches live capital without passing forward validation; no model swap without shadow comparison. |

## Status — as of 2026-06-13

- **Core: complete and independently gated.** Phases 0–3 (deterministic core,
  mechanical paper path, belief pipeline, closing loop) have EXIT-met evidence
  in [BUILD_PLAN.md](BUILD_PLAN.md). The first completion claim was falsified
  by an independent verification gate and rebuilt to an ACCEPT — the record of
  that, plus build statistics and deviations from spec, is
  [FINAL_REPORT.md](FINAL_REPORT.md).
- **Daemon: built, gated, soak-GO.** `fortuna-live` (Sim venue, real-time
  cadence, Postgres-backed audit, Slack routing, dead-man heartbeat) passed
  its GO/NO-GO gate for the 7-day Phase-4 EXIT soak —
  [docs/reviews/soak-go-gate-2026-06-12.md](docs/reviews/soak-go-gate-2026-06-12.md)
  (ACCEPT; workspace battery 803/0/0; 10,000-seed DST stages clean). Starting
  the soak is an operator action (outward-facing secrets + release build).
- **ROTA: live.** The read-only gold/black operator console is served by the
  running daemon at `/rota` and passed its browser acceptance pass
  ([docs/reviews/GATE-FINDINGS-LATEST.md](docs/reviews/GATE-FINDINGS-LATEST.md),
  R12). Remaining T4.3 items (full money model, audit-recents queries) are
  listed at the [BUILD_PLAN.md](BUILD_PLAN.md) T4.3 entry.
- **Operator CLI: built.** `fortuna start/stop/status/logs/config check` plus
  `halt/rearm/kill` (T4.4 ticked in [BUILD_PLAN.md](BUILD_PLAN.md); design at
  [docs/design/fortuna-cli.md](docs/design/fortuna-cli.md)).
- **Perps (Phase 5): merged to main.** Research, the spec 5.15 amendment, and the
  perishable-data recorder (T5.0/B0/B1) plus the full perp pipeline — the
  `perp_event_basis` basis-trader and the zero-capital `funding_forecast`
  belief-producer, the PerpTick ingestion seam, scalar-belief persistence, and the
  daemon composition — are now on main (gate-ACCEPT merges `9c4026e`, `72adb7a`,
  `95799cc`, 2026-06-13). The strategies are propose-only (I6) and INERT in
  pure-sim until an operator opts in a recorded perp feed.
- **Live trading: NEVER enabled.** `sim` is the only bootable venue
  ([config/fortuna.example.toml](config/fortuna.example.toml) `[daemon]`); the
  Kalshi adapter refuses to boot without operator-recorded fixture clearance;
  promotions, re-arms, and live capital are operator-only actions (I7). No
  venue credentials are committed anywhere in this repository.

Current open items, verifier-owned:
[docs/reviews/GATE-FINDINGS-LATEST.md](docs/reviews/GATE-FINDINGS-LATEST.md).

## How it is verified

Work that has not survived an independent gate counts as zero. Every batch is
reviewed by a separate verifier session in a detached worktree, which re-runs
the full battery (`cargo fmt --check`, `clippy -D warnings`,
`cargo test --workspace`, `scripts/run-dst.sh`) and grades claims against
executed evidence — verdicts are committed under
[docs/reviews/](docs/reviews/). Deterministic simulation testing is the
primary methodology: seeded fault injection (crashes, duplicate fills, venue
outages, cognition failures, settlement chaos) across thousands of randomized
seeds per run, byte-identical replay per seed, with a regression corpus that
is never deleted. Tests themselves are verified by mutation checks — the gate
breaks the tested surface and requires the test to go red; green-only
verification of a test is not verification. The doctrine, the red-seed story,
and the gate protocol are in [docs/verification.md](docs/verification.md).

## Documentation map

| Document | What it is |
|---|---|
| [CLAUDE.md](CLAUDE.md) | The constitution: seven invariants, house conventions, definition of done. |
| [docs/spec.md](docs/spec.md) | Design authority (v0.9): purpose, principles, invariants, every component spec. |
| [PROMPT.md](PROMPT.md) | Master build instruction and the acceptance checklist. |
| [BUILD_PLAN.md](BUILD_PLAN.md) | Phased task list; ticks carry commit hashes and phase EXIT evidence. |
| [GAPS.md](GAPS.md) | Honesty ledger: everything deferred, blocked, or operator-pending, with unblock steps. |
| [ASSUMPTIONS.md](ASSUMPTIONS.md) | Every decision made where the spec is silent, with rationale. |
| [FINAL_REPORT.md](FINAL_REPORT.md) | What was built, deviations from spec, verification statistics, operator runbooks. |
| [AGENTS.md](AGENTS.md) | Agent front door: non-negotiables and where verified truth lives. |
| [docs/quickstart.md](docs/quickstart.md) | Zero to a running Sim daemon + ROTA, with the test battery. |
| [docs/architecture.md](docs/architecture.md) | The three planes (cognition/harness/safety), crate map, data flow. |
| [docs/verification.md](docs/verification.md) | The verification doctrine: independent gates, DST, mutation checks. |
| [docs/operations.md](docs/operations.md) | The operator's console: CLI as built, ROTA tour, daily rhythm. |
| [docs/runbooks/](docs/runbooks/) | Procedures: soak start, halt and re-arm, kill-switch drill, troubleshooting, secrets, fixtures. |
| [docs/design/](docs/design/) | Design decisions: orchestration tracks, ROTA, CLI, perps module plan, signal contract. |
| [docs/reviews/](docs/reviews/) | Independent gate verdicts; `GATE-FINDINGS-LATEST.md` is the live findings bus. |
| [docs/research/](docs/research/) | Dated, sourced venue research (fees, APIs, perps mechanics) — venue facts live here, never in memory. |

## Layout

Sixteen-crate Rust workspace under [`crates/`](crates/) (core, gates, exec,
state, venues, ledger, cognition, ops, runner, live, paper, cli, recorder,
killswitch, sources, invariants — see spec §5.1 and
[docs/architecture.md](docs/architecture.md)). Operator-recorded venue API
captures under [`fixtures/`](fixtures/). DST, replay, and the kill-switch
drill under [`scripts/`](scripts/). Config in [`config/`](config/) — the
example is committed, the real file is operator-local, secrets are env-only.
