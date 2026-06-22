# WS4 Demo Surface — Operator Runbook

**Purpose:** End-to-end walkthrough for the WS4 paper-demo head-to-head: Aeolus
forecast beliefs vs meteorologist persona beliefs, with the live `/api/rota/v1/chain`
surface showing both producers' CLV, Brier, and the GO/NO-GO validation verdict.

**Honesty:** The `validate` step produces a Brier-primary GO/NO-GO verdict derived
honestly from the replayed archive. On a thin archive (< 30 resolved periods per
scope after the G-PIT leak guard) `validate` returns `Insufficient` — this is the
correct, expected result for a fresh demo install. The verdict becomes `Go` or `NoGo`
only when the archive has enough resolved history. **Never overclaim** — `Insufficient`
is not a failure; it is the gate refusing to fabricate a verdict from thin data.

---

## Prerequisites

- Rust toolchain installed, workspace builds clean (`cargo build --workspace`).
- PostgreSQL reachable at the superuser socket (`/tmp`, or set `DATABASE_URL`).
- Aeolus stable source: the `FORTUNA_WS3_ARCHIVE` env var points at a bounded,
  read-only SQLite slice of the Aeolus production archive (`aeolus_kalshi.db`).
  The WS3 live smoke can also build a fixture DB from the committed SQL fixture if
  `FORTUNA_WS3_ARCHIVE` is unset; the fixture yields `Insufficient` (it is small
  by design — real GO/NoGo requires real resolved history).
- Required env vars set (see `config/fortuna.example.toml` for key names):
  `DATABASE_URL`, `FORTUNA_SLACK_BOT_TOKEN`, `FORTUNA_DEADMAN_URL`, the five
  `FORTUNA_SLACK_CHANNEL_*` vars, and `ANTHROPIC_API_KEY` (needed for the
  meteorologist persona). Production Kalshi credentials are NOT required for
  paper-demo mode: `KALSHI_API_KEY_ID` + `KALSHI_PRIVATE_KEY_PATH` are only
  needed when `data_source = "kalshi_prod"` is set.

---

## Step 1 — Health check (`fortuna doctor`)

```bash
fortuna doctor --offline
```

Expected output: all checks green (`READY`). The `--offline` flag skips the
live Kalshi reachability check for CI-determinism; omit it in a real demo to
verify the Kalshi endpoint is up.

Exit code 0 = READY. Any non-zero exit means a required credential or migration
is missing — inspect the failed check and fix before proceeding.

---

## Step 2 — Seed the archive (`fortuna backtest aeolus-archive`)

```bash
export FORTUNA_WS3_ARCHIVE=/path/to/aeolus_kalshi.db   # or use the fixture
export DATABASE_URL=postgres:///fortuna_demo?host=/tmp  # or your demo DB

fortuna backtest aeolus-archive --from 2026-01-01 --to 2026-12-31
```

Expected output: `written=N beliefs` (N > 0 on a real archive; 0 on a
re-run — idempotent). The command imports Aeolus historical beliefs from the
bounded archive slice into the `beliefs` table with `provenance.source =
"historical-import"`, ready for `validate` to score.

Aeolus stable-source note: point `FORTUNA_WS3_ARCHIVE` at a **read-only,
bounded export** of the Aeolus production `aeolus_kalshi.db` (e.g., a SQLite
VACUUM COPY of the slice `2026-01-01..2026-06-15`). The import is idempotent
(duplicate belief IDs are skipped via `ON CONFLICT DO NOTHING`), so re-running
with the same archive is safe.

---

## Step 3 — Validate (`fortuna validate`)

```bash
fortuna validate --scope forecast:KNYC --producer historical-import
```

Expected output on a real archive with >= 30 resolved periods:

```
verdict: Go    brier_edge=+0.18  brier_pbo=0.00  brier_spa_p=0.002  effective_n=48
```

OR (insufficient resolved history — correct and expected on a thin archive):

```
verdict: Insufficient   effective_n=12   (need >= 30 resolved periods)
```

**Honesty (operator contract):** Do NOT re-run validate with loosened parameters
to get a Go verdict. `Insufficient` means the archive has not yet accumulated
enough resolved, leak-free history to power the Brier test. The right response
is to wait for more resolved markets, or import a larger bounded archive slice.
A `NoGo` verdict means the model shows no skill over the market baseline on the
validated scope — honest and correct per the Brier gate.

The validate gate is Brier-primary and conjunctive (all of: `effective_n >= 30`,
`brier_edge > 0`, `brier_pbo <= 0.05`, `brier_spa_p < 0.05`). CLV is reported
as a corroborating axis only and cannot create a `Go`.

---

## Step 4 — Start the paper demo (`fortuna start paper-demo`)

```bash
fortuna start paper-demo
```

This asserts `execution_mode = paper_ledger` in the resolved config (hard fail
if not paper-safe — see `doctor` mode_safe check). It starts the daemon against
the live Kalshi data feed with local paper execution:

- Discovery wiring: `world_forward_discovery` synthesizes `watch:` events from
  live Kalshi market data + signals, fans scoreable candidates to beliefs
  (attributed `world-forward`, propose-only, I6/I7).
- Personas wiring: the meteorologist persona runs `link_persona_market_edges`
  at belief formation — inserting the persona event → the shared Aeolus market
  edge so the CLV resolver can compute `clv_bps` for the meteorologist.
- Both producers mint disjoint edge IDs: discovery uses `01EDG*`, personas
  `01EDP*` — no PK collision on co-run (W6a fix, confirmed by the ws4 live smoke).

The daemon writes a DB pointer to `data/runtime/current-demo-db-url` on boot
(via `maybe_write_demo_db_pointer`, gated on `paper_ledger` mode).

---

## Step 5 — Query the chain (`GET /api/rota/v1/chain`)

```bash
curl "http://localhost:3000/api/rota/v1/chain?event=<event_linkage>"
```

Where `<event_linkage>` is an event URL that has a belief from BOTH Aeolus and
the meteorologist — e.g., a resolved NYC temperature bracket. The ROTA server
must be running (it starts as part of `fortuna start paper-demo`).

Expected response: a `ChainView` JSON object with:
- `producers`: two entries — `{"producer": "aeolus", "clv_bps": <N>, "brier": <B>}`
  and `{"producer": "meteorologist", "clv_bps": <N>, "brier": <B>}`.
- `validation.verdict`: the GO surface from `fortuna validate` (or `null` if
  validate has not yet been run for this scope).
- `safety.execution_mode`: `paper_ledger` (confirms paper-safe mode).

The meteorologist `clv_bps` should equal Aeolus's `clv_bps` (market-level drift —
both producers share the same Kalshi market; the CLV benchmark is the same shared
book mid). This is honest and documented: Brier, not CLV, is the per-producer
differentiator. See GAPS.md "p_cal always Some" for the UI honesty note.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `doctor` reports mode_safe RED | `execution_mode` is not `paper_ledger` in config | Set `execution_mode = "paper_ledger"` + `orders_enabled = false` in `[runtime]` |
| `backtest` writes 0 beliefs | Archive already imported (idempotent) | Normal — re-run skips duplicates |
| `validate` returns `Insufficient` | < 30 resolved periods in the scope | Use a larger bounded archive slice or wait for more resolved markets |
| Meteorologist `clv_bps = null` | Edge ID collision from old run | Clear the demo DB and restart (the `01EDP` prefix fix prevents future collisions) |
| `/chain` returns 404 | Event linkage not in DB | Verify the event was imported by `backtest`; check the `events` table |
| ROTA server not responding | Daemon not started or crashed | Check `fortuna doctor` + daemon logs |
