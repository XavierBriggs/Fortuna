# Source vetting dossier — nws_climate

> Layer 0 admission (design §4.4). Grounded in NWS API documentation and live
> captures fetched 2026-06-13 (see Evidence). Facts are cited, not recalled.

---

## Identity

- **source_id:** `nws_climate` (instances keyed per office/product as ingested;
  each its own registry row/trust record)
- **Publisher / operator:** U.S. National Weather Service (NOAA), a U.S.
  Government agency
- **Domain tags:** weather
- **Primary URL (pinned host):** `https://api.weather.gov`
- **Acquisition class:** `nws` (REST/JSON; CLI text product)
- **Resolution-source eligible?** Yes — this is the headline use. The CLI
  (Climatological Report — Daily) carries the **official** daily MAXIMUM and
  MINIMUM temperature of record for a station. It is THE grader for weather
  temperature beliefs (spec 5.12): it is the same record the prediction market
  (Kalshi) and Aeolus settle temperature against.

## Six-dimension score (design §4.4 Layer 0)

| # | Dimension | Score | Justification |
|---|-----------|:-----:|---------------|
| 1 | **Authority** | 10 | The agency's own official daily climate record — the ground-truth max/min of record, not a report about it or a derived max-of-hourly-observations. |
| 2 | **Directness** | 10 | Primary source: the issuing office's own CLI product via the agency's own API. |
| 3 | **Contract stability** | 8 | Versioned, documented REST API with a stable ontology; docs warn an API key "will be replaced … in the future" — a watched change, not a current break. The CLI **text body** is fixed-width and fragile (see Risks). |
| 4 | **Latency-to-event** | 7 | The data is for the prior day; the CLI issues the morning after. Authoritative-but-late by design (it is a settlement record, not an early signal). |
| 5 | **ToS cleanliness** | 10 | "All of the information presented via the API is intended to be open data, free to use for any purpose"; "we do not charge any fees." No scraping — a blessed public API. |
| 6 | **Resolution eligibility** | 10 | The official daily-extreme record itself — THE grader for weather temperature beliefs (5.12), the same record the market resolves against. |

## Initial trust tier

- **Proposed tier (0–10):** `10`
- **Band rationale:** official government source → 8–10 band, and within it the
  maximum: this source IS the ground truth the market settles on, the official
  resolution record itself. Contrast the `nws` alerts/AFD source held at tier 9
  — that source informs beliefs but is not the settlement record; this one is.
- **Consumption consequences at this tier (design §4.4 Layer 4):**
  - Resolution-source floor (default 8): **may** declare resolution source for
    weather temperature watchlist events (tier 10 is well above the floor).
  - Trigger floor (default 5): **may** wake a decision cycle.

## Operational facts (for the `[sources.<id>]` config)

