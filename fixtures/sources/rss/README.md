# RSS / Atom fixtures

Real feeds captured 2026-06-13 with `User-Agent: (fortuna-research,
xbriggs03@gmail.com)`. The generic `RssSource` adapter parses these via
`feed-rs`, which unifies RSS 2.0 / Atom / RDF / JSON-Feed into one entry shape.
Adapter tests parse THESE bytes; never invent feed behavior (house rule).

| file | source | format | used to test |
|------|--------|--------|--------------|
| `fed_press_rss2.xml` | Federal Reserve Board press releases (`federalreserve.gov/feeds/press_all.xml`) | RSS 2.0 | RSS-2.0 parse → `rss.item` signals; `pubDate` → `published` |
| `sec_edgar_atom.xml` | SEC EDGAR recent 8-K filings (`sec.gov/cgi-bin/browse-edgar?...&output=atom`) | Atom | Atom parse → the SAME `rss.item` shape; `<updated>` → `updated` |
| `malformed.xml` | hand-truncated | broken XML | parse failure → `SignalError`, never a panic |

Both real feeds are trimmed to the first 2 items/entries for compactness;
every other byte is as the source returned it. `malformed.xml` is an
intentionally unterminated document.

Re-record:

```sh
UA="(fortuna-research, you@example.com)"
curl -sS -H "User-Agent: $UA" "https://www.federalreserve.gov/feeds/press_all.xml" -o fed_press_rss2.xml   # then trim to 2 <item>
curl -sS -H "User-Agent: $UA" "https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=8-K&company=&dateb=&owner=include&count=10&output=atom" -o sec_edgar_atom.xml   # then trim to 2 <entry>
```
