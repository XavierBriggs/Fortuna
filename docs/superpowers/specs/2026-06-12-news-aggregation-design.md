# News Aggregation Subsystem — Design

Date: 2026-06-12
Status: draft, pending operator approval
Authority: conforms to docs/spec.md v0.9 (sections 5.7–5.12). Where this design extends
the spec, the extension is flagged in "Spec deltas" below. The spec wins on conflict.

## 1. Purpose

Give FORTUNA's existing hypothesis pipeline its missing input layer. The world-forward
discovery loop (spec 5.12, built in T3.2) synthesizes candidate events *from the signals
store*; today that store receives almost nothing. This subsystem populates it with news
and scheduled-release signals across four domains — macro/econ, politics/elections,
weather, entertainment — and simultaneously feeds the trigger engine for fast
event-window reactions.

North-star framing: the scientific-method pipeline (signals → hypotheses → beliefs →
calibration → promotion gates → tracked strategies → retirement) already exists in the
spec and largely in code. This design builds the sensory organ, not a new brain. Nothing
downstream of the signals table changes.

## 2. Decisions already made (with operator, 2026-06-12)

- **Domains (v1):** macro/econ releases, politics/elections, weather, entertainment
  (review scores, awards).
- **Acquisition posture:** free structured feeds first; polite scraping where no feed
  exists; LLM/MCP-mediated fetching allowed but config-gated and off by default; paid
  APIs only with demonstrated ROI.
- **Consumers (v1):** both the world-forward discovery loop (15–60 min freshness) and
  fast triggers around scheduled events (seconds-scale inside configured event windows).
- **Architecture:** Option 2 — deterministic Rust fetch substrate and structured-feed
  adapters; LLM-assisted *extraction* (not fetching) as a designed-in escape hatch for
  scrape-class sources; MCP acquisition as a config-gated source class.

## 3. Relationship to the spec

- **5.11 Signal ingestion subsystem** is the contract this implements: dumb per-source
  adapters behind the one `Source` trait, normalizer envelope
  `{source, type, received_at, payload, content_hash}` with dedup, append-only signals
  store, fail-closed `source_registry` allowlist with trust tiers, trigger engine on
  top. 5.11 explicitly lists "RSS, REST, webhook, MCP plumbing, scraper, file drop" as
  acquisition means hidden behind `Source`. Quality bar, verbatim: "Adding a source must
  remain an afternoon of work."
- **5.12 World-forward discovery** consumes the signals store and requires every
  watchlist event to declare a resolution source from the registry. Sources registered
  by this subsystem double as resolution sources (e.g. BLS is both the "CPI printed"
  signal and the grader of the CPI belief). Source selection therefore expands which
  hypotheses the system is permitted to generate.
