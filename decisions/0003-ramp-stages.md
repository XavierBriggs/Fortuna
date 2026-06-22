# 0003. Four-stage ramp (Sim/Paper/LiveMin/Scaled) with human promotion

Status: Accepted. Date: reconstructed 2026-06-22.

## Context

(Inferred from spec Section 11, invariant I7, and the `Stage` enum in `crates/fortuna-runner/src/lib.rs:91`.) Capital must never be exposed to an unproven strategy or an unproven model swap, and the system must fail safe by demoting automatically while requiring a human to promote.

## Options

1. Binary on/off (paper then live). Rejected: too coarse; no graduated exposure or shadow comparison.
2. A four-stage ladder Sim -> Paper -> LiveMin -> Scaled with explicit per-stage thresholds, human-gated promotion, automatic demotion on breach, and a separate shadow track for model swaps. Chosen.

## Decision

Strategies declare a `Stage`. A `Sim` runner refuses non-`Sim` strategies. Each stage has explicit entry gates (CONSTITUTION ramp gates); promotion is a human CLI action, demotion is automatic on drawdown halt, and a scale-up step resets on any halt. Model swaps require a shadow period meeting Brier/CLV parity.

## Trade-offs

More states and operator ceremony than a binary switch. Buys graduated risk, auditable promotion records, and a fair model-comparison mechanism. The forward rails are unit-tested at construction but not yet DST-driven end to end (CONSTITUTION gap list).

## Consequences

The `Stage` enum, the promotion-record and shadow-comparison invariant tests (I7), and the LiveMin $500 cap all follow. STATE.md reports each strategy's current stage.
