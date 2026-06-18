# Area 4 — Module Boundaries & Legibility

## Summary

The three largest files — `daemon.rs` (4854 lines), `runner.rs` (3972 lines), `repos.rs` (2479 lines) — each contain 4–7 distinct, separable responsibilities tangled into one compilation unit. None of this is immediately demo-blocking (the paper loop runs), but `daemon.rs` is the most critical readiness risk: it is so large that onboarding any new loop feature requires navigating 4800 lines to find the right insertion point, and `drive()`'s 80-parameter signature is already causing maintainers to pass data as untyped `Option<...>` bundles rather than typed structs. `repos.rs` has the opposite problem — it is a pure dump of every SQL type in the database, making it impossible to find a repo without text-searching. `rota.rs` conflates HTTP handler logic with embedded raw SQL queries that belong in `repos.rs`. `runner.rs`'s size is partially justified by the complexity of the tick loop, but the test module (lines 2993–3972) at 980 lines and two separate concerns (the type-level A3 guard plus paper-live tests) could be split without any semantic change.

## Findings

| Severity | Readiness | Finding | Evidence (path:line) | Why it matters | Root cause | Recommended fix | Suggested test |
|---|---|---|---|---|---|---|---|
| P2 | BLOAT-cut | `daemon.rs` is 4854 lines with 7 tangled responsibilities | `crates/fortuna-live/src/daemon.rs:1–4854` | A developer adding a new loop hook (e.g. a new belief producer) must navigate the entire file to find the right segment boundary in `drive()`; risk of inserting in the wrong scope | Incremental feature accretion (each track added its slice to the end of the file) with no splitting discipline | Split into `daemon/composition.rs` (compose_runner + compose_kalshi_* + credential helpers), `daemon/drive.rs` (drive() + segment-boundary helpers), `daemon/belief_persist.rs` (persist_beliefs, persist_scalar_beliefs), `daemon/scoring.rs` (resolve_and_score_*), `daemon/cognition_cycle.rs` (reconciliation, weekly/monthly review), `daemon/schedulers.rs` (DailyScheduler, WeeklyScheduler, MonthlyScheduler, digest fns), `daemon/alerts.rs` (route_alerts, build_slack_router, deadman_tick) | Compile-only (all existing smokes remain green) |
| P2 | BLOAT-cut | `drive()` has 20 parameters (80-line signature) with several typed as `Option<PgPool>` bundles | `crates/fortuna-live/src/daemon.rs:1724–1803` | The signature is already too large to read without scrolling; each new optional feature adds another `Option<...>` argument; tests calling `drive()` must thread 20 arguments, most of which are `None` | No `DriveContext` wrapper was introduced when the 4th and 5th optional arguments were added | Introduce a `DriveContext` (or `DriveWiring`) struct grouping the optional capabilities (synthesis_refresh, scalar_belief_persist, reconciliation, reviews, personas, discovery, resolution_pool, perp_tick_rx, perp_tick_feed) and pass it as one argument | `DriveContext::default()` in tests passes all-None; explicit tests only set the fields they exercise |
| P2 | BLOAT-cut | `repos.rs` is 2479 lines containing 18 unrelated repository types in a single file | `crates/fortuna-ledger/src/repos.rs:1–2479` | Finding the `BeliefsRepo` API requires knowing it lives in the same file as `FillsRepo`, `HaltsRepo`, `PersonasRepo`, and `FundingRatesHistoricalRepo`; no logical grouping; `lib.rs` re-exports everything in one giant pub-use blob | All repos were added to one file over the lifetime of the project; ledger never gained sub-modules | Split repos.rs into logical modules: `repos/fills.rs`, `repos/halts.rs`, `repos/events.rs`, `repos/beliefs.rs` (BeliefsRepo + ScalarBeliefsRepo + BeliefScoresRepo), `repos/calibration.rs` (CalibrationParamsRepo + TradabilityRepo), `repos/cognition.rs` (PersonasRepo + DomainAnalysesRepo + LessonsRepo + JournalRepo), `repos/signals.rs`, `repos/infrastructure.rs` (FillsRepo + SettlementsRepo + DiscrepanciesRepo + ReservationsRepo + SnapshotsRepo + FundingRatesHistoricalRepo) | All existing ledger integration tests continue to pass |
| P2 | BLOAT-cut | `rota.rs` (2227 lines) embeds 17 raw `sqlx::query` calls that duplicate the ledger-boundary pattern already established in `repos.rs` | `crates/fortuna-ops/src/rota.rs:417–427` (recent_fills), `:488–504` (recent_discovery_events), `:620–648` (recent_discovery_edges), `:713–735` (persona_registry), etc. | Dashboard queries bypass the repo layer; a schema change (e.g. adding a column to `fills`) must be mirrored in both `repos.rs` AND inside `rota.rs`; inconsistent SQL style (some queries compile-time via sqlx macros in repos, runtime `query_as` in rota) | ROTA queries were added directly in the handler file under the "audit-tail precedent" comment (the audit tail was legitimately not in repos.rs), and the pattern was then applied to every subsequent read | Move all named helper queries (`recent_fills`, `recent_discovery_events`, `recent_discovery_edges`, `persona_registry`, etc.) into `fortuna-ledger/src/repos/` sub-modules; keep the ROTA handler as a thin JSON-shaper over repo calls | Existing route tests remain green; add a ledger test for each migrated query |
| P2 | PARK | `runner.rs` (3972 lines) has 980 lines of `mod tests` at the bottom (lines 2993–3972) covering two orthogonal concerns: the A3 type-level guard (lines 2993–3064) and paper-live/settlement tests (lines 3472–3972) | `crates/fortuna-runner/src/runner.rs:2993–3972` | The test block is large but not uniquely entangled with `SimRunner`'s implementation — the A3 guard uses only `SimRunner::new` and type bindings; the paper-live tests could live in a separate integration test file | All runner tests were collocated in the single-file impl (simpler at Phase 0) | Move `mod a3_type_level` and the paper-live/settlement tests to `crates/fortuna-runner/tests/runner_type_safety.rs` and `tests/settlement.rs` respectively | Tests still pass after move |
| P3 | PARK | `ActiveRunner` enum in `daemon.rs` (lines 1082–1398) is a 316-line delegation wrapper that manually triples every `SimRunner` method call | `crates/fortuna-live/src/daemon.rs:1082–1398` | Every time a new method is added to `SimRunner`, it must be added to `ActiveRunner` in 3 parallel `match` arms; the repetition is mechanical | Rust generics can't directly represent a `SimRunner<dyn Venue, _>` without boxing, so the enum was chosen as the safe alternative | This is a known Rust limitation; the mechanical delegation is acceptable. The ONLY improvement path is extracting a `RunnerOps` trait with a blanket impl on `SimRunner<V,J>` so `ActiveRunner` holds `Box<dyn RunnerOps>` — acceptable P3 refactor, not urgent | N/A |
| P3 | PARK | `event_text_tokens`, `normalized_event_text`, `event_text_similarity`, `event_family_key`, `find_world_forward_duplicate_event`, `summarize_belief_evidence` are text-processing utilities in `daemon.rs` (lines 1584–1722) with no connection to daemon lifecycle | `crates/fortuna-live/src/daemon.rs:1584–1722` | Discovery text helpers are buried inside the daemon file rather than the `fortuna-cognition` crate where the discovery logic lives | These helpers were added during discovery feature development and placed nearest their call site | Move to `fortuna-cognition/src/discovery.rs` or a new `fortuna-cognition/src/text_util.rs`; the daemon calls through the crate boundary | N/A |

