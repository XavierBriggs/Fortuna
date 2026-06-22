# L — Ledger / Audit-Log Integrity (I5) Re-Verification

Re-verify of append-only integrity (I5, `docs/spec.md:44`) + scoring set-once
after the additive change. Review target: `/Users/xavierbriggs/fortuna-main`
(branch `main`, HEAD `1bb6959`). Baseline `a70daee`. All reads in the main
worktree. READ-ONLY.

## Diff surface (scope confirmation)

- `crates/fortuna-ledger`: 33 files (28 sqlx `.json` query-cache, 1 new
  migration, `lib.rs`, `repos.rs` +582, 2 new test files).
- `crates/fortuna-scoring`: 9 files — all `src/deflation/*` + `scorecard.rs` +
  `tests/deflation.rs`; **pure compute, zero new files persist**.
- `crates/fortuna-invariants`: 1 file —
  `tests/i4_killswitch_revocation.rs` (NEW, I4 not I5; additive).
- New migrations dated 20260621*: exactly ONE —
  `20260621000002_validation_runs.sql`.
- `20260609000001_initial.sql` diff vs baseline is **EMPTY** (byte-identical):
  `fortuna_refuse_mutation()` (line 14) and `fortuna_beliefs_guard()` (line 79)
  are UNCHANGED. The C1 set-once guard is intact.

## 1. New-table append-only census

Only one persistent table was added since baseline. (Pre-existing
scoring/audit tables shown for context — all carry triggers; all unchanged.)

| table | added since baseline? | append-only? | trigger (migration path:line) | verdict |
|---|---|---|---|---|
| `validation_runs` | **YES (only new table)** | YES (blunt-refuse) | `migrations/20260621000002_validation_runs.sql:44-46` `validation_runs_append_only BEFORE UPDATE OR DELETE … EXECUTE FUNCTION fortuna_refuse_mutation()` | **PASS** — append-only, decision/score data protected |
| `scorecards` | no (20260621000001, present at baseline) | YES | `migrations/20260621000001_scorecards.sql:45-47` | PASS |
| `trade_scores` | no | YES | `migrations/20260618000002_trade_scores.sql:21-22` | PASS |
| `bus_recordings` | no | YES | `migrations/20260618000001_phase_c_persistence.sql:42-44` | PASS |
| `beliefs` | no | guard (content-immutable, scoring set-once) | `migrations/20260609000001_initial.sql:98-99` (`fortuna_beliefs_guard`) | PASS (C1 exception) |
| `audit` | no | YES | `initial.sql:117-118` | PASS |

Full trigger census (all migrations) cross-checked: every CREATE TABLE that
holds decision/audit/score/belief data has a matching `_append_only` or
`_guard` trigger. The only tables WITHOUT a trigger are `source_registry`
(`initial.sql:136`) and `exec_cursors` (`initial.sql:159`) — both pre-existing,
mutable-by-design operational tables (a registry of sources and a per-stream
read-cursor), NOT decision/audit/score data. No new unguarded table.

**No unguarded new persistent table holding decision/audit/score data. I5
surface fully covered at the DB layer.**

## 2. Repo-layer UPDATE / DELETE audit

`git diff a70daee HEAD -- crates/fortuna-ledger/src/repos.rs | grep '^[+-]'`
filtered to UPDATE/DELETE returns **EXIT=1 (no match)**: the +582 lines added
NO new UPDATE or DELETE and modified/removed NONE. Added lines are exclusively
SELECT (read paths) and INSERT (`insert_historical`, `ValidationRunsRepo::insert`,
proposals/chain reads). All UPDATE/DELETE in the file therefore pre-date the
baseline; re-classified for completeness:

| repos.rs:line | statement | added since baseline? | classification |
|---|---|---|---|
| 635 | `UPDATE events SET status` | no | mutable-by-design (`events` has no trigger; lifecycle FSM) — OK |
| 646 | `UPDATE events SET status='dead'` | no | mutable-by-design — OK |
| 657 | `UPDATE events SET unscoreable=TRUE` | no | mutable-by-design — OK |
| 1509 | `UPDATE beliefs SET status='superseded'` | no | C1 — flips status only; supersede chain (guard allows status change) — OK |
| 1630 | `UPDATE beliefs SET status,outcome,brier,clv_bps … WHERE outcome IS NULL` (`resolve_and_score`) | no | **C1 scoring-set-once** — sets exactly the 4 scoring columns, once (`WHERE outcome IS NULL`, asserts `rows_affected()==1`) — OK |
| 1656 | `UPDATE beliefs SET status='abandoned' WHERE status='open'` | no | C1 — status-only lifecycle, guard-permitted — OK |
| 2021 | `UPDATE beliefs SET p` (`try_mutate_content_for_test`) | no | **test-only hook** — documented "never used by production code" (1014/2013), a positive control that proves the guard refuses content mutation — OK |
| 2656 | `UPDATE domain_analyses SET status='superseded'` | no | mutable-by-design status flip (`domain_analyses_guard` permits status) — OK |
| 2839 | `UPDATE scalar_beliefs …` | no | C1-analog scoring path (`scalar_beliefs_guard`) — OK |

New INSERT-only writers added this change:
- `ValidationRunsRepo::insert` (repos.rs:3403-3424) — `INSERT … ON CONFLICT
  (scope, producer, computed_at) DO NOTHING`. INSERT-only, idempotent; a re-run
  is a new row, never an edit.
