> ARCHIVED 2026-06-22. Superseded by CHANGELOG.md and decisions/. Kept for provenance; not source of truth.

# FORTUNA — Close-The-Loop Fix List

> Compiled 2026-06-18 from a live audit of the running paper-on-live soak (DB `fortuna_demo`)
> + the code. Each item: **evidence** (verified), **impact**, **fix**, **confidence**.
> Priority: **P0** blocks the loop / real PnL · **P1** blocks honest validation · **P2**
> correctness/cost/reliability · **P3** cleanup / verify-only.
>
> Headline: the engine (ingest → belief → propose → gate → paper-fill) runs and is safe.
> Nothing is *validated* yet because the **right side of the loop is open** (no settlement →
> no realized PnL; no resolved weather beliefs → no calibration → model arm can't trade) and
> the one numeric "verdict" (perp funding) is a measurement artifact.

---

## P0 — Blocks the loop / no real PnL

### F1. Paper-live settlement is not wired
- **Evidence:** `crates/fortuna-live/src/daemon.rs:1452` comment marks `account()/positions()/settlements_since` a "Phase-2 follow-on". Live DB: `settlement_entries = 0`, `market_snapshots = 0`, zero settle/resolve lines in `daemon.log`, despite markets (KXBTC-26JUN1717 @17:00 UTC, KXHIGHPHIL-26JUN17 @EOD) being hours past resolution.
- **Impact:** every paper fill stays open forever; **no realized PnL for any strategy**. The trade half of the loop cannot close → the $50k-PnL north-star metric does not exist. This is THE blocker.
- **Fix:** drive `settlements_since`/`PaperVenue::settle_market` on resolved markets in the daemon segment loop; write `settlement_entries`; compute realized PnL per intent; reconcile positions to closed. TDD: a resolved market closes its paper fills into realized PnL; a fill on an unresolved market stays open. Plumbing exists (`paper_live.rs:181`, `paper.rs:272 settle_market`, `paper.rs:776`).
- **Confidence:** HIGH (confirmed).

---

## P1 — Blocks honest validation / model arm

### F0. Calibration is FIT but NEVER PERSISTED → the model arm can never trade (NEW, critical)
- **Evidence:** `run_weekly_review` (`daemon.rs:4184`) fits a Platt calibration once a scope has ≥50 resolved beliefs (`FULL_AUTONOMY_N`, `calibration.rs:29`; fit at `review.rs:101`), but step 4 (`daemon.rs:4292`) only audits counts and returns — comment: *"I7: recommendations only — the daemon NEVER promotes."* The field is named `fitted_version_would_be`. **Repo-wide grep finds NO caller of `CalibrationParamsRepo::insert`** (only `latest`/`scopes` reads). `calibration_params` is never written in production → stays 0 forever.
- **Impact:** synthesis reads calibration → finds none → `calibration_quality=0` → sizes ZERO. The model/synthesis arm **can never trade regardless of soak length or resolved-belief count.** It's a disconnected wire, not a cold-start. More fundamental than F1 for the *model* arm.
- **Fix:** either (a) auto-persist the fitted calibration when `stage="paper"` (safe — no real capital; the point of the soak), or (b) add an operator "apply reviewed calibration" command/path. Decide the I7 boundary: applying calibration in paper is not capital promotion.
- **Confidence:** HIGH (confirmed — no insert caller exists).

### F2. Scalar-belief oversampling makes funding "validation" vacuous AND gamifies promotion
- **Evidence:** live DB `belief_scores = 58,435` = `11,687 resolved scalar beliefs × 5 rules`, but only **4 distinct resolved `horizon` windows**; realized funding ≈ 0 (2 distinct values, avg 0.0000458). One belief is emitted per perp tick, all targeting the same funding settlement → ~2,900× oversampling. `resolve_and_score_funding_beliefs` mints 5 score rows per belief (`daemon.rs:3633`).
- **Impact:** (a) the A2d CRPS comparison vs carry-forward/last-rate is statistically meaningless (effective n≈4, all near-zero) — the apparent "carry-forward beats the model" verdict is noise; (b) **gamifies the promotion gate** — `[review] min_resolved_beliefs_synthesis = 100` counts belief rows, satisfiable from ~1 window with zero real evidence.
- **Fix:** score/compare per *distinct window* (dedup by `(producer, horizon)` — take the last belief per window, or aggregate), not per belief row. Make the `[review]` resolved-belief gate count distinct windows. Add a min-distinct-windows AND a realized-value-dispersion floor before any A2d verdict is reported as meaningful.
- **Confidence:** HIGH (confirmed).

### F3. Calibration cold-start: weather beliefs never resolve → synthesis can never size
- **Evidence:** live DB `beliefs = 108` (all `aeolus`, all `status=open`, `count(brier)=0`); `calibration_params = 0`. Synthesis sizing keys on `calibration_quality`; missing ⇒ size ZERO (fail-closed). So the model arm has never placed an order.
- **Impact:** the entire model→trade arm is inert. "Validate model edges" is impossible until weather beliefs resolve → score (brier/clv) → fit Platt calibration → `calibration_params` row exists for `(synthesis_model,"synth_events","weather","platt")`.
- **Fix:** verify the weather belief resolution path end-to-end: NWS `nws_climate` grader resolves open weather beliefs → sets `outcome/brier/clv_bps` → the calibration job fits + writes `calibration_params`. Confirm each link runs on the live daemon (none have produced a single resolved weather belief yet). Likely needs the grader cadence + a sustained run past resolution.
- **Confidence:** HIGH (confirmed state; the broken link within the chain still needs to be pinpointed — grader not running vs no resolutions due yet).

### F4. World-forward watch events dead-end (Mind reasoning leads nowhere)
- **Evidence:** live DB — every non-`synthesis` event (categories `macro`, `Macro/Fed`, `politics`, `macro/regulatory`, `monetary_policy`, `economy`, `weather`, …) has **0 beliefs attached** (LEFT JOIN beliefs = 0). The Mind mints watch events with reasoning (Miran successor, June-24 stress test, FOMC, Warsh, NY wind) and dedupes them — but they get no probability, no market mapping, no score; many flip to `dead`.
- **Impact:** the Mind genuinely reasons about the world, but that output is a write-only watchlist. None of it becomes a belief, a market mapping, a proposal, or a scored outcome. The macro/IPO/energy half of the "world signal → trade" thesis is unwired.
- **Fix:** decide the intended path: world-forward watch event → belief (with p) → market mapping (market-back/T4.2) → proposal → score. At minimum, attach a belief + a resolution/scoring path so watch events are evaluated even when no tradeable market exists (the "watchlist-only, scoreable" mode from the Close-The-Loop doc).
- **Confidence:** HIGH (confirmed).

---

## P2 — Correctness / cost / reliability

### F5. Synthesis burns Mind budget on calibration-gated markets
- **Evidence:** `synthesis.rs:174` calls `cycle.run(mind, …)` on every edge `BookSnapshot` with a two-sided book — before any calibration/size gate. Sizing happens later in the harness; with `calibration=0` the proposals size to zero. Mind tests confirm failed/empty/malformed calls still debit (`failed_calls_burn_into_spent_today`). Live DB: all 371 audited `cognition` rows are `triage` degrades; the prior $10/day cap drained in ~8.5h on these no-ops.
- **Impact:** real Anthropic spend produces no tradeable output (and, when cycles fail, no belief either) during cold-start. Raising the budget just burns faster.
- **Fix:** gate the paid `decide()`/cycle behind calibration-readiness (or run triage-only / skip the synthesis tier until a `calibration_params` row exists), while still accruing the cheap belief substrate. Keep the existing empty-quotes guard.
- **Confidence:** HIGH (confirmed code path + audit).
- **Note:** `daily_budget_cents` raised $15→$30 in `config/fortuna.toml` 2026-06-18 (tuning, not a fix).

### F6. Aeolus source: ephemeral tunnel + static date window
- **Evidence:** `config/fortuna.toml [sources.aeolus_*]` URLs are `https://aaa-bloom-acquire-lay.trycloudflare.com/...?from=2026-06-17&to=2026-06-24` — an EPHEMERAL cloudflare quick-tunnel with a hardcoded date window. Config comment itself flags "EPHEMERAL — a stable URL is needed for a long soak."
- **Impact:** when the tunnel rotates/dies or the date window goes stale, weather forecasts stop → weather belief formation stalls. (The abandoned green DB froze at 12:50; the live `fortuna_demo` feed is currently alive to 22:43, but the fragility remains.)
- **Fix:** stable Aeolus URL (not a quick-tunnel) + rolling date window (adapter-injected `from`/`to`, or an endpoint "latest" mode). Add a source-staleness alert.
- **Confidence:** HIGH (confirmed config).

### F7. `mech_structural` pointed at expired brackets
- **Evidence:** `config/fortuna.toml [kalshi].bracket_sets` lists `KXHIGHNY-26JUN16-*` (June 16) while the soak runs June 17–18; live DB shows 0 `mech_structural` proposals.
- **Impact:** the arb arm has no live ladder to scan → contributes nothing. (Also a general problem: dated tickers in config expire daily.)
- **Fix:** ensure `scripts/refresh-demo-markets.sh` rewrites `bracket_sets` (and the perp ladder) to live dated tickers on every boot; verify mech_structural sees a real ladder. Longer-term: discover brackets from the live catalog instead of hardcoding.
- **Confidence:** HIGH (confirmed config + 0 proposals).

### F8. Dead-man pinger failing
- **Evidence:** `daemon.log`: `dead-man ping FAILED: transport failure: error sending request`.
- **Impact:** the I4-adjacent heartbeat to the monitor is down → loss of out-of-band liveness signal during the soak.
- **Fix:** confirm the monitor endpoint is up and the ping target/config is correct; restore the heartbeat; alert on ping failure.
- **Confidence:** MEDIUM (observed in log; root cause not yet traced).

### F9. Category vocabulary is uncontrolled
- **Evidence:** live DB `events.category`: `macro`, `Macro/Fed`, `macro/regulatory`, `macro/personnel`, `macro/monetary-policy`, `monetary_policy`, `economy`, `economics`, `politics`, `weather`, `Weather`, `x`. The Mind invents strings freely.
- **Impact:** breaks any downstream that filters/gates/aggregates by category (discovery allowlist, dashboard grouping, per-category calibration). Splits identical concepts across many spellings.
- **Fix:** controlled vocabulary — enum/allowlist the Mind must map into (validated at ingest, reject/normalize unknowns), or a normalization layer on the discovery output.
- **Confidence:** HIGH (confirmed).

### F10. Uncommitted gate-path work on a dirty tree
- **Evidence:** `git status`: modified `crates/fortuna-runner/src/runner.rs` (canonical-event → gate check 9 per-event exposure via `market_events`/`event_reservations`) + `crates/fortuna-exec/src/manager.rs` (+ tests), uncommitted.
- **Impact:** in-flight, unverified changes to the gate/reservation path. Risk of relying on half-wired gate-9 behavior; lost work if clobbered.
- **Fix:** finish per DoD — tests + `scripts/run-dst.sh` + `scripts/check-protected-invariants.sh` green — then commit. Decide if `require_event_mapping` flips on once mappings exist.
- **Confidence:** HIGH (confirmed).

### F11. Stale `current-demo-db-url` pointer misleads ops/analysis
- **Evidence:** `data/runtime/current-demo-db-url` → `…green_044732` (abandoned, frozen 12:50 UTC), while the live daemon writes to `fortuna_demo` (data to 23:56 UTC). This caused a full mis-analysis during this audit.
- **Impact:** anyone (human or agent) inspecting "the demo DB" reads a dead snapshot.
- **Fix:** make the daemon write the true live `DATABASE_URL` to the pointer on boot (or remove the pointer and read `.env`). Have `demo-launch.sh`/`fortuna status` print the live DB.
- **Confidence:** HIGH (confirmed).

---

## P3 — Cleanup / verify-only

### F12. Verify `fills` table vs the intent journal
- **Evidence:** canonical fills live in `intent_events` as `fill_applied`; the `fills` table reads 0 in every demo DB.
- **Impact:** if ROTA "Recent Fills" / any PnL attribution reads the `fills` table, it shows empty while real fills exist in the journal.
- **Fix:** confirm what populates `fills` and what ROTA/PnL read; either populate `fills` in paper-live or point readers at `intent_events`.
- **Confidence:** MEDIUM (needs a quick code check).

### F13. Verify funding `realized_value` matches the settled rate (NOT a known bug)
- **Evidence:** all resolved scalar beliefs realized = 0, while `funding_rates_historical` has 119 non-zero rates. The resolver reads the historical rate at `(market, funding_time)` (`daemon.rs:3633+`); BTC perp funding is genuinely 0 in many windows (233/352 raw = 0), so this is *probably correct*.
- **Impact:** low — but if the lookup were matching the wrong row, A2d scoring would be silently wrong.
- **Fix:** spot-check 2–3 resolved windows: assert `scalar_beliefs.realized_value` == `funding_rates_historical.funding_rate` at the exact `funding_time`. Verify-only.
- **Confidence:** LOW that it's a bug (likely correct).

### F14. Abandoned demo DBs accumulating
- **Evidence:** several `fortuna_demo_paper_green_2026061704****` snapshots + `fortuna_demo_paper_live`.
- **Impact:** disk + confusion (see F11).
- **Fix:** retention/cleanup of stale green snapshots; document the blue-green/CI harness that creates them.
- **Confidence:** HIGH (confirmed); low severity.

---

## Suggested sequence

1. **F1 settlement** (unlocks realized PnL — the whole point).
2. **F11 pointer + F7 ticker refresh + F6 stable feed** (so the next soak is observable and doesn't rot).
3. **F2 window-dedup scoring** + **F5 calibration-gated mind spend** (stop lying metrics / wasted spend).
4. **F3 weather resolution → calibration** (turn the model arm on, honestly).
5. **F10 commit gate-9**, then **F4 macro watch path**, then P3 cleanup.

Mechanical reality: the only arm trading today is `mech_extremes` (2 fills, KXBTC bin). Wiring F1 gives it a real settled PnL track within days — the first genuine validated number.
