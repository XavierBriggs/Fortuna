# Implementer loop — TRACK C (re-missioned 2026-06-14)

Track-C owns the perp basis-v2 strategy + the belief/scoring pipeline. **Prior mission
COMPLETE** (scalar beliefs, 3-tier cognition, perp basis-v2 V0–V5, A2d funding pipeline,
daemon wire-in — all merged + verifier-gated). This file is the **new** queue.

Authority: `docs/spec.md` > `CLAUDE.md` > `docs/design/implementer-loop.md` (base
protocol) > this file (queue). Read the bus (`docs/reviews/GATE-FINDINGS-LATEST.md`) at
priority (a) each cycle; a BLOCK naming track-c preempts the queue.

Worktree `/Users/xavierbriggs/fortuna-wt-c`, branch `track-c`, based on main.

## Queue (top-down, one slice per cycle)

### C-next-1 — PerpTick producer (make the basis-v2 arm non-inert)
The perp basis-v2 strategy is wired into the daemon but **INERT until a `PerpTick`
producer feeds it** (the gap track-C itself found in slice 4a; `KineticsPerpObservation`
exists). Build the producer: Kinetics-demo market data → `PerpTick`, on the live
ingestion path, **fixtures-first** (`docs/research/venue/kinetics-perps-2026-06-10/`;
never invent venue behavior). Opt-in + fail-closed like the funding poller (gated on the
same `[perp_event_basis_v2]` presence). Clock-injected. Gate target: the v2 arm receives
ticks and produces **UNSIZED** proposals (I6) in Sim/demo (I7) — no live, no sizing.

### C-next-2 — recorder e2e fixture
The remaining GAPS residual: the recorder end-to-end fixture for paper-fill realism
(maker fills count only on trade-through, never touch). Per
`docs/runbooks/fixture-recording.md`. Real, provenanced recordings — never synthetic
fills.

## DoD + Ralph protocol
Per `docs/design/implementer-loop.md` and `CLAUDE.md` "Definition of done": tests from
spec BEFORE code; **venue behavior fixture-confirmed, never invented**; the model stays
**propose-only / UNSIZED (I6)**; **Sim/demo only, no live promotion (I7)**; no
`panic!`/`unwrap`/`expect` in money paths; all time via `Clock`; **`fortuna-invariants`
additions-only** (run `scripts/check-protected-invariants.sh` before commit). Green
before every commit: `cargo fmt --check`; `SQLX_OFFLINE=true
DATABASE_URL=postgres:///fortuna?host=/tmp cargo clippy --workspace --all-targets --
-D warnings`; `cargo build -p fortuna-killswitch` **then** `cargo test --workspace`;
`scripts/run-dst.sh`. Update GAPS/ASSUMPTIONS/CHANGELOG + tick BUILD_PLAN. Commit on
`track-c`; **never push**; never simulate operator actions. Queue empty ⇒ RALPH STOP
(a line in GAPS).