- `BeliefsRepo::insert_historical` (repos.rs:1535-1564) — `INSERT INTO beliefs
  (… 8 content cols …) ON CONFLICT (belief_id) DO NOTHING`. Sets ONLY content
  columns; never touches status/outcome/brier/clv_bps (they default
  NULL/'open'). Respects C1.

**No new UPDATE/DELETE. Zero violations. The only belief-touching UPDATE
(`resolve_and_score`) is the textbook C1 set-once path and pre-dates baseline.**

## 3. validation_runs / sweep / DSR-deflation persistence

- **Compute layer is pure.** `fortuna-scoring` declares "no sqlx, tokio,
  Postgres" (`src/lib.rs:6`); `Cargo.toml` has no sqlx/ledger/postgres dep
  (grep EXIT=1). The new `src/deflation/{cscv,dsr,spa,purge,effective_n,mod}.rs`
  and `scorecard.rs` compute the deflated surface (PBO/CSCV, SPA p, MinTRL,
  effective-N, DSR) and persist nothing themselves.
- **Persistence is a single append-only INSERT.** The GO surface is written
  only via `ValidationRunsRepo::insert`, called from
  `fortuna-cli/src/backtest_cmd.rs:260-269` (`run_validate → run_sweep →
  insert`). The whole-truth deflated surface (run_id, scope, producer,
  trial_space, n_trials, family_n_trials [joint scope×config grid = the
  deflation N], selected_config, brier_edge/pbo/spa_p, clv_edge/pbo/spa_p,
  effective_n, mintrl_ok, sharpe_dsr, verdict, computed_at) is serialized
  verbatim into the JSONB `payload` (migration comment lines 8-13;
  backtest_cmd.rs:258). Brier is the gated headline, CLV corroborating,
  `family_n_trials` the joint grid — never a single flattering N.
- **Reproducible / append-only.** `run_id = run_id_for(scope, producer,
  computed_at_epoch)` — a pure function of inputs (`backtest_cmd.rs:255-256`),
  so a deterministic re-run yields the same id and the `ON CONFLICT … DO
  NOTHING` makes it an idempotent no-op (no edit). A genuine correction is a
  NEW row at a later `computed_at`; the read path `latest()`
  (repos.rs:3431-3449) takes newest-per-(scope,producer) via
  `idx_validation_runs_scope_latest` (migration line 39). DB trigger refuses any
  UPDATE/DELETE. **Append-only + reproducible: confirmed.**

## 4. Invariant-test coverage of the new surface

- The single invariants-crate change is `tests/i4_killswitch_revocation.rs` —
  I4 (kill-switch revocation/re-arm), NOT I5, and strictly ADDITIVE (header
  lines 11-15: "ADDITIONS-ONLY … does NOT modify, weaken, rename, or delete any
  existing test"). No I5 invariant test was edited or weakened.
- **The I5 invariant test
  (`crates/fortuna-invariants/tests/i5_audit_append_only.rs`) is `audit`-table
  specific** — it exercises UPDATE/DELETE refusal on the canonical `audit` log
  only (lines 142, 149). It is NOT parametric over all append-only tables, so it
  does NOT directly cover `validation_runs`. This is unchanged behavior (the
  test was already table-specific at baseline) and consistent with its scope:
  the canonical I5 audit-log property.
- **`validation_runs` append-only IS tested — in the ledger crate**, not the
  invariants crate: `crates/fortuna-ledger/tests/validation_runs.rs:126-172`
  (`validation_runs_append_only`) inserts a row, asserts a raw `UPDATE … SET
  verdict='tampered'` is `is_err()` (138-146), a raw `DELETE` is `is_err()`
  (148-156), and the row survives untouched with `verdict=='go'` (158-172). This
  is real DB-trigger coverage against a live Postgres (`#[sqlx::test]`).

**Coverage gap (minor, not a defect): the canonical I5 invariant test does not
enumerate `validation_runs` (nor `scorecards`/`trade_scores`/`bus_recordings`);
those tables' append-only guards are proven only by their owning crate's tests,
not by the protected invariants crate.** Append-only enforcement for the new
table is verified end to end; what is missing is its presence in the single
spec-invariant harness.

## Verdict

**I5 holds end to end.** The only new persistent table (`validation_runs`) is
append-only (DB trigger, migration 20260621000002:44-46), the repo layer added
zero UPDATE/DELETE, the scoring layer is pure compute, persistence is a single
reproducible INSERT, and the belief scoring-set-once guard
(`fortuna_beliefs_guard`) is byte-identical to baseline. The new table's
append-only property is tested (ledger crate). Append-only is intact across the
full ~16-table surface.

## Open questions / flags

- **Minor coverage flag:** the protected I5 invariant test
  (`i5_audit_append_only.rs`) remains `audit`-only and was not extended to
  assert `validation_runs` (or the other append-only score tables). Consider an
  additive parametric I5 test over the trigger-bearing table set so new
  append-only tables are caught by the invariant harness, not only by per-crate
  tests. Not a regression (pre-existing posture); flagged per scope item 4.
- `validation_runs.UNIQUE (scope, producer, computed_at)` treats NULL producer
  as distinct (Postgres default). For an append-only store this is benign
  (re-runs carry distinct run_id/computed_at; read takes newest) and is
  documented in the migration (lines 30-34). No action.
