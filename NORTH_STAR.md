# FORTUNA NORTH STAR

> Purpose: state what FORTUNA is for, and what it is explicitly not for.
> Holds: the goal, the thesis, success metrics (process metrics only), intended-use scope, and explicit out-of-scope.
> Excludes: how it is built (ARCHITECTURE.md), what must be true (CONSTITUTION.md), and why specific choices were made (decisions/).

## Goal

FORTUNA is a single-operator autonomous trading system for prediction markets (Kalshi, Polymarket, and successors). A frontier LLM performs synthesis and decision proposal; a deterministic Rust harness owns state, execution, risk, and accountability. The model is the mind; FORTUNA is everything that makes the mind safe, stateful, and accountable.

In one paragraph: signals come in, beliefs update, proposals are gated, orders execute, outcomes are reconciled, lessons persist. It is an operating system for beliefs and trades.

## Thesis

Model capability is a rising commodity. Durable edge lives in the harness (context assembly, memory, calibration post-processing, fee-aware execution, capacity-tier positioning) and in proprietary signal inputs (Aeolus and successors), not in the model weights. The belief ledger is the proof-of-edge mechanism: every belief is scored against reality whether or not it was traded.

## Success metrics (process, never realized PnL)

FORTUNA is judged on process quality, not on a PnL number. Realized PnL is never a promotion criterion (see CONSTITUTION ramp gates). The metrics that matter:

- **CLV (closing-line value)** measured against pre-benchmark liquid snapshots, not settlement. Positive CLV is the primary GO signal.
- **Brier score** per (model, strategy, category), beating the market-implied baseline, on the forward record only.
- **Calibration curves** per category, with a minimum resolved-belief count before autonomous weight is granted.
- **Fee/PnL ratio** under 0.35 (Alpha Arena's losers died of fees; this bug class is excluded by design).
- **Forward-only validation.** LLM-decision performance is only ever measured on data the system encountered live (paper or real). Backtests validate deterministic components only.

The system-level honesty check: if after 90 live days no strategy sustains positive CLV, the synthesis pipeline is shelved and only mechanical strategies run.

## Intended-use scope

- Decision cadence of minutes to days. Maker-first execution. Prediction-market event contracts as the launch venue class.
- A perpetual-futures domain (Kinetics) exists as a designed capability with its own dedicated capital envelope and gates; see ARCHITECTURE and STATE for its current ramp status.
- Mechanical strategies run without the model and must keep making money if the brain is down. The brain is upside.
- Single operator. Human review at promotion points only; the loop runs on its own cadence otherwise.

## Explicit out-of-scope

- Not a product or an API offering.
- Not a backtesting playground for LLM decisions. LLM decisions are validated forward-only; the backtest subsystem validates deterministic components (forecast probabilities, fee models, adapters) and never LLM-decision PnL.
- Not a high-frequency or microsecond system.
- Not multi-operator. No withdrawal-scoped venue credentials, ever.
- Not a fine-tuning system. Improvement is by memory distillation into future context, never by assuming model weights carry system state.
