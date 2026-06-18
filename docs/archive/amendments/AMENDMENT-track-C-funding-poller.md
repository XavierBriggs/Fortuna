# AMENDMENT — TRACK C — A2d slice-3 Part 2: the funding-rates POLLER (fill the store)

**Hand this to track-C's loop.** Operator-endorsed 2026-06-14. Bus (priority (a)):
`docs/reviews/GATE-FINDINGS-LATEST.md`. Extends `AMENDMENT-track-C-funding-capture.md`. Grounding:
`docs/research/venue/kinetics-perps-2026-06-10/` (`perps_openapi.yaml`) + the real captured responses.

## What changed
A2d slice-3 has THREE parts. **Part 1** (the `funding_rates_historical` STORE) MERGED @b8f9299;
**Part 3** (the resolve→score loop) MERGED @db17fe8. **Part 2 — the POLLER that FILLS the store — is the
missing piece.** Today nothing writes the store outside tests (`daemon.rs` only READS it via
`realized_rate`), so in production the store is EMPTY and the scoring loop has nothing to resolve
against. The poller closes that — it is the only thing standing between "the loop is built" and "the
loop is running on real data."

## The build (a PUBLIC-GET poller; no creds, host-pinned)
1. A read-only client call to `GET /margin/funding_rates/historical?ticker=&start_ts=&end_ts=` —
   **PUBLIC, no auth** (`perps_openapi.yaml:887`). If the Kinetics client already exposes the path,
   reuse it; else add the unauthenticated method. **PIN the host** (prod/demo constant); NEVER derive the
   URL from any payload (the track-D SSRF BLOCK is the cautionary tale).
2. A Clock-driven poll loop (takes `&dyn Clock` + `CancellationToken`; nothing sleeps on wall time):
   BACKFILL once (omit `start_ts` ⇒ "earliest available data"), then poll just past each 8h boundary
   (04:00/12:00/20:00 UTC). Read the incremental cursor from
   `FundingRatesHistoricalRepo::latest_funding_time(market)`.
3. For each finalized record `{market_ticker, funding_time, funding_rate, mark_price}`, call
   `FundingRatesHistoricalRepo::insert(...)` (idempotent `ON CONFLICT DO NOTHING` — a re-poll is a no-op,
   never an UPDATE; the store's trigger refuses UPDATE/DELETE, I5).
4. The response is UNTRUSTED DATA (spec 5.11): validate the shape; a non-conforming payload is
   refuse-and-quarantine (alert + skip, never write garbage), never a panic.

## Discipline (non-negotiable — verifier gates on the MERGED tree, mutation-proven)
- NO creds (the endpoint is public); secrets stay env-only; nothing new to wire.
- `funding_rate` is forecast-domain `f64` (a rate, NOT money); `mark_price` stays a VERBATIM string —
  no `f64` touches a `Cents`. No `unwrap`/`panic` anywhere; a fetch/parse failure alerts-and-continues.
- Clock-injected; no `SystemTime`. Append-only/idempotent end to end.
- Tests against the REAL provenanced fixtures (`…/raw/live_prod_funding_hist_all.json` etc. — already on
  disk): parse → insert → assert idempotent re-poll returns `Ok(false)` + `latest_funding_time` advances.
  `cargo fmt --check`, clippy `-D warnings`, full workspace test, `scripts/run-dst.sh` green; tick the
  BUILD_PLAN box; ledger in GAPS.
- Honest depth caveat: the product launched 2026-06-03, so a backfill is SHALLOW (~11 days, ~64% zeros);
  the poller makes the store GROW — the statistical beats-baselines verdict accrues over the soak. That
  is correct (an I7 forward-validation gate is time-gated); the poller's job is to START the accrual.

## When you finish
Push; the verifier gates on the MERGED tree, mutation-proven. This completes A2d slice-3 END-TO-END
(store + poller + resolve/score → funding forecasts scored against accruing ground truth). A BLOCK
naming track-C preempts your queue.
