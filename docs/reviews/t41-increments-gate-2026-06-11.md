# Review: t41-increments (range gate) — 2026-06-11
Base: 16478bb  Head: 1b3fabb  Verdict: BLOCK
Protected crate touched: no (`git diff 16478bb..1b3fabb --stat -- crates/fortuna-invariants` is empty)

Commits under review: 8544c3f (T5.B1 tick), 4f107bb (ledger fixes 1-4), d8bd003
(journal-generic SimRunner + Pg intent journal), 25d89a2 (verdict recording),
6ed4099 (PgAuditSink bridge), 1b3fabb (graceful-shutdown contract).
Reviewed from a detached worktree at 1b3fabb; implementer reasoning not read;
only docs/reviews/GATE-FINDINGS-LATEST.md consulted per the gate charter.

## Criteria (fixed before reading the diff)

### A. Remediation of GATE-FINDINGS-LATEST.md (4 items)
- A1 GAPS false-claim correction, corrected-visibly-never-erased (MAJOR x2 history):
  **FIXED** — GAPS.md now carries a bracketed correction naming what was false
  ("claimed the degrade_alerts/CalibrationParamsRepo ASSUMPTIONS entry and a
  Polymarket '95' erratum already existed"), since when ("false from the f-batch
  closure until 2026-06-11"), and what is now true. Both real corrections exist
  AND are themselves accurate, independently verified:
  - ASSUMPTIONS.md degrade_alerts/CalibrationParamsRepo wiring-status entry
    present (also self-describes the prior false claim).
  - research.md ERRATUM is the honest-reformulation branch: "neither '96' nor
    '95' matched a canonical tally; 38 rows in the sources table; 93 archived
    files (80 pages + 13 schemas)". Verified by count: Sources section has 40
    pipe-lines = 38 data rows; `ls raw/pages | wc -l` = 80, `ls raw/schemas` = 13.
- A2 F5 disposition in GAPS: **FIXED** — "F5 DISPOSITION (ledger-gate fix 2)"
  block present; claims verified (`.gitignore` lines 8-9 cover data/ and
  .playwright-mcp/; `git ls-files data/ .playwright-mcp/` = 0 tracked).
- A3 WS public `trade` frame in fixture follow-up list: **FIXED** — GAPS
  operator-session list now requires capturing "a public WS `trade` frame
  (never observed in the 60-capture session — ledger-gate fix 3)".
- A4a recorder single-writer ASSUMPTIONS entry: **FIXED** — entry present,
  names the JSONL-interleave corruption mode and the T4.4 enforcement plan.
- A4b T5.B0 note correction: **FIXED** — note now says the log header records
  "RUNNING PARAMETERS (not a command — ledger-gate fix 4b)" and supplies the
  actual restart command inline.
- A4c ROTA V-5 correction: **FIXED and accurate** — note now says cognition is
  DEV-ONLY; verified: fortuna-ledger/Cargo.toml has fortuna-cognition under
  `[dev-dependencies]` only; runtime deps = core, venues, exec, gates.

### B. PgAuditSink bridge (6ed4099) — spec I5 (line 44), failure table (line 367)
- B1 Append failure halts order flow (test-proven): **PASS with Major caveat (F1)**
  — crates/fortuna-live/tests/pg_audit.rs
  `daemon_audit_rows_land_in_postgres_and_audit_death_halts`: DROP TABLE audit
  → next audit-bearing tick reports `halted` → following tick submits 0 orders.
  Bridge is fail-synchronous: `append` blocks on a per-job reply channel; Pg
  error / dead thread / failed spawn all map to `RunnerError::AuditFailed`,
  which the runner answers with `set_halt(HaltScope::Global, ...)`
  (runner.rs `fn audit`). The caveat is F1 below: the halt does not abort an
  in-flight, already-gated proposal group.
- B2 No silent loss window: **PASS on the healthy path** — there is no buffer:
  every `append` blocks until the INSERT commits (audit_bridge.rs round-trip;
  AuditWriter::append awaits `execute`). Gate-decision rows are appended in
  Phase A strictly before Phase B's network submission (runner.rs ~826-880),
  so an order's gate audit row is durable before placement. Spec line 367
  ("Audit write failure | Trading halts (no audit, no trading)") permits no
  loss window; the failure-path window found is F1, and it is unledgered
  (no GAPS/ASSUMPTIONS mention) → Major per the fixed rubric.
- B3 Deterministic ordering: **PASS (structural)** — single-threaded runner →
  sequential blocking appends → one mpsc sender/consumer pair → single writer
  thread → monotonic AuditIds from one IdGen. i5_audit_append_only asserts
  byte-identical replay reads. No dedicated bus-order-vs-audit-order
  cross-check test exists; noted, not required by the contract.
- B4 DST/sim path Pg-free: **PASS** — `grep -c sqlx` = 0 in fortuna-runner and
  fortuna-exec Cargo.toml; bridge lives in edge crate fortuna-live;
  all DST harnesses compose `SimRunner::new` (0 uses of `new_with_journal`
  under crates/fortuna-runner/tests/); `memory_journal_default_unchanged`
  pins the default path (same seed → same 3 submissions, no Postgres).

### C. Journal-generic SimRunner + Pg intent journal (d8bd003)
- C1 Sim/DST keeps MemoryJournal: **PASS** — `SimRunner<J = MemoryJournal>`
  default; historical constructor unchanged in signature; DST files use it.
- C2 Pg journal same state-machine semantics, integration-proven: **PASS** —
  fortuna-ledger/tests/ledger.rs `pg_journal_survives_crash_and_recovers_identically`
  (crash → fresh handle → byte-identical `intents()` Debug, Filled/Acked
  statuses preserved, `boot_reconcile` adopted/closed_unsubmitted/
  orphans_cancelled all asserted) + range-added pg_journal.rs (real tick →
  3 intent lineages, Created-before-SubmitAttempted rows in Pg, fresh-handle
  recover fold OK). Crash-resubmit AlreadyExists→Acked rides the SHARED
  generic OrderManager (fortuna-exec/tests/manager.rs:198-234); encoding
  fidelity proven by the byte-identical fold.
- C3 No money-path semantics changed: **PASS** —
  `git diff 16478bb..1b3fabb -- crates/fortuna-exec` is EMPTY; runner diff
  adds shutdown() and the generic constructor only; sweep found 34 asserts
  added, 0 removed, no #[ignore], no tolerance/case changes anywhere in range.

### D. Graceful-shutdown contract (1b3fabb) vs BUILD_PLAN T4.1 BINDING text
- D1 SIGTERM handler exists, SignalKind::terminate-inclusive: **FAIL (absent)**
  — `grep -rn SignalKind crates --include='*.rs'` finds no signal handling;
  fortuna-live main.rs bails ("the runtime composition is not wired yet").
  T4.1 is honestly unticked, so no completion claim is violated — but the
  BINDING contract is unmet at HEAD and the smoke cannot yet assert
  "SIGTERM →". See also F3 (a test comment claims the handler exists).
- D2 cancel-working-orders + final-audit-row ASSERTED: **PASS at the
  shutdown()-function level** — tests/shutdown.rs asserts cancelled >= 1,
  journaled cancel events in Pg, and `COUNT(*) FROM audit WHERE
  kind='daemon_shutdown'` == 1. The SIGTERM→ variant pends D1.
- D3 Idempotent and bounded: **PASS (sim scope)** — second shutdown() asserted
  clean (working=0); cancel-timeout fault asserted honest (`unknown>=1,
  cancelled==0` — never folded into cancelled); `release_if_terminal` only
  releases on terminal statuses, so unknown-state orders keep reservations.
  Boundedness against an unresponsive REAL venue is delegated to adapter
  timeouts and untestable until T4.2 (venue selection refuses kalshi).
- D4 `fortuna stop` A1 dependency satisfiable: **PASS in principle** — the
  daemon_shutdown row lands durably in Pg through the bridge (test-asserted);
  the observable line exists once D1 wires SIGTERM → shutdown().

### E. Battery at 1b3fabb
- fmt: **PASS** — `cargo fmt --check` exit 0.
- clippy: **FAIL (F2)** — `cargo clippy --workspace --all-targets -- -D warnings`
  exit 101: "error: unused import: `set_arb_books`" + "error: function
  `set_arb_books` is never used" → "error: could not compile `fortuna-live`
  (test \"shutdown\") due to 2 previous errors".
- workspace tests: **PASS** — tests_exit=0, totals: passed=681 failed=0 ignored=0.
- invariants per-test: **PASS** — i1 2/2, i2 2/2, i3 1/1, i4 1/1 (9.04s),
  i5 1/1, i6 3/3, i7 3/3, +3 doctests (incl. 2 compile-fail). All ok.
- DST at 10000: **PASS, all stages** —
  `[dst] regression corpus: 0 seed(s)` (corpus empty by history; identical on
  main repo — pre-existing, not a range defect);
  `[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations`;
  `[synthesis-dst] master seed 1781178101381 -> 10000 scenario(s)` /
  `totals: 27230 orders, 41786 proposals, 134848 cognition failures, 119568 beliefs`;
  `[settlement-dst] master seed 1781178045201 -> 10000 scenario(s)` / arms
  `{SettleClean: 919, SettleThenCorrect: 882, Void: 903, Dispute: 918,
  VenueMismatch: 841, CanonicalDivergence: 897, OrphanScan: 909,
  AuditDeath: 923, WideBook: 940, Overdue: 968, MultiLegGroup: 900}` — all 11
  arms hit; DST_EXIT=0, SYNTH_EXIT=0.
- mechanical sweep: **PASS** — no unwrap/expect/panic/wall-clock/HashMap in
  src/ additions (all unwraps are in test files); `manager.intents()` is a
  BTreeMap (deterministic shutdown cancel order); no new `place(` sites; no
  GatedOrder constructors outside fortuna-gates; secrets grep hits are
  doc-text only (descriptions of sweeps, no literals).
- protected crate: **untouched**.

## Findings

- [Major] F1 — Audit-death fall-through: an audit append failure landing
  mid-Phase-A, AFTER legs have passed the gates, still submits the staged
  group in Phase B. Reproduction (scratch probe, NOT committed): a custom
  AuditSink failing exactly at append index 21 (= the third leg's first
  gate_decision record in the standard 3-leg sim composition: seed 42,
  t0 2026-06-11T12:00Z, fixtures from crates/fortuna-live/tests/common/mod.rs)
  yields `orders_submitted=3 halted=true` in the same tick — three orders
  placed at the venue after the audit store died, with leg-3's gate_decision
  rows AND all subsequent order/ack audit rows lost (`audit_dead`
  short-circuits every later append). Spec line 367: "Audit write failure |
  Trading halts (no audit, no trading)"; I5 (line 44): "Sufficient to replay
  any decision after the fact" — those decisions are not replayable from the
  audit store. Mitigations observed: the halt does engage in the same tick
  (nothing later passes the gates); the durable Pg intent journal
  independently records the full order lifecycle (journal-before-network);
  the window is bounded to one in-flight proposal group. The code path is
  pre-existing runner logic, but the commit under review claims "I5 holds
  through the daemon's Postgres path" and this window is unledgered →
  Major per the fixed rubric (B2: "any unledgered loss window is Major").
  Fix direction: on `audit_dead` flipping during Phase A, abort the staged
  group (release reservations, submit nothing) — symmetric with the existing
  group_rejected abort; or ledger the window with explicit operator sign-off.
  Probe source preserved at /tmp/i5probe (path-deps pointed at the removed
  review worktree; re-point at the repo to re-run).
- [Minor] F2 — clippy -D warnings red at HEAD: unused import + dead helper in
  crates/fortuna-live/tests/shutdown.rs / tests/common/mod.rs. Violates
  CLAUDE.md DoD item 2 ("clippy -D warnings ... pass", no exceptions).
  Trivial fix; must be green before the next gate.
- [Minor] F3 — Ledger-accuracy class: crates/fortuna-live/tests/shutdown.rs:6-7
  states "the SIGTERM handler in main() calls exactly this function" —
  present tense, false at HEAD (main.rs has no signal handling and refuses to
  run). Given finding 1's MAJOR x2 history, present-tense claims about
  not-yet-existing wiring are exactly the defect class under watch. Reword to
  future/contract tense or land the handler.
- [Minor] F4 — The BINDING SIGTERM contract (BUILD_PLAN T4.1) remains unmet at
  HEAD (no SignalKind::terminate handler; smoke cannot assert SIGTERM →
  cancel + final audit row). Honestly unticked, so recorded as an open
  contract item, not a false claim. Also: shutdown boundedness against an
  unresponsive real venue is undefined until T4.2 venue adapters land.

## Verdict reasoning

A-items: all four remediations FIXED and independently re-verified accurate
(the corrections themselves were fact-checked; none is wrong-again). B/C/D
increments are well-tested, deterministic, and Pg-free in the core. The BLOCK
is carried by F1 (Major, reproduced, unledgered, directly against the spec
line the commit claims to uphold) plus F2 (battery red: clippy). Per the
taxonomy, Major → BLOCK unless the operator waives; F1's fix is small and
symmetric with existing abort logic.

## Commands run (verbatim verdict lines)

```
cargo fmt --check                                  → FMT_EXIT=0
cargo clippy --workspace --all-targets -- -D warnings
  → error: could not compile `fortuna-live` (test "shutdown") due to 2 previous errors
  → CLIPPY_EXIT=101
cargo test --workspace                             → tests_exit=0
  totals: passed=681 failed=0 ignored=0
cargo test -p fortuna-invariants                   → all suites ok (13 tests + 3 doctests)
./scripts/run-dst.sh 10000                         → DST_EXIT=0
  [dst] OK: 0 corpus + 10000 random seeds, zero invariant violations
  [synthesis-dst] master seed 1781178101381 -> 10000 scenario(s)   (SYNTH_EXIT=0)
  [settlement-dst] master seed 1781178045201 -> 10000 scenario(s)  all 11 arms hit
probe (scratch, /tmp/i5probe):
  pass1 orders_submitted=3
  pass2 fail_at=21 orders_submitted=3 halted=true
  FINDING REPRODUCED: 3 orders submitted AFTER the audit store died
git diff 16478bb..1b3fabb --stat -- crates/fortuna-invariants  → (empty)
```