## Trace / narrative

### File size survey (verified, not assumed)

```
4854  crates/fortuna-live/src/daemon.rs         ← verified
3972  crates/fortuna-runner/src/runner.rs       ← verified
3080  crates/fortuna-ops/tests/rota.rs          (test file, not in scope)
2829  crates/fortuna-runner/tests/perp_event_basis_v2.rs  (test file)
2479  crates/fortuna-ledger/src/repos.rs        ← verified
2227  crates/fortuna-ops/src/rota.rs            ← verified
```

The session evidence claimed daemon.rs > 4500 and runner.rs, repos.rs, and rota.rs were oversized. All four claims are confirmed:

- `daemon.rs` is 4854 lines (exceeds claimed 4500).
- `runner.rs` is 3972 lines (confirmed).
- `repos.rs` is 2479 lines (confirmed).
- `rota.rs` is 2227 lines (confirmed).

### daemon.rs — 7 responsibilities in one file

Scanning the top-level items (`crates/fortuna-live/src/daemon.rs:103–4668`) reveals seven clearly distinct clusters:

1. **Cognition mind construction** (lines 186–271): `mind_from_env`, `triage_from_env`, `SYNTH_MIND_*` constants — belongs alongside boot/config, not in the drive loop.

2. **Runner composition** (lines 274–1026): `compose_runner`, `resolve_kalshi_*_creds`, `build_kalshi_*_transport`, `compose_kalshi_*_runner_with_transport`, `compose_paper_live_runner_with_transport`, `kalshi_fee_model`, `exec_policy_for_runtime`, `compose_kalshi_family_runner_with_venue` — the purest candidate for a `composition.rs` module.

