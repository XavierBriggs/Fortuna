# Source vetting dossier — rss_sec_edgar

> Layer 0 admission (design §4.4). Grounded in the live feed captured
> 2026-06-13. Acquisition class: `rss` (generic RssSource, Atom format).

---

## Identity

- **source_id:** `rss_sec_edgar` (the "recent filings" stream; per-form variants
  e.g. `rss_sec_edgar_8k` get their own rows/tiers)
- **Publisher / operator:** U.S. Securities and Exchange Commission (EDGAR)
- **Domain tags:** macro, equities/markets
- **Primary URL (pinned host):** `https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=8-K&output=atom`
- **Acquisition class:** `rss` (Atom)
- **Resolution-source eligible?** Partial — EDGAR is the authoritative record
  that a filing OCCURRED (a material-event 8-K, an S-1, etc.); it grades
  "company X filed Y by date Z" events. It does not grade the filing's market
  consequence.

## Six-dimension score (design §4.4 Layer 0)

| # | Dimension | Score | Justification |
|---|-----------|:-----:|---------------|
| 1 | **Authority** | 10 | The SEC's own filing system — the legal record that a filing exists. |
| 2 | **Directness** | 10 | Primary: filings land in EDGAR directly from filers. |
| 3 | **Contract stability** | 8 | Long-standing EDGAR Atom output; the `browse-edgar` query surface is stable. |
| 4 | **Latency-to-event** | 8 | Filings appear in the current feed at acceptance (near real-time during market hours). |
| 5 | **ToS cleanliness** | 9 | Public government data; EDGAR has a published fair-access policy (a `User-Agent` with contact is expected, generous limits). Polite by default. |
| 6 | **Resolution eligibility** | 7 | Authoritative for filing-occurrence events. |

## Initial trust tier

- **Proposed tier (0–10):** `9`
- **Band rationale:** official government source → 8–10; 9 pending the Layer-3
  record.
- **Consumption consequences (Layer 4):** resolution-source eligible (≥8) for
  filing-occurrence events; may wake a decision cycle (≥5).

## Operational facts (for the `[sources.<id>]` config)

- **Endpoint:** `GET .../browse-edgar?action=getcurrent&type={form}&output=atom`
  → Atom; `<entry>` fields: title, link, summary, updated, category, id.
- **Auth:** none; a descriptive `User-Agent` (with contact) is expected by SEC
  fair-access policy and set by the transport.
- **Update cadence (observed):** continuous during market hours. Proposed
  `base_interval`: 5–15m (filings are frequent).
- **Conditional GET:** ETag/Last-Modified honored by the `FetchClient`.
- **Rate limits / politeness:** SEC requests reasonable rates + a UA with
  contact; proposed `rate_budget_per_min`: 6 (well under EDGAR's fair-access
  guidance).
- **Payload + content-hash basis:** the per-`<entry>` normalized JSON; the
  filing accession in `id`/`link` makes items unique for dedup.
- **Claimed-time field (Layer 1):** Atom `<updated>` → `updated`;
  `rss_claimed_time` falls back to it when `published` is absent (EDGAR uses
  `updated`).

## Risks & failure modes

- **Query-surface coupling:** the `browse-edgar` CGI query is the pin; a change
  there would need a config update. EDGAR also offers newer JSON APIs — a future
  migration option, not needed now.
- **Volume:** high during market hours — bounded by the Layer-1 per-tick
  envelope; filter by form type (`type=8-K`) per source instance.
- **Fair-access:** must keep the UA + conservative rate, or SEC may throttle.

## Evidence (cited, dated)

- Live feed `https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=8-K&...&output=atom`,
  captured 2026-06-13: Atom feed, 10 `<entry>` (8-K filings), each with
  title (e.g. "8-K - Caro Holdings Inc. (...) (Filer)"), link, summary,
  updated, category, id. Fixture: `fixtures/sources/rss/sec_edgar_atom.xml`
  (trimmed to 2 entries).

## Decision

- [x] Admitted at tier `9` — registry row + config entry at scheduler-wiring
  time (D9); generic RssSource adapter (D5).
- [ ] Rejected — n/a

Reviewer: Track D implementer · Date: 2026-06-13
