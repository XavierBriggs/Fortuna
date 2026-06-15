# Runbook: demo bring-up — from zero to watching FORTUNA run

**Who this is for:** the operator standing the whole system up in **Kalshi DEMO /
paper** and watching it run end-to-end (signals → beliefs → proposals → gates →
paper fills → scoring), with the dashboard and Slack alerts live.

**What this is:** the **umbrella** sequence. Each step links the detailed runbook
that owns it; this page is the order to do them in and the "are we ready" gate.

**Hard safety frame (unchanged, non-negotiable):** DEMO = mock funds. Live is
REFUSED at boot (`venue = "kalshi", stage = "paper"` composes the demo runner; a
prod/live stage is refused — see [demo-flip.md](demo-flip.md)). No real capital is
ever at risk in this procedure. Profitability is **unproven** — the point of the
demo run is to *measure*, not to earn.

Related: [demo-flip.md](demo-flip.md) · [key-rotation-and-secrets.md](key-rotation-and-secrets.md) ·
[kill-switch-drill.md](kill-switch-drill.md) · [halt-and-rearm.md](halt-and-rearm.md) ·
[rota-local-bringup.md](rota-local-bringup.md) · [ingestion-ops.md](ingestion-ops.md) ·
[soak-start.md](soak-start.md) · [troubleshooting.md](troubleshooting.md)

All commands run from the repo root. Times are UTC.

---

## 0. Readiness gate (do not start the run until ALL are true)

- [ ] Postgres reachable; `fortuna-ledger` migrations applied (the daemon migrates
      on connect at boot — see §3).
- [ ] Release build green: `cargo build --release -p fortuna-live -p fortuna-cli -p fortuna-killswitch`.
- [ ] `config/fortuna.toml` exists (copied from `config/fortuna.example.toml`) with
      the demo venue + the opt-in sections you want ON (§2).
- [ ] `.env` carries every required secret (§2) — secrets are **env-only**, never in
      the repo, config, logs, or audit payloads.
- [ ] The kill switch is reachable and its **revocation file path matches** the
      runtime config (§4) — verify with the drill in §7 BEFORE the run.
- [ ] The verifier's **demo-boot smoke** is green (boots clean against the demo
      config, one full decision cycle, zero invariant violations, kill→revoke→refuse
      →clear→re-arm drill). Ask the verifier to run it; do not start without it.

---

## 1. Build

```
cargo build --release -p fortuna-live -p fortuna-cli -p fortuna-killswitch
```

`fortuna-live` is the daemon; `fortuna-cli` is the operator front-end (status, halt,
re-arm, kill, audit tail); `fortuna-killswitch` is the standalone out-of-band switch
(its own creds; no Postgres/cognition — I4).

## 2. Configure + secrets

**Config** (`config/fortuna.toml`, from `config/fortuna.example.toml`). For a demo
run you want, at minimum:
- `[daemon]` venue/stage for the demo flip (see [demo-flip.md](demo-flip.md));
- `[cognition]` model tiers (synthesis / mid / triage) + budget rails;
- the opt-in **strategy/data** sections whose mere *presence* composes them (absent ⇒
  not composed, fail-closed): `[synthesis]`, `[ingestion] enabled = true`, and
  `[perp_event_basis_v2]` **only if** you want the perps arm (see §6 caveat);
- `[killswitch] revocation_file` (§4);
- `[slack]` channel routing.

**Secrets** (`.env`, see [key-rotation-and-secrets.md](key-rotation-and-secrets.md)):
- `DATABASE_URL` — Postgres.
- `ANTHROPIC_API_KEY` — the real mind. Absent ⇒ the inert StubMind (the daemon
  refuses to boot silently on no-key unless `[cognition] allow_stub_mind = true`).
- `FORTUNA_SLACK_*` — bot token + channel ids.
- Kalshi DEMO creds for the daemon's venue adapter.
- The kill switch's **own** pair: `FORTUNA_KILLSWITCH_KALSHI_*` (and
  `FORTUNA_KILLSWITCH_KINETICS_*` if perps) — separate from the runtime's, so the
  switch works when everything else is dead.

## 3. Postgres up + migrate

Bring Postgres up; the daemon runs the `fortuna-ledger` migrations on connect at
boot. Nightly backups + the restore drill: [backup-restore.md](backup-restore.md).

## 4. Kill switch + I4 revocation wiring (do this BEFORE the run)

The kill switch both **flattens** (cancel open orders + reduce-only IOC closes) **and
revokes** order-placing capability (I4, spec.md:43). Revocation is a durable sentinel
file the switch writes and the daemon reads:

- Set `[killswitch] revocation_file` in `config/fortuna.toml` to the **same path** the
  switch writes — the `KILLSWITCH_REVOKED` sibling of the switch's `--journal`.
