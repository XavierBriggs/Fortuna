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
| Cognition | `/cognition` | snapshot + R7 ledger | B (queries) | DONE + verified; persona-provenance render = item 1 |
| Settlement | `/settlement` | snapshot | live (A) | DONE + verified |
| Streams | `/streams` | snapshot + recorder fs | B | DONE + verified |
| Audit | `/audit` | ledger | B | DONE + verified |
| Ingest — Live Feed (D V1) | `/ingest/feed` | `IngestionTelemetry` | D | frontend buildable; live data BLOCKED on D struct + A shaping |
| Ingest — Sources (D V2) | `/ingest/sources` | `IngestionTelemetry` | D | "" |
| Ingest — Funnel (D V3) | `/ingest/funnel` | `IngestionTelemetry` | D | "" |
| Vendor Scorecard (D V4) | `/ingest/scorecard` | `source_reliability` | D (Layer-3 job) | BLOCKED on the Layer-3 trust-attribution job |
| Forecast→Outcome (D V5) | `/forecast_outcome` | beliefs+events+settlements+signals | mixed | BLOCKED on the data flow |
| Hypothesis Lifecycle (D V6) | `/hypotheses` | beliefs+intents+settlements | mixed | BLOCKED on the data flow |
| Forecasts scorecard (C 9.1) | `/forecasts` | `scalar_beliefs`/`belief_scores` | C | frontend buildable; query BLOCKED on C tables |
| Perps regime/basis (C 9.2) | `/perps` | `scalar_beliefs`/`belief_scores` | C | "" |
| Personas (E 20.1) | `/personas` | `personas` | E | frontend buildable; query BLOCKED on E tables |
| Analyses (E 20.2) | `/analyses` | `domain_analyses` | E | "" |
| Persona pipeline (E 20.4) | `/persona_pipeline` | metrics + ledger | E | "" |

Legend — "BLOCKED on X tables" = ROTA's read-only query needs a table not yet on
main; the panel ships frontend + honest-degraded (`available:false`) and lights
up when the owning track merges. "A shaping" = a live in-memory board must be
shaped daemon-side in `fortuna-live::views_from` (R2 — fortuna-ops cannot depend
on the runner), a cross-track seam tracked in GAPS. Telemetry families (D §3, C
§8, E §19) wire into `fortuna-ops` `MetricsRegistry` (integer-only) after the
seams.

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
- `architecture.md` gets a FURTHER targeted pass WHEN the first new board lands
  (not before — naming unbuilt boards would itself be staleness).

## Changelog

- **2026-06-13** — Mission 2 kickoff.
  1. Validation + sequenced build queue + cross-track data-seam requests recorded
     (commit `29d4a93`; GAPS + rota-dashboard.md §4 pointer).
  2. Local bringup harness `examples/rota_local` landed; the 7 existing boards
     screenshot-verified with real rows off a seeded local DB (Health / Money /
     Gates / Cognition incl. a persona-provenance belief + a resolved belief with
     Brier+CLV / Settlement / Streams recorder-live / Audit). Hardened by a
     feature-dev code-reviewer pass (real `GateCheck` names, `discrepancies_open`,
     null `book_age_ms`, propagated clock errors). This doc + the runbook + the
     operations/architecture pointers added.
