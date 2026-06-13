# Source vetting dossier — nws

> Layer 0 admission (design §4.4). Grounded in NWS API documentation fetched
> 2026-06-13 (see Evidence). Facts are cited, not recalled.

---

## Identity

- **source_id:** `nws` (instances keyed per feed: `nws_alerts_<area>`,
  `nws_afd_<office>` — each its own registry row/trust record)
- **Publisher / operator:** U.S. National Weather Service (NOAA), a U.S.
  Government agency
- **Domain tags:** weather
- **Primary URL (pinned host):** `https://api.weather.gov`
- **Acquisition class:** `nws` (REST/JSON)
- **Resolution-source eligible?** Yes — NWS is the authoritative issuer for
  U.S. weather alerts and forecast products; a weather belief ("a Severe
  Thunderstorm Warning is active for area X at time T") is gradable directly
  against `/alerts/active`.

## Six-dimension score (design §4.4 Layer 0)

| # | Dimension | Score | Justification |
|---|-----------|:-----:|---------------|
| 1 | **Authority** | 10 | NWS is the ground-truth issuer of U.S. watches/warnings and AFDs — not reporting about them. |
| 2 | **Directness** | 10 | Primary source: the issuing offices' own products via the agency's own API. |
| 3 | **Contract stability** | 8 | Versioned, documented REST API with a stable ontology; docs warn an API key "will be replaced … in the future" — a watched change, not a current break. |
| 4 | **Latency-to-event** | 9 | Alerts publish at issuance; the API is "cache-friendly … based upon the information life cycle," so polling catches them promptly. |
| 5 | **ToS cleanliness** | 10 | "All of the information presented via the API is intended to be open data, free to use for any purpose"; "we do not charge any fees." No scraping — a blessed public API. |
| 6 | **Resolution eligibility** | 9 | Active-alerts and product endpoints are checkable resolution sources for weather events. |

## Initial trust tier

- **Proposed tier (0–10):** `9`
- **Band rationale:** official government source → 8–10 band. Held at 9 (not
  10) pending the announced API-key change and the empirical earliness/accuracy
  record (Layer 3) before maxing it.
- **Consumption consequences at this tier (design §4.4 Layer 4):**
  - Resolution-source floor (default 8): **may** declare resolution source for
    weather watchlist events.
  - Trigger floor (default 5): **may** wake a decision cycle.

## Operational facts (for the `[sources.<id>]` config)

- **Endpoint(s):**
  - Alerts: `GET https://api.weather.gov/alerts/active?area=<state>` →
    GeoJSON `FeatureCollection` (one `nws.alert` signal per feature).
    NOTE: `/alerts/active` rejects a `limit` param (→ 400); filter by `area`.
  - Forecast discussions: `GET https://api.weather.gov/products?type=AFD` →
    JSON-LD `@graph` list of product summaries (one `nws.afd` signal per
    entry). Full text is a second hop `GET /products/{id}` (`productText`) —
    see follow-up note below.
- **Auth:** none today; **a `User-Agent` header is REQUIRED** to identify the
  application (contact info recommended). No secret — but the UA string is set
  in the transport, not config.
- **Update cadence (observed):** AFDs issue a few times daily per office;
  alerts are event-driven. Proposed `base_interval`: alerts 5–10m, AFD 30–60m.
- **Conditional GET:** "designed with a cache-friendly approach"; ETag/
  Last-Modified honored by the `FetchClient` conditional path.
- **Rate limits / politeness:** "The rate limit is not public information, but
  allows a generous amount for typical use"; on exceed, retry "typically within
  5 seconds." Proposed `rate_budget_per_min`: 30 (conservative vs. "generous").
- **Payload + content-hash basis:** the per-feature / per-`@graph`-entry JSON
  object, passed through verbatim; the normalizer hashes the canonical payload.
- **Claimed-time field (Layer 1):** alert → `properties.sent`; AFD →
  `issuanceTime` (both RFC3339 with offset; `nws_claimed_time` extracts them).

## Risks & failure modes

- **Announced API-key change** — monitor; will become an env-var secret.
- **`limit` not accepted on `/alerts/active`** — area-filtered queries only
  (encoded in the dossier so a future config author doesn't trip the 400).
- **Volume spikes** during severe-weather outbreaks — bounded by the Layer-1
  per-tick volume envelope (§7).
- **Corroboration:** NWS is a single authoritative origin (not syndicated);
  Layer-2 corroboration counting treats it as one high-tier origin.

## Evidence (cited, dated)

- NWS API documentation, `https://www.weather.gov/documentation/services-web-api`,
  fetched 2026-06-13: base URL `https://api.weather.gov`; "A User Agent is
  required"; "This will be replaced with an API key in the future"; "The rate
  limit is not public information, but allows a generous amount for typical
  use"; retry "typically within 5 seconds"; "All of the information presented
  via the API is intended to be open data, free to use for any purpose"; "we do
  not charge any fees"; "designed with a cache-friendly approach."
- Live responses captured 2026-06-13 under `fixtures/sources/nws/` (AFD list,
  AFD product, active-alerts FeatureCollection, 400 error envelope).

## Decision

- [x] Admitted at tier `9` — registry row + config entry to be created when the
  scheduler (D9) wires sources; adapter + fixtures landed in D4.
- [ ] Rejected — n/a

Reviewer: Track D implementer · Date: 2026-06-13

---

### Follow-up (ledgered)

AFD full-text is a two-hop fetch (`/products` list → `/products/{id}`). D4
emits the product **summary** signal (id, office, issuanceTime, code) from the
list; wiring the second hop to attach `productText` is a follow-up (recorded in
GAPS). The summary already carries enough to dedup and to drive a "new AFD
issued" trigger; the text hop enriches, it does not block.
