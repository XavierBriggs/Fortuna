# Quickstart — zero to a running Sim daemon + ROTA

**Who this is for:** anyone with a fresh checkout who wants the FORTUNA daemon
running locally on the Sim venue, the ROTA console in a browser, and the test
battery green. Read it once, in order. Live trading is not part of this
document — this quickstart uses the Sim venue; the Kalshi DEMO also boots
(`venue="kalshi", stage="paper"` + a `[kalshi]` section), with its live run
operator-gated. To stand the whole system up in Kalshi demo and watch it run
end-to-end, follow the umbrella runbook
[runbooks/demo-bringup.md](runbooks/demo-bringup.md) (flip mechanics:
[runbooks/demo-flip.md](runbooks/demo-flip.md))
([config/fortuna.example.toml](../config/fortuna.example.toml) `[daemon]`);
the live path is [FINAL_REPORT.md](../FINAL_REPORT.md) §6.

Steps marked **OPERATOR-RUN** start a long-running process and require
operator-provisioned secrets; their contract is the soak GO verdict
([docs/reviews/soak-go-gate-2026-06-12.md](reviews/soak-go-gate-2026-06-12.md)).
Everything else has been executed as written.

## 1. Prerequisites

- **Rust** via rustup. The toolchain is pinned by
  [rust-toolchain.toml](../rust-toolchain.toml) (stable + rustfmt + clippy);
  rustup picks it up automatically.
- **PostgreSQL** running on localhost, with a role that can create databases.
  The workspace default `DATABASE_URL` is `postgres://localhost/fortuna_dev`
  ([.cargo/config.toml](../.cargo/config.toml)); migrations run automatically
  at daemon boot, and `sqlx` tests create throwaway `_sqlx_test_*` databases
  on that server — tests never touch an operator database.

Create the dev database (no-op if it already exists):

```sh
createdb fortuna_dev || true
```

## 2. Build

From the repo root:

```sh
cargo build --release -p fortuna-live -p fortuna-cli
```

This produces `target/release/fortuna-live` (the daemon) and
`target/release/fortuna` (the operator lifecycle CLI). For a debug-build dev
loop, `cargo run -p fortuna-live -- config/fortuna.toml` is equivalent to
running the daemon binary directly.

## 3. Config

The committed example is the whole config shape — every key is commented in
place ([config/fortuna.example.toml](../config/fortuna.example.toml)). The
real file is operator-local and gitignored; it carries **no secrets**
(secrets are env-only, below).

```sh
[ -f config/fortuna.toml ] || cp config/fortuna.example.toml config/fortuna.toml
```

What matters for a first boot:

- `[daemon]` — `venue = "sim"` for this quickstart (`kalshi` also boots at
  `stage="paper"` with a `[kalshi]` section — its live run is operator-gated),
  `metrics_bind = "127.0.0.1:9187"`,
  `tick_interval_ms`, `halt_poll_ms`.
- `[cognition]` — `allow_stub_mind = false` by default: booting **without**
  `ANTHROPIC_API_KEY` is a hard refusal unless you set
  `allow_stub_mind = true` to opt into the inert stub mind explicitly
  (fail-closed boot; [crates/fortuna-live/src/main.rs](../crates/fortuna-live/src/main.rs)).
  Budgets (`daily_budget_cents`, `per_cycle_budget_cents`) bind the live mind.
- `[sim]` — the Sim venue's market world (required when `venue = "sim"`).
- `[gates.*]`, `[envelopes]`, `[fees.*]`, `[review]`, `[sizing]`, `[slack]`,
  `[deadman]` — risk limits, capital, fee schedules, review thresholds; the
  example's comments cite the spec section and research doc for each.

## 4. The `.env` contract

Secrets come from the environment only — never the repo, config, logs, or
audit payloads ([CLAUDE.md](../CLAUDE.md)). Create your env file from the
committed template (never overwrites an existing one):

```sh
[ -f .env ] || cp .env.example .env
```

Then replace every placeholder. The daemon's boot validation
(`validate_env`, [crates/fortuna-live/src/boot.rs](../crates/fortuna-live/src/boot.rs))
refuses missing **and** placeholder values, naming only the offending
variable — never its value.

| Variable | Required | What it does |
|---|---|---|
| `DATABASE_URL` | yes | Postgres for repos, migrations, and the append-only audit writer. |
| `FORTUNA_SLACK_BOT_TOKEN` | yes | Slack bot token for outbound routing (every message is also an audit row). |
| `FORTUNA_SLACK_CHANNEL_TRADING` | yes | Channel ID (C…, not a display name) for trade notices. |
| `FORTUNA_SLACK_CHANNEL_ALERTS` | yes | Channel ID for halt/runaway alerts. |
| `FORTUNA_SLACK_CHANNEL_REVIEW` | yes | Channel ID for lesson candidates. |
| `FORTUNA_SLACK_CHANNEL_DIGEST` | yes | Channel ID for the daily digest + weekly review summary. |
| `FORTUNA_SLACK_CHANNEL_OPS` | yes | Channel ID for degrade/ops messages and operator drills. |
| `FORTUNA_DEADMAN_URL` | yes | External dead-man monitor; the daemon pings it every minute (the system cannot report its own death). |
| `ANTHROPIC_API_KEY` | optional | The cognition feature flag: present ⇒ live AnthropicMind under config budgets; absent ⇒ stub mind, only if `[cognition] allow_stub_mind = true`. |

