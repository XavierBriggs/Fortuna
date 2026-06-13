# NWS (api.weather.gov) fixtures

Real responses captured from the live public NWS API on 2026-06-13 with
`User-Agent: (fortuna-research, xbriggs03@gmail.com)`. NWS data is U.S.
Government public-domain, free, no API key (see the dossier at
`docs/research/sources/nws/dossier.md`). Adapter tests parse THESE bytes;
never invent NWS response shapes (house rule). Re-record with the commands
below when the schema is suspected to have drifted.

| file | endpoint | shape | used to test |
|------|----------|-------|--------------|
| `afd_list.json` | `/products?type=AFD&limit=2` | `@graph` list of product summaries | products-list parse → one `nws.afd` signal per entry |
| `afd_product.json` | `/products/{id}` | single product with `productText` | (single-product shape; full-text two-hop is a documented follow-up) |
| `alerts_active.json` | `/alerts/active?area=TX` | GeoJSON `FeatureCollection` (trimmed to 1 feature) | alerts parse → one `nws.alert` signal per feature; `claimed_time` from `properties.sent` |
| `error_400.json` | `/alerts/active?limit=1` | NWS `BadRequest` problem+json | error-envelope handling (non-2xx path) |

`alerts_active.json` is the real response with its `features` array trimmed
to the first entry for compactness; every other byte is as the API returned
it.

Re-record:

```sh
UA="(fortuna-research, you@example.com)"
curl -sS -H "User-Agent: $UA" "https://api.weather.gov/products?type=AFD&limit=2" -o afd_list.json
PID=$(python3 -c "import json;print(json.load(open('afd_list.json'))['@graph'][0]['id'])")
curl -sS -H "User-Agent: $UA" "https://api.weather.gov/products/$PID" -o afd_product.json
curl -sS -H "User-Agent: $UA" "https://api.weather.gov/alerts/active?area=TX" -o alerts_active.json   # then trim features to 1
curl -sS -H "User-Agent: $UA" "https://api.weather.gov/alerts/active?limit=1" -o error_400.json        # 'limit' is rejected → 400
```
