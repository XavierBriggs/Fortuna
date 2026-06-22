# 0008. Overfitting-deflated, point-in-time backtest with a Brier-primary GO surface

Status: Accepted (WS3, 2026-06-21). Date: reconstructed 2026-06-22.

## Context

(From `crates/fortuna-backtest`, `crates/fortuna-scoring` deflation, and the WS3 design/research docs.) A parameter sweep over historical data trivially finds a configuration that looks good by chance. A naive backtest would manufacture false GO signals through look-ahead leakage and multiple-testing. The system needed a validation path that is honest about overfitting and that cannot promote a non-edge on luck, while staying forward-only for LLM decisions.

## Options

1. Simple historical replay with a PnL or Sharpe headline. Rejected: look-ahead leakage, multiple-testing inflation, and a PnL headline contradict the process-gate decision (ADR 0002).
2. A point-in-time replay with explicit overfitting controls and a Brier-primary GO surface: strict-`<` as-of join (G-PIT), engaged-set coverage with a voided/NO guard (G-DEAD), parity with the live scoring path (G-PARITY), and deflation (purged/embargoed CSCV/PBO, Hansen SPA_c, MinTRL/effective-N, DSR against the joint family trial count). Chosen.

## Decision

The backtest is generic over a read-only `HistoricalSource`, recomputes scores through the same path as live, and emits a Brier-primary GO surface (G-TRUTH) where CLV can corroborate but never create a GO. DSR deflates against the joint family trial count, not the per-config count. Purge/embargo windows come from a real `EdgeProvider`; an empty window list is the no-purge baseline that understates overfitting and is asserted against. Runs are reproducible via FNV-1a content-hash run ids and recorded append-only in `validation_runs`. The path is read-only and constructs no order.

## Trade-offs

Considerably more statistical machinery than a naive backtest, and an honest residual: the trial-grid recal-method labels are illustrative while the applied transform is temperature scaling (disclosed in code and GAPS). Buys defensible promotion evidence and a validation path that cannot leak the future or promote a fluke.

## Consequences

fortuna-backtest, the scoring deflation library, the four named gates, and the `validation_runs` append-only table follow. This decision is the deterministic-component validation arm of the forward-only thesis (NORTH_STAR); it never validates LLM-decision PnL.