3. **Venue polymorphism layer** (lines 1028–1398): `PgHaltPoller`, `ActiveRunner` enum and its delegation impl, `paper_data_rota_views` — could be `active_runner.rs`.

4. **Drive loop** (lines 1477–3326): `ReviewWiring`, `PersonasWiring`, `DiscoveryWiring`, `drive()` body, `edge_refresh_transition` — the actual runtime loop, the natural core of a `drive.rs` module.

5. **Belief persistence** (lines 3327–3631): `registry_from`, `persist_beliefs`, `persist_scalar_beliefs` — clean, isolated IO operations.

6. **Belief scoring / resolution** (lines 3632–4047): `resolve_and_score_funding_beliefs`, `resolve_and_score_weather_beliefs` — scoring logic that calls into `fortuna-cognition` and `fortuna-sources`; currently misplaced in the daemon.

7. **Cognition cycles + scheduling + alerts** (lines 4049–4668): `run_daily_reconciliation`, `run_weekly_review`, `run_monthly_review`, `route_alerts`, `build_slack_router`, `deadman_tick`, `DailyScheduler`, `WeeklyScheduler`, `MonthlyScheduler`, `terse_daily_digest`, `rich_daily_digest`.

The `drive()` function itself (lines 1724–3326, approximately 1600 lines) is the single largest function body. Its 80-line parameter signature (`crates/fortuna-live/src/daemon.rs:1724–1803`) has 20 parameters, of which 12 are optional capabilities passed as `Option<T>`. Each of `PersonasWiring`, `DiscoveryWiring`, `ReviewWiring` is already a bundled struct, but `synthesis_refresh`, `scalar_belief_persist`, `reconciliation`, `resolution_pool`, and `perp_tick_rx` are still raw `Option<PgPool>` / `Option<Arc<dyn Mind>>` arguments added individually.

### runner.rs — well-structured core with oversized tests

`runner.rs`'s `SimRunner` struct (`line 115–209`) holds 24 fields across clear concerns: bus, venue, gates, order manager, position/settlement/reservation state, telemetry, and cognition scaffolding. The public API (`line 574–2971`) is coherent — every method does one thing. The problem is the 980-line `mod tests` block at line 2993. It contains:

- `mod a3_type_level` (lines 2993–3064): a pure type-level compile test with no runtime logic.
- Paper-live integration tests (lines 3472–3972): 500 lines covering market refresh, synthesis edges, external positions, and settlement pages that would read more naturally in `crates/fortuna-runner/tests/`.

The test infrastructure (minimal_config helper, local mock Strategy, mock Venue) at lines 3064–3470 is the real anchor keeping the tests collocated.

### repos.rs — a flat dump of every table

`repos.rs` has 134 public items (verified via `grep -c`). The module comment at line 1 still says "Phase-0 repos: fills mirror, halt persistence, reservation events" — it was never updated when 15 additional repositories were added. The `lib.rs` re-export at line 33–42 lists every struct in one flat `pub use repos::{...}` blob with no grouping. The section comments (`// ------- Track E -------` at line 1818, `// ------- Calibration -------` at line 1459, etc.) are present but not enforced by module boundaries.

### rota.rs — HTTP handlers plus embedded SQL

