# Source vetting dossier — calendar_bls

> Layer 0 admission (design §4.4). Grounded in the live BLS feeds captured
> 2026-06-13. Acquisition class: `calendar` (CalendarSource).

---

## Identity

- **source_id:** `calendar_bls` (instances: `calendar_bls_schedule` for the ICS,
  `calendar_bls_latest` for the RSS — each its own registry row)
- **Publisher / operator:** U.S. Bureau of Labor Statistics (BLS), a U.S.
  Government agency
- **Domain tags:** macro
- **Primary URLs (pinned host `www.bls.gov`):**
  schedule `https://www.bls.gov/schedule/news_release/bls.ics`;
  latest `https://www.bls.gov/feed/bls_latest.rss`
- **Acquisition class:** `calendar` (iCalendar schedule + RSS latest)
- **Resolution-source eligible?** Partial — BLS is the authoritative issuer of
  the indicators it publishes (CPI, employment, etc.); the latest-numbers feed /
  the published release is the primary record for "indicator X printed value Y."
  The SCHEDULE feed is not a grader — it announces, it does not resolve.

## Six-dimension score (design §4.4 Layer 0)

| # | Dimension | Score | Justification |
|---|-----------|:-----:|---------------|
| 1 | **Authority** | 10 | BLS publishes the BLS indicators — ground truth for CPI/employment/etc. |
| 2 | **Directness** | 10 | Primary: the agency's own schedule and release feeds. |
| 3 | **Contract stability** | 8 | Long-standing public iCalendar + RSS on stable gov URLs. |
| 4 | **Latency-to-event** | 7 (schedule) / 8 (printed) | The schedule is published far ahead; the latest-numbers RSS updates at release. The microsecond CPI-print race belongs to dedicated tooling, not this feed. |
| 5 | **ToS cleanliness** | 10 | Public U.S. Government data, free, published for consumption. |
| 6 | **Resolution eligibility** | 7 | Authoritative for the indicator values it publishes. |

## Initial trust tier

- **Proposed tier (0–10):** `9`
- **Band rationale:** official government source → 8–10; 9 pending the Layer-3
  record.
- **Consumption consequences (Layer 4):** resolution-eligible (≥8) for the
  indicator-print events it publishes; may wake a decision cycle (≥5).

## Operational facts (for the `[sources.<id>]` config)

- **Schedule endpoint:** `GET .../bls.ics` → iCalendar; each `VEVENT` carries
  `UID`, `DTSTART;TZID=US-Eastern:<naive>`, `SUMMARY` (release name),
  `CATEGORIES`. The adapter converts `US-Eastern` → UTC via `America/New_York`
  (the VTIMEZONE block confirms standard US Eastern DST rules). Emits
  `release_scheduled`.
- **Latest endpoint:** `GET .../bls_latest.rss` → RSS 2.0; one rolling "Major
  Economic Indicators Latest Numbers" item with `pubDate`. Emits
  `release_printed`.
- **Auth:** none; polite `User-Agent` set by the transport.
- **Update cadence:** the schedule changes rarely (poll daily); the latest feed
  updates at each release. Proposed `base_interval`: schedule 1d, latest 15m,
  with event windows around scheduled release times (the `release_scheduled`
  signals FEED that cadence — design §3.4).
- **Conditional GET:** ETag/Last-Modified honored by the `FetchClient`.
- **Rate limits / politeness:** none published for these static feeds; proposed
  `rate_budget_per_min`: 6.
- **Claimed-time (Layer 1):** `release_scheduled` → **None** by design (its
  `scheduled_at` is a FUTURE time carried as data, not an occurred-at claim — it
  must NOT trip the future-dated reject). `release_printed` → the publish time.

## Risks & failure modes

- **Timezone correctness** is the load-bearing detail — tested against a real
  EST event (Jan → UTC−5) and a real EDT event (Jul → UTC−4). A BLS switch away
  from the `US-Eastern` TZID would fail-closed (unknown TZID refused, not
  guessed).
- **`bls_latest.rss` is a single rolling item**, not per-release — coarse for
  `release_printed`; pair with the schedule + downstream correlation. A
  per-release feed (or the published-release page) is a future refinement.
- **Volume:** the schedule is hundreds of VEVENTs (full year) — bounded by the
  Layer-1 per-tick envelope downstream.

## Evidence (cited, dated)

- `https://www.bls.gov/schedule/news_release/bls.ics`, captured 2026-06-13:
  VCALENDAR with a `US-Eastern` VTIMEZONE (EDT from 2nd Sun Mar, EST from 1st
  Sun Nov) and 313 VEVENTs. Fixture `fixtures/sources/calendar/bls_schedule.ics`
  (trimmed to the VTIMEZONE + a Jan + a Jul VEVENT).
- `https://www.bls.gov/feed/bls_latest.rss`, captured 2026-06-13: RSS 2.0
  "Major Economic Indicators Latest Numbers." Fixture
  `fixtures/sources/calendar/bls_latest.rss` (verbatim).

## Decision

- [x] Admitted at tier `9` — registry rows + config at scheduler-wiring (D9).
- [ ] Rejected — n/a

Reviewer: Track D implementer · Date: 2026-06-13

---

### FRED (deferred — operator-blocked)

The other intended calendar source, FRED release dates
(`api.stlouisfed.org/fred/releases/dates`), requires a free API key. Stubbed +
recorded in GAPS; no fixture until a key is provisioned (env `FRED_API_KEY`,
via the F1 auth-header substrate). BLS covers the macro release calendar in the
meantime.
