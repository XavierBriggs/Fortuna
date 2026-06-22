# STATE.md generator specification

> Purpose: specify how STATE.md is produced, so it is never hand-written.
> Holds: inputs, output schema, refresh cadence, and degradation behavior of `gen-state.sh`.
> Excludes: the inventory content itself (that is STATE.md, the output) and risk-tier rationale (the canon review and ADRs).

## Why a generator

STATE.md answers "what is actually running right now," which drifts constantly. Hand-writing it guarantees staleness. It is emitted by `tools/gen-state.sh` from three sources and is the one canonical document that is a tool output, not prose.

## Inputs

1. `tools/state-inputs.tsv` (committed). Static facts the running system cannot self-report: the module list, the risk tier (a review judgment = materiality x change frequency), the design-intended declared stage, and known-issue reference tokens. Tab-separated, comment lines start with `#`.
2. The ledger (Postgres), used only when `DATABASE_URL` is set and `psql` is available:
   - latest `validation_runs.computed_at` per scope -> the "Last validated" column.
   - the GO/NO-GO surface per scope -> the "GO surface" column.
   - (extension point) operator stage records -> the effective stage, overlaying the declared stage.
3. `GAPS.md` (the open-issue ledger): the open-section count, and a per-module match on the known-issue reference token.

## Output schema (STATE.md)

A generated header (purpose/holds/excludes, generated-at stamp, git rev, DB-connected flag), then:
- A model-and-strategy inventory table: `Module | Kind | Risk tier | Declared stage | Last validated | GO surface | Known issues`.
- An "Open issues" section pointing to GAPS.md with the open-section count (never duplicating GAPS text).
- A risk-tier legend.

## Refresh cadence

Run `tools/gen-state.sh` (a) in CI on every merge to the default branch, (b) at the start of any weekly/monthly review, and (c) before any promotion decision. The generated-at stamp and git rev make staleness visible. STATE.md carries a "DO NOT hand-edit" banner; edits belong in the inputs or the system.

## Degradation

Without a database, "Last validated" and "GO surface" degrade to `n/a (no db)` and the header records "DB: not connected." The static inventory and GAPS-derived columns still populate, so the document is always emittable. The generated-at stamp is a point-in-time report timestamp and is not a decision-path time read (STANDARDS S3 does not apply to it).

## Promotion note

On promotion to the repo root, `gen-state.sh`, `state-inputs.tsv`, and this spec move under `tools/`; STATE.md is written to the repo root and is added to `canon.manifest` under `[generated]`.
