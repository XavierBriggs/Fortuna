# Re-verification: prior gaps + CLI/ops/live/killswitch/invariants delta

Target: **`/Users/xavierbriggs/fortuna-main`**, branch `main`, HEAD `1bb6959`.
Baseline for "since baseline": `a70daee`. All reads in the main worktree. READ-ONLY pass.

Changed-file counts since baseline (confirmed): cli 8, ops 8, live 12, killswitch 2, invariants 1.

---

## PART 1 — Prior findings status

| # | Finding | Status | Evidence (path:line) |
|---|---------|--------|----------------------|
| 1 | `prompt_hash` not recorded in belief provenance (spec `docs/spec.md:181` mandates `{model_id, prompt_hash, context_manifest_hash, cost_cents}`) | **STILL-PRESENT** | All THREE production provenance write sites omit `prompt_hash`: `crates/fortuna-cognition/src/mind.rs:692-696`, `crates/fortuna-cognition/src/shadow.rs:109-114`, `crates/fortuna-cognition/src/discovery.rs:796-800`. Repo-wide grep finds `prompt_hash` only in spec/docs + ONE test (`crates/fortuna-cognition/tests/beliefs.rs:36`) — no production write site. The discovery.rs comment even enumerates the 3-field set it stamps (`{model_id, context_manifest_hash, cost_cents}`), confirming the 4th field is absent by design-as-shipped. |
| 2 | Protected-invariant guard scope hole: `check-protected-invariants.sh` diffs only `tests/`, not `src/lib.rs` (where I1 `compile_fail` doctests live) | **STILL-PRESENT** | `scripts/check-protected-invariants.sh` sets `DIR="crates/fortuna-invariants/tests"` and diffs only `$DIR`; no `src`/`lib` reference (grep returns nothing). The I1 propose-only compile-fail doctests live in `crates/fortuna-invariants/src/lib.rs` (```compile_fail``` blocks at lines 20, 28, 46, 53). A weakening edit to `src/lib.rs` would pass the guard unchecked. |
| 3 | `InstrumentKind` vestigial — defined but unreferenced in production | **STILL-PRESENT (still vestigial)** | The only non-test reference to `InstrumentKind` in all of `crates/` is its own definition at `crates/fortuna-core/src/perp.rs:68`. It is NOT a field of `MarketView` (`crates/fortuna-core/src/market.rs:29`) nor referenced in any gate/position production path. `ASSUMPTIONS.md:742` explicitly documents "`InstrumentKind` … is NOT yet threaded through the shared `Market` structs" (threading deferred to B3/B4). |
| 4 | `actor=NULL` on daemon audit rows: bridge hardcodes `actor=None` | **STILL-PRESENT** | Bridge is `crates/fortuna-live/src/audit_bridge.rs` (NOTE: lives in fortuna-**live**, not fortuna-ops as the prompt stated). The `AuditWriter::append` signature is `(kind, actor: Option<&str>, ref_id, payload)` (`crates/fortuna-ledger/src/audit.rs:54-60`). `PgAuditSink`'s worker calls `writer.append(&kind, None, ref_id.as_deref(), payload)` at `audit_bridge.rs:103` — `actor` hardcoded `None`. The `AuditSink::append` impl (`audit_bridge.rs:119-145`) accepts no actor at all. `git diff a70daee..HEAD -- audit_bridge.rs` is EMPTY → unchanged since baseline; no system actor stamped. (The `actor` hits in `crates/fortuna-ops/src/rota.rs` are a READ-side audit-tail viewer — `SELECT … actor …`, lines 2250/2262/2360 — not the write site.) |

---

## PART 2 — Delta review

### 5. CLI `fortuna backtest` / `fortuna validate` — I1 / paper-safety verdict: **SAFE (read-only / paper-safe; constructs NO order path)**

