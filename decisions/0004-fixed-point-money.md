# 0004. Fixed-point integer money (Cents/PerpPrice), never float

Status: Accepted. Date: reconstructed 2026-06-22.

## Context

(Inferred from Olympus house style, spec Principle 9, spec 5.15, and `crates/fortuna-core/src/money.rs`, `perp.rs`.) Floating-point money accumulates rounding error and makes fee and exposure math unauditable. A trading system that rounds the wrong way against itself bleeds slowly and invisibly.

## Options

1. `f64` money with care. Rejected: rounding error, non-determinism, no type-level safety.
2. `Decimal` everywhere. Rejected: slower, and still needs a discipline for rounding direction and for the venue tick domains.
3. Integer-cent newtypes (`Cents(i64)`), a venue-scoped `PerpPrice(i64)` in ten-thousandths for perps, checked arithmetic, `Decimal` only at conversion boundaries, rounding always against us, and type-level separation of `PerpPrice` from `Cents`. Chosen.

## Decision

Money is integer cents in newtypes. Perps ride in `PerpPrice` and convert to `Cents` only at notional/PnL/fee boundaries, rounded against us. `Decimal` appears only when parsing venue payloads. No `f64` touches money, prices, sizes, or fees in the core.

## Trade-offs

Conversion boundaries require care and explicit rounding direction. Buys determinism, auditability, and compile-time prevention of cross-domain price assignment.

## Consequences

`Cents`, `PerpPrice`, the fee-rounds-against-us rule, and the type separation are codified in STANDARDS and promoted as structural invariant S2 (CONSTITUTION). The review found zero float-on-money defects, which this decision is what makes achievable.
