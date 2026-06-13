# Runbook: flipping the daemon to Kalshi demo mode

**Who this is for:** the operator who will eventually flip the daemon from
the Sim venue to the Kalshi DEMO environment (mock funds).
**When to read it:** now, to know why the flip is currently refused; again
when T4.2 clears, to execute it.
**Status:** accurate as of commit `334612d` (2026-06-12). **The flip's
precondition is NOT met** — this runbook documents a blocked procedure
honestly rather than pretending it is available.

Related: [soak-start.md](soak-start.md) ·
[fixture-recording.md](fixture-recording.md) ·
[kill-switch-drill.md](kill-switch-drill.md)

---

## 1. Current state: the flip is refused at boot, by design

Setting `[daemon] venue = "kalshi"` today makes the daemon refuse to start
with exactly this
([crates/fortuna-live/src/boot.rs](../../crates/fortuna-live/src/boot.rs),
`validate_bootable`):

```
venue kalshi cannot boot: adapter is cleared for Sim development only until
operator fixture clearance completes (GAPS.md Kalshi section; T4.2)
```

This is fail-closed gating, not a bug. The operator directive recorded in
BUILD_PLAN.md is explicit: "demo-mode startup is itself gated on T4.1+T4.2
clearance". T4.1 (the daemon) is done; **T4.2 is not** — its BUILD_PLAN box
is unticked, covering: the Kalshi WS dial (signed handshake, keep-alive,
redial with resubscribe-on-gap), venue-generic recorded-stream replay into
PaperVenue, **kalshi adapter paper/live clearance vs fixtures**, the
kill-switch KalshiVenue plug (`FORTUNA_KILLSWITCH_*` creds), and the Slack
Socket Mode listener (BUILD_PLAN.md, T4.2 POST-FIXTURE tranche). The
clearance residue is itemized in GAPS.md "Operator-blocked: Kalshi
fixtures" ("REMAINING for clearance (T4.2): adapter re-pointed at
recordings + nested-envelope fix; settlement capture …; prod-parity
read-only re-record before live").

Do not edit the gate away. When T4.2's independent gate verdict exists, the
refusal will be removed by that work, not by this runbook.

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

## 3. The flip itself (when T4.2 clears) — REVIEW-VERIFIED, not executable today

**OPERATOR-JUDGMENT** — flipping venue points the daemon at a real external
venue (demo environment, mock funds). Preconditions: the T4.2 clearance
gate verdict exists under `docs/reviews/`; demo credentials confirmed per
§2; the kill-switch KalshiVenue plug landed with its own credential pair
(it is part of the same T4.2 tranche — incident response on a real venue
without it is degraded, see
[kill-switch-drill.md](kill-switch-drill.md)).

1. Edit `config/fortuna.toml` — `[daemon] venue` from `"sim"` to the value
   T4.2 defines for the demo environment (`"kalshi"` is the venue name the
   boot gate reserves today; whether demo-vs-prod selection is a host key
   or a mode key is T4.2's to define — re-verify this step against the
   T4.2 verdict before executing).
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

- The boot refusal in §1 disappears WITHOUT a T4.2 gate verdict in
  `docs/reviews/` → someone weakened the gate; stop and treat as a
  protected-path incident (CLAUDE.md: fail-closed gating is not editable
  convenience).
- Demo behavior diverges from a fixture-confirmed behavior → record it;
  fixtures are demo-recorded and the divergence discipline (spec.md v0.9)
  exists exactly for this; do not "fix" the adapter live.
