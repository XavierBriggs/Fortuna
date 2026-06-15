# Runbook — ROTA local bringup (seeded, no daemon)

Stand the read-only ROTA console up locally against a SEEDED throwaway Postgres,
to see / screenshot-verify the boards with real rows WITHOUT running the daemon
or any trading loop. This is the rig track B uses to verify every board.

For the DAEMON-served ROTA (live Sim/demo data on `:9187`) see
[operations.md §2](../operations.md). This runbook is the standalone harness.

Related: [demo-bringup.md](demo-bringup.md) — the umbrella demo bring-up
sequence; for the demo run you watch the daemon-served ROTA (above), not this
seeded harness.

## Prerequisites
- Local Postgres reachable at `localhost:5432` (the dev default).
- The workspace builds: `cargo build -p fortuna-ops` clean.

## Bring it up
```bash
# 1. One-time: create the throwaway DB (the harness WIPES+SEEDS only this DB).
createdb fortuna_rota_local        # or: psql -c 'CREATE DATABASE fortuna_rota_local'

# 2. Run the harness (connects, migrates, seeds, serves). Defaults to :8799.
cargo run -p fortuna-ops --example rota_local

# 3. Open the console.
open http://127.0.0.1:8799/rota
```
Env overrides: `ROTA_LOCAL_ADDR` (bind address, default `127.0.0.1:8799`),
`ROTA_LOCAL_DATABASE_URL` (default `postgres://localhost/fortuna_rota_local`).

## Safety (read before changing the DB URL)
The harness WRITES (seeds) the database it points at. It therefore reads ONLY
`ROTA_LOCAL_DATABASE_URL` — never the ambient `DATABASE_URL` — and REFUSES any
URL whose database name does not contain `rota_local`. Never point it at real
data or the operator's database.

## What it seeds (representative, never zeros)
One weather event; three beliefs (one resolved-and-scored with a Brier + CLV,
one carrying persona provenance so the cognition expander shows the
`persona_id`/`analysis_id` block); two calibration scopes; five audit rows of
distinct kinds; and a temp perishable dir with a today-dated stream file so the
Streams recorder section reads live. The daemon-shaped scalar boards
(health / money / gates / settlement / streams) are filled from a representative
snapshot mirroring `fortuna-live::views_from`, so the rendered shapes match the
live daemon.

## Screenshot-verify
Open the URL in a browser (or drive it headless via the chrome-devtools / 
playwright MCP) and confirm every board renders real rows. The console
short-polls each panel on load, so the boards populate within ~1s.

## Reset / teardown
```bash
# Stop: Ctrl-C the cargo run.
# For an exactly-representative fresh seed, recreate the DB before a re-run:
dropdb --if-exists fortuna_rota_local && createdb fortuna_rota_local
# Full teardown (remove the throwaway DB):
dropdb --if-exists fortuna_rota_local
```
