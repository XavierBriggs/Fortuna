# Source vetting dossier — rss_fed_press

> Layer 0 admission (design §4.4). Grounded in the live feed captured
> 2026-06-13 (see Evidence). Acquisition class: `rss` (generic RssSource).

---

## Identity

- **source_id:** `rss_fed_press`
- **Publisher / operator:** U.S. Federal Reserve Board (the central bank itself)
- **Domain tags:** macro, politics
- **Primary URL (pinned host):** `https://www.federalreserve.gov/feeds/press_all.xml`
- **Acquisition class:** `rss` (RSS 2.0)
- **Resolution-source eligible?** Partial — Fed press releases ARE the
  authoritative record of FOMC rate decisions and Board actions, so they can
  grade "did the Fed change the target range" / "did the Board issue X" events.
  Not a grader for market-price events; for the policy-action events it
  announces, it is the primary record.

## Six-dimension score (design §4.4 Layer 0)

| # | Dimension | Score | Justification |
|---|-----------|:-----:|---------------|
| 1 | **Authority** | 10 | The Fed publishing the Fed's own actions — ground truth for monetary policy. |
| 2 | **Directness** | 10 | Primary: the institution's own press wire, not reporting about it. |
| 3 | **Contract stability** | 8 | Long-standing RSS 2.0 feed on a stable gov URL; format unchanged for years. |
| 4 | **Latency-to-event** | 8 | Posts at release; the headline FOMC statement hits the feed at the 14:00 ET decision. Fast for a news feed (the millisecond race belongs to the dedicated FOMC tooling, not this). |
| 5 | **ToS cleanliness** | 10 | Public U.S. Government data, free; an official RSS feed published for consumption. |
| 6 | **Resolution eligibility** | 7 | Primary record for the policy actions it announces (rate decisions, rules). |

## Initial trust tier

- **Proposed tier (0–10):** `9`
- **Band rationale:** official government/central-bank source → 8–10 band; 9,
  reserving 10 until the empirical earliness/accuracy record (Layer 3) is in.
- **Consumption consequences (design §4.4 Layer 4):**
  - Resolution-source floor (default 8): **may** be a resolution source for the
    Fed-action events it directly reports.
  - Trigger floor (default 5): **may** wake a decision cycle.

## Operational facts (for the `[sources.<id>]` config)

- **Endpoint:** `GET https://www.federalreserve.gov/feeds/press_all.xml` → RSS
  2.0; `<item>` fields: title, link, guid, description, category, pubDate.
- **Auth:** none. A polite `User-Agent` is set by the transport.
- **Update cadence (observed):** event-driven; several releases per week,
  clustered around FOMC meetings (8/year). Proposed `base_interval`: 15–30m,
  with an event window tightened around scheduled FOMC announcement times.
- **Conditional GET:** standard static-file serving; ETag/Last-Modified
  honored by the `FetchClient` conditional path.
- **Rate limits / politeness:** none published; treat as a public static file.
  Proposed `rate_budget_per_min`: 10 (conservative).
- **Payload + content-hash basis:** the per-`<item>` normalized JSON (id/guid,
  title, link, summary, published, categories); the normalizer hashes it. The
  `guid`/`link` make items unique for dedup.
- **Claimed-time field (Layer 1):** RSS `pubDate` → `published`; `rss_claimed_time`
  reads it.

## Risks & failure modes

- **Corroboration / syndication:** the Fed feed is a single authoritative
  ORIGIN. Wire stories quoting it are NOT independent corroboration (Layer 2
  must count origins, not copies).
- **Volume spike** around FOMC day — bounded by the Layer-1 per-tick envelope.
- **Latency:** for sub-second FOMC reaction, a dedicated low-latency path would
  beat an RSS poll; this feed serves the discovery loop and minute-scale
  triggers, not the microsecond race. (Honest scope note.)

## Evidence (cited, dated)

- Live feed `https://www.federalreserve.gov/feeds/press_all.xml`, captured
  2026-06-13: RSS 2.0, channel "FRB: Press Release - All Releases",
  description "All recent press releases from the Federal Reserve Board"; 20
  items, each with title/link/guid/description/category/pubDate. Fixture:
  `fixtures/sources/rss/fed_press_rss2.xml` (trimmed to 2 items).

## Decision

- [x] Admitted at tier `9` — registry row + config entry created when the
  scheduler (D9) wires sources; adapter is the generic RssSource (D5).
- [ ] Rejected — n/a

Reviewer: Track D implementer · Date: 2026-06-13