- **Mind relationship:** the subsystem *feeds* Mind (signals become quoted context
  items, data-not-instructions per 5.11) and, in exactly one bounded place, *uses* the
  Mind machinery: a cheap-tier model extracts structured fields from already-fetched
  raw bytes, via a narrow `Extractor` interface over `MindTransport` + `CostBudget`
  (tiering is per-instance config, the same pattern as 5.9's triage/synthesis split).
  The model never chooses what to fetch and never mutates state. I5/I6 intact.

## 4. Architecture

### 4.1 Placement

New crate `crates/fortuna-sources`, at the IO edge like `fortuna-venues`. It depends on
`fortuna-cognition` (for the `Source` trait and `RawSignal`) and `fortuna-core` (Clock,
ids). The ingestion scheduler loop is wired into `drive()` in `fortuna-live` alongside
the existing loops. No changes to core, gates, exec, or invariants.

### 4.2 Components

**FetchClient (shared HTTP substrate).** One reqwest-based client used by every
adapter, following the Kalshi transport pattern (injected `Clock`, error classification
into `RateLimited`/`Timeout`/`Outage`, no retries in the transport — retry policy
belongs to the scheduler). Adds:

- per-host politeness token bucket (config: requests/min per domain);
- conditional GET (ETag / If-Modified-Since cache) so steady-state polling of unchanged
  feeds costs near zero;
- https-only, hosts pinned to the source's registry entry, redirects re-validated
  against the pin before following;
- response size cap and timeout cap;
- robots.txt fetch-and-cache, honored for scrape-class sources.

**Adapters (Source impls).** Each is deliberately dumb: fetch, emit envelopes.

| Adapter | Domain(s) | Mechanism | v1? |
|---|---|---|---|
| `CalendarSource` | macro | BLS/Fed/FRED release calendars (REST/JSON). Emits `release_scheduled` and `release_printed` signal types. | yes |
| `NwsSource` | weather | api.weather.gov products: forecast discussions, alerts. | yes |
| `RssSource` | politics, entertainment, general | RSS/Atom via `feed-rs`; one impl, N configured feeds. | yes |
| `GdeltSource` | politics, general | GDELT 2.0 doc API, queries derived from domain tags. | yes |
| `ScrapeSource` | entertainment long tail | Raw HTML fetch (robots-respecting) + extraction stage. | designed, built when first needed |
| `McpSource` | anything unreachable otherwise | Drives an MCP web-fetch tool; output enters the same envelope. | designed, config-gated, off by default |

**Extraction stage (the escape hatch).** Per-source config `extraction = "none" |
"model"`. For `model`: the raw payload is persisted first (its hash rides in the
envelope), then a cheap-tier model extracts against a JSON schema (headline, summary,
entities, event_time, claims). Mechanically this is a narrow `Extractor` interface
built on the existing Mind machinery — `MindTransport` + its own `CostBudget` instance
+ a stub implementation for tests — not a call to `Mind::decide` (whose signature is
decision-shaped). Tiering follows the established pattern: a separate cheap-model
instantiation with operator-configured model id, mirroring spec 5.9's triage/synthesis
split. Schema-invalid output is
rejected and logged, never repaired (5.9's rule). The derived signal carries provenance
`{model_id, prompt_hash, raw_content_hash}` and trust tier = min(source tier,
configured extraction cap). Hard daily extraction budget; when exhausted, raw signals
still land and skipped extractions are counted in metrics — no silent drop. Because the
raw bytes are preserved, extraction is replayable and re-runnable.

**Ingestion scheduler.** A single loop owning the adapter fleet: takes `&dyn Clock` and
a `CancellationToken`, never sleeps on wall time. Per-source cadence from config:

- `base_interval` (e.g. RSS 30m, GDELT 60m, calendars daily);
- optional `event_windows`: declared windows (date/time ranges, typically derived from
  `release_scheduled` signals or static schedules) during which the source polls at a
  boosted interval (e.g. 10s for 12:25–12:40 UTC on CPI day);
- push-class sources (future webhooks) drain a queue on the same tick.

Each tick: due sources → fetch → existing `normalize_and_dedup` (registry check,
envelope, dedup) → `SignalsRepo.insert` → `TriggerEngine.signal_matches` → possibly
wake a decision cycle. Per-source failure isolation: one erroring source never blocks
the fleet.

**Trust attribution job.** Extends the weekly review: join resolved belief outcomes
against belief evidence source refs; compute per-source correlation with good/bad
beliefs; write append-only trust-tier adjustment rows. Demotions are automatic
on-the-record; promotions require operator confirmation (humans gate upward moves).
This automates 5.11's "a source whose evidence correlates with bad beliefs is demoted
on the record."

### 4.3 Configuration and registry

`source_registry` (exists) remains the fail-closed allowlist: source_id, trust tier
0–10, domain tags, enabled. Operational knobs live in `config/fortuna.toml`:

```toml
[sources.bls_release]
kind = "calendar"             # calendar | rss | gdelt | nws | scrape | mcp
url = "https://api.bls.gov/..."
base_interval = "1d"
# Phase A: static windows. Phase B may derive them from release_scheduled signals.
event_windows = [{ days = "bls_cpi_dates", from = "12:25Z", to = "12:40Z", interval = "10s" }]
extraction = "none"
rate_budget_per_min = 6

[sources.rotten_tomatoes_pages]   # later phase, illustrative
kind = "scrape"
extraction = "model"
extraction_trust_cap = 4
enabled = false
```

Registry decides *whether* a source may exist; config decides *how* it behaves. A
source present in config but absent/disabled in the registry is refused (existing
behavior).

## 5. Data flow

```
        ┌──────────────────── fortuna-sources (NEW, IO edge) ─────────────────────┐
        │                                                                          │
 BLS/Fed calendar ─▶ CalendarSource ┐                                              │
 NWS API ──────────▶ NwsSource ─────┤   ┌─────────────┐ raw bytes ┌─────────────┐  │
 RSS/Atom ─────────▶ RssSource ─────┼──▶│ FetchClient │──────────▶│ Extraction  │  │
 GDELT ────────────▶ GdeltSource ───┤   │ politeness  │ (scrape/  │ cheap Mind, │  │
 HTML pages ───────▶ ScrapeSource ──┤   │ cond-GET    │  mcp only)│ raw kept,   │  │
 MCP web tools ────▶ McpSource ─────┘   │ host-pinned │           │ budgeted    │  │
 (config-gated)                         └──────┬──────┘           └──────┬──────┘  │
        └──────────────────────────────────────┼─────────────────────────┼─────────┘
                                               ▼                         ▼
        ┌────────────────── fortuna-cognition (EXISTS, unchanged) ────────────────┐
        │  normalizer: {source, type, received_at, payload, content_hash} + dedup │
        │  + source_registry check (fail-closed)                                  │
        │                          │                                              │
        │                          ▼                                              │
        │            signals table (append-only, partitioned)   ◀── THE SEAM      │
        │              │                              │                           │
        │   fast path  ▼                              ▼  slow path                │
        │   TriggerEngine                 world-forward discovery (5.12)          │
        │   keyword/divergence rules      signals → watchlist events + beliefs    │
        │              │                              │                           │
        │              ▼                              ▼                           │
        │   decision cycle: triage → context assembler → Mind → calib → sizing    │
        └──────────────────────────────│───────────────────────────────────────────┘
                                       ▼
                     gates (I1) → exec → venues     (untouched; I6 propose-only)
```

### Worked example: CPI day

1. T-1 day: `CalendarSource` emits "May CPI scheduled 2026-06-11 12:30 UTC."
2. World-forward's next run synthesizes a watchlist event with resolution source
   `bls.gov` and writes a belief — no capital, thesis pre-positioned.
3. Market-back later matches Kalshi CPI brackets to the event; the existing "market
   matched to event with open belief" trigger wakes a decision cycle.
4. Release morning: the scheduler's event window boosts `bls_release` to 10s polling;
   the print lands at 12:30:10; keyword trigger fires; the burst coalesces into one
   decision cycle; the context assembler packs the print + open belief under a hashed
   manifest; synthesis proposes; calibration, sizing, gates, paper fill.
5. Settlement: belief Brier/CLV-scored against the declared resolution source; trust
   attribution credits the source on the record. Every step left an audit row; the day
   replays byte-for-byte.

## 6. Security and abuse posture

- **SSRF / URL discipline:** adapters fetch only URLs derived from registry-pinned
  hosts; https-only; redirects re-validated against the pin. URLs found *inside*
  ingested content are data, never fetch targets. Auto-following article links is
  prohibited; if a linked host deserves ingestion, it gets its own registry row.
- **Prompt injection:** ingested text is untrusted (5.11). It reaches the model only
  inside the context assembler's delimited data blocks; blast radius bounded by I6
  (propose-only) and I1 (gates) regardless. The extraction stage adds one more model
  exposure; its output is schema-validated, trust-capped, and never executable.
- **Politeness / legal:** per-host rate budgets, conditional GETs, robots.txt honored
  for scrape-class, identifying User-Agent. Scrape targets chosen with ToS in mind;
  paid APIs preferred when a scrape target is both load-bearing and ToS-hostile.
- **Secrets:** API keys (BLS, GDELT if needed) via env vars only, never config/logs.

## 7. Error handling

- **Per-source health state machine:** `healthy → degraded(n) → quarantined`.
  Consecutive failures escalate backoff (jittered exponential, capped); crossing the
  quarantine threshold disables polling and raises a Slack alert + audit row. Operator
  re-enables (registry `enabled` flag or CLI). No silent death: quarantine is loud.
- **Failure isolation:** adapter errors are per-source; the scheduler tick never aborts
  on one source's failure. The transport's "request may have executed" caveat doesn't
  apply here (reads are idempotent), so retries are safe.
- **Feed anomaly containment:** a source suddenly emitting 100x its normal volume is
  throttled at a per-source per-tick envelope cap and flagged (poisoned/looping feed,
  or a real news burst — either way the trigger engine's debounce already coalesces;
  the cap bounds storage).
- **Budget breach (extraction):** degrade, don't die — raw signals continue landing,
  extraction skips and counts. Mirrors 5.9's "budget breach degrades to
  mechanical-only."
- **Clock/window edge cases:** event windows are computed from injected Clock; a missed
  window (process down) is a logged gap, never a backfill-fetch pretending to be
  point-in-time (received_at honesty over completeness).

## 8. Testing

Per house rules: tests written from this document before implementation.

- **Fixture-driven adapter tests:** recorded fixtures under `fixtures/sources/` (BLS
  JSON, NWS products, RSS/Atom documents including malformed ones, GDELT responses) —
  same discipline as `fixtures/kalshi/`. Never invent feed behavior; missing fixture →
  stub + GAPS.md entry.
- **Property tests:** dedup (same content via different whitespace/ordering →
  one signal), politeness bucket (never exceeds budget under arbitrary schedules),
  envelope normalization (arbitrary payload bytes → valid envelope or typed error).
- **DST scenarios (new, added to the corpus):** scheduler under fault injection — source
  timeout mid-window, 429 storms, process crash inside an event window and recovery,
  burst coalescing (N near-simultaneous signals → one decision cycle), quarantine
  escalation and operator re-enable.
- **Extraction tests:** schema-invalid model output rejected (scripted Mind stub),
  budget exhaustion degrades correctly, provenance fields present, trust cap applied.
- **Invariant non-regression:** full fortuna-invariants suite; this subsystem adds no
  order paths, so I1–I7 tests must pass untouched.

## 9. Scaling

- Source count scales by registry rows + config, not code paths (structured classes);
  scrape-class scales by config + extraction schema once `ScrapeSource` exists.
- Storage: signals table already monthly-partitioned and deduped; envelope cap bounds
  pathological feeds.
- LLM cost: trigger-engine triage gates decision-cycle spend (exists); extraction has
  its own hard daily cap; world-forward is already budget-capped and first-throttled.
- Network: conditional GETs make steady-state polling cheap; event windows concentrate
  spend where freshness pays.

## 10. Phasing

- **Phase A (v1):** `fortuna-sources` crate, FetchClient, scheduler wired into
  `drive()`, CalendarSource + NwsSource + RssSource + GdeltSource, registry rows +
  config, fixtures + tests + DST scenarios. Pure deterministic Rust; no model in the
  ingestion path yet.
- **Phase B:** trust attribution job in the weekly review; event-window automation from
  `release_scheduled` signals.
- **Phase C (when first needed):** ScrapeSource + extraction stage + extraction budget.
- **Phase D (config-gated):** McpSource; webhook push class.

## 11. Spec deltas (for GAPS.md at implementation time)

1. **Model-assisted extraction stage** — spec 5.11 is silent on extraction; it requires
   adapters to be dumb and the normalizer to own cleverness. This design treats
   extraction as a distinct, audited, budget-capped stage between fetch and
   normalization. Conservative reading adopted: raw bytes always persisted, derived
   signals trust-capped, stage optional per source.
2. **Event-window cadence boost** — spec is silent on polling cadence; conservative
   extension (pure scheduling, no new data semantics).
3. **Per-source health/quarantine states** — operationalizes 5.11's "fetch, retry,
   emit" with explicit failure handling; no spec conflict.
4. **Trust attribution automation** — 5.11 requires demotion "on the record" but does
   not specify the mechanism; this design proposes the weekly-review job with
   operator-gated promotions.

## 12. Open questions (deferred, not blocking Phase A)

- Whether `release_scheduled` signals should auto-create event windows (Phase B) or
  windows stay static config until the calendar source proves reliable.
- GDELT query design per domain tag (needs a short research pass against current GDELT
  docs before implementation — docs/research/ per house practice).
- Entertainment structured sources: TMDb/OMDb licensing posture vs. scrape-class
  Rotten Tomatoes (Phase C decision).
