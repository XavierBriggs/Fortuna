# Subsystem Review: `crates/fortuna-backtest` (WS3 — generic backtest / validation)

**Target:** `/Users/xavierbriggs/fortuna-main` @ `main` HEAD `1bb6959`. All citations are path:line in that worktree. READ-ONLY review; no code/doc modified.

**Authoritative docs:** `docs/spec.md` (v0.9, invariants I1–I7); WS3 design `docs/superpowers/specs/2026-06-21-ws3-generic-backtest-design.md`; plan `docs/superpowers/plans/2026-06-21-ws3-generic-backtest.md`; research grounding `docs/research/2026-06-21-ws3-backtest-overfitting-grounding.md`.

**Verification run (offline, this review):**
- Pure backtest tests PASS: gpit 6, gdead 14, sweep 8, decoupling 3, validate_real_edges 5, records 11.
- `fortuna-scoring` deflation integration tests PASS: 21/21, incl. `purged_cscv_bites_on_known_overlap`.
- `aeolus_archive` PASS: 8/8.
- `cargo build -p fortuna-backtest --tests` (SQLX_OFFLINE=true) compiles clean.
- Postgres-backed DST tests (`backtest_dst.rs`, `harness.rs`) require a migrated `fortuna` DB; not executed here (sqlx online-check fails without DB). Read + logic-verified; flagged as as-built-by-reading, not re-executed.

---

## 1. Subsystem inventory

**Responsibility:** Replay *any* historical source through the already-proven WS1/WS2 scoring rules and the same ledger write path as live, producing an honest, overfitting-deflated GO/NO-GO. It validates DETERMINISTIC COMPONENTS (forecast probabilities → recomputed Brier/CLV), never LLM-decision PnL. GO criterion is Brier-skill primary; CLV corroborating; PnL/Sharpe walled-off context (DSR).

