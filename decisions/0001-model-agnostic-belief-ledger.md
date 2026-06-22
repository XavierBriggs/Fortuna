# 0001. Model-agnostic belief ledger as the proof-of-edge mechanism

Status: Accepted. Date: reconstructed 2026-06-22 (decision predates, spec v0.1-0.9).

## Context

(Inferred from spec Principles 3, 5, 6 and `crates/fortuna-cognition`.) The system's durable edge was judged to live in the harness and in proprietary signals, not in model weights, which are a rising commodity. A way was needed to measure edge that did not depend on which model produced it, and that scored ideas whether or not they were traded.

## Options

1. Trade-first: the model emits trades; measure the trades. Rejected: conflates decision quality with execution and sizing, and scores only what was traded.
2. Belief-first with a model-agnostic ledger: the model emits structured beliefs behind a `Mind` trait; trades are derived deterministically by comparing calibrated beliefs to prices; every belief is scored against reality. Chosen.
3. Fine-tune a model on outcomes. Rejected: assumes weights carry state; not auditable; not model-agnostic.

## Decision

Beliefs are first-class and immutable (new rows supersede). The model speaks one schema over the `Mind` trait. The belief ledger plus per-(model, strategy, category) scoring is the proof-of-edge mechanism. Swapping models is a config change plus a mandatory shadow period.

## Trade-offs

More machinery than trade-logging; requires a scoring job, calibration, and a benchmark-snapshot CLV definition. Buys model-agnosticism, edge attribution, and the ability to score untraded beliefs.

## Consequences

The `beliefs` table, provenance, the scoring job, and calibration all exist because of this. It is the reason promotion can be governed by process metrics (ADR 0002) and the reason model swaps are a shadow comparison (CONSTITUTION I7).
