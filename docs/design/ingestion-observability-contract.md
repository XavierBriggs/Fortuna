# Ingestion Observability Contract (telemetry + ROTA views)

Date: 2026-06-13. Status: CONTRACT for track-b to implement.

BUILD STATUS (2026-06-15): the §2 seam is DELIVERED — `IngestionTelemetry`,
`SourceTelemetry`, `FunnelCounts`, `SignalRecord`, `TickTelemetry` live in
crates/fortuna-sources/src/scheduler.rs with the field set below. The live ROTA
views V1–V3 are BUILT: V1 Live Feed = `GET /api/rota/v1/ingest_feed`, V2 Sources
Health = `/api/rota/v1/ingest_sources`, V3 Funnel = `/api/rota/v1/ingest_funnel`
(handlers in fortuna-ops/src/rota.rs; the daemon shapes them via
`fortuna_live::views::merge_ingest_views`). V4 Scorecard / V5 Forecast→Outcome /
V6 Hypothesis-Lifecycle remain as specced below — V4 still awaits the Layer-3
`source_reliability` job (§7).
Scope: full observability of the news-aggregation / weather-signal pipeline —
the system running, the signals coming in + their data, and the outcomes of
each source/vendor and of the whole scientific-method process.

This is the END CONTRACT. Track-D (me) owns the **data surface** (§2, in
`fortuna-sources` + the ingestion loop); track-B owns the **export + views**
(§3–§4, in `fortuna-ops`/`fortuna-live` ROTA). The seam between us is ONE struct
(§2 `IngestionTelemetry`) plus the existing persisted tables. Read-only
throughout (ROTA's absolute doctrine: zero mutating endpoints).

## 1. Observability goals (what the operator must see)

1. **The system is running** — per-source health/liveness, last poll, next due,
   quarantines, 304-vs-fetch ratio. Is ingestion alive and polite?
2. **Signals coming in + their data** — a live feed of recently-ingested signals
   with their actual payload fields, accepted/dropped status, and why.
3. **The process** — the funnel from fetch → validate → normalize → persist →
   trigger, with counts and drop-reasons at each stage.
4. **The outcomes of each vendor** — per-source empirical record: did this
   source's evidence correlate with good beliefs? For Aeolus: its self-reported
   skill vs FORTUNA's independently measured skill (the closed Layer-3 loop).
5. **The whole process outcome** — per weather event: forecast → calibrated
   belief → market-implied → graded outcome → Brier/CLV → PnL.

## 2. The data surface (track-D owns; the seam)

A single live snapshot, updated by the ingestion loop each tick, behind an
`Arc<RwLock<…>>` (mirrors the daemon's existing `snapshot` pattern — built once,
written between ticks, read by both the Prometheus renderer and the ROTA
handlers). One source of truth → telemetry and views never disagree.

```rust
/// Live ingestion telemetry. ONE writer (the ingestion loop), many readers.
pub struct IngestionTelemetry {
    pub generated_at: String,                 // UTC ISO8601 (injected Clock)
    pub sources: Vec<SourceTelemetry>,        // per-source live state + counters
    pub funnel: FunnelCounts,                  // process-wide stage totals
    pub recent: RingBuffer<SignalRecord>,      // last N ingested/dropped signals
    pub last_tick: TickTelemetry,              // the most recent tick's outcome
}

pub struct SourceTelemetry {
    pub source_id: String,
    pub kind: String,                          // nws.alert | rss.item | nws.cli | …
    pub domain_tags: Vec<String>,              // weather | macro | …
    pub trust_tier: u8,
    pub health: &'static str,                  // healthy | degraded | quarantined
    pub last_poll_at: Option<String>,
    pub last_success_at: Option<String>,
    pub next_due_at: Option<String>,
    // counters (monotonic; the SourceMetrics already built in D9 + additions):
    pub polls: u64, pub empty_polls: u64, pub fetch_errors: u64,
    pub accepted: u64,
    pub dropped_future: u64, pub dropped_republished: u64, pub dropped_over_volume: u64,
    pub quarantines: u64, pub rearms: u64,
    pub last_error: Option<String>,            // redacted (never secrets/tokens)
}

pub struct FunnelCounts {                       // process-wide, since boot
    pub fetched: u64,                           // raw items returned by adapters
    pub validated_accepted: u64,                // passed Layer-1
    pub validated_dropped: u64,                 // refused by Layer-1 (sum by reason)
    pub normalized: u64,                        // became SignalEnvelopes
    pub deduped: u64,                           // dropped by the ledger UNIQUE
    pub persisted: u64,                         // written to the signals table
    pub persist_failures: u64,
}

pub struct SignalRecord {                       // for the live feed — DATA, redacted
    pub at: String,                             // received_at
    pub source_id: String,
    pub kind: String,
    pub claimed_time: Option<String>,
    pub status: &'static str,                   // accepted | dropped:<reason>
    pub summary: String,                        // a few key payload fields, truncated
}
```

**Track-D delivers** `SourceTelemetry`/`FunnelCounts`/`SignalRecord` populated by
the ingestion loop (most of `SourceTelemetry` already exists as D9
`SourceMetrics`; the recent-feed + funnel + live timestamps are the upgrade).
The `summary` is a short, redacted projection of the payload (e.g. an alert's
`event`, an RSS `title`, a CLI `report_date`) — never raw secrets; untrusted
content stays quoted data (spec 5.11).

## 3. Prometheus export (track-B wires from §2)

Derive metrics from the snapshot each scrape — names/labels stable so dashboards
don't churn. Per-source counters carry a `source` label; drops carry `reason`:

```
fortuna_ingest_polls_total{source}
fortuna_ingest_accepted_total{source}
fortuna_ingest_dropped_total{source,reason}        # future|republished|over_volume
fortuna_ingest_fetch_errors_total{source}
fortuna_ingest_quarantines_total{source}
fortuna_ingest_source_health{source}               # gauge 0 healthy/1 degraded/2 quarantined
fortuna_ingest_funnel_total{stage}                 # fetched|accepted|dropped|persisted|…
fortuna_ingest_persist_failures_total
fortuna_ingest_last_success_age_seconds{source}    # liveness (now - last_success_at)
```
Expansion is trivial: a new source = new label values (no code); a new drop
reason = a new `reason` value; a new counter = one field + one render line.

## 4. ROTA views (the deliverable for track-B)

Read-only, gold-on-black. Every view is a pure projection — live views from the
§2 snapshot, historical/outcome views from the persisted tables (queried per
segment, like the existing ROTA boards). All share ONE envelope so the frontend
renders any board generically and adding a column/row is trivial:

```json
{ "title": "...", "generated_at": "ISO8601",
  "columns": [{"key":"source_id","label":"Source"}, ...],
  "rows":    [{"source_id":"nws_alerts","health":"healthy", ...}, ...],
  "summary": {"...": "headline rollups"} }
```

### V1 — Live Signal Feed  *(goal 2: "signals coming in + their data")*
The marquee view. The last N `SignalRecord`s newest-first: time, source, kind,
claimed-time, status (accepted / dropped:reason), and the `summary` (the actual
data — "Severe Thunderstorm Warning", "Federal Reserve announces…", CLI
`report_date`). Filterable by source/kind/status. This is "the system running +
here's the data" at a glance. Source: §2 `recent`.

### V2 — Sources Health board  *(goal 1: "the system is running")*
One row per source: health badge, last-success age, polls, accepted,
dropped-by-reason (3 columns), 304-rate (`empty_polls/polls`), quarantines,
next-due. `summary`: counts healthy/degraded/quarantined + total accepted/dropped
this session. Surfaces the AFD-firehose (huge `dropped_over_volume`) immediately.
Source: §2 `sources`.

### V3 — Ingest Funnel  *(goal 3: "the process")*
The pipeline as stage counts with drop-offs: `fetched → accepted (− dropped by
future/republished/over_volume) → normalized → (− deduped) → persisted (−
failures)`. Rendered as a funnel/sankey or a stage table with retention %. Shows
where signal is lost and why. Source: §2 `funnel`.

### V4 — Source / Vendor Scorecard  *(goal 4: "outcomes of each vendor")*
GENERIC per-vendor scorecard (instantiates for any source — Aeolus, NWS, an RSS
feed; the same shape would serve a venue vendor too). Per source: trust tier,
domain, #beliefs attributed, **accuracy** (Brier correlation of its evidence
with good/bad outcomes), **earliness** (CLV-for-sources: did it carry the info
before the benchmark moved). For Aeolus specifically, two extra columns:
**self-reported skill** (`skill.crpss_vs_raw`) vs **FORTUNA-measured skill** —
agreement reinforces trust, a gap is a flagged anomaly (the honest Layer-3 loop;
remember the producer's caveat — Aeolus's edge over the market is unproven, so
this view is where it's earned, not asserted). Source: the `source_reliability`
table (Layer 3, populated by the weekly trust-attribution job) + the
`source_registry`. Empty/“insufficient data” until beliefs settle — show n.

### V5 — Forecast → Outcome board (weather)  *(goal 5: "whole process outcome")*
The scientific-method loop for one domain, per event/day: the Aeolus forecast
(μ, σ, model_version, run_at), the market threshold + market-implied p,
FORTUNA's calibrated belief p, the **graded outcome** (the NWS observed daily
high from the CLI grader, F2), and the score (Brier, CLV). One glance answers
"did the forecast beat the market, and did we?". Source: `beliefs` +
`events`/`settlements` + the `aeolus.forecast` + `nws.cli` signals.

