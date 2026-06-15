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
| `KALSHI_API_DEMO_KEY_ID` + `KALSHI_DEMO_PRIVATE_KEY_PATH` | Kalshi **demo runtime** venue credentials (mock funds). Read by `compose_kalshi_runner` / `build_kalshi_demo_transport` when `venue = "kalshi", stage = "paper"`; the demo base URL is a built-in const (`KALSHI_DEMO_BASE_URL`). | **NOT-YET** — required before the Kalshi demo *run* (the boot gate is pure over config and never reads these; an absent key is a Compose error naming the var). |
| `KALSHI_API_KEY_ID` + `KALSHI_PRIVATE_KEY_PATH` | Kalshi **live/prod** venue credentials. | **NOT-YET** — reserved names (`.env.example`); no live path is wired yet (promotion past Paper is the I7 gate). |
| `FORTUNA_KILLSWITCH_KALSHI_API_KEY_ID` + `FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH` + `FORTUNA_KILLSWITCH_KALSHI_BASE_URL` | The kill-switch's **own** Kalshi credential set (I4 — must never share keys with the runtime). The standalone `fortuna-killswitch freeze --venue kalshi` is wired (`7f69b81`) and reads all three; `_BASE_URL` has **no default** (prod vs demo must be explicit). | **NOT-YET** — required before the first live/demo `freeze --venue kalshi`; until set, that path fails closed (exit 4) and only `self-test` runs. |

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
| **Cognition / synthesis / mech-extremes / review** | Their `[cognition]` / `[synthesis]` / `[mech_extremes]` / `[review]` sections (presence composes the strategy; fail-closed when absent). `[cognition]` carries the 3-tier model ids — `synthesis_model` (Opus), `mid_model` (Sonnet, the daily reconciliation), `triage_model` (Haiku, the cheap gate); a `ModelRegistry` maps tier→model. | Optional sections in `boot.rs`/`compose.rs`. |
| **Perp funding-rate belief producer** | A `[funding_forecast]` section (no required fields). Optional `ticker_feed_jsonl = "<path>.jsonl"` replays RECORDED kinetics `ticker` frames as PerpTicks so it fires in a Sim soak. | `Option` in `boot.rs`; **PRESENCE composes** the propose-nothing `FundingForecast`. Absent ⇒ not composed; inert in pure-sim with no feed. |
| **Perp/bracket basis strategy** | A `[perp_event_basis]` section (all fields required: `perp_market`, `fee_floor_dollars`, `min_basis_dollars`, `edge_premium_cents`, and a non-empty `ladder` of `market → { kind = between\|greater\|less, floor_dollars, cap_dollars }`). | `Option` in `boot.rs`; **PRESENCE composes** `PerpEventBasis`, ladder STRICTLY validated by `build_perp_event_basis_config` (`compose.rs`). Absent ⇒ not composed. |
| **Persona analysis step** (ingestion→beliefs) | A `[personas]` section with `enabled = true` plus its `[[personas.persona]]` entries. Reads ingested signals, runs the operator-authored personas, persists `domain_analyses` + beliefs — never orders (I6). | `Option<PersonasSection>` in `boot.rs`; **absent or `enabled = false` ⇒ the step never runs**, daemon byte-identical. A hash/version/status mismatch vs the registry **refuses to boot**. |
| **Discovery step** (ingestion→beliefs) | A `[discovery]` section with `enabled = true` (prefilter knobs `category_allowlist` / `min_volume_contracts` / `min_category_quality`). Drives world-forward (signals→`watch:` events + beliefs) and, when a venue catalog is wired, market-back (catalog→events→auto-confirmed low-stakes edges; high-stakes routed to review). Data-only — never orders (I6); orders still cross the gate (I1). | `Option<DiscoverySection>` in `boot.rs`; **absent or `enabled = false` ⇒ the step never runs**, daemon byte-identical. Market-back is INERT in prod until the Kalshi adapter supplies a catalog (GAPS). |

