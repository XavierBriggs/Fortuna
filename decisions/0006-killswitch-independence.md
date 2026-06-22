# 0006. Out-of-band kill switch with no shared dependencies

Status: Accepted. Date: reconstructed 2026-06-22.

## Context

(Inferred from invariant I4, spec 5.15, and `crates/fortuna-killswitch`.) A kill switch that shares dependencies with the system it kills is useless exactly when needed: if Postgres, the event loop, or the LLM provider is wedged, a kill path that depends on them is wedged too.

## Options

1. A kill command inside the main runtime. Rejected: dies with the runtime.
2. A standalone process holding its own venue credentials, with no dependency on Postgres, the cognition runtime, or the event loop, reachable by Slack and local CLI, with a durable on-disk revocation sentinel. Chosen.

## Decision

The kill switch is a separate binary, dependency-locked at the Cargo level to exclude ledger/sqlx/Postgres/cognition. Its default action is freeze-and-cancel; emergency flatten is best-effort without the planner. Perps coverage uses its own credential pair and reduce-only IOC closes that still pass the perp gate. Re-arm is refused while a kill sentinel is present or unverifiable. Re-arm and kill reversal are CLI-only.

## Trade-offs

Duplicated credential handling and a separate process to operate and test. Buys a kill path that works when nothing else does. The Slack-trigger path and the monthly test cadence are operational and not yet pinned by a test (CONSTITUTION gap list).

## Consequences

fortuna-killswitch's dependency lock and the revocation-guard tests (I4) follow. The flatten-planner exemption is explicit so the kill path never depends on cost estimation. This decision is why I4 names "no Postgres" in the constitution.
