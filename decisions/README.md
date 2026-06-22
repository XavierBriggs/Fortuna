# Architecture Decision Records

> Purpose: record why FORTUNA is the way it is, one decision per file.
> Holds: append-only ADRs. Each is context, options, decision, trade-offs, consequences.
> Excludes: current state (STATE.md) and what-must-be-true (CONSTITUTION.md). ADRs are superseded by new ADRs, never edited.

These ADRs were reconstructed during the 2026-06-22 canon review from `docs/spec.md`, the code, and the spec changelog. Sections marked (inferred) are reconstruction, not contemporaneous record. A future significant decision adds a new numbered file here; superseding an ADR adds a new ADR that references it.

| # | Decision | Status |
|---|---|---|
| [0001](0001-model-agnostic-belief-ledger.md) | Model-agnostic belief ledger as the proof-of-edge mechanism | Accepted |
| [0002](0002-process-gates-over-realized-pnl.md) | Process gates (CLV/Brier), not realized PnL, govern promotion | Accepted |
| [0003](0003-ramp-stages.md) | Four-stage ramp (Sim/Paper/LiveMin/Scaled) with human promotion | Accepted |
| [0004](0004-fixed-point-money.md) | Fixed-point integer money (Cents/PerpPrice), never float | Accepted |
| [0005](0005-in-house-deterministic-core.md) | In-house deterministic core instead of NautilusTrader | Accepted |
| [0006](0006-killswitch-independence.md) | Out-of-band kill switch with no shared dependencies | Accepted |
| [0007](0007-append-only-audit-with-c1-exception.md) | Append-only audit log with the C1 scoring set-once exception | Accepted |
| [0008](0008-backtest-overfitting-controls.md) | Overfitting-deflated, point-in-time backtest with a Brier-primary GO surface | Accepted |
