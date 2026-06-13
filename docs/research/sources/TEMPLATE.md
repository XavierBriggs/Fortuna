# Source vetting dossier — TEMPLATE

> Layer 0 admission gate (design §4.4). **No `source_registry` row, and no
> `[sources.<id>]` config entry, without a completed dossier here.** This is the
> mechanism behind the operator priority "we want the best sources": a source is
> admitted only after it is scored on the six dimensions below and assigned an
> initial trust tier with a written justification.
>
> House rule (research-loop): every factual claim in a dossier — ToS terms,
> endpoint shapes, update cadence, licensing — is grounded in a cited source
> (the provider's own docs/ToS page, fetched and dated), never written from
> memory. Put the citations in the **Evidence** section. A dossier with
> uncited claims is not done.
>
> Copy this file to `docs/research/sources/<source_id>/dossier.md` and fill it
> in. `<source_id>` must equal the `[sources.<id>]` key and the
> `source_registry.source_id`. Each adapter PR (D4–D7) lands its source's
> dossier alongside the adapter and its fixtures.

---

## Identity

- **source_id:** `<matches config + registry>`
- **Publisher / operator:** <who runs it>
- **Domain tags:** <macro | politics | weather | entertainment | general>
- **Primary URL (pinned host):** `https://<host>/...`
- **Acquisition class:** <calendar | rss | gdelt | nws | scrape | mcp>
- **Resolution-source eligible?** <yes/no — can it GRADE beliefs (spec 5.12),
  not merely inform them?>

## Six-dimension score (design §4.4 Layer 0)

Score each 0–10 with a one-line justification. The initial trust tier is
argued from these (it is not their average — authority and directness
dominate; a contract-unstable primary source still outranks a stable
aggregator for trust, though stability informs operational cadence).

| # | Dimension | Score | Justification |
|---|-----------|:-----:|---------------|
| 1 | **Authority** — ground truth vs. reporting about it | | |
| 2 | **Directness** — primary > wire > aggregator > commentary | | |
| 3 | **Contract stability** — versioned API > RSS > HTML page | | |
| 4 | **Latency-to-event** — how early it carries the information | | |
| 5 | **ToS cleanliness** — feed/API blessed > scrape-tolerated > hostile | | |
| 6 | **Resolution eligibility** — can it grade beliefs (5.12)? | | |

## Initial trust tier

- **Proposed tier (0–10):** `<n>`
- **Band rationale:** <official statistical/government/venue-resolution 8–10;
  established wire/major outlet 5–7; aggregator 3–5; model-extracted/MCP capped
  per design §4.2 regardless of publisher>
- **Consumption consequences at this tier (design §4.4 Layer 4):**
  - Resolution-source floor (default 8): <may / may not declare resolution
    source for watchlist events>
  - Trigger floor (default 5): <may / may not wake a decision cycle>

## Operational facts (for the `[sources.<id>]` config)

- **Endpoint(s):** <exact URL(s), method, auth (env-var name only, never the
  secret)>
- **Update cadence (observed):** <how often it actually changes>
- **Proposed `base_interval`:** <e.g. 30m>
- **Event windows (if scheduled-release):** <e.g. CPI 12:25–12:40Z on release
  days>
- **Conditional GET supported?** <ETag / Last-Modified observed?>
- **Rate limits / politeness:** <published limit if any → proposed
  `rate_budget_per_min`>
- **Payload shape + content-hash basis:** <what bytes are hashed for dedup>
- **Claimed-time field (Layer 1):** <which field carries the event/publish
  time, if any, for the future-dated check>

## Risks & failure modes

- <redesign risk, ToS change risk, syndication/corroboration notes (is this an
  independent origin or does it carry someone else's wire?), known outage
  patterns>

## Evidence (cited, dated)

- <URL of the provider's API docs / ToS, fetched YYYY-MM-DD, with the exact
  fact it supports>
- <...>

## Decision

- [ ] Admitted at tier `<n>` — registry row + config entry created.
- [ ] Rejected — reason: <...>

Reviewer: <who> · Date: <YYYY-MM-DD>
