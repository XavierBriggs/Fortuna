# ROTA observability — track-B mission 2 (the operator's single pane of glass)

Living status doc + changelog for the TOTAL ROTA OBSERVABILITY mission (track B,
re-missioned 2026-06-13). The 7 original boards (T4.3, [rota-dashboard.md](rota-dashboard.md))
are DONE+merged; mission 2 extends ROTA into the single read-only pane over
belief formation, the full pipeline, trades, discovery/events, the DB, and
telemetry on every layer — consuming the C/D/E observability contracts.

- **Doctrine (absolute):** read-only (zero mutating endpoints), gold-on-black,
  honest nulls (`—`, never a faked zero), every board screenshot-verified with
  real rows before its box is ticked.
- **Coordination, blocked items, cross-track data-seam requests, and the
  sequenced build queue (items 0–6):** GAPS.md, section "TRACK B — RE-MISSIONED
  ... TOTAL ROTA OBSERVABILITY" (the honesty ledger — single source for those).
- **Design sources:** [rota-dashboard.md](rota-dashboard.md) (the 7 boards + R1–R12);
  the three cross-track contracts — [ingestion-observability-contract.md](ingestion-observability-contract.md)
  (D), [perp-strategies-and-scalar-claims.md](perp-strategies-and-scalar-claims.md)
  §8–9 (C), [domain-analysis-personas-design.md](domain-analysis-personas-design.md)
  §14,§19–20 (E). (The contracts live on the track-D/C/E branches until merged.)

## Board status matrix