- While that file exists, the daemon's halt poller refuses **every** order (the global
  halt; the loop polls before it ticks, so even a restart boots halted).
- Clearing is operator-only and out-of-band: `fortuna-killswitch clear-revocation
  --journal <path>` removes the sentinel; then re-arm (§7). A normal halt re-arm does
  **not** clear it — the sentinel must be gone first.

Full procedure + the monthly test: [kill-switch-drill.md](kill-switch-drill.md).

## 5. Launch the daemon

Start via the operator CLI (status / halt / re-arm / kill all route through it):
`fortuna-cli` with your `config/fortuna.toml`. For the operator-gated start sequence
and restart handling during a sustained run, follow [soak-start.md](soak-start.md)
(the demo run is the Phase-4 soak against demo data rather than Sim).

## 6. Turn on the data so it isn't idle

- **Ingestion** (`[ingestion] enabled = true`): NWS, RSS, calendar, and **Aeolus**
  forecasts flow in as signals. See [ingestion-ops.md](ingestion-ops.md).
- **Aeolus** is the zero-capital `aeolus_eval` scoring vehicle — weather's daily
  resolution makes it the fastest-feedback validation path.
- **Kalshi demo market sync** feeds the binary-event arms (synthesis, mech_structural,
  mech_extremes, weather/F7, funding_forecast).
- **Perps caveat:** the basis-v2 arm is opt-in via `[perp_event_basis_v2]` (default
  OFF). Setting that section now does TWO things: it composes the arm AND spawns the
  LIVE `PerpTick` producer alongside the trading loop (the public, unauthenticated
  Kinetics market + funding GETs — NO credential read; Sim/demo only), which feeds the
  arm live ticks. A healthy boot then prints `fortuna-live: live PerpTick producer
  ACTIVE` ([crates/fortuna-live/src/main.rs](../../crates/fortuna-live/src/main.rs),
  `run_perp_tick_producer`; [crates/fortuna-live/src/perp_tick_producer.rs](../../crates/fortuna-live/src/perp_tick_producer.rs)).
  Leave the section absent if you do not want the perps arm — the binary-event arms run
  regardless.

## 7. Verify you can stop it (drill BEFORE trusting the run)

Run the end-to-end safety drill once before the soak and monthly thereafter:
kill → confirm orders refused (revocation) → `clear-revocation` → re-arm → confirm
flow resumes. Steps: [kill-switch-drill.md](kill-switch-drill.md) +
[halt-and-rearm.md](halt-and-rearm.md). Re-arm and kill reversal are **CLI-only** by
design (a compromised Slack token must not be able to un-halt the system).

## 8. Watch it run

- **ROTA dashboard** — health, the scalar-belief / forecast feed, the §9.2 perps
  board, telemetry, and the audit tail. Bring-up: [rota-local-bringup.md](rota-local-bringup.md).
- **Slack** — `#fortuna-trading` (fills/opens), `#fortuna-alerts` (halts, drawdown
  approaches, reconciliation divergence — halts @-mention you), `#fortuna-review`
  (edge/promotion/lesson items), `#fortuna-digest` (daily/weekly/monthly),
  `#fortuna-ops` (cost, heartbeat, infra). Every Slack message is also an audit row.
- **Dead-man heartbeat** — a missed heartbeat escalates (Slack-delivery failures
  escalate through this path).
- **Audit tail** — `fortuna-cli` surfaces recent `halt` / `gate_decision` / `order`
  rows.

## 9. What "watching it run" should show — and what it won't

**Should show:** the real mind forming beliefs; the calibration layer adjusting them;
the deterministic engine deriving sized candidate orders; the gates passing/rejecting
with reasons; **paper** fills (maker fills count ONLY on trade-through, never on
touch); daily reconciliation; Brier/CLV accumulating per strategy/category; full
telemetry.

**Won't show (yet):** proven edge or profit — that is what the run *measures*, over
weeks. And never live capital (refused by design).

## 10. The exit evidence the run produces (why we do this)

- **Phase 4 exit:** a continuous week with the real mind, no invariant violations.
- **Phase 2 exit:** ≥ 60 resolved scored beliefs, a calibration report, and the
  **Aeolus four-diagnoses verdict** (is it calibrated? edge over market? edge eroded
  by fees? mechanical defect?).
- **Promotion (I7) — separate, later:** Section 11 forward-validation gates (≥ 30
  paper days mechanical / ≥ 60 resolved beliefs synthesis, positive CLV, Brier beating
  baseline, fee/PnL < 0.35, zero invariant violations). Promotion to live capital is
  an operator action, never automatic.

---

**If something looks wrong:** [troubleshooting.md](troubleshooting.md). **If you need
to stop NOW:** §7 — kill, confirm revocation, then decide.
