# AMENDMENT — TRACK C — A2d slice-3 UNBLOCKED: build the `funding_rates_historical` capture

**Hand this to track-C's loop.** Operator-endorsed 2026-06-14. This SUPERSEDES your
`BUILD-BLOCKED` ledger (GAPS @c8775c9 — "A2d SLICE 3 design-complete, blocked on a realized-funding
source"). The source EXISTS, is PUBLIC, and is already fixture-backed. Bus entry (binding, read at
priority (a)): `docs/reviews/GATE-FINDINGS-LATEST.md` → LATEST → "A2d SLICE-3 DATA SOURCE — FOUND".
This amendment extends `AMENDMENT-track-C-slice-3b-v2.md` (§2.6 A2d); everything there still holds.

---

## What changed

Slice-3 was design-complete but blocked: you correctly found there was no realized-funding feed to
resolve `funding_forecast` against (no table, no `realized_rate` on `PerpTick`, Sim pays no funding).
**Verifier research resolved it.** Kalshi exposes finalized funding rates on a **public, no-auth**
endpoint, and it's already captured to disk. So the blocker is a small DATA-CAPTURE build, not a
missing capability. Build it; then complete the A2d slice-3 resolve/score loop on top.

## The data source (grounded — re-verified 2026-06-14 vs the archived spec + live captures)

- **Endpoint:** `GET /margin/funding_rates/historical?ticker=&start_ts=&end_ts=` — **PUBLIC, no
  auth** (`perps_openapi.yaml:887`). Host: prod `external-api.kalshi.com/trade-api/v2`, demo
  `external-api.demo.kalshi.co/trade-api/v2` (demo tickers carry a `1` suffix, e.g. `KXBTCPERP1`).
  Omit `start_ts` → "earliest available data" (full history since the 2026-06-03 launch). `ticker`
  optional → all markets in one call.
- **Response shape** (confirmed against the capture):
  `{ "funding_rates": [ { "market_ticker": "KXBTCPERP", "funding_time": "2026-06-11T04:00:00Z",
  "funding_rate": -0.000397, "mark_price": "6.2658" }, ... ] }`
  - `funding_time` — RFC3339, an **exact 8h boundary** (04:00 / 12:00 / 20:00 UTC).
  - `funding_rate` — decimal fraction per 8h, **FINALIZED** at `next_funding_time` (number/double).
    The <0.01% zero-threshold means **~64% of records are exactly 0** — that is real, keep them.
  - `mark_price` — `FixedPointDollars` string (per-contract dollars).
- **This is the scoring target.** It is exactly what `funding_forecast` predicts and the realized
  outcome all four A2d baselines (carry-forward, last-rate, estimate-RW, persistence-RW) score
  against. The companion estimate feed (the estimate-RW baseline input) is the public
  `GET /margin/funding_rates/estimate?ticker=` — already in your A2d slice-2.
- **Fixtures already on disk (real, provenanced — wire tests against these, do NOT fabricate):**
  `docs/research/venue/kinetics-perps-2026-06-10/raw/live_prod_funding_hist_all.json` (100 records,
  11 markets, 36 nonzero), `…_funding_hist_btc.json`, `…_funding_estimate_btc.json`.

## Honest depth caveat (build for this; don't oversell the result)

The product launched 2026-06-03, so a backfill is currently SHALLOW: ~11 days × 3/day ≈ 33
points/market, ~64% zero. ⇒ **Slice-3 = wire + correctness-validate NOW** (the loop resolves a
forecast against its finalized rate; prove the scoring math on the fixture + a live backfill). The
**statistical** beats-baselines verdict (A2d) ACCRUES over the soak as the series grows — which is
correct: an I7 forward-validation gate is time-gated by nature. You are unblocked to BUILD the loop;
you are NOT yet able to DECLARE an edge, and must not.

## The build (each part gate-clean + full battery)

1. **Ledger table (`fortuna-ledger`, new migration):**
   `funding_rates_historical(market_ticker, funding_time, funding_rate, mark_price, captured_at)`,
   `UNIQUE(market_ticker, funding_time)`. The finalized rate is **immutable** — re-poll dedups via
   `ON CONFLICT DO NOTHING` (a no-op, never an UPDATE; consistent with append-only — no superseding
   rows because finalized funding never changes). INSERT-only repo. One migration, sqlx
   compile-checked, `cargo sqlx prepare` for offline CI.
2. **Read-only Kalshi client method (`fortuna-venues::kalshi`):** `get_funding_rates_historical`
   over the **public** path — **no creds, no signing** (it is unauthenticated). **PIN the host**
   (prod/demo constants); never derive the URL from any payload (the track-D SSRF BLOCK is the
   cautionary tale). Parse against the schema above.
3. **The capture loop:** takes `&dyn Clock` + `CancellationToken` (house pattern — nothing sleeps on
   wall time). Backfill once (no `start_ts`), then poll just past each 8h boundary. The payload is
   **untrusted data (spec 5.11)** — validate shape; on a non-conforming payload, refuse-and-quarantine
   (do not write garbage into the ledger), record in GAPS, continue. No model in this path.
4. **Complete A2d slice-3:** wire the resolve/score loop to read finalized rates from the new table,
   match each resolvable `funding_forecast` window (and the four baselines) to its realized
   `funding_rate` at `funding_time`, and write the side-by-side score via the existing
   `(belief_id, rule_id)` rows (§1.3 / §2.6 A2d). Test that the comparison is COMPUTED on the
   fixture. Stays DATA-ONLY — no auto-promotion (I7); the edge is the operator's call on the
   measured, accrued result.

## Sequencing

- Slice-3 is now buildable. **A3+A6 (the per-bracket EV trader) is INDEPENDENT of this data feed** —
  you may run it in parallel while the captured series accrues depth. Do whichever is in front of
  you; both are in your queue. (Watch the bus — a BLOCK naming track-C preempts.)

## Discipline (non-negotiable — the verifier gates each part on the merged tree, mutation-proven)

- **No creds in this path** — the endpoint is public. Secrets stay env-only; nothing new to wire.
- **Money/forecast split:** `funding_rate` is forecast-domain `f64` (a rate, not money) — fine,
  mirrors `FundingObservation`. `mark_price` crosses the `PerpPrice`/`Cents` boundary as the existing
  type does; no `f64` touches a `Cents`.
- No `panic!`/`unwrap`/`expect` anywhere in the capture or scoring path. Every degenerate/missing/
  malformed input degrades to "skip + quarantine + GAPS", never a crash or a fabricated rate.
- All time via the injected `Clock`; no `SystemTime::now()`. Boundary math (next funding_time) is
  Clock-derived and DST-tested.
- Untrusted-data boundary (5.11): a doctored funding payload must NOT mutate state beyond a
  quarantined record — expect the verifier to gate with a malformed-fixture mutation check.
- Protected crate `fortuna-invariants/` UNTOUCHED (this slice adds no invariant; if it tempts you to,
  STOP and ledger in GAPS).
- Each part: `cargo fmt --check`, clippy `-D warnings`, full suite, `scripts/run-dst.sh` all green;
  new failure modes → DST corpus; GAPS/ASSUMPTIONS updated; tick the BUILD_PLAN box with a one-line
  note. Tests from the spec text BEFORE implementation. Ledger your build response in GAPS — do NOT
  edit the bus.

## When you finish
Push your branch; the verifier gates it on the MERGED tree, mutation-proven, part-by-part. The
capture (parts 1–3) and the slice-3 wiring (part 4) may land as one branch or two — your call.
