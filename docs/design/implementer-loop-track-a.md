# Implementer loop — TRACK A (re-missioned 2026-06-14)

Track-A owns the `fortuna-live` daemon + composition surface. **Prior mission
COMPLETE** (T4.1 daemon, OBS-2 ingestion funnel, F7 Aeolus↔Kalshi weather plug-in,
`drive()` resolver wiring, kill-switch perp flatten — all merged + verifier-gated).
This file is the **new** queue.

Authority: `docs/spec.md` > `CLAUDE.md` > `docs/design/implementer-loop.md` (base
protocol — read it for the full Ralph + DoD rules) > this file (queue). Read the bus
(`docs/reviews/GATE-FINDINGS-LATEST.md`) at priority (a) each cycle; a BLOCK naming
track-a preempts the queue.

Worktree `/Users/xavierbriggs/fortuna-wt-a`, branch `track-a`, based on main.

## Queue (top-down, one slice per cycle)

### A-next-1 — S5b: config-driven synthesis calibration model id
**Audit Major.** `crates/fortuna-live/src/daemon.rs` hard-codes
`SYNTH_CALIBRATION_MODEL = "claude-fable-5"` as the calibration-scope key. The
runtime *mind* is already config-driven (the `[cognition]` model registry / tiers);
only the calibration **scope key** still pins the constant. Thread the configured
synthesis-tier model id into the calibration scope so a model swap keys calibration
under the right `(model_id, strategy, category, kind)` (spec 5.10) instead of a stale
constant. Tests-first: a test that the scope uses the configured id, not the literal.
Small + surgical; no behavior change beyond the keying.

>> DONE (on main as S5b; merged into track-a @0424134). The calibration scope now keys
   on the configured `[cognition].synthesis_model` (`compose_runner` ×2 + `run_weekly_
   review`); the `SYNTH_CALIBRATION_MODEL = "claude-fable-5"` constant is retired.
   track-a built an equivalent S5b in parallel (@e3b59bc) before main's landed; dropped
   as a duplicate when main's canonical (gated) version merged.

### A-next-2 — `fortuna-live` library-boundary refactor
**Audit Major (untraced).** Trace what binary-grade orchestration lives in the
`fortuna-live` **library** that belongs behind a bin/ops boundary. Characterize first
(`daemon_smoke` must stay byte-identical), then move it without behavior change. If
the trace shows the boundary is actually fine, record that finding in GAPS and close
the item — a verified "no action needed" is a valid outcome.

>> DONE 2026-06-15. TRACE (verifier, GAPS dispositions 2026-06-15): part A (the ingestion
   env-read in the lib) was already fixed on main @ba95430; part B was the kalshi-demo
   transport builder doing env + PEM-FILE IO inside the lib. FIXED part B: extracted
   `resolve_kalshi_demo_creds(env)` (the credential IO — env-gate + PEM file read,
   ISOLATED + testable), made `build_kalshi_demo_transport(key_id, key_pem, clock)`
   IO-FREE, and REMOVED the IO+compose mixer `compose_kalshi_runner`. main.rs now
   orchestrates resolve → build → `compose_kalshi_runner_with_transport` at the process
   edge. The 3 credential-gate tests re-point to `resolve_kalshi_demo_creds`
   (missing/placeholder/unreadable — all KEPT, none weakened). RESIDUAL (deliberate): the
   PEM file read stays in the testable resolver rather than literally inlined in main.rs —
   inlining would force dropping the unreadable-path test; coverage wins. `daemon_smoke`
   byte-identical (sim path untouched). Full battery green; invariants + fortuna-gates
   UNTOUCHED.

NET: A's re-mission queue is COMPLETE (A-next-1 + A-next-2 both DONE). RALPH STOP — see
the stop entry in GAPS.md.

## DoD + Ralph protocol
Per `docs/design/implementer-loop.md` and `CLAUDE.md` "Definition of done": tests from
spec BEFORE code; no `panic!`/`unwrap`/`expect` in money paths; all time via the
injected `Clock`; **`fortuna-invariants` additions-only** (never weaken an assertion);
`fortuna-gates` source untouched unless the slice is itself a gate change. Green before
every commit: `cargo fmt --check`; `SQLX_OFFLINE=true
DATABASE_URL=postgres:///fortuna?host=/tmp cargo clippy --workspace --all-targets --
-D warnings`; `cargo build -p fortuna-killswitch` **then** `cargo test --workspace`;
`scripts/run-dst.sh`. Update GAPS/ASSUMPTIONS/CHANGELOG + tick BUILD_PLAN. Commit on
`track-a`; **never push**; never simulate operator actions. Queue empty ⇒ RALPH STOP
(a line in GAPS).
