# Review: overnight-code-gate (Part 1: code commits) — 2026-06-11
Base: b8fa0c8  Head: 16478bb  Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no (git diff b8fa0c8...16478bb -- crates/fortuna-invariants/ is EMPTY; zero commits in range touch it)

Scope: C1 94d651a (T5.B0 recorder), C2 213e41f (T5.B1 spec v0.9), C3 f551d84
(env-leak fix), C4 3eaa5e4 (T4.1 increment), C5 1485d98+f9b18e1 (fit-validation
notes), D standard battery at HEAD. Worktree /tmp/fortuna-g1 (detached 16478bb),
DATABASE_URL=postgres://localhost/fortuna_dev. Incident/ledger/fixtures side
covered by the concurrent Part 2 verifier — not duplicated here. No file under
docs/reviews/ was read.

## Criteria (fixed before reading the diff)

### C1 — 94d651a fortuna-recorder (T5.B0)
- C1.1 Edge binary; no library crate gained ambient time: PASS — grep over all
  crates/*/src: SystemTime::now/Instant::now only in fortuna-recorder/src/main.rs
  (binary edge) + the pre-existing record_kalshi_fixtures example; both ledgered
  in ASSUMPTIONS.md:1187-1194. lib.rs is pure (no clock, no IO).
- C1.2 No panic/unwrap/expect in non-test lib code: PASS — diff sweep: every
  expect/panic! hit is inside #[cfg(test)] or tests/; main.rs uses anyhow
  Context + unwrap_or defaults (binary, allowed).
- C1.3 No f64 price mangling: PASS — prices parsed by to_tenthousandths into
  i64 (checked mul/add, refuses negatives/>4dp/garbage); wire body stored
  VERBATIM as string; derived top-of-book is integer ten-thousandths. Live
  output row inspected (data/perishable/2026-06-11/perp_orderbook.jsonl):
  body verbatim string + integer derived fields. f64 grep over diff: only a
  doc comment.
- C1.4 Append semantics / single-writer documented: PARTIAL — OpenOptions
  create+append confirmed (main.rs:92-95); single-writer assumption NOT
  documented in crate docs, plan doc, or ASSUMPTIONS.md (Minor finding F1).
- C1.5 8 unit tests green: PASS — `cargo test -p fortuna-recorder`: "test
  result: ok. 8 passed; 0 failed". Tests cover parser refusals, ordering
  independence (live-capture fixture contradicting spec ordering), zero-qty
  levels, one-sided/empty book refusal, row shape, UTC day rollover.
- Corroboration: recorder live at review time (pid 79813, started 11:52PM PDT
  = 06:52 UTC matching the BUILD_PLAN note), 5 JSONL streams growing.

### C2 — 213e41f spec v0.9 (T5.B1)
- C2.1 Version bump + reference updates: PASS — spec preamble "Version 0.9
  (build-ready draft). June 11, 2026"; CLAUDE.md:4, PROMPT.md:4, and
  .claude/skills/fortuna/SKILL.md all updated v0.8→v0.9 in the same commit;
  verified still v0.9 at HEAD.
- C2.2 Section 3 invariants byte-for-byte unchanged: PASS — SHA-256 of
  `sed -n '/^## 3\./,/^## 4\./p'` IDENTICAL at b8fa0c8, 213e41f^, 213e41f,
  16478bb: a2dce5ddbcb22080e30ac49d7982917d13aeea45ae4e6ced5db4563cf31af808.
- C2.3 5.15 grounded in research.md (>=5 claims): PASS — traced: funding 8h
  TWAP capped ±2% (research.md:31-32); IM = 1.3×MM + maintenance formula
  unpublished, approximated from leverage_estimates by notional
  (research.md:34,40,150-153,290-299); liquidation run by clearinghouse,
  order_source "system" (research.md:34,329-344,491,822); per-asset leverage
  ~5.9x BTC to ~2x (research.md:33,148-153); fees as decimal fractions of
  notional via /margin/fee_tiers, $0 launch promo (research.md:35-36,387-394);
  tick $0.0001 (research.md:173); portfolio margin (research.md:277-283);
  reduce_only⇒IOC/FOK (research.md:472,741,793). All eight BUILD_PLAN T5.B1
  content items present in the 5.15 text.
- C2.4 5.2 fee corrections match fee research: PASS — Intl per-category
  quadratic taker 0.03-0.07 + maker rebates (polymarket-fees research.md:15,
  92-99); US taker 0.05 / maker -0.0125 banker's rounding (polymarket-us
  research.md:345-349); Kalshi ceil-against-trader 0.07 quadratic w/ 0.5
  multipliers + 0.0175 maker (kalshi-fees research.md:49,57,144-147);
  superseded 0.8 claims corrected-not-erased in the text.
- C2.5 Plan-doc consistency: PASS — PerpPrice ten-thousandths, funding_carry
  60d data-only, fee-trap, liquidation-distance floor + leverage caps all
  consistent with docs/design/kinetics-perps-module-plan.md (lines 10-20,59,
  144-153,172-173,192).
- Note: T5.B1 checkbox still [ ] at HEAD — consistent with the repo's
  tick-after-gate convention; this review is that gate.

### C3 — f551d84 env-leak fix
- C3.1 Mechanism understood: PASS — new .cargo/config.toml [env] sets
  DATABASE_URL={value="postgres://localhost/fortuna_dev", force=false}. Cargo
  injects a real env var into every cargo-launched process, so sqlx/dotenvy's
  .env fallback (the leak: operator's fortuna_app role, no CREATEDB → 42501)
  never fires; force=false means an exported real env var still wins.
- C3.2 i5 not weakened: PASS — commit touches only .cargo/config.toml +
  docs/kickoff/T4.1-kickoff.md; i5_audit_append_only.rs untouched, still a
  real #[sqlx::test] (migrated throwaway DB, DB-trigger UPDATE/DELETE
  refusal, byte-identical replay, no-audit-no-trading halt). Executed three
  ways:
  - WITH DATABASE_URL exported: "test result: ok. 1 passed; 0 failed" (0.22s)
  - WITHOUT (env -u DATABASE_URL): "test result: ok. 1 passed; 0 failed" (0.18s)
  - WITHOUT env var + hostile .env (DATABASE_URL=postgres://no_createdb_role@
    localhost/forbidden_db planted in scratch worktree, removed after):
    "test result: ok. 1 passed; 0 failed" (0.24s) — the cargo [env] default
    beats dotenvy, which is exactly the leak being fixed.
- C3.3 Protected crate untouched by this commit: PASS (stat shows 2 files).

### C4 — 3eaa5e4 T4.1 increment (graded as increment only)
- C4.1 Compiles in workspace: PASS — clippy --workspace --all-targets
  -D warnings exit 0 includes fortuna-live.
- C4.2 Fail-closed boot: PASS — typed BootError (thiserror) in lib; missing
  env → MissingEnv naming the var (values never enter errors); placeholder
  detection; Secret newtype redacts Debug; missing [daemon]/[cognition]
  sections refuse; kalshi venue refuses citing fixture clearance; unknown
  venue refuses; halt_poll<=500ms pin; positive budgets; stub-mind degrade
  requires explicit allow_stub_mind opt-in; binary refuses to pretend to run.
  11/11 boot tests green. Functional probe: bare-env run →
  "Error: environment rejected ... required env var FORTUNA_SLACK_BOT_TOKEN
  is not set" (refuses even though cargo [env] injected DATABASE_URL — the
  kickoff daemon corollary holds in practice). No unwrap/expect/panic in lib.
- C4.3 SHUTDOWN CONTRACT: PASS (pending, not dropped) — no signal handling
  installed or claimed in this increment; BUILD_PLAN.md:291-295 still carries
  the BINDING SIGTERM contract on the unchecked T4.1 box; commit subject says
  "req 1, 7 partial", claiming no shutdown work.

### C5 — 1485d98 + f9b18e1 fit-validation notes
- C5.1 Notes in both docs under fit-validation headings: PASS —
  rota-dashboard.md:383 "### Fit-validation notes"; fortuna-cli.md:256
  "## 11. Fit-validation notes".
- C5.2 Every checklist item graded with evidence: PASS — ROTA V-1..V-12 +
  R7 precondition all graded with file:line cites, including honest nuances
  (V-2 meaning inverted by R4 cut; V-10 first frames lack seq); CLI items
  1-12 all graded (6 N/A by amendment A6 with reason; 8 DECIDED with
  mechanism; pending-GAPS items named).
- C5.3 Spot-checks against codebase: PASS —
  ROTA V-5: fortuna-ledger deps = core/venues/exec/gates (+ cognition as
  DEV-dep only, Cargo.toml:20); fortuna-ops absent → no cycle. CONFIRMED
  (with Minor mislabel, finding F3).
  ROTA V-7: CREATE TABLE discrepancy_resolutions at migrations/
  20260609000001_initial.sql:247, exactly 1 file. CONFIRMED.
  ROTA V-6: `pub async fn recent(&self, kind: &str, limit: i64) ->
  Result<Vec<AuditRow>, LedgerError>` at audit.rs:75. CONFIRMED.
  ROTA R7: zero `fn recent` in repos.rs (BeliefsRepo::recent absent). CONFIRMED.
  CLI 3: cli deps = core/gates/ledger; killswitch absent;
  Command::new("fortuna-killswitch") at main.rs:116. CONFIRMED.
  CLI 7: FORTUNA_RUNTIME_DIR 0 hits in crates/. CONFIRMED.
  CLI 8: nix absent from Cargo.lock → kill -15 shell-out decision correct.
  CONFIRMED.
- C5.4 "BUILDABLE AS AMENDED" supported: PASS — all checks pass on
  re-execution; body-vs-amendment conflict lists (SSE/shared-pool/rate-gauge/
  p50; mode/db-migrate-status/tmp-dir) are accurate guards against building
  from the superseded body text. The "676/0" battery claim in 1485d98's
  message matches my independent run (676 passed / 0 failed).

### D — standard battery at 16478bb
- D1 cargo fmt --check: PASS (exit 0, no output).
- D2 clippy --workspace --all-targets -- -D warnings: PASS — "Finished `dev`
  profile [unoptimized + debuginfo] target(s) in 21.25s", exit 0.
- D3 cargo test --workspace: PASS — aggregate over all 95 result lines:
  676 passed, 0 failed, 0 ignored.
- D4 fortuna-invariants per-test: PASS — i1: 2/0, i2: 2/0, i3: 1/0, i4: 1/0
  (8.50s), i5: 1/0, i6: 3/0, i7: 3/0. All ok.
- D5 scripts/run-dst.sh 10000: PASS, all stages —
  - "[dst] regression corpus: 0 seed(s)" (corpus dir has held only README.md
    since its T0.4 creation; empty range diff — not weakening)
  - "[dst] master seed 1781170461617 -> 10000 random scenario(s)"
  - "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations"
  - "[synthesis-dst] master seed 1781170847685 -> 10000 scenario(s)"
  - "[synthesis-dst] totals: 26888 orders, 41425 proposals, 132713 cognition
    failures, 117911 beliefs" → "test result: ok. 1 passed ... 92.47s"
  - "[settlement-dst] master seed 1781170943248 -> 10000 scenario(s)"
  - "[settlement-dst] arms {SettleClean: 896, SettleThenCorrect: 886, Void:
    882, Dispute: 935, VenueMismatch: 970, CanonicalDivergence: 913,
    OrphanScan: 923, AuditDeath: 881, WideBook: 882, Overdue: 903,
    MultiLegGroup: 929}; 1887 discrepancies, 2761 watchdog rows, 1853 halts"
    → "test result: ok. 1 passed ... 16.48s". Every arm hit; script exit 0.
- D6 mechanical + test-weakening sweep (six scoped commits, combined diff
  4863 lines): PASS — added panic/expect only in test code; ambient time only
  in recorder main.rs (ledgered); no f64 on money/prices; no HashMap/HashSet
  in ordered paths (fortuna-live uses BTreeMap); zero deleted asserts, zero
  new #[ignore], zero proptest reductions (also zero diff-wide across the
  whole range); no secret literals (env var NAMES only); no GatedOrder
  constructors outside fortuna-gates (only spec prose mentions). .gitignore
  at HEAD correctly ignores .keys/ and data/ (git check-ignore verified).
- D7 protected-crate touch, whole range: PASS — empty diff, zero commits;
  no patch to quote.

## Findings
- [Minor] F1: recorder single-writer assumption undocumented — JSONL
  append-mode files corrupt on concurrent writers; nothing in the crate,
  plan doc, or ASSUMPTIONS.md states the one-process assumption (T4.4's
  "start REFUSES on an unmanaged running fortuna-recorder" depends on it).
  Reproduction: grep -rn "writer|concurren" crates/fortuna-recorder/
  docs/design/kinetics-perps-module-plan.md ASSUMPTIONS.md → 0 relevant hits.
  Ledger in GAPS.md/ASSUMPTIONS.md.
- [Minor] F2: BUILD_PLAN T5.B0 note claims "restart cmd in data/recorder.log
  header"; the header is the parameter line ("fortuna-recorder (B0):
  host=... out=... interval=30s series=[...]"), not a literal command.
  Cosmetic completion-note inaccuracy.
- [Minor] F3: ROTA fit-validation V-5 evidence line lists "cognition" among
  fortuna-ledger deps; fortuna-cognition is a [dev-dependencies] entry
  (Cargo.toml:20), not a regular dep. Load-bearing no-cycle claim unaffected.

## Commands run (verbatim verdict lines)
- cargo fmt --check → exit 0
- cargo clippy --workspace --all-targets -- -D warnings → "Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.25s" (exit 0)
- cargo test --workspace → 95 suites: 676 passed, 0 failed, 0 ignored
- cargo test -p fortuna-recorder → "test result: ok. 8 passed; 0 failed"
- cargo test -p fortuna-live → "test result: ok. 11 passed; 0 failed"
- i5 with DATABASE_URL → "test result: ok. 1 passed; 0 failed; ... 0.22s"
- i5 without DATABASE_URL (env -u) → "test result: ok. 1 passed; 0 failed; ... 0.18s"
- i5 hostile .env, no env var → "test result: ok. 1 passed; 0 failed; ... 0.24s"
- scripts/run-dst.sh 10000 → "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations"; synthesis "ok ... 92.47s"; settlement "ok ... 16.48s"; exit 0
- Section 3 SHA-256 at b8fa0c8/213e41f^/213e41f/16478bb → identical (a2dce5dd…)
- git diff b8fa0c8...16478bb -- crates/fortuna-invariants/ → empty