| Board | View (endpoint TBD at build) | Data source | Owner of data | Status |
|---|---|---|---|---|
| Health | `/health` | snapshot | live (A) | DONE + screenshot-verified (mission 1) |
| Money | `/money` | snapshot | live (A) | DONE + verified |
| Gates | `/gates` | snapshot | live (A) | DONE + verified |
| Cognition | `/cognition` | snapshot + R7 ledger + lifecycle agg | B (queries) | DONE + verified; DEEPENED with the belief LIFECYCLE (status distribution + resolved calibration Brier/CLV via real `GROUP BY`/`AVG`); **§20.3 provenance LEGIBILITY** — each belief's expander now shows a labeled `persona@v · model · cost · analysis · run` line (which source/persona drove it), cross-referencing the Personas/Analyses boards |
| Settlement | `/settlement` | snapshot | live (A) | DONE + verified |
| Streams | `/streams` | snapshot + recorder fs | B | DONE + verified |
| Audit | `/audit` | ledger | B | DONE + verified |
| Trades — Recent Fills (item 3) | `/fills` | `fills` ledger | B | **DONE** — executed-trades board (runtime sqlx + `cents` flag) |
| Trades — Strategy P&L (item 3) | `/strategies` | `runner.digest_snapshot()` | B (views_from) | **DONE** — per-strategy realized PnL/fees/fills/open-exposure (views_from + `cents`); unrealized-PnL gap is a follow-on (mark loop, GAPS) |
| Trades — Working Orders (item 3) | `/working_orders` | `runner.manager().intents()` | B (views_from) | **DONE** — the intents resting at the venue (submitted/acked/partially-filled) with market/side/action/limit($)/qty/filled/status; views_from fold filtered by `is_working()`, pure panic-free read (daemon_smoke 15/15) |
| Discovery — Events (item 4) | `/discovery` | `events` ⋈ `market_event_edges` | B | **DONE** — canonical events + status + DISTINCT mapped-market count (runtime sqlx); benchmark detail + per-event drill-in + sources inventory are follow-ons (GAPS) |
| Discovery — Edges (T4.5 a / item 4) | `/discovery_edges` | `market_event_edges` ⋈ `events` ⋈ `tradability_scores` | B | **DONE** — the live (non-superseded) market↔event mappings JOINed to their event statement: market · venue · mapping · confidence · **Trad** (latest `tradability_scores.score` per market, honest-null when unscored — the T4.5(a) Tradability⋈Edges join) · confirmed/proposed status · proposer/confirmer. "The markets/series UNDER the events." Runtime sqlx, newest-event-first; confirmed=green pill, proposed/unscored→honest-null; untrusted strings esc'd. Screenshot (pre-Trad): `rota-discovery-edges-2026-06-14.png`; the Trad column is curl+test-verified (browser screenshot deferred — MCP disconnected). events→edges drill-in superseded by this board |
| Database (item 5) | `/db` | all 24 ledger tables | B | **DONE** — exact `COUNT(*)` sweep over every ledger table (incl. the `scalar_beliefs`/`belief_scores` plane), busiest-first, with a `{tables,total_rows}` summary (runtime sqlx, literal names — no injection; honest `0` for empty tables); reltuples-at-scale + per-table drill-in are follow-ons (GAPS) |
| Telemetry (item 6) | `/telemetry` | `MetricsRegistry` | B (shape) + live (A) | **DONE** — the metric series the daemon exports (the same registry `/metrics` renders), grouped by subsystem (one row per series: subsystem + metric + type + value). R2-clean: the daemon shapes via `MetricsRegistry::telemetry_board` (additive `views["telemetry"]`), ROTA serves via `read_view` — no Prometheus-text parsing. **Completes the single pane of glass across all 6 mission items.** Help-text + metric search are follow-ons (GAPS) |
| Ingest — Sources (D V2) | `/ingest_sources` | `IngestionTelemetry.sources` | B (OBS-2c) | **LIVE + OBS-3 attrs + operational** — handler + `boardTable`; daemon shapes via `merge_ingest_views`. Surfaces the source_registry admission attributes **Domains** (`domain_tags`) + **Tier** (`trust_tier`), AND the operational-health fields **Fetch err** (`fetch_errors`) + **Rearm** (`rearms`) + **Last error** (`last_error`, redacted/capped; honest-null when clean) — the "why" behind a degraded source, beyond the validation-drop counts. Screenshot (pre-operational): `rota-sources-health-domains-2026-06-13.png` (operational cols curl+test-verified; browser screenshot deferred — MCP) |
| Ingest — Live Feed (D V1) | `/ingest_feed` | `IngestionTelemetry.recent` | B (OBS-2c) | **LIVE** — marquee feed board (`boardTable` + `pill` status); daemon shapes via `merge_ingest_views` from the recent-signals ring |
| Ingest — Funnel (D V3) | `/ingest_funnel` | `IngestionTelemetry.funnel` | B (OBS-2c) | **LIVE** — funnel-as-stage-table; daemon shapes via `merge_ingest_views`; loop-stages real (OBS-2a); honest gate skips an unticked funnel |
| Vendor Scorecard (D V4) | `/ingest/scorecard` | `source_reliability` | D (Layer-3 job) | BLOCKED on the Layer-3 trust-attribution job |
| Forecast→Outcome (D V5) | `/forecast_outcome` | beliefs+events+settlements+signals | mixed | BLOCKED on the data flow |
| Hypothesis Lifecycle (D V6) | (on Cognition) | beliefs (+ intents/settlements) | mixed | PARTIAL — status + calibration live on Cognition; the full belief→strategy→PnL is DATA-BLOCKED (no belief→trade link / `strategy` column / per-belief PnL on the schema — explorer-confirmed; needs a schema change, ledgered) |
| Forecasts scorecard (C 9.1) | `/forecasts` | `scalar_beliefs`⋈`belief_scores` | B (query) | **DONE** (calibration half) — per (producer, rule): mean CRPS (lower=better) + the 0.1–0.9 BAND COVERAGE % (~80 ideal) + resolved_n + unit (runtime aggregate; reads only the q-boundary numbers — raw fan/provenance NOT exposed). Degrades honest-unavailable until track-C daemon persist (slice 4) writes the tables. sparkline is a follow-on (GAPS) |
| Forecast Feed (C 9.1) | `/forecast_feed` | `scalar_beliefs` (`ScalarBeliefsRepo::recent`) | B (query) | **DONE + RICH + screenshot-verified** — the operator "completely see the belief and everything" want (2026-06-13). Each recent scalar belief is a click-to-expand `<details>` (the /cognition belief-panel precedent): SUMMARY = producer · event · q=0.5 median · unit · resolved/pending pill · → realized (honest null while pending); EXPAND = the WHOLE quantile FAN (q/v pairs) + the producer's EVIDENCE (e.g. estimate/point_forecast/remaining_candles) + provenance. The daemon wraps `{"provenance":…,"evidence":…}` into the one provenance column (persist_scalar_beliefs) — SPLIT back here (both-keys detection; non-wrapped shown whole). Untrusted-data (5.11): `clean_quantiles` reads only numeric q/v, `truncate_evidence` size-caps, JSON esc'd. No ledger change. Screenshot: `docs/reviews/rota-visual/rota-forecast-feed-rich-2026-06-13.png` |
| Perps regime/basis (C 9.2) | `/perps` | funding regime + basis | C | frontend buildable; the funding-regime/basis board is the next C slice |
| Personas (E 20.1) | `/personas` | `personas` | B (query) | **DONE** (registry half) — every (persona_id, version) grouped/versioned with status pill, tier, 8-char method hash, flattened `reads_signal_kinds`, effective date (runtime sqlx) |
| Persona Scorecard (E 20.1) | `/persona_scores` | `beliefs` ⋈ provenance | B (query) | **DONE** (outcomes half — unblocked by track-E persona runtime) — per persona, n_resolved + mean Brier (↓) + mean CLV bps (↑) aggregated from `beliefs` grouped by `provenance->>'persona_id'`, honest `evaluating (n/60)` verdict (PROMOTABLE/RETIRE + baselines + calibration_quality OMITTED — unpersisted/cognition logic, never faked) |
| Analyses (E 20.2) | `/analyses` | `domain_analyses` (+ `beliefs` fanout) | B (query) | **DONE + §20.2 EXPANDER** — artifact ledger as a per-analysis click-to-expand `<details>` (the /cognition precedent): summary = persona `id@version` · region · produced_at · cost $ · belief-FANOUT count · supersession status; EXPAND = the persona's **findings** + the **signal_manifest** it read. findings/manifest are UNTRUSTED model output (5.11) — size-capped (`truncate_evidence`) + esc'd JSON, never interpreted. Custom renderer; curl+test-verified (browser screenshot deferred — MCP). beliefs-list json_agg is a further (covered by the fanout count + /cognition) |
| Persona pipeline (E 20.4) | `/persona_pipeline` | `personas`+`domain_analyses`+`beliefs` | B (query) | **DONE** — per persona, the pipeline funnel analyses → beliefs → resolved (a registry-universe LEFT-JOIN aggregate; idle personas read honest 0s; counts only — no content). The conversion at each stage is the pipeline-health signal |