`rota.rs` has 17 `sqlx::query`/`sqlx::query_as` calls (verified). Of these, 14 are in named public helper functions (`recent_fills`, `recent_discovery_events`, `persona_registry`, `persona_scorecard`, `persona_pipeline`, `recent_analyses`, `forecast_scorecard`, `recent_funding_rates`, `funding_forecast_scores`, `db_table_counts`, `audit_tail_page`, `recent_gate_rejections_page`, `recent_watchdog_events_page`, `recent_discovery_edges`) that have already been extracted from the handler bodies. These belong in `fortuna-ledger/src/repos/` to honour the ledger-as-persistence-boundary design.

Three of the 17 queries (`recent_funding_section`, `funding_edge_gate_section`, `belief_lifecycle`) are private async helpers embedded in the file — also candidates for movement.

## Self-adversarial pass

**Finding 1 (daemon.rs split):** The strongest counterargument is "the file is large, but it compiles cleanly and all the smokes pass, so splitting carries real risk of merge conflicts." This is true — the branch has uncommitted parallel-agent changes, and a split during active development increases the chance of conflicts. The rating is P2 (delivery risk) rather than P1 precisely because the current state does not break demo functionality. The split IS the right long-term move; its urgency depends on how many more tracks plan to add items to daemon.rs.

**Finding 2 (drive() signature):** There is a valid argument that the current explicit-parameter style is MORE readable than a `DriveContext` struct, because each optional is documented in place. The counterargument (the one I maintain) is that 20 parameters means `None, None, None, None, None, None` is unreadable at call sites, and the current `main.rs` call already requires line-by-line comparison to know which `None` is which capability. `DriveContext { synthesis_refresh: Some(...), ..Default::default() }` is strictly more readable.

**Finding 3 (repos.rs):** The repos split is low-risk (pure refactor, no logic change) and high-payoff (the ledger crate is touched by almost every other crate). The P2 rating is appropriate.

**Finding 4 (rota.rs SQL):** I noted 17 sqlx queries. The "audit-tail precedent" comment justifies runtime sqlx for read-only dashboard queries, but it does NOT justify placing those queries in the HTTP handler file rather than in a named repo type. The finding is accurate.

**Potential false positive:** I rated `ActiveRunner` as P3/PARK. One could argue it belongs in the composition module, not `daemon.rs` proper. However, moving it to a new file does not decompose any responsibilities — it would just relocate 316 lines. I am comfortable that this is genuinely P3.

**What I may have missed:** I did not inspect `crates/fortuna-live/src/compose.rs` (973 lines) in depth. At first glance it contains `synthesis_edges`, `calibration_for_scope`, `DegradeScrape`, `SynthesisSection`, `ReviewSection`, and perp-event-basis config builders — all support functions for `daemon.rs` composition. It is well-sized and well-named; I do not see a legibility problem there, but a deeper read might surface one. I also did not investigate whether `fortuna-cli/src/main.rs` (1365 lines) duplicates any composition logic from `daemon.rs`.

## Open questions for the Lead

1. **Merge window:** The recommended daemon.rs split will conflict with any in-flight track that also adds a segment to `drive()`. Should the split be scheduled as a dedicated refactor commit BEFORE the next track lands, or after the branch merges?

2. **DriveContext vs explicit params:** Is there a project preference for "explicit args in public function signatures" over "struct bundling" for testability? If the answer is "always explicit for testability", the `drive()` signature stays as-is and this finding is downgraded to P3.

3. **rota.rs SQL ownership:** The audit-tail queries (`audit_tail_page`, `recent_gate_rejections_page`, `recent_watchdog_events_page`) were intentionally placed outside `repos.rs` because they are read-only dashboard queries with no write path. Should the ledger crate grow a `ReadRepos` module, or should these stay in `rota.rs` under the explicit precedent?

4. **Belief resolution in daemon.rs:** `resolve_and_score_funding_beliefs` and `resolve_and_score_weather_beliefs` (lines 3633–4047) call into `fortuna-cognition` and `fortuna-sources`. Should these move to a `fortuna-cognition/src/resolution.rs` module to keep all scoring logic in the cognition crate, or does the daemon's ownership of the DB pool justify keeping them here?
