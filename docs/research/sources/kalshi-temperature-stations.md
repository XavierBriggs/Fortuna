# Kalshi temperature-market grading stations (F7 station→series grounding)

**Captured 2026-06-14, READ-ONLY, from the Kalshi DEMO API** via
`crates/fortuna-venues/examples/kalshi_discover_markets.rs` (the GRADING-STATION
PROBE: `GET /markets?series_ticker=…&limit=1` per discovered temperature series,
printing each market's `rules_primary`). The `rules_primary` text IS the
settlement contract; this file transcribes the GRADING STATION each series names.
Nothing here is invented — every station below is quoted from a recorded rule.

This grounds `crates/fortuna-live/src/aeolus_venue.rs::station_series`, which maps
an **Aeolus forecast station code** → a **Kalshi series**. The map keys on the
station Kalshi GRADES on. Safety: it fires only when Aeolus emits that exact
station code, in which case the Aeolus forecast and the Kalshi market resolve
against the SAME physical station (correct by construction). Any other Aeolus
code → `None` → not traded (a wrong/missing pairing can only MISS a trade, never
mis-resolve one).

## Conservative mapping rule (why some series are NOT mapped)

A series is **mapped** only when its `rules_primary` names a station precisely
enough to pin one unambiguous ICAO/NWS code. When the rule names only a CITY
(not a station), the exact NWS Climatological-Report station is not nailed by the
contract text alone, so mapping it would require asserting an ICAO the rule does
not state — and if that ICAO were wrong AND Aeolus emitted it, the result would
be a mis-resolved trade. The conservative option (spec: "When the spec is silent,
choose the conservative option") is `None`. These are DORMANT regardless: Aeolus
forecasts only KNYC today (`fixtures/sources/aeolus/knyc_tmax.json`,
`knyc_tmin.json`).

## Daily HIGH series (KXHIGH*) — the tmax markets

| Series | `rules_primary` grading station (verbatim phrase) | ICAO | Mapped? |
|---|---|---|---|
| KXHIGHNY | "Central Park, New York" | KNYC | ✅ `(KNYC, Tmax)` — explicit; Aeolus emits KNYC |
| KXHIGHAUS | "Austin Bergstrom" | KAUS | ✅ `(KAUS, Tmax)` — explicit airport |
| KXHIGHCHI | "Chicago Midway, IL" | KMDW | ✅ `(KMDW, Tmax)` — explicit airport |
| KXHIGHLAX | "Los Angeles Airport, CA" | KLAX | ✅ `(KLAX, Tmax)` — explicit airport |
| KXHIGHMIA | "Miami International Airport" | KMIA | ✅ `(KMIA, Tmax)` — explicit airport |
| KXHIGHPHIL | "Philadelphia International Airport" | KPHL | ✅ `(KPHL, Tmax)` — explicit airport |
| KXHIGHDEN | "Denver, CO" | (KDEN?) | ❌ city-named, not a station |
| KXHIGHTATL | "Atlanta" | (KATL?) | ❌ city-named |
| KXHIGHTBOS | "Boston" | (KBOS?) | ❌ city-named |
| KXHIGHTDAL | "Dallas" | ambiguous (KDFW/KDAL) | ❌ ambiguous multi-airport |
| KXHIGHTDC | "Washington DC" | ambiguous (KDCA/KIAD) | ❌ ambiguous multi-airport |
| KXHIGHTHOU | "Houston" | ambiguous (KIAH/KHOU) | ❌ ambiguous multi-airport |
| KXHIGHTLV | "Las Vegas" | (KLAS?) | ❌ city-named |
| KXHIGHTMIN | "Minneapolis" | (KMSP?) | ❌ city-named |
| KXHIGHTNOLA | "New Orleans" | (KMSY?) | ❌ city-named |
| KXHIGHTOKC | "Oklahoma City" | (KOKC?) | ❌ city-named |
| KXHIGHTPHX | "Phoenix" | (KPHX?) | ❌ city-named |
| KXHIGHTSATX | "San Antonio" | (KSAT?) | ❌ city-named |
| KXHIGHTSEA | "Seattle" | (KSEA?) | ❌ city-named |
| KXHIGHTSFO | "San Francisco" | (KSFO?) | ❌ city-named |

The `(KXXX?)` candidates are the plausible single-airport NWS CLI stations, but
they are NOT in the rule text, so they stay UNMAPPED until a future capture pins
them (e.g. a rule revision that names the station, or an Aeolus forecast for that
station whose `nws_station_id` we can cross-check). To promote one: confirm the
NWS Climatological-Report station the city's CLI is issued from and add the entry.

## Daily LOW series (KXLOWT*) — the tmin markets

All KXLOWT* rules name only a CITY ("minimum temperature recorded at <CITY>"),
e.g. KXLOWTNYC = "New York City", KXLOWTCHI = "Chicago" (note: NOT "Chicago
Midway" — the LOW rules are LESS specific than the matching HIGH rules). So by the
conservative rule, the only LOW mapping is the one Aeolus can actually use today:

| Series | grading phrase | ICAO | Mapped? |
|---|---|---|---|
| KXLOWTNYC | "New York City" | KNYC | ✅ `(KNYC, Tmin)` — Aeolus emits KNYC tmin; NYC's NWS CLI station is Central Park (KNYC), confirmed by KXHIGHNY's explicit "Central Park" |
| KXLOWT{CHI,DEN,LAX,…} | "<city>" | — | ❌ city-named + DORMANT (Aeolus emits only KNYC) |

## Excluded — different product / authority

| Series | rule excerpt | why excluded |
|---|---|---|
| KXTEMPNYCH | "temperature recorded at Central Park … 11 AM EDT … **The Weather Company** (for coordinates KNYC)" | HOURLY market graded by The Weather Company, NOT the NWS daily HIGH/LOW. Different authority + cadence; out of scope for the Aeolus daily tmax/tmin seam. |

## Sources

- READ-ONLY Kalshi DEMO `rules_primary`, captured 2026-06-14 via
  `kalshi_discover_markets.rs` (the authoritative settlement text).
- NWS Climatological Report (Daily) mechanism — one CLI per city from a primary
  ASOS station: <https://forecast.weather.gov/product.php?site=NWS&product=CLI&issuedby=DEN>,
  <https://www.weather.gov/media/directives/010_pdfs/pd01010004curr.pdf>.