Legend — "BLOCKED on X tables" = ROTA's read-only query needs a table not yet on
main; the panel ships frontend + honest-degraded (`available:false`) and lights
up when the owning track merges. "D publish (OBS-2)" = the `IngestionTelemetry`
STRUCT is on main (track-D OBS-1, `fortuna-sources/scheduler.rs`), but the live
PUBLISH into `snapshot.views["ingest_*"]` is the daemon drive-seam slice
sequenced vs track A (track-D GAPS OBS-2). ROTA serves the board envelope from
`snapshot.views` (R2 — pure projection, zero ingestion-crate dependency); the
generic `boardTable` renderer + harness seed let it screenshot-verify now, and it
lights up live when the publish lands. Telemetry families (D §3, C §8, E §19)
wire into `fortuna-ops` `MetricsRegistry` (integer-only) after the seams.

## Local bringup harness (the screenshot rig)

`crates/fortuna-ops/examples/rota_local.rs` stands the console up standalone
against a SEEDED throwaway Postgres — no daemon, no trading loop. It is how every
board is screenshot-verified with real rows. Runbook:
[rota-local-bringup.md](../runbooks/rota-local-bringup.md). Safety: it reads only
`ROTA_LOCAL_DATABASE_URL` (never the operator's `DATABASE_URL`) and refuses any
DB whose name lacks `rota_local`. The harness — not a static screenshot — is the
durable, never-stale artifact: it regenerates current truth on demand.

## Shared-doc maintenance log (this mission)

Per operator standing instruction (2026-06-13): track B keeps its own docs (this
file + its changelog) current, and amends shared docs by TARGETED edit as work
lands — no staleness, no speculation. Touched so far:
- `operations.md` §2 — pointer to the standalone local-harness bringup.
- `architecture.md` — fortuna-ops crate-map line: ROTA = the single read-only
  observability pane fed by the cross-track contracts.
- `runbooks/rota-local-bringup.md` — new.
- `architecture.md` stays accurate at the PANE level (ROTA = the observability
  pane); per-board status lives in the matrix above + the root CHANGELOG, not in
  architecture.md (board-granularity there would be churn). Revisit only if a
  board changes a SUBSYSTEM boundary.
- Changelog migrated to the root `CHANGELOG.md` (track-B subsection) per the bus
  doc-ownership directive (2026-06-13: one root changelog, no per-track files).

## Changelog

Track B's change history lives in the root [CHANGELOG.md](../../CHANGELOG.md)
("Track B — ROTA observability") per the doc-ownership model (one root changelog,
per-track subsections — GATE-FINDINGS-LATEST 2026-06-13). The board-status matrix
above is the at-a-glance current state.
