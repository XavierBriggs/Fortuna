# FORTUNA

> Purpose: orient a newcomer and get the system building and testing.
> Holds: a one-paragraph description, the build/run/test commands, and the index of the canon.
> Excludes: every standalone fact. This file points; it does not hold. Each topic lives in exactly one canonical document linked below.

FORTUNA is a single-operator autonomous trading system for prediction markets. A frontier LLM proposes structured beliefs and trade ideas; a deterministic Rust harness owns state, execution, risk, and accountability, and is the only thing that can reach a venue. The model proposes; the harness disposes. What it is for, and not for, is in [NORTH_STAR.md](NORTH_STAR.md).

## Build, run, test

Rust workspace (18 crates under `crates/`), toolchain pinned in `rust-toolchain.toml`.

```bash
# Build everything
cargo build --workspace

# Test everything
cargo test --workspace

# Deterministic simulation corpus (seeded fault injection)
scripts/run-dst.sh 2000

# Protected-invariant guard (run before any commit touching crates/fortuna-invariants)
scripts/check-protected-invariants.sh

# Canon guardrail (closed set + invariant-to-test coverage)
tools/check-canon.sh
```

Binaries: `fortuna` (operator CLI: lifecycle, halt, re-arm, backtest, validate, paper-demo), `fortuna-live` (daemon), `fortuna-killswitch` (standalone kill switch), `fortuna-recorder` (read-only capture), `replay-verify`. Config is TOML (`config/fortuna.example.toml`); secrets only via environment, never in the repo. The Postgres-backed tests provision their own database via `#[sqlx::test]`.

## The canon (the only source-of-truth documents)

| Document | The one question it answers |
|---|---|
| [NORTH_STAR.md](NORTH_STAR.md) | What is FORTUNA for, and not for? |
| [CONSTITUTION.md](CONSTITUTION.md) | What must always be true? (invariants, each mapped to a test) |
| [ARCHITECTURE.md](ARCHITECTURE.md) | How does the system fit together? (crate map, data flow, boundaries) |
| [STANDARDS.md](STANDARDS.md) | How do I write code here, in ways a linter cannot enforce? |
| [STATE.md](STATE.md) | What is actually running right now? (generated model inventory) |
| [decisions/](decisions/) | Why is it this way? (append-only ADRs) |
| [canon.manifest](canon.manifest) | Is the canon still closed? (the CI guardrail) |
| [.claude/CLAUDE.md](.claude/CLAUDE.md) | Repo rules and where truth lives |

Working ledgers that are not canon: `GAPS.md` (open issues and deferrals), `ASSUMPTIONS.md`, `CHANGELOG.md`, `BUILD_PLAN.md`. Design reference (no longer source of truth for invariants or the crate map): `docs/spec.md`.
