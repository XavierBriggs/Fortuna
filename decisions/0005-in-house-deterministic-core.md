# 0005. In-house deterministic core instead of NautilusTrader

Status: Accepted (spec v0.2). Date: reconstructed 2026-06-22.

## Context

A mature trading framework (NautilusTrader) was available. Its main value is a high-fidelity backtest engine. FORTUNA validates LLM decisions forward-only, so that value does not apply, and the hard parts (prediction-market adapters, settlement metadata, oracle-delay handling) do not exist in it.

## Options

1. Adopt NautilusTrader as a dependency. Rejected: its core value (backtest matching) is unused under forward-only validation; its surface is large; the venue-specific hard parts would be written in-house regardless.
2. Greenfield in-house core that adopts Nautilus's patterns (single-threaded deterministic bus, replay discipline) without the dependency. Chosen.

## Decision

Build an in-house deterministic core: a single-threaded event bus, an injected `Clock`, a sealed-order gate pipeline, and deterministic simulation testing (a sim venue with seeded fault injection) as the primary test methodology. Adopt Nautilus's bus and replay discipline as patterns only.

## Trade-offs

More code to own up front. Buys a small, auditable surface, full control of the prediction-market adapters, and determinism the DST corpus can exercise. Documented revisit trigger: the equities rung (post-$25k), where broker integrations may justify revisiting.

## Consequences

fortuna-core's bus and `Clock`, the DST harness, and the sealed `GatedOrder` all follow. Determinism becomes a structural invariant (S3) and the backtest's reproducibility (ADR 0008) builds on the same `Clock` discipline.