- No `GatedOrder`, `.place(`, `place_order`, `Venue::…place`, or `submit_order` anywhere in `crates/fortuna-cli/src/` (grep clean).
- `run_backtest` (`crates/fortuna-cli/src/backtest_cmd.rs:97-171`): opens the archive with `AeolusArchiveSource::open_read_only` (SQLITE_OPEN_READ_ONLY; `backtest_cmd.rs:138`), runs `ReplayHarness::replay` (`:166-169`), returns a `ReplayReport`. The only ledger write is beliefs (knowledge), never orders.
- `run_validate` (`backtest_cmd.rs:208-272`): `run_sweep` is a pure no-IO function (`:233/:242`); the sole IO is `ValidationRunsRepo::insert` of a metric row (`:261-269`). No order/venue construction.
- The backtest crate itself never places a real order — `crates/fortuna-backtest/src/records.rs:25` states "The backtest subsystem is **paper-only**: no real order is ever placed." Comment hits for "place" in `fortuna-backtest/src` are pool-placement of outcome records, not order placement.
- The only venue-touching CLI path is `fortuna start paper-demo`, which HARD-ASSERTS `[runtime].execution_mode == "paper_ledger"` and fails closed otherwise (`crates/fortuna-cli/src/main.rs:545-577`; `assert_paper_demo_safe`); the live safety wall is `compose_paper_live_runner_with_transport` + the `live_data_only_accepts_paper_live_with_orders_disabled` test (`crates/fortuna-live/src/boot.rs:1033`). `doctor.rs` (`check_mode_safe`, `:282-334`) independently re-asserts paper-safety.
- Conclusion: the backtest/validate subcommands cannot place a real order. I1/I6 posture holds.

### 6. Invariants change — **ADDITIVE (not a weakening)**

- Sole changed invariants file: `crates/fortuna-invariants/tests/i4_killswitch_revocation.rs` (file EXISTED at baseline; modified by APPEND only).
- Diff: one new `use` import + two new `#[test]` fns (`rearm_guard_refuses_while_kill_sentinel_present`, `rearm_guard_refuses_when_sentinel_unverifiable`) pinning the fail-closed three-way re-arm guard. `git diff a70daee..HEAD -- crates/fortuna-invariants/tests | grep '^-'` (excluding `---`) returns NOTHING → zero removed/changed lines in existing assertions; pure additions. STRENGTHENS I4.
- Note: this particular change WOULD be caught by the guard script (it is under `tests/`), but Finding 2's hole (`src/lib.rs` unscanned) remains independent.

### 7. Ops / live / killswitch delta notes

- **Killswitch (2 files): purely additive.** `crates/fortuna-killswitch/src/lib.rs` +38 lines add `enum RevocationGuard` (`:271`) and `pub fn revocation_guard` (`:282+`) — a three-way Present/Absent/**Unverifiable** guard using `try_exists`, failing closed on the unverifiable state (the bug a `!is_revoked` re-arm would inherit). No existing logic touched. New test file `tests/revocation_guard.rs` (+97). This is the production safety improvement the Finding-6 invariants test pins.
- **No new UPDATE/DELETE/DROP/TRUNCATE on audit-like state** in any changed ops/live src file. The two `+`-line grep hits are false positives: a code comment in `daemon.rs` ("…silently drop persona CLV") and a docstring in `rota.rs` ("Truncate a `serde_json::Value` to a display string" = string truncation, not SQL).
- **New observability surface (additive, read-only):** `crates/fortuna-ops/src/chain_view.rs` (WS4 E3) — pure `Serialize`/`Deserialize` DTO of a per-event chain for the UI render; no endpoint, no mutation. Minor tracked item (not a gap): its `validation` field is forward-declared as raw `serde_json::Value` pending WS3's `ValidationRun: Serialize` (`chain_view.rs:9-13` header) — flagged for reconciliation when WS3 merges, consistent with the WS3→WS4 dependency.
- The audit-tail viewer added to `rota.rs` (`:2221-2362`) is strictly read-side (`SELECT audit_id, at, kind, actor, ref_id FROM audit`), so it does not threaten I5 append-only.

---

## Summary

- Prior gaps 1, 2, 3, 4: **ALL FOUR STILL-PRESENT** (none fixed since baseline `a70daee`).
- CLI backtest/validate: **paper-safe / read-only; no order path** — I1/I6 hold.
- Invariants change: **ADDITIVE** (two new I4 re-arm tests; zero weakening).
- Ops/live/killswitch delta: clean — killswitch + chain_view additions only; no new UPDATE/DELETE on audit state; no new observability gap (one tracked WS3→WS4 type reconciliation).
