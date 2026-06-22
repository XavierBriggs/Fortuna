# 0002. Process gates (CLV/Brier), not realized PnL, govern promotion

Status: Accepted. Date: reconstructed 2026-06-22.

## Context

(Inferred from spec Principles 4, 7 and Section 11.) Realized PnL over short windows is dominated by variance and by fees; promoting on it rewards luck and the fee-bleed failure mode that killed comparable systems. The system needed a promotion signal that measures skill early and cannot be gamed by a lucky streak.

## Options

1. Promote on realized PnL. Rejected: high variance, slow, fee-blind, gameable.
2. Promote on process metrics: CLV against pre-benchmark liquid snapshots, Brier beating the market baseline, fee/PnL ratio, calibration quality, all on the forward record. Chosen.
3. Promote on backtested PnL. Rejected: LLM-decision backtests are contaminated; forward-only validation is mandatory.

## Decision

Promotion gates are process metrics only. CLV is the primary GO signal; Brier and calibration corroborate; fee/PnL must stay under 0.35. Realized PnL is never a promotion criterion. The backtest validates deterministic components only, never LLM-decision PnL.

## Trade-offs

Requires a rigorous CLV definition (benchmark snapshots, liquidity filter) and resolved-belief counts before trusting a category. Slower to declare success than a PnL number, but honest and early.

## Consequences

CLV/Brier scoring, the benchmark-snapshot machinery, and the 90-day system-level kill criterion all follow. The backtest subsystem's GO surface is Brier-primary for the same reason (ADR 0008). This decision is load-bearing for NORTH_STAR success metrics.
