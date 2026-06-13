# Operator action list

This is the **operator's** to-do surface: the things that genuinely require a human
(API keys, enable flags, signatures/approvals, promotions, and what to view). The
agents do everything else — code, tests, fixtures, gates, vendor adapters, recordings.
If it isn't on this page, it isn't your job.

Verified against the code/config on 2026-06-13. Every env var name and config key below
was grepped from the source, not assumed.

Legend: **NOW** = needed for today's Sim-only operation · **NOT-YET** = reserved for a
future step (live venue, the Slack listener), provision when you reach it.

---

## 1. Secrets / API keys — env-only, never in repo

All secrets are read from environment variables only. They are never in config TOML,
never committed, redacted in logs/audit. Template: `.env.example` (committed, no
secrets); your real file is `.env` (gitignored, `chmod 600`). Load with
`set -a; source .env; set +a`.

| Env var | Unlocks | When |
|---|---|---|
| `DATABASE_URL` | Postgres (sqlx migrations, repos, audit writer, halt/rearm). | **NOW** — already set in your `.env`; not a to-do. |
| `ANTHROPIC_API_KEY` | Cognition. The key's *presence* is the feature flag: absent ⇒ `StubMind`, present ⇒ `AnthropicMind`. | **NOW** (optional) — Sim runs without it; set to exercise the model. |
| `FORTUNA_SLACK_BOT_TOKEN` (`xoxb-…`) | Slack **outbound** (`chat.postMessage`): trading/alert/review/digest/ops messages. | **NOW** (optional) — paired with the five channel IDs below. |
| `FORTUNA_SLACK_CHANNEL_TRADING` / `_ALERTS` / `_REVIEW` / `_DIGEST` / `_OPS` | The five channel **IDs** (`C…`, not display names) the router posts to. | **NOW** (optional) — required iff Slack is used. |
| `FORTUNA_DEADMAN_URL` | External dead-man monitor ping (the system can't report its own death). | **NOW** (optional) — off-box monitor URL. |
| Slack **app-level token** (`xapp-…`, scope `connections:write`) | Socket Mode **inbound** listener — button clicks / approvals over WebSocket. | **NOT-YET — pending track-A Slack listener.** Not implemented (`slack.rs` is send-only; the listener is later-phase per BUILD_PLAN). No env var exists in code yet. |
| `AEOLUS_API_TOKEN` | The Aeolus weather-forecast vendor source (`x-api-key` auth header). Wired via the `[sources.<id>] auth_env = "AEOLUS_API_TOKEN"` key; the factory's `secret_resolver` reads it from env. | **NOT-YET** — only when you enable an `aeolus` source. |
| `KALSHI_API_KEY_ID` + `KALSHI_PRIVATE_KEY_PATH` | Kalshi **runtime/trading** venue credentials. | **NOT-YET** — required before any live/demo Kalshi venue connection. Reserved names; no live path yet. |
| `FORTUNA_KILLSWITCH_KALSHI_API_KEY_ID` + `FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH` | The kill-switch's **own** Kalshi credential pair (I4 — must never share keys with the runtime). | **NOT-YET** — required before any live/demo Kalshi venue connection. |

Sources: `crates/fortuna-ops/src/config.rs` (`ENV_SLACK_BOT_TOKEN`, `ENV_DEADMAN_URL`,
`ENV_DATABASE_URL`, `ENV_SLACK_CHANNEL_PREFIX`), `crates/fortuna-sources/src/factory.rs`
+ `config.rs` (`auth_env`/`auth_header`, `AEOLUS_API_TOKEN`),
`crates/fortuna-killswitch/src/main.rs` (`FORTUNA_KILLSWITCH_*`), `.env.example`,
`docs/research/ops/slack-api-2026-06-09/research.md` (the `xapp-` Socket Mode contract).

---

## 2. Enable flags — operator config in `config/fortuna.toml`

The system is **default-off / dormant** for anything optional. Copy the example and edit:
`cp -n config/fortuna.example.toml config/fortuna.toml`, then `fortuna config check`.
The example config ships **no** `[ingestion]` and **no** `[sources.*]` sections — so out
of the box, zero ingestion runs.

| To enable | Add / set | Default-off mechanism |
|---|---|---|
| **News/weather ingestion loop** | `[ingestion]` section with `enabled = true` (also requires `user_agent`). | The whole section is `Option` in `boot.rs`; **absent section or `enabled = false` ⇒ no ingestion**, daemon byte-unchanged. (`enabled` is a required field *within* the section — there is no implicit default, so you must write it.) |
| **A specific data source** | A `[sources.<id>]` block (e.g. `kind`, `url`, `base_interval`, `rate_budget_per_min`, `enabled = true`, optional `auth_header`/`auth_env`). | Parsed by `fortuna-sources` `SourcesConfig`; fail-closed — unknown kinds/fields, non-https URLs, and Phase-A-unbuildable kinds are hard errors. A source defaults disabled unless `enabled = true`. |
| **Cognition / synthesis / mech-extremes / review** | Their `[cognition]` / `[synthesis]` / `[mech_extremes]` / `[review]` sections (presence composes the strategy; fail-closed when absent). | Optional sections in `boot.rs`/`compose.rs`. |

Sources: `crates/fortuna-live/src/boot.rs` (`IngestionSection { enabled: bool, … }`,
`Option<IngestionSection>`), `crates/fortuna-sources/src/config.rs` (`SourceConfig`,
`SourceKind`, fail-closed parse), `config/fortuna.example.toml`.

---

## 3. Signatures / approvals — operator-only

| Approval | What it unblocks | Where |
|---|---|---|
| **27-item Kalshi clearance record** | `venue = "kalshi"`. Today the daemon **refuses to boot** with `venue = "kalshi"` (`boot.rs` `validate_bootable`: "cleared for Sim development only"). | Checklist = `docs/research/venue/kalshi-api-2026-06-10/research.md §Uncertainties` (27 items). Fixtures were operator-recorded against the **demo** env 2026-06-11 (`fixtures/kalshi/README.md`); items **#26** (prod-parity re-record) and **#27** (live `GET /exchange/status`) remain **before first live use**. Residue itemized in `GAPS.md` "Operator-blocked: Kalshi fixtures" (T4.2). |
| **Per-track design / build approvals** | A track's next slice. | The operator-decision queue in `docs/reviews/GATE-FINDINGS-LATEST.md` (and `operator-decisions-*.md`). |

Note: fixture *recording* itself is agent work; the **sign-off that the record is
complete enough to point the adapter at a real venue** is the operator action.

---

## 4. Promotion ladder — out-of-band, operator-only

Each rung is a deliberate human step. None auto-advances (I7).

1. **Start the Sim soak** — operator-run release build + start. Runbook:
   `docs/runbooks/soak-start.md`. (GO verdict: `docs/reviews/soak-go-gate-2026-06-12.md`.)
2. **Flip to Kalshi demo (mock funds)** — set `[daemon] venue = "kalshi"` *after* the
   T4.2 clearance (§3). Currently boot-refused. Runbook: `docs/runbooks/demo-flip.md`.
3. **I7 forward validation** — a strategy passes its forward-validation gate before it
   touches live capital. Operator judgement on the gate.
4. **Scale to live capital** — deliberate operator step after demo + validation.
5. **Re-arm after a drawdown halt (I2)** — **CLI-only, out-of-band**:
   `fortuna rearm <global|strategy:<id>|venue:<id>> --reason "…" --operator <name>`.
   Requires `DATABASE_URL`; both `--reason` and `--operator` are mandatory. A re-arm
   clears the durable ledger halt but is **restart-gated** — the CLI tells you the exact
   restart command. Runbook: `docs/runbooks/halt-and-rearm.md`.
   (`crates/fortuna-cli/src/main.rs`.)
6. **Out-of-band kill switch (I4)** — `fortuna kill [--flatten] [--journal <path>]`, or
   the standalone binary directly:
   `fortuna-killswitch <freeze|report|self-test> --journal <path> [--venue kalshi]`.
   No Postgres, no runtime, no Slack dependency by construction. Live venue freeze needs
   the `FORTUNA_KILLSWITCH_*` creds (§1); until a live adapter is wired only `self-test`
   runs. Runbook: `docs/runbooks/kill-switch-drill.md`.

---

## 5. Infra the operator controls

- **Free machine disk** — the build throttle. Disk pressure is why full
  `test --workspace` + DST runs are deferred to warm-target checks; keep headroom.
- **Do NOT kill the running recorder** (or the running daemon mid-soak) — let it own
  `target/`; killing it corrupts in-flight state.
- **Local Postgres** — must be running on localhost with a role that can create
  databases (`DATABASE_URL`, default `postgres://localhost/fortuna_dev`). Tests spin up
  their own DBs; they never touch an operator database.

---

## 6. What to view

| View | How |
|---|---|
| **ROTA console** (read-only, gold-on-black) | The daemon serves it on the metrics listener: `[daemon] metrics_bind = "127.0.0.1:9187"` ⇒ open `http://127.0.0.1:9187/rota`. Versioned JSON under `http://127.0.0.1:9187/api/rota/v1/`; Prometheus text at `/metrics`. Every route is GET-only. (`docs/operations.md` §2, `docs/quickstart.md`, `crates/fortuna-ops/src/rota.rs`.) |
| **Operator-decision queue** | `docs/reviews/GATE-FINDINGS-LATEST.md` — the single coordination surface; gate findings and what each track needs from you. |
