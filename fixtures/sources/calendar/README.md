# Macro release-calendar fixtures (BLS)

Real BLS sources captured 2026-06-13 with `User-Agent: (fortuna-research,
xbriggs03@gmail.com)`. U.S. Government public-domain. The `CalendarSource`
adapter parses these; never invent feed behavior (house rule).

| file | source | format | used to test |
|------|--------|--------|--------------|
| `bls_schedule.ics` | BLS Economic News Release Schedule (`bls.gov/schedule/news_release/bls.ics`) | iCalendar (VCALENDAR/VEVENT) | schedule parse → `release_scheduled` signals; `DTSTART;TZID=US-Eastern` → UTC |
| `bls_latest.rss` | BLS Major Economic Indicators Latest Numbers (`bls.gov/feed/bls_latest.rss`) | RSS 2.0 | latest-releases parse → `release_printed` signals; `pubDate` → published |

`bls_schedule.ics` is trimmed to the real `VTIMEZONE` block + **two** real
VEVENTs chosen to exercise BOTH offsets: a January event (EST, UTC−5 →
15:00:00Z) and a July event (EDT, UTC−4 → 14:00:00Z). The VTIMEZONE confirms
`US-Eastern` uses the standard US Eastern DST rules (EDT from the 2nd Sunday of
March, EST from the 1st Sunday of November), i.e. `America/New_York` — the
mapping the adapter uses for the UTC conversion. `bls_latest.rss` is the real
feed verbatim (one rolling "latest numbers" item).

**FRED** (`api.stlouisfed.org/fred/releases/dates`) is the other intended
calendar source but requires a free API key — operator-blocked, stubbed +
recorded in GAPS (no fixture until a key is provisioned).

Re-record:

```sh
UA="(fortuna-research, you@example.com)"
curl -sS -H "User-Agent: $UA" "https://www.bls.gov/schedule/news_release/bls.ics" -o bls.ics   # then keep VTIMEZONE + a Jan + a Jul VEVENT
curl -sS -H "User-Agent: $UA" "https://www.bls.gov/feed/bls_latest.rss" -o bls_latest.rss
```