Sources: `crates/fortuna-live/src/boot.rs` (`IngestionSection { enabled: bool, … }`,
`Option<IngestionSection>`, `Option<FundingForecastSection>`,
`Option<PerpEventBasisSection>`, `Option<PersonasSection>`, `Option<DiscoverySection>`),
`crates/fortuna-live/src/compose.rs`
(`FundingForecastSection`, `PerpEventBasisSection`, `build_perp_event_basis_config`),
`crates/fortuna-live/src/daemon.rs` (the perp-strategy composition + the `drive()`
persona/discovery steps),
`crates/fortuna-sources/src/config.rs` (`SourceConfig`, `SourceKind`, fail-closed
parse), `config/fortuna.example.toml`.

---

## 3. Signatures / approvals — operator-only

| Approval | What it unblocks | Where |
|---|---|---|
| **27-item Kalshi clearance record** | The Kalshi **demo** run. The daemon now **boots** at `venue = "kalshi", stage = "paper"` (mock funds — `boot.rs` `validate_bootable`); every live stage (`live_min`/`scaled`) is still REFUSED at the boot gate (promotion needs the I7 gate). The clearance is the sign-off to point the adapter at the real demo venue for the live demo *run* (runbook: `docs/runbooks/demo-bringup.md`). | Checklist = `docs/research/venue/kalshi-api-2026-06-10/research.md §Uncertainties` (27 items). Fixtures were operator-recorded against the **demo** env 2026-06-11 (`fixtures/kalshi/README.md`); items **#26** (prod-parity re-record) and **#27** (live `GET /exchange/status`) remain **before first live use**. Residue itemized in `GAPS.md` "Operator-blocked: Kalshi fixtures" (T4.2). |
| **Per-track design / build approvals** | A track's next slice. | The operator-decision queue in `docs/reviews/GATE-FINDINGS-LATEST.md` (and `operator-decisions-*.md`). |

Note: fixture *recording* itself is agent work; the **sign-off that the record is
complete enough to point the adapter at a real venue** is the operator action.

---

## 4. Promotion ladder — out-of-band, operator-only

Each rung is a deliberate human step. None auto-advances (I7).

1. **Start the Sim soak** — operator-run release build + start. Runbook:
   `docs/runbooks/soak-start.md`. (GO verdict: `docs/reviews/soak-go-gate-2026-06-12.md`.)
2. **Flip to Kalshi demo (mock funds)** — set `[daemon] venue = "kalshi", stage =
   "paper"` (+ a `[kalshi]` section) *after* the T4.2 clearance (§3). The CODE is
   merged and BOOTS at paper; the live demo *run* is operator-gated on the
   clearance + demo credentials. Umbrella runbook: `docs/runbooks/demo-bringup.md`
   (zero-to-watching-it-run); flip mechanics: `docs/runbooks/demo-flip.md`.
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
   `fortuna-killswitch <freeze|report|self-test|flatten-perps> --journal <path>
   [--venue kalshi]`. No Postgres, no runtime, no Slack dependency by construction.
   `freeze --venue kalshi` (wired `7f69b81`) cancels every open Kalshi order; the
   `flatten-perps` verb (spec 5.15, T5.B8) cancels-all + closes each Kinetics perp
   with a reduce-only IOC through the real perp gate. Both need their own
   `FORTUNA_KILLSWITCH_*` creds (§1); without them they fail closed (exit 4) and
   only `self-test` runs. Runbook: `docs/runbooks/kill-switch-drill.md`.

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
| **Scalar-belief board** (Forecasts + Forecast Feed) | `/api/rota/v1/forecasts` (per-producer CRPS calibration + band coverage) and `/api/rota/v1/forecast_feed` (recent scalar beliefs, each a `<details>` expander with the full quantile fan + the producer's evidence + provenance). Untrusted-data safe (the fan/evidence/provenance are rendered as data, never interpreted). Empty until a scalar producer persists. (`view_forecasts` / `view_forecast_feed`, `crates/fortuna-ops/src/rota.rs`.) |
| **Sources Health board** | `/api/rota/v1/ingest_sources` — per-source health with `domain_tags` (weather\|macro\|…) and `trust_tier`, fed from ingestion telemetry; populated only when the `[ingestion]` loop is on. (`crates/fortuna-live/src/views.rs` "Sources Health".) |
| **Operator-decision queue** | `docs/reviews/GATE-FINDINGS-LATEST.md` — the single coordination surface; gate findings and what each track needs from you. |
