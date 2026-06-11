# SOURCES — Kalshi/Kinetics crypto perpetual futures research

All sources fetched **2026-06-11 05:30–06:10 UTC** (evening of 2026-06-10 US time; the
directory is dated per the assignment). Fetch tools: `curl` (raw archival), WebFetch
(analysis), Playwright headless Chromium (kalshi.com pages behind the Vercel bot
checkpoint; NFA BASIC, CNBC — JS-rendered). For browser-fetched pages the archived file
is the extracted `document.body.innerText` (`.txt`); where curl returned real HTML, the
raw `.html` is archived alongside the extracted `.txt`.

Live API responses were captured unauthenticated from the public production and demo
REST endpoints; each `live_*.json` is the verbatim response body.

| ID | File (under raw/) | URL | Type |
|----|-------------------|-----|------|
| P1 | `perps_openapi.yaml` | https://docs.kalshi.com/perps_openapi.yaml | Official REST spec (OpenAPI 3.0.0, "Kalshi Trade API Manual Endpoints" v0.0.1) |
| P2 | `perps_asyncapi.yaml` | https://docs.kalshi.com/perps_asyncapi.yaml | Official WS spec (AsyncAPI 3.0.0, "Kalshi Perps WebSocket API" v2.0.0) |
| P3 | `perps_scm_openapi.yaml` | https://docs.kalshi.com/perps_scm_openapi.yaml | Official Self-Clearing Member (Klear) API spec |
| P4 | `llms.txt` | https://docs.kalshi.com/llms.txt | Docs index |
| P5 | `pages/margin.md` | https://docs.kalshi.com/margin.md | "Perps API" getting-started page |
| P6 | `pages/margin_price-banding.md` | https://docs.kalshi.com/margin/price-banding.md | Price banding rules |
| P7 | `pages/margin-rest_*.md` (24 files) | https://docs.kalshi.com/margin-rest/... (append `.md`) | Per-endpoint REST reference pages |
| P8 | `pages/margin-ws_*.md` (7 files) | https://docs.kalshi.com/margin-ws/websockets/... | WS channel reference pages |
| P9 | `pages/fix-margin_*.md` (5 files) | https://docs.kalshi.com/fix-margin/... | Margin FIX reference pages |
| P10 | `pages/cfbenchmarks_rest-passthrough.md` | https://docs.kalshi.com/cfbenchmarks/rest-passthrough.md | CF Benchmarks REST passthrough |
| P11 | `pages/changelog_index.md` | https://docs.kalshi.com/changelog/index.md | API changelog |
| P12 | `pages/getting_started_rate_limits.md` | https://docs.kalshi.com/getting_started/rate_limits.md | Rate limits & tiers (incl. perps buckets) |
| H1 | `pages/help_collection_perpetual-futures.{html,txt}` | https://help.kalshi.com/en/collections/19654073-perpetual-futures | Help-center collection index |
| H2 | `pages/help_15357561-what-are-perpetual-futures.{html,txt}` | https://help.kalshi.com/en/articles/15357561 | Help: What are perpetual futures |
| H3 | `pages/help_15357566-what-perpetuals-are-available-on-kalshi.{html,txt}` | https://help.kalshi.com/en/articles/15357566 | Help: Available perpetuals table |
| H4 | `pages/help_15357587-btc-perpetual-futures-contract-specifications.{html,txt}` | https://help.kalshi.com/en/articles/15357587 | Help: BTC contract specifications |
| H5 | `pages/help_15357594-how-margin-works.{html,txt}` | https://help.kalshi.com/en/articles/15357594 | Help: How margin works |
| H6 | `pages/help_15357608-your-perpetuals-margin-account.{html,txt}` | https://help.kalshi.com/en/articles/15357608 | Help: Margin account |
| H7 | `pages/help_15357613-how-funding-works.{html,txt}` | https://help.kalshi.com/en/articles/15357613 | Help: How funding works |
| H8 | `pages/help_15357617-order-types.{html,txt}` | https://help.kalshi.com/en/articles/15357617 | Help: Order types / leverage |
| H9 | `pages/help_15357637-take-profit-and-stop-loss-tp-sl.{html,txt}` | https://help.kalshi.com/en/articles/15357637 | Help: TP/SL |
| H10 | `pages/help_15357641-closing-or-modifying-a-position.{html,txt}` | https://help.kalshi.com/en/articles/15357641 | Help: Closing/modifying positions |
| H11 | `pages/help_15357646-understanding-liquidation.{html,txt}` | https://help.kalshi.com/en/articles/15357646 | Help: Liquidation |
| H12 | `pages/help_15357656-applying-for-perpetuals-access.{html,txt}` | https://help.kalshi.com/en/articles/15357656 | Help: Access application; Kinetics/SCM |
| H13 | `pages/help_15357664-depositing-transferring-withdrawing.{html,txt}` | https://help.kalshi.com/en/articles/15357664 | Help: Deposits/transfers/withdrawals |
| H14 | `pages/help_15357671-risks-of-trading-perpetual-futures.{html,txt}` | https://help.kalshi.com/en/articles/15357671 | Help: Risks |
| K1 | `pages/kalshi_perpetuals_learn.{html,txt}` | https://kalshi.com/perpetuals/learn | Kalshi perps learn page (browser; .html is the bot-checkpoint shell from curl, .txt is the rendered text) |
| K2 | `pages/kalshi_fee-schedule.txt` | https://kalshi.com/fee-schedule | Kalshi fee schedule page (browser) |
| K3 | `pages/kalshi_kinetics_page.txt` | https://kalshi.com/kinetics | Kinetic Markets LLC financials + disclosure links (browser) |
| K4 | `kalshi_dcm_rulebook_v1.28.{pdf,txt}` | https://kalshi-public-docs.s3.amazonaws.com/regulatory/rulebook/Kalshi%20DCM%20Rulebook%20v.1.28%20clean.docx.pdf | KalshiEX DCM Rulebook v1.28 (June 1, 2026) |
| K5 | `kalshi_dco_rulebook_v1.4.{pdf,txt}` | https://kalshi-public-docs.s3.amazonaws.com/regulatory/rulebook/Kalshi%20DCO%20Rulebook%201.4.pdf | Kalshi Klear DCO Rulebook v1.4 (April 27, 2026) |
| K6 | `kinetics_155k_fcm_disclosure.{pdf,txt}` | https://kalshi-public-docs.s3.amazonaws.com/regulatory-kinetics/documents/155(k)%20FCM%20Disclosure.pdf | Kinetic Markets CFTC 1.55(k) FCM disclosure (as of June 3, 2026) |
| K7 | `kinetics_affiliation_disclosure.{pdf,txt}` | https://kalshi-public-docs.s3.amazonaws.com/regulatory-kinetics/documents/Affiliation%20Disclosure.pdf | Kinetics affiliation disclosure |
| K8 | `kinetics_fee_disclosure.{pdf,txt}` | https://kalshi-public-docs.s3.amazonaws.com/regulatory-kinetics/documents/Fee%20Disclosure%20Statement.pdf | Kinetics fee disclosure statement (as of June 3, 2026) |
| K9 | `kinetics_risk_disclosure.{pdf,txt}` | https://kalshi-public-docs.s3.amazonaws.com/regulatory-kinetics/documents/Risk%20Disclosure.pdf | Kinetics risk disclosure statement |
| K10 | `kinetics_privacy_notice.pdf` | https://kalshi-public-docs.s3.amazonaws.com/regulatory-kinetics/documents/Privacy%20Notice.pdf | Kinetics privacy notice |
| R1 | `cftc_btcperp_order_26601.pdf` | https://www.cftc.gov/filings/documents/2026/orgdcmkexbtxperporder26601.pdf | CFTC Order Approving BTCPERP (issued May 29, 2026) |
| R2 | `pages/cftc_pr_9240-26.html` | https://www.cftc.gov/PressRoom/PressReleases/9240-26 | CFTC press release: BTCPERP approval |
| R3 | `pages/cftc_pr_8957-24.html` | https://www.cftc.gov/PressRoom/PressReleases/8957-24 | CFTC press release: Kalshi Klear DCO registration |
| R4 | `pages/nfa_basic_kinetic_markets.{html,txt}` | https://www.nfa.futures.org/BasicNet/basic-profile.aspx?nfaid=laKM6t6wuBQ%3D | NFA BASIC profile: KINETIC MARKETS LLC, NFA ID 0574784 (browser; .html is the pre-JS shell) |
| N1 | `pages/news_kalshi_launch_post.{html,txt}` | https://news.kalshi.com/p/kalshi-launches-perpetual-futures-america | Official Kalshi launch announcement (May 29, 2026) |
| N2 | `pages/cnbc_perps_1b_volume.txt` | https://www.cnbc.com/2026/06/09/kalshi-perpetual-futures-trading-perps-crosses-1-billion-in-volume-within-a-week-of-launch.html | CNBC volume story (browser) |
| W1 | (not archived; WebFetch summary only) | https://www.cfbenchmarks.com/indices/BRTI | CF Benchmarks BRTI product page |
| W2 | (search results only) | https://www.coinbase.com/blog/perpetual-futures-have-arrived-in-the-us | Coinbase US perps blog (403 to WebFetch; cited from search snippets only — LOW) |
| L1–L11 | `live_prod_margin_markets.json`, `live_prod_market_btc.json`, `live_prod_orderbook_btc.json`, `live_prod_trades_btc.json`, `live_prod_margin_exchange_status.json`, `live_prod_risk_parameters.json`, `live_prod_funding_estimate_btc.json`, `live_prod_funding_hist_btc.json`, `live_prod_funding_hist_all.json`, `live_demo_margin_markets.json`, `live_demo_funding_estimate_btc.json`, `live_demo_risk_parameters.json` | `https://external-api.kalshi.com/trade-api/v2/margin/...` and `https://external-api.demo.kalshi.co/trade-api/v2/margin/...` | Live unauthenticated API captures, 2026-06-11 ~05:50–06:05 UTC |

Notes:
- docs.kalshi.com served every page and all three YAML specs to plain `curl` with no
  challenge. kalshi.com (incl. `kalshi.com/docs/kalshi-fee-schedule.pdf`) is behind a
  Vercel bot checkpoint for curl/WebFetch; a real browser passes it. The
  `kalshi-public-docs` S3 bucket is fully open.
- The Wayback Machine's only copy of `kalshi-fee-schedule.pdf` is 2026-02-18
  (pre-perps); not archived here because it predates the product.