| Module | Path | Role |
|---|---|---|
| records | `src/records.rs` | Generic, JSONL-serializable records (`HistoricalBelief/Outcome/Snapshot/Trade`, `Provenance`, `BeliefPayload`). Money = `Cents`; probabilities = `f64`. `HistoricalTrade::new` enforces `orders==0` (`records.rs:182`). |
| manifest | `src/manifest.rs` | `UniverseManifest`/`EngagedMarket` (engaged set) + `enforce_gdead` (G-DEAD). |
| source | `src/source.rs` | `HistoricalSource` trait (sync `Iterator`, deliberate deviation from spec's `Stream`, documented `source.rs:42-58`) + bitemporal/`event_linkage` contract docs. |
| asof | `src/asof.rs` | `asof_join` — the G-PIT enforcement point. |
| harness | `src/harness.rs` | `ReplayHarness` — streaming replay → as-of join → idempotent ledger write → parity scorecard → G-DEAD. Holds `content_ulid`/`run_id_for` (FNV-1a). |
| sweep | `src/sweep.rs` | `run_sweep` — G-TRUTH driver; assembles CSCV matrices, calls deflation toolkit, emits `ValidationRun`. |
| edge_provider | `src/edge_provider.rs` | `LedgerEdgeProvider` (W7) — the REAL provider: as-of-joins source → per-period OOS Brier-skill/CLV + label windows, scoring via the shared assembler. |
| sources/aeolus_archive | `src/sources/aeolus_archive.rs` | The ONLY source-coupled adapter; maps `aeolus_kalshi.db` → records; the post-resolution leak-trap boundary. |

**Boundary contracts / dependency graph** (`crates/fortuna-backtest/Cargo.toml`):
- Depends on `fortuna-core` (Clock/Cents/Ulid), `fortuna-scoring` (deflation math + `decide`/`DeflatedView`/`Scorecard`), `fortuna-cognition` (`scorecard_agg::assemble_from_samples` — the SAME live assembler; G-PARITY), `fortuna-ledger` (write path + `SOURCE_HISTORICAL_IMPORT`), `rusqlite` (S6 adapter only). No cycle: cognition/scoring/ledger do NOT depend on fortuna-backtest (Cargo.toml comment + verified by grep).
- **Consumed-by:** `crates/fortuna-cli/src/backtest_cmd.rs` (`run_backtest`/`run_validate`), registered in `crates/fortuna-cli/src/main.rs:1193,1201,1227,1233`. No daemon/runtime dep (`fortuna-ops/src/rota.rs:2117` notes "no runtime fortuna-backtest dep").
- **`Source` trait** (`source.rs:109`): yields `beliefs/outcomes/snapshots/trades` iterators + `universe_manifest`. **`EdgeProvider` trait** (`sweep.rs:143`): `edges(scope, config_index) -> ConfigEdges` + `windows(scope) -> (Vec<LabelWindow>, Duration)`; blanket impl for `Fn` (`sweep.rs:163`) and the real `LedgerEdgeProvider` impl (`edge_provider.rs:239`).

---

## 2. The four gates

| Gate | Asserts | Path:line (impl) | Path:line (test) | Status |
|---|---|---|---|---|
| **G-PIT** | A belief enters a decision iff `available_at < decided_at` (STRICT). Equality is a leak → `LookAheadRejected` + counted. CLV snapshot = latest `at < decided_at`. Outcome labels (not gated). | `asof.rs:78` (the `<`), `asof.rs:82-86` (snapshot strict `<`), `asof.rs:100-104` (reject). Harness counts at `harness.rs:162-164`. | `gpit.rs:78` strict-excludes-equal; `gpit.rs:114` rejects-future; `gpit.rs:128` latest-prior-snapshot; `validate_real_edges.rs:447` `leak_guard_rejects_future_belief`. 6/6 PASS. | **WIRED, asserts as specified.** Mutation note `<`→`<=` reds `gpit_strict_excludes_equal_timestamp` (asof.rs:77). |
| **G-DEAD** | Every TERMINAL (`resolved \|\| voided`) manifest market must appear in scored set (one-directional `manifest⊆scored`). Voided/NO-resolved present. PENDING (`!resolved && !voided`) EXEMPT (cannot be scored). Un-forecast markets do NOT false-positive. | `manifest.rs:137` `enforce_gdead`; terminal filter `manifest.rs:151-153`; voided ScoredRow synthesis in harness `harness.rs:221-234`. | `gdead.rs` 14/14 PASS: voided-omit-violation (`:58`), NO-resolved-omit (`:97`), coverage (`:129`), pending-exempt (`:252`+`:265`), resolved-still-bites-despite-pending (`:295` mutation-proof), voided-still-bites (`:324`). | **WIRED, asserts as specified.** Pending-exemption is narrow + mutation-guarded. |
| **G-PARITY** | Replay uses the IDENTICAL `fortuna_cognition::scorecard_agg::assemble_from_samples` + the same `ScorecardsRepo`/`BeliefsRepo` write path as live; only deltas = `source="historical-import"` + preserved original timestamps. Never a reimplementation of Brier. | harness scorecard `harness.rs:257-266`; belief write `harness.rs:337-348` (`insert_historical`); edge_provider scoring `edge_provider.rs:268-276`. Source stamp = `fortuna_ledger::SOURCE_HISTORICAL_IMPORT` (no literal in src/). | `harness.rs` test target (Postgres; not re-executed here). Parity-by-construction: same assembler symbol, verified by import + call-site. | **WIRED by construction** (shared assembler + shared repo). Byte-identity claim depends on scoring purity (WS2-proven); not independently re-run here (needs DB). |
| **G-TRUTH** | The deflated GO surface. Brier-skill PRIMARY (`brier_edge>0 && brier_pbo<=0.05 && brier_spa_p<alpha`); CLV corroborating-only (never gates); verdict ∈ {Go, NoGo, Insufficient}; `effective_n<30` OR `n_logits==0` → Insufficient. | `sweep.rs:271` `run_sweep`; verdict via pure `fortuna_scoring::decide` (`scorecard.rs:358`, CLV absent from GO condition `:368`); `pbo==0.0` footgun handled `scorecard.rs:362`. | `sweep.rs` 8/8: GO-requires-all-conjuncts (`:57`), CLV-cannot-rescue (`:99`), Insufficient-on-thin-N/`n_logits==0` (`:122`), family-count (`:190`), DSR-deflates-family (`:277`). `validate_real_edges.rs:321` real-verdict. | **WIRED, asserts as specified.** Brier-primary + CLV-walled-off enforced in the pure `decide`. |

---

## 3. Determinism (reproducibility-critical)

| Check | Verdict | Evidence |
|---|---|---|
| Deterministic `run_id` via FNV-1a (NOT `DefaultHasher`) | **CONFIRMED** | `content_ulid` (`harness.rs:406-425`) hand-rolls FNV-1a 64-bit twice → 128-bit ULID; doc explicitly states "NOT `DefaultHasher`, whose output is unstable across releases" (`harness.rs:402-405`, `:432`). `run_id_for` (`harness.rs:440`) uses same. `grep DefaultHasher crates/fortuna-backtest` → none. CLI run_id also FNV (`backtest_cmd.rs:252-256`). |
| Clock-determinism (no wall-clock; injected `Clock`) | **CONFIRMED** | Harness generic `<C: Clock>` (`harness.rs:98`); `clock` held but row content derived from records not clock (`harness.rs:100-104`); scorecard `computed_at` = epoch-0 sentinel, not wall-time (`harness.rs:371`). DST `backtest_clock_determinism` (`backtest_dst.rs:457`) asserts byte-identical belief_id sets under two different `SimClock` instants. Sweep PRNG = seeded `SplitMix64` (`sweep.rs:373`). |
| Idempotent rerun | **CONFIRMED** | Content-hash ids + `ON CONFLICT DO NOTHING` (`harness.rs:316,337`; scorecard `:355,374`). `ReplayReport.skipped_idempotent` (`harness.rs:87`). DST `backtest_rerun_idempotent` (`backtest_dst.rs:229`): 2nd replay writes 0, skips N. |
| Partial-replay recovery | **CONFIRMED** | DST `backtest_partial_replay_recovery` (`backtest_dst.rs:363`): crash-at-K → resume writes N−K, skips K, total==N; K=0 and K=N edge cases included. |

DST scenarios are Postgres-backed (`#[sqlx::test]`); logic + assertions read-verified, not re-executed in this review (no migrated DB). The 3 pure determinism mechanisms (FNV content-hash, seeded SplitMix64, deterministic period sort `edge_provider.rs:216`) are exercised by the passing pure suites.

---

## 4. Statistical rigor (adversarial quant lens)

| Control | WIRED? | Evidence / verdict |
|---|---|---|
| **DSR consumes trial count (`family_n_trials`)** | **WIRED** | `dsr(...)` called with `family_n_trials as f64` as `n_eff_trials` (`sweep.rs:417-424`), NOT `n_configs`. `family_n_trials = \|scopes\| × n_configs` (`sweep.rs:99`). `dsr` uses N in `expected_max_sharpe` Gumbel term (`dsr.rs:47,66-73`). Behavioral test `sweep_dsr_deflates_against_family_n_trials` (`sweep.rs:277`) asserts DSR TIGHTENS as family grows; comment states a `family_n_trials→n_configs` mutation reds it. PASS. |
| **Trial count N = joint scope × config grid (BLOCK-2)** | **WIRED** | `family_n_trials()` (`sweep.rs:99`); `sweep_n_trials_counts_scope_x_config_grid` (`sweep.rs:190`): 3 scopes × 8 configs = 24, not 8. PASS. Romano–Wolf StepM family-wise control is a recorded deferral (sweep.rs:22 doc); N-counting itself is done. |
| **PBO via purged + embargoed CSCV** | **WIRED (math) + WIRED (production path, post-W7)** | CSCV impl `cscv.rs:63` `pbo`; purge applied per-combo ONLY when `label_windows.len()==t` (`cscv.rs:107,124-129`) calling `purge_embargo` (`purge.rs:71`). Purge/embargo formula matches research §2 (overlap `train.t0≤test.t1' && train.t1≥test.t0`, one-sided embargo extension `purge.rs:77-81`). `purged_cscv_bites_on_known_overlap` (deflation.rs) PASS. |
| **Purge actually WIRED into the production CSCV split (not just present as formula)** | **WIRED (RESOLVED by W7, commit `0344e30`)** | `LedgerEdgeProvider::windows` returns EXACTLY `t` label windows (`edge_provider.rs:313-319`); `run_sweep` asserts `windows.len()==t` then passes them into BOTH `pbo()` calls (`sweep.rs:353-370`) — the assertion (`sweep.rs:359`) prevents a silent no-purge no-op. CLI `run_validate` builds the real provider via `build_edge_provider`→`LedgerEdgeProvider::from_source` (`backtest_cmd.rs:232,282-303`). `validate_real_edges.rs::purge_bites_directionally` (`:402`) asserts `purged.pbo > nopurge.pbo + 0.05` on a leaky fixture — DIRECTIONAL. PASS. |
| **Hansen SPA_c (consistent variant, preferred over White RC)** | **WIRED** | `spa_c` (`spa.rs:114`); consistent recenter threshold `−√(2 ln ln n)` (`spa.rs:170-174,182-189`); returns `p_c` (gated), `p_l`, `p_u`(=White RC) (`spa.rs:237-242`). Gated on `brier_spa_p < alpha` (`scorecard.rs:368`). Seeded `SplitMix64` bootstrap (`sweep.rs:373`). |
| **MinTRL / effective-N guard** | **WIRED** | `effective_n` AR(1) + general fallback (`effective_n.rs:27`); `mintrl` Bailey–LdP Eq.13, returns `+∞` when `sr≤sr*` (`effective_n.rs:94-101`); `mintrl_ok = eff_n >= min_trl` (`sweep.rs:406-410`). `MIN_EFFECTIVE_N=30` floor → `Insufficient` (`scorecard.rs:343,362`). |
| **Leak-trap in aeolus_archive (issuance-time beliefs + recomputed scores)** | **WIRED** | belief `available_at = forecast_init_time` (issuance), `decided_at = target_date` (`aeolus_archive.rs:369-375`); payload = `predicted_prob` ONLY (`:396-397`); `aeolus.db` `scorecards` (CRPS/PIT) explicitly NOT imported (`:6,20-25,119-121`); FORTUNA recomputes scores → keeps G-PARITY honest. Snapshot join-key routed through the SAME `event_linkage` helper to prevent namespace drift (`:448-462`). Outcomes `resolved_at = settled_at` (`:432-433`). |

### Status of commit `c26abc6` guardian findings (the prompt's explicit ask)
`c26abc6` (2026-06-21 22:12) recorded TWO findings in `GAPS.md:224-251`:
- **G1 — "validate ships a placeholder edge-provider; purge/embargo unreachable in production" (Important).**
  **CURRENT STATUS: RESOLVED in code, but GAPS.md entry is STALE (not struck through).** The fix landed in commit `0344e30` "feat(ws4): W7 real validate edge-provider + purged/embargoed CSCV" (after c26abc6). HEAD now has: real `LedgerEdgeProvider` (`edge_provider.rs`), CLI `build_edge_provider` wiring it into `run_validate` (`backtest_cmd.rs:232,282`), real per-row `windows` passed to `pbo` (`sweep.rs:353-369`), and the executable proof `validate_real_edges.rs` (`validate_yields_honest_verdict` asserts NON-`Insufficient`; `purge_bites_directionally` asserts purged.pbo > nopurge.pbo). All 5 tests PASS. The GAPS.md G1 text still reads as open ("operator queue") and cites stale line numbers (`backtest_cmd.rs:173-178`); it predates the W7 fix and should be marked resolved. **Flag: documentation drift, not a code gap.**
- **G2 — "Decoupling + scoring-purity not enforced by an executable test" (Minor).**
  **CURRENT STATUS: RESOLVED in code; GAPS.md entry STALE.** `tests/decoupling.rs` now exists: Test 1 greps `fortuna-backtest/src/` (excl. `sources/`) for banned source literals (`decoupling.rs:95`); also `scoring_cargo_has_no_stochastic_or_io_deps` (`:140`) and `scoring_src_has_no_async_or_db_imports` (`:169`) — exactly what G2 said was missing. 3/3 PASS.

**Adversarial residue (genuine, minor):**
- The trial-grid knobs (`calibration_window`, `recal_method ∈ {Platt,Isotonic,None}`) are NOT the transform actually applied: `LedgerEdgeProvider::recalibrate` applies a per-config TEMPERATURE scaling `τ=0.7+0.3·index` keyed on the FLAT config index (`edge_provider.rs:340-353`), so `method=None` does not mean "raw probabilities." This is DISCLOSED (relabeled, not hidden) in `format_go_surface` + `GAPS.md:209-222` ("recalibrate provenance — RELABELED"), pinned by `go_surface_discloses_recal_is_a_temperature_index_not_the_named_knobs`. Honest but the trial space is a single recalibration family (temperature), not the literal {Platt/Isotonic/window-length} grid the spec §2 D3 names. **As-built ≠ as-intended; disclosed.**
- Scalar belief payloads are unsupported by both the harness (`harness.rs:384`) and the edge provider (`edge_provider.rs:155-159`) — binary scopes only. Documented as a later slice. Aeolus brackets are binary so not blocking; a scalar source would error loudly (no silent drop).

---

## 5. I1 / safety — read-only / paper-safe

**VERDICT: STRICTLY READ-ONLY / PAPER-SAFE. No GatedOrder, no venue, no real order on this path.**
- `grep -rE 'GatedOrder|place_order|submit_order|fortuna_exec|fortuna_venue|VenueAdapter|live_order' crates/fortuna-backtest/src/` → **NONE**. The crate has no exec/venue dependency in `Cargo.toml`.
- Source archive opened **read-only**: `AeolusArchiveSource::open_read_only` uses `SQLITE_OPEN_READ_ONLY` (`aeolus_archive.rs:175-189`); CLI uses it in both `run_backtest` (`backtest_cmd.rs:138`) and `run_validate` (`backtest_cmd.rs:288`). Doc: "spec §10 prohibits any write to the source archive" (`aeolus_archive.rs:170-174`).
- Paper-only invariant machine-enforced: `HistoricalTrade::new` rejects `orders != 0` with `RecordError::RealOrderForbidden` (`records.rs:182-184`); the aeolus adapter constructs trades from `shadow_intents` with `orders=0` (`aeolus_archive.rs:485-492`). Trade records carry no order id and never reach an exec path.
- Ledger writes are append-only beliefs/events/scorecards via `insert_historical` + `ON CONFLICT DO NOTHING` (I5), source-stamped `historical-import`, original timestamps preserved (`harness.rs:310,346`) — they never masquerade as a live decision. I6 honored: no model authority introduced; replays recorded beliefs, recomputes scores.

---

## 6. Money / numeric

**VERDICT: NO float-on-money in any PnL/exposure/fee path. CLEAN.**
- All money is `Cents` (i64): `HistoricalSnapshot.price: Cents` (`records.rs:131`), `HistoricalTrade.price: Cents` (`records.rs:155`). Docs explicitly "never `f64` for money" (`records.rs:127,154`).
- `f64` appears only as legitimate process metrics/probabilities: belief `p`, outcomes (score labels), Brier/CLV/DSR/Sharpe/PBO/SPA, `family_n_trials as f64` (a COUNT cast for the Gumbel formula, `sweep.rs:423` — not money).
- The one money-adjacent f64 is a CONVERSION-BOUNDARY read: `yes_mid_cents: f64` is read from SQLite (stored REAL) and immediately `Cents::new(yes_mid_cents.round() as i64)` (`aeolus_archive.rs:445,464`) before entering any path — consistent with the house rule "Decimal/float only at conversion boundaries." `reference_price_cents` read as `i64` directly (`aeolus_archive.rs:474,488`).
- The de-vig baseline divides a `Cents` price by 100.0 to a probability (`edge_provider.rs:176,186`) — that f64 is a PROBABILITY (baseline_p, CLV bps), not money. Correct.

---

## Open questions
1. **G-PARITY byte-identity** is by-construction (shared assembler + shared repo) and the parity test target (`harness.rs` Postgres tests) was not re-executed here (no migrated DB). The WS2 parity seam it extends is cited as proven; not independently re-verified in this review.
2. **DST corpus** (`backtest_dst.rs`) read-verified but Postgres-backed; the idempotency/recovery/clock claims were not re-run end-to-end here. The pure determinism primitives (FNV, SplitMix64, sort) ARE exercised by the passing pure suites.
3. **GAPS.md G1/G2** are stale-open (resolved by W7 `0344e30` / decoupling.rs) — a doc-drift cleanup, not a code defect. Recommend the operator strike them.
