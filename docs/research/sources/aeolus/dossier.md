# Source vetting dossier — aeolus

> Layer 0 admission (design §4.4). Grounded in the Aeolus → FORTUNA source
> contract (rev 3, 2026-06-13) and the REAL captured forecast envelope
> (`fixtures/sources/aeolus/knyc_tmax.json`, `knyc_tmin.json`, captured
> 2026-06-13). Facts are cited, not recalled.
>
> **Trust framing is deliberately SOBER.** Aeolus is admitted HIGH on
> AUTHENTICITY (it is the operator's own authenticated system) but at a MODEST
> EMPIRICAL tier. The dossier does NOT assert a market edge the loop has not
> measured; Layer 3 earns the empirical tier. See the contract §1/§5.

---

## Identity

- **source_id:** `aeolus` (instances keyed per station/variable feed; one
  registry row / trust record for the source, beliefs keyed by
  `(station, target_date, variable, run_at)`)
- **Publisher / operator:** the operator's own **Aeolus** system — a
  PROPRIETARY, internal, CRPS-validated probabilistic temperature-forecast
  engine (SAR-SEMOS, model_version `sar-semos-v1`). Not a third party.
- **Domain tags:** weather
- **Primary URL (pinned host):** pinned in `source_registry`; https-only,
  `x-api-key` auth. Endpoint `GET /v2/forecasts?station&variable&from&to`.
  NOTE: currently served over an **ephemeral Cloudflare quick-tunnel** for
  integration testing (a watched contract-stability caveat — see Risks).
- **Acquisition class:** `aeolus` (REST/JSON pull; FORTUNA polls — no
  push/webhook in v2)
- **Resolution-source eligible?** **No.** Aeolus FORECASTS; it does not
  RESOLVE. The grader is the NWS observed daily high/low
  (`resolution.authority = nws_observed_high`), served by the separate
  `nws_climate` observed-daily-extreme source (contract §3.2, §5.12). That
  independence is the point: Aeolus beliefs are graded by a source other than
  the forecaster (not self-graded; V4-clean).

## Six-dimension score (design §4.4 Layer 0)

| # | Dimension | Score | Justification |
|---|-----------|:-----:|---------------|
| 1 | **Authority** | 7 | It IS the operator's own proprietary forecast — high on OWNERSHIP/authenticity. But it is a FORECAST, not ground truth: μ is empirically ~4–8% *worse* than the raw GEFS mean; the authority is over "what Aeolus believes," not over the observed high. |
| 2 | **Directness** | 10 | Primary: the operator's own system, straight from its `forecast_log` (production SAR-SEMOS state), not reporting about a forecast. |
| 3 | **Contract stability** | 7 | Versioned wire schema (`aeolus.forecast/v2`), `deny_unknown_fields` strict parse + lockstep co-evolution (§8) — strong. Held below 9 because it is currently served over an EPHEMERAL Cloudflare quick-tunnel (host churns until a stable host is pinned). |
| 4 | **Latency-to-event** | 9 | Tracks the GEFS cadence (~6h per station); `next_run_at` advertises the next run so the scheduler arrives just after publish (§3.4). `run_at` = forecast init_time, point-in-time honest. |
| 5 | **ToS cleanliness** | N/A | Internal, operator-owned, `x-api-key`-authenticated system — no third-party ToS to honor. Not scored (not a public feed). |
| 6 | **Resolution eligibility** | 2 | LOW — it forecasts, it does not grade. The grader is NWS observed-high via `nws_climate`; Aeolus cannot declare itself the resolution source. |

## Initial trust tier

- **Proposed tier (0–10):** `7`
- **Band rationale:** Layer 0 admits HIGH on AUTHENTICITY (operator-owned,
  `x-api-key`-authenticated, directness 10), so it sits above an aggregator. But
  the EMPIRICAL trust is modest and unproven: Aeolus's point forecast (μ) is
  COMMODITY (~4–8% worse than raw GEFS); its edge over raw is CALIBRATION (σ);
  its edge over the MARKET — what FORTUNA actually trades against — is parity at
  best, historically negative (contract §1, producer handoff §5). Tier 7 is the
  honest compromise: **high enough** to be a real input and clear the trigger
  floor (default 5), but **deliberately NOT 9–10** — it does not pre-bake a
  market edge the loop has not measured. **This dossier must not assert an edge
  the loop has not seen.** The empirical tier rises (or falls) only as the
  independent Layer-3 scoring measures it (Brier/CLV at settlement against the
  `nws_climate` observed-high grader). `crpss_vs_raw > 0` means "better
  CALIBRATED than raw," NOT "beats NOAA" and NOT "beats the market" — and it
  ships `null` at first anyway.
- **Consumption consequences at this tier (design §4.4 Layer 4):**
  - Resolution-source floor (default 8): **may not** declare resolution source —
    correct by construction (Aeolus is a forecaster; resolution eligibility is
    LOW). The grader is `nws_climate`.
  - Trigger floor (default 5): **may** wake a decision cycle (7 ≥ 5).

## Operational facts (for the `[sources.<id>]` config)

- **Endpoint(s):**
  - `GET /v2/forecasts?station={id}&variable={tmax|tmin}&from={YYYY-MM-DD}&to={YYYY-MM-DD}`
    → `200 { "forecasts": [ <aeolus.forecast/v2 envelope>, ... ] }` (latest GEFS
    run per `(station, target_date)` in range — NOT every historical run).
    `304` on `If-None-Match` (forecast-scoped ETag); `4xx/5xx` →
    `{ "error": { "code", "message" } }`.
- **Auth:** **`x-api-key` header** (Aeolus Enterprise-tier key, no daily cap).
  Token is env-only (**`AEOLUS_API_TOKEN`**) — never in repo, config, logs, or
  audit payloads; redacted everywhere it could surface (contract §3.1, house
  rule). The F1 substrate is a GENERIC per-source header injector, so the
  header name is config-driven (Bearer remains a drop-in).
- **Variables emitted:** **`tmax` / `tmin` ONLY.** Aeolus has NO predictive
  degree-day model (DD is observed-only), so `hdd`/`cdd` are NOT on the wire;
  FORTUNA must not build enum handling for them yet (contract §3.3, §11).
- **Update cadence (observed):** GEFS cadence ~6h per station; the captured
  KNYC envelope shows `run_at 00:00Z`, `next_run_at 06:00Z`. Proposed
  `base_interval`: release-aware — idle between runs, tight poll in the window
  around the advertised `next_run_at` (contract §3.4, D9 scheduler).
- **Event windows (release-aware):** the scheduler CONSUMES `next_run_at` to
  schedule the next poll around the advertised next run, plus the GEFS ~6h
  release pattern, so FORTUNA arrives just after Aeolus publishes (§3.4).
- **Conditional GET:** yes — FORTUNA sends `If-None-Match`; Aeolus returns a
  **forecast-scoped ETag** computed over the identity tuple + `distribution` +
  `resolution` + `brackets`, **EXCLUDING `skill.*` and `next_run_at`**. A
  skill-only recompute on an unchanged forecast therefore 304s (no re-ingest,
  no spurious "new forecast" signal); a corrected μ/σ at the same `run_at` DOES
  change the ETag and supersedes (contract §3 rule 4).
- **Rate limits / politeness:** Enterprise-tier key, no daily cap; the shared
  `FetchClient` per-host politeness budget still applies. Steady-state polling
  is near-free via the 304 path.
- **Payload shape + content-hash basis:** one `aeolus.forecast/v2` envelope per
  `(station, target_date, variable)`. Dedup is on the **forecast-identity
  tuple** `(station, target_date, variable, run_at)` — NOT a hash of the whole
  payload — so volatile `skill.*` telemetry does not masquerade as a new
  forecast (contract §4, rev 2). The adapter is DUMB: it emits the raw envelope
  untouched as the `RawSignal` payload; the strict v2 parse + σ>0/units checks +
  μ/σ→p live cognition-side (F6, `reconciliation.rs`).
- **Claimed-time field (Layer 1):** **`run_at`** — the forecast `init_time`
  (when Aeolus produced the run), NOT the API response time. This is the
  point-in-time authority for the belief's evidence/freshness and the
  future-dated check (contract §2, §3 rule 3).

## Risks & failure modes

- **Ephemeral Cloudflare quick-tunnel (contract-stability caveat).** The
  endpoint is currently served over a throwaway quick-tunnel for integration
  testing; the host churns. The pinned `source_registry` host must be updated to
  a stable host before steady-state operation — host-pinning + https-only still
  bound it, but a churned host is a fetch outage, not a security hole.
- **Empirical edge unproven / could be negative.** μ is commodity (~4–8% worse
  than raw GEFS); the market edge is parity-at-best, historically negative. The
  tier-7 admission is on authenticity, not measured edge — if Layer 3 measures
  no edge (or negative CLV), the empirical tier falls. The dossier asserts no
  edge; the loop must measure it.
- **`skill.*` is self-reported and nullable.** `crpss_vs_raw` ships `null` until
  the Aeolus fast-follow scorer lands; `n_scored` is the small windowed count
  (30 in the live capture — confirmed, NOT the 11,174 of the doc example).
  Never gate fast-triggers on a self-reported skill claim; Layer 3 re-scores
  independently against the grader.
- **Resolution depends on a SEPARATE registered grader.** Aeolus weather
  beliefs are unscoreable (and thus excluded, §5.12) unless the `nws_climate`
  observed-daily-extreme source exists and is registered. That dependency is
  sequenced before/with `AeolusSource` (contract §3.2).
- **Replay determinism for μ/σ→p.** `P = 1 − Φ((t−μ)/σ)` needs an `erf`; a
  platform `libm` erf is not bit-identical across toolchains and would break I5
  byte-identical replay. The μ/σ→p helper must use a pinned in-repo
  deterministic erf, not the system math library (contract §7).
- **Trusted, but still DATA not instructions (§5.11).** Operator-owned does not
  exempt the payload — it reaches the model only inside delimited data blocks;
  I1 (gates) and I6 (propose-only) bound it regardless. A poisoned/buggy run
  (σ absurd, μ off-planet) fails the strict parse or the bracket-vs-distribution
  cross-check, not silently traded.
- **Corroboration:** Aeolus is a single proprietary origin (not syndicated);
  Layer-2 corroboration uses the raw-NWS / observed source as a genuine
  corroboration input, not a mere divergence alarm, until Layer 3 has measured
  Aeolus.

## Evidence (cited, dated)

- **Aeolus → FORTUNA source contract, rev 3** (`docs/design/aeolus-fortuna-source-contract.md`,
  2026-06-13): wire schema `aeolus.forecast/v2`; `x-api-key` auth, env-only
  `AEOLUS_API_TOKEN`, redacted (§3.1, §11); tmax/tmin only — no degree-day model
  (§3.3, §11); trust sobered — μ commodity ~4–8% worse than raw GEFS, edge over
  raw is calibration, edge over market parity-at-best/negative, admit HIGH on
  Layer 0 but MODEST empirical tier, Layer 3 earns it (§1, §5, §11);
  `crpss_vs_raw` nullable = "no live skill claim," never gate fast-triggers on
  it (§2, §5, §11); `brackets[].p` clamp-not-reject to `[1e-6, 1−1e-6]` (§2, §5,
  §11); forecast-scoped ETag excludes `skill.*`/`next_run_at` (§3); dedup on the
  identity tuple (§4); resolution grader is the separate registered NWS
  observed-daily-extreme source (§3.2, §5.12); pinned deterministic erf for
  μ/σ→p (§7); `next_run_at` consumed for release-aware cadence (§3.4); adapter is
  a dumb raw-envelope passthrough, strict parse is cognition-side F6 (§4).
- **Live captured envelopes** (`fixtures/sources/aeolus/knyc_tmax.json`,
  `knyc_tmin.json`, captured 2026-06-13): confirm the rev-3 reconciliation —
  `schema "aeolus.forecast/v2"`, station `KNYC`, variable `tmax`,
  `distribution{family "normal", mu 87.347, sigma 1.903, model_version
  "sar-semos-v1"}`, **`skill.crpss_vs_raw` is `null`** and **`skill.n_scored` is
  `30`** (windowed, `window_days 30`) — exactly as the contract predicted;
  `resolution.authority "nws_observed_high"`, `nws_station_id "NYC"`,
  `settles_after 2026-06-14T10:00:00Z`; 14 `brackets[]` each with
  `event_hint`/`threshold_f`/`comparison "ge"`/`p` (all `p ∈ (0,1)`,
  pre-clamped); `run_at 2026-06-13T00:00:00Z`, `next_run_at
  2026-06-13T06:00:00Z`, `valid_until 2026-06-13T04:00:00Z`.

## Decision

- [x] Admitted at tier `7` — registry row + config entry to be created when the
  scheduler (D9) wires sources; `AeolusSource` adapter (F3) + fixtures land with
  it. Admitted HIGH on authenticity, MODEST empirical tier; Layer 3 earns the
  empirical trust against the independent `nws_climate` grader. The dossier
  asserts no market edge the loop has not measured.
- [ ] Rejected — n/a

Reviewer: Track F implementer · Date: 2026-06-13

---

### Follow-up (ledgered)

1. **Stable host before steady-state.** The pinned `source_registry` host must
   move off the ephemeral Cloudflare quick-tunnel to a stable host. Until then,
   contract-stability is held at 7 (recorded so a future config author pins the
   right host and doesn't trust the tunnel URL).
2. **Resolution grader must exist first.** `nws_climate` (NWS observed
   daily-extreme) must be registered before any Aeolus weather belief is
   scoreable (§3.2, §5.12) — sequenced before/with `AeolusSource`.
3. **Empirical tier is provisional.** Tier 7 is a Layer-0 admission, not a
   measured edge. The Layer-3 scorer re-scores every Aeolus belief by Brier/CLV
   against the grader; the empirical tier follows the measurement, up or down.