The reserved live-path Kalshi names (`KALSHI_*`) and the kill-switch's own set
(`FORTUNA_KILLSWITCH_*`) are documented in [.env.example](../.env.example). The
Kalshi **demo** run additionally reads `KALSHI_API_DEMO_KEY_ID` /
`KALSHI_DEMO_PRIVATE_KEY_PATH` (the demo compose path,
[crates/fortuna-live/src/daemon.rs](../crates/fortuna-live/src/daemon.rs)); the
[demo-bringup runbook](runbooks/demo-bringup.md) is the secrets-and-bring-up
procedure. Nothing in this Sim quickstart reads any of them.

Load the file into your shell without committing anything:

```sh
set -a && source .env && set +a
```

## 5. Run — OPERATOR-RUN

Per the verified start contract in
[docs/reviews/soak-go-gate-2026-06-12.md](reviews/soak-go-gate-2026-06-12.md)
and [FINAL_REPORT.md](../FINAL_REPORT.md) §5. From the repo root, pick one:

```sh
# Foreground (simplest to watch; Ctrl-C stops it cleanly):
./target/release/fortuna start --foreground

# Managed (detached; pidfiles under data/runtime/; also starts the
# perishable-data recorder; a second start is a clean no-op):
./target/release/fortuna start

# Raw binary (equivalent to --foreground):
./target/release/fortuna-live config/fortuna.toml
```

Note: a managed `fortuna start` REFUSES if an unmanaged `fortuna-recorder`
process is already running — by design, so it never adopts or duplicates a
writer ([BUILD_PLAN.md](../BUILD_PLAN.md) T4.4). Stop the unmanaged recorder
first, or run `--foreground` (which manages nothing).

## 6. What you should see

Boot lines on stderr, in order (exact strings from
[crates/fortuna-live/src/main.rs](../crates/fortuna-live/src/main.rs)):

```
fortuna-live: synthesis mind = StubMind (no ANTHROPIC_API_KEY; inert)
fortuna-live: composed (venue=sim, markets from [sim], journal+audit in Postgres)
fortuna-live: metrics at http://127.0.0.1:9187
fortuna-live: dead-man heartbeat armed (pings the monitor every interval)
fortuna-live: Slack routing active (alerts -> #fortuna-alerts/#fortuna-ops)
```

(With a real key the first line reads
`synthesis mind = AnthropicMind (live; model from [cognition])`.)

Then:

- **Metrics** (GET-only Prometheus text):
  `curl -s http://127.0.0.1:9187/metrics | head`
- **ROTA** — open `http://127.0.0.1:9187/rota`. The gold/black read-only
  console with seven panels: Health/Wheel, Money, Gates, Cognition,
  Settlement/Watchdogs, Venue/Streams, and the audit tail. Panels render
  honest `unavailable`/null states rather than fabricated zeros; an active
  halt takes over the whole screen. (Design:
  [docs/design/rota-dashboard.md](design/rota-dashboard.md) §4; browser
  acceptance: the R12 pass in
  [docs/reviews/GATE-FINDINGS-LATEST.md](reviews/GATE-FINDINGS-LATEST.md).)
- **Versioned JSON views** under `http://127.0.0.1:9187/api/rota/v1/`
  (read-only; every mutating method is rejected).

## 7. Stop

```sh
./target/release/fortuna stop
```

SIGTERM to the daemon then the recorder — never SIGKILL. Success requires the
daemon's `fortuna-live: clean shutdown` line in the managed log *after* the
signal; process exit alone is not success
([crates/fortuna-cli/src/main.rs](../crates/fortuna-cli/src/main.rs), A1). A
foreground daemon stops on SIGTERM or Ctrl-C — both run the same graceful
shutdown: cancel working orders, write a final audit row (the T4.1 shutdown
contract, [BUILD_PLAN.md](../BUILD_PLAN.md)).

## 8. The test battery + DST

The commit-gate battery ([CLAUDE.md](../CLAUDE.md) definition of done). All
four were executed as written for this document; Postgres from step 1 must be
up:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/run-dst.sh
```

`scripts/run-dst.sh [N]` replays the committed regression corpus, then runs N
randomized seeds (default 2000) through four stages: the core DST world, the
synthesis decision-loop chaos stage, the settlement/watchdog chaos stage, and
the daemon-composition smoke. Verification gates run it at 10000
(`scripts/run-dst.sh 10000`). Any invariant violation prints the offending
seed and fails the run; reproduce one with `scripts/replay.sh --seed <N>`.
The doctrine behind all of this is [docs/verification.md](verification.md).

## Where next

- Operate it day to day (CLI, ROTA tour, rhythm): [docs/operations.md](operations.md)
- Procedures (soak start, halt/re-arm, kill-switch drill, troubleshooting):
  [docs/runbooks/](runbooks/)
- What the system actually is: [docs/architecture.md](architecture.md),
  then [docs/spec.md](spec.md)
