# Kalshi fixtures (operator-supplied)

The Kalshi adapter (BUILD_PLAN T1.1) is built ONLY against recorded request/response
pairs in this directory. The agent must never invent API behavior.

Operator: capture from your live account (sanitize credentials) one file per call:
auth flow, GET markets (paginated), GET orderbook, POST order (maker accepted; taker
fill; rejection), DELETE order, GET fills (cursor pagination), settlement notice,
balance. Name as <area>__<case>.json with a sibling .meta.json (timestamp, endpoint,
status). Until present, the adapter is stubbed behind the Venue trait and the need is
recorded in GAPS.md.