### V6 — Hypothesis Lifecycle / Pipeline board  *(the north star)*
Beliefs by status (open / resolved / superseded / abandoned), each with
provenance (which source/persona, model_id, run_at — click-to-expand the
evidence + provenance JSONB, per the ROTA cognition-panel doctrine), the
strategy it fed, and the realized PnL. The end-to-end scientific-method view:
signal → hypothesis → trade → outcome → trust update. Source: `beliefs` +
`intents`/`settlements`.

## 5. Design principles (so expansion is trivial)

- **One snapshot, two consumers.** Prometheus and ROTA both derive from the §2
  `IngestionTelemetry` (live) + the same persisted tables (historical). Never a
  second source of truth → they can't disagree.
- **Read-only projections only.** No view introduces mutating state or an
  endpoint that writes — ROTA's absolute doctrine. A view is a `fn(state) ->
  Board`.
- **One board envelope.** Every view emits `{title, generated_at, columns, rows,
  summary}`. The frontend renders any board generically; a new column is a map
  entry, a new view is a new projection fn — no frontend change.
- **Per-entity keying.** Rows key on `source_id` / `event_id` / `belief_id`, so a
  new source or event appears with zero code change.
- **Live vs historical split is explicit.** Live (V1–V3) reads the in-memory
  snapshot (cheap, every refresh); outcome/historical (V4–V6) queries Postgres
  per segment (like the existing boards). Don't block the live views on DB.
- **Redaction is mandatory.** No secrets/tokens in `last_error`, `summary`, or
  any payload preview (house rule + spec 5.11). Untrusted content is quoted
  data, never rendered as instruction.
- **Degrade gracefully.** V4–V6 show "insufficient data (n=…)" until beliefs
  settle — never a blank or a fabricated number (mirrors the calibration
  shrinkage discipline).

## 6. Suggested build order for track-B

1. Wire the §2 `IngestionTelemetry` snapshot into the metrics renderer +
   ROTA snapshot (the seam). 2. **V2 Sources Health** + **V1 Live Feed** (the
   live observability the operator wants first). 3. **V3 Funnel**. 4. The
   Prometheus metrics (§3). 5. **V5/V6** once beliefs flow. 6. **V4 Scorecard**
   once the Layer-3 `source_reliability` job lands.

## 7. Open items / coordination

- The §2 snapshot struct is delivered by track-D (mostly built as D9
  `SourceMetrics`; the recent-feed + funnel + live timestamps are a focused
  addition). Track-B builds against the struct as specced here — start now; the
  field names are stable.
- V4/V5/V6 depend on the cognition Layer-3 scoring + the `source_reliability`
  table (a cognition-track item) — V4 shows `n` and "insufficient data" until
  then. The live views (V1–V3) need none of that.
