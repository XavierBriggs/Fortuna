# 0007. Append-only audit log with the C1 scoring set-once exception

Status: Accepted (C1 amendment 2026-06-14). Date: reconstructed 2026-06-22.

## Context

(From invariant I5, spec 5.5, and `crates/fortuna-ledger/migrations/20260609000001_initial.sql`.) Replaying and auditing any decision requires that the record never be edited or deleted. But a belief is scored only after the event resolves, so its outcome and Brier/CLV columns must be written once, later. A naive "never update anything" rule and "score the belief in place" requirement directly conflicted.

## Options

1. Strict immutability, store scores in a separate table. Rejected as the chosen path's alternative: workable but splits a belief's record across tables and complicates the calibration queries.
2. Append-only everywhere, with a narrow, DB-enforced exception: a belief's four scoring columns (status, outcome, brier, clv_bps) may be set exactly once; decision content stays immutable; all other stores stay strictly append-only. Chosen (C1).

## Decision

Two Postgres triggers enforce it: `fortuna_refuse_mutation` rejects UPDATE/DELETE on every append-only store; `fortuna_beliefs_guard` allows the one-time scoring write and refuses any change to belief content or a re-score. Corrections everywhere else are new superseding rows. New append-only tables must carry the trigger.

## Trade-offs

A trigger-enforced exception is more subtle than blanket immutability and must be guarded by tests. Buys a single coherent belief record plus a hard, database-level append-only guarantee that the application layer cannot bypass.

## Consequences

The triggers, the set-once `resolve_and_score` path, and the I5 invariant test follow. The exception is scoped narrowly enough that audit-replay completeness defects (missing prompt_hash, no replay tool, actor=NULL) are tracked separately as defects in GAPS.md and do not touch this guarantee. A parametric I5 test over all append-only tables is an open coverage item (CONSTITUTION gap list).
