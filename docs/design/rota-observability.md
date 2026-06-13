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
| Cognition | `/cognition` | snapshot + R7 ledger + lifecycle agg | B (queries) | DONE + verified; DEEPENED with the belief LIFECYCLE (status distribution + resolved calibration Brier/CLV via real `GROUP BY`/`AVG`); persona-provenance renders |
| Settlement | `/settlement` | snapshot | live (A) | DONE + verified |
| Streams | `/streams` | snapshot + recorder fs | B | DONE + verified |
| Audit | `/audit` | ledger | B | DONE + verified |
| Ingest — Sources (D V2) | `/ingest_sources` | `IngestionTelemetry` (on main) | D publish | **DONE** — handler + generic `boardTable` renderer + populated test + screenshot (harness seed); prod data pending track-A OBS-2 publish |
| Ingest — Live Feed (D V1) | `/ingest_feed` | `IngestionTelemetry.recent` (on main) | D publish | **DONE** — marquee feed board (reuses `boardTable` + a data-driven `pill` status flag) + populated test + screenshot; prod data pending OBS-2 publish |
| Ingest — Funnel (D V3) | `/ingest_funnel` | `IngestionTelemetry.funnel` (on main) | D publish | **DONE** — funnel-as-stage-table (reuses `boardTable`): fetched→validated→normalized→persisted with retention % + drop-offs + populated test + screenshot; prod data pending OBS-2 (loop-stages null-until-wired, never a fabricated 0) |
| Vendor Scorecard (D V4) | `/ingest/scorecard` | `source_reliability` | D (Layer-3 job) | BLOCKED on the Layer-3 trust-attribution job |
| Forecast→Outcome (D V5) | `/forecast_outcome` | beliefs+events+settlements+signals | mixed | BLOCKED on the data flow |
| Hypothesis Lifecycle (D V6) | (on Cognition) | beliefs (+ intents/settlements) | mixed | PARTIAL — status + calibration live on Cognition; the full belief→strategy→PnL is DATA-BLOCKED (no belief→trade link / `strategy` column / per-belief PnL on the schema — explorer-confirmed; needs a schema change, ledgered) |
| Forecasts scorecard (C 9.1) | `/forecasts` | `scalar_beliefs`/`belief_scores` | C | frontend buildable; query BLOCKED on C tables |
| Perps regime/basis (C 9.2) | `/perps` | `scalar_beliefs`/`belief_scores` | C | "" |
| Personas (E 20.1) | `/personas` | `personas` | E | frontend buildable; query BLOCKED on E tables |
| Analyses (E 20.2) | `/analyses` | `domain_analyses` | E | "" |
| Persona pipeline (E 20.4) | `/persona_pipeline` | metrics + ledger | E | "" |

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