- **Endpoint(s):** two-hop.
  - List: `GET https://api.weather.gov/products?type=CLI` → JSON-LD `@graph`
    of product summaries (`id`, `issuingOffice`, `issuanceTime`, `productCode`,
    `productName`). One summary per CLI product.
  - Full text: `GET https://api.weather.gov/products/{id}` → the product object
    with `productText` (the "DAILY CLIMATE REPORT" / "CLIMATE SUMMARY FOR
    <date>" body). Per-product texts are immutable.
- **Auth:** none today; **a `User-Agent` header is REQUIRED** to identify the
  application (contact info recommended). No secret — the UA string is set in
  the transport, not config.
- **Update cadence (observed):** CLI products issue the morning after the day
  they cover (one per office per day, plus corrections). Proposed
  `base_interval`: 30–60m.
- **Conditional GET:** the `/products?type=CLI` **list** is polled with the
  cache-friendly conditional path (ETag/Last-Modified). The per-product texts
  are immutable, so once fetched they need no re-fetch; a per-tick fetch cap +
  a **seen-set of product ids** bounds the second hop.
- **Rate limits / politeness:** "The rate limit is not public information, but
  allows a generous amount for typical use"; on exceed, retry "typically within
  5 seconds." Proposed `rate_budget_per_min`: 30 (conservative vs. "generous").
- **Payload + content-hash basis:** for the adapter signal, the per-`@graph`
  summary plus the fetched `productText`, passed through verbatim; the
  normalizer hashes the canonical payload.
- **Claimed-time field (Layer 1):** `issuanceTime` (RFC3339 with offset;
  `nws_claimed_time` extracts it). It is a **past** time — the CLI issues after
  the day it covers — so the future-dated check is naturally satisfied.

## Risks & failure modes

- **The adapter is deliberately DUMB about the temperatures.** The CLI text is
  fixed-width and fragile to parse: adjacent columns jam together (observed in
  the live capture, the minimum line reads `MINIMUM     7676` — the observed
  `76` and the record `76` run together with no separator). Because a silent
  mis-parse of a settlement value would mis-grade a belief, `NwsClimateSource`
  carries the **RAW `productText` (authoritative)** plus a robustly-parsed
  `report_date` for indexing, and **defers the high-stakes max/min extraction
  to the GRADER** (cognition, at settlement), where a parse ambiguity can be
  flagged for review rather than silently mis-grade. The adapter never asserts
  a temperature.
- **Station mapping is a grader-side concern.** CLI products are issued per
  WFO/office; mapping a specific market station (e.g. KNYC) to the correct CLI
  product is the grader's job, not the adapter's. Recorded as a follow-up.
- **Announced API-key change** — monitor; will become an env-var secret.
- **Two-hop volume** — bounded by the per-tick fetch cap and the seen-set of
  product ids; the Layer-1 per-tick volume envelope (§7) is the backstop.
- **Corroboration:** NWS is a single authoritative origin (not syndicated) and,
  for resolution, the canonical one; Layer-2 corroboration counting treats it
  as one high-tier origin.

## Evidence (cited, dated)

- NWS API documentation, `https://www.weather.gov/documentation/services-web-api`,
  fetched 2026-06-13: base URL `https://api.weather.gov`; "A User Agent is
  required"; "This will be replaced with an API key in the future"; "The rate
  limit is not public information, but allows a generous amount for typical
  use"; retry "typically within 5 seconds"; "All of the information presented
  via the API is intended to be open data, free to use for any purpose"; "we do
  not charge any fees"; "designed with a cache-friendly approach."
- Live responses captured 2026-06-13 under `fixtures/sources/nws_climate/`:
  `cli_list.json` (the `?type=CLI` `@graph` of summaries with `issuingOffice`,
  `issuanceTime`, `productCode` "CLI") and `cli_product.json` (the
  `/products/{id}` object with `productText`). The captured product is a CLI
  for Palau (CLITKR), `issuanceTime` `2026-06-13T17:00:00+00:00`, body
  "DAILY CLIMATE REPORT … THE PALAU CLIMATE SUMMARY FOR 12 JUNE 2026" — data
  for the prior day, issued the morning after — and the jammed `MINIMUM 7676`
  line that motivates deferring extraction to the grader.

## Decision

- [x] Admitted at tier `10` — registry row + config entry to be created when the
  scheduler (D9) wires sources; adapter + fixtures landed alongside this dossier.
- [ ] Rejected — n/a

Reviewer: Track D implementer · Date: 2026-06-13

---

### Follow-up (ledgered)

CLI full-text is a two-hop fetch (`/products?type=CLI` list → `/products/{id}`).
`NwsClimateSource` emits the raw `productText` plus a parsed `report_date`. The
official max/min EXTRACTION — the GRADER — is now BUILT (2026-06-14):
`fortuna_sources::nws_cli_realized(product_text, station) -> Option<RealizedExtreme>`
extracts the official daily high/low °F, FAIL-LOUD (a jam like `MINIMUM 7676`, a
missing `MM`, an absent line, an inverted high<low, or an unparseable date returns
`None`, never a fabricated temperature). It is the resolution input for F9
(`aeolus_reliability::score_reliability`).

REMAINING (operator/cross-track): (1) the **`source_registry` seed** for this
resolution-source role (tier 10) is the only operator prereq — the role is
declared here; (2) mapping a market station to the correct office's CLI product
(and splitting a rare multi-station product) stays a routing follow-up. Both are
in GAPS "TRACK D — NWS-CLI realized-extreme GRADER".
