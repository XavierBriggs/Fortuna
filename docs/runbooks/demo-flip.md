# Runbook: flipping the daemon to Kalshi demo mode

**Who this is for:** the operator who will eventually flip the daemon from
the Sim venue to the Kalshi DEMO environment (mock funds).
**When to read it:** now — the demo-flip CODE is merged and the daemon BOOTS
at Kalshi/paper; this is how you execute the live demo run (still gated on
operator preconditions, §1).
**Status:** the Kalshi demo-flip (Phase 1+2) is MERGED (main @ `0586bab`,
2026-06-14): `venue = "kalshi", stage = "paper"` + a `[kalshi]` section composes
the Kalshi demo runner at `Stage::Paper`. The CODE is no longer refused; the
LIVE RUN is operator-gated — see §1.

Related: [demo-bringup.md](demo-bringup.md) ·
[soak-start.md](soak-start.md) ·
[fixture-recording.md](fixture-recording.md) ·
[kill-switch-drill.md](kill-switch-drill.md)

---

## 1. Current state: the flip BOOTS at paper; the LIVE RUN is operator-gated

`[daemon] venue = "kalshi"` + `stage = "paper"` + a non-empty `[kalshi]`
section now COMPOSES the Kalshi demo runner at `Stage::Paper`
([crates/fortuna-live/src/boot.rs](../../crates/fortuna-live/src/boot.rs),
`validate_bootable`; merged @`0586bab`). The boot gate refuses ONLY:

- `venue=kalshi` at `stage=sim` — a mis-wiring (the Sim world is `venue=sim`);
- `venue=kalshi` at `stage=live_min`/`scaled` — I7: promotion past Paper needs
  the forward-validation gate (a human action); the daemon never auto-promotes;
- `venue=kalshi, stage=paper` with an empty/absent `[kalshi].series`.

So the CODE is ready. What still gates the LIVE demo RUN (per
[kalshi-demo-flip.md](../design/kalshi-demo-flip.md) §"Operator-blocked"):

1. **Demo credentials** in `.env` — `KALSHI_API_DEMO_KEY_ID` +
   `KALSHI_DEMO_PRIVATE_KEY_PATH` (the PEM: chmod 600, gitignored, outside the
   repo; confirm it is the ROTATED key post the 2026-06-11 incident).
2. **T4.2 fixture clearance** — the 27-item Kalshi checklist (GAPS) closes via
   the operator recording session before running against the real demo API.
3. **`[kalshi].series`** tickers, from a demo-account inspection.

Do not edit the boot gate. If a NON-paper stage ever boots for `venue=kalshi`
(or `kalshi`+`paper` boots with an empty `[kalshi]`), the I7 gate was weakened
— stop and treat it as a protected-path incident.

## 2. What is already prepared

The config side is ready — commit `304f746` ("prepare the demo config")
closed the gaps in [config/fortuna.example.toml](../../config/fortuna.example.toml):

- `[cognition] synthesis_model` — the field name now matches the example
  config (previously the daemon silently dropped the operator's model
  choice to the default; fixed in that commit).
- `[envelopes] synthesis_cents = 200_000` — capital for the opt-in
  synthesis arm.
- `[gates.per_strategy.synthesis]` — without it a composed synthesis order
  is gate-rejected fail-closed (I1).

Demo credentials are an independent precondition: confirm the configured
key id is the DEMO-environment key and `KALSHI_PRIVATE_KEY_PATH` points at
its PEM, chmod 600, outside the repo (GAPS.md "Operator-blocked:
credentials"; [key-rotation-and-secrets.md](key-rotation-and-secrets.md)).

## 3. Executing the live demo run (code-ready; needs the §1 operator preconditions)

**OPERATOR-JUDGMENT** — flipping venue points the daemon at a real external
venue (the Kalshi DEMO environment, mock funds). Preconditions (§1): demo
credentials in `.env`; the 27-item T4.2 fixture clearance closed; `[kalshi].series`
set; the kill-switch KalshiVenue plug is wired (LIVE `freeze --venue kalshi` is
built — only the operator-run live exercise remains, see
[kill-switch-drill.md](kill-switch-drill.md)).

1. Edit `config/fortuna.toml` — set `[daemon] venue = "kalshi"`, `stage = "paper"`,
   and add a `[kalshi]` section with your demo `series` tickers (the boot gate
   requires all three). Demo-vs-prod is selected by the env credentials + the
   Kalshi base URL the transport reads — the demo creds/URL point at the demo
   environment.
2. Check it parses and passes the boot rules:
   `./target/release/fortuna config check`
3. Restart — config changes are RESTART-GATED:
   `./target/release/fortuna stop && ./target/release/fortuna start`
4. Verify: `./target/release/fortuna status` shows
   `config on disk: venue=kalshi …`, ROTA Health shows the venue row, and
   the boot log shows a kalshi composition line (the current sim line
   `composed (venue=sim, …)` must be GONE).

Why a restart and not a live mode switch: the CLI deliberately has no
`mode` command — `status` prints "config on disk: … (daemon may differ
until restart)" instead, because the running daemon reads its config once
at boot and a live-mutating mode verb was cut from the design
([docs/design/fortuna-cli.md](../design/fortuna-cli.md), amendment A6).
The restart is the same unambiguous human act that gates re-arms
([halt-and-rearm.md](halt-and-rearm.md)).

## 4. What changes, and what stays as it is

Changes with the flip:

- The venue adapter: real signed traffic against the Kalshi DEMO hosts,
  mock funds, real order lifecycles.
- Risk-parameter and fee values get read from the TARGET environment at
  runtime — never baked from fixtures (spec.md v0.9, "Demo/prod divergence
  discipline": demo runs different tickers, different risk parameters, and
  a newer API build than production).

Stays exactly as it is:

- **Every invariant.** The gates (I1), drawdown halts (I2), rate limits
  (I3), kill switch (I4), audit (I5), propose-only model (I6) are
  venue-independent by construction.
- **I7: demo is still PRE-promotion.** Mock funds are not live capital, and
  passing a demo soak promotes nothing by itself — "No strategy touches
  live capital without passing its forward validation gate" (CLAUDE.md
  I7), and promotions remain operator actions (BUILD_PLAN operator
  directive: "promotions, re-arms, live capital stay with the operator").
- The `[sim]` bracket-set world stays in the config for sim runs; it is
  simply unused by a kalshi-venue boot (boot.rs requires `[sim]` only when
  `venue = "sim"`).
- Live/prod remains separately gated behind the prod-parity re-record and
  the rest of the clearance list (GAPS.md Kalshi section) — the demo flip
  is not a step toward skipping any of it.

## When to stop and escalate

- A NON-paper stage (`live_min`/`scaled`) boots for `venue=kalshi`, or
  `kalshi`+`paper` boots with an empty `[kalshi]` → the I7 boot gate was
  weakened; stop and treat as a protected-path incident (CLAUDE.md: fail-closed
  gating is not editable convenience).
- Demo behavior diverges from a fixture-confirmed behavior → record it;
  fixtures are demo-recorded and the divergence discipline (spec.md v0.9)
  exists exactly for this; do not "fix" the adapter live.
