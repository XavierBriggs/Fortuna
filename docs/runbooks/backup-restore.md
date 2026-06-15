# Runbook: backup and restore

**Who this is for:** the operator deciding what must survive a disk failure,
and anyone about to "clean up" the data directory.
**When to read it:** before the soak starts (know what you would lose), and
at the monthly restore drill.
**Status:** accurate as of commit `334612d` (2026-06-12). Be warned up
front: **there is no automated backup yet** — see §4 before assuming
otherwise.

Related: [troubleshooting.md](troubleshooting.md) ·
[key-rotation-and-secrets.md](key-rotation-and-secrets.md)

---

## 1. What the persistent state actually is

Three things on this machine hold state that cannot be regenerated:

1. **The Postgres ledger** — the database `DATABASE_URL` names (the
   `fortuna` database, 23 relations owned by `fortuna_app` — GAPS.md
   "Operator-blocked: credentials"). This is THE system of record: the
   append-only `audit` trail (I5), `halt_events` (I2 state),
   `intent_events`/`exec_cursors`/`fills` (execution mirror),
   `settlement_entries`/`discrepancies`(+`_resolutions`),
   `beliefs`/`journal`/`lessons`/`calibration_params` (cognition memory),
   `reservation_events` (capital envelopes), `events`/`market_event_edges`,
   `market_snapshots`/`price_snapshots`/`signals`/`source_registry`, and
   `tradability_scores` — the full list is the two migration files in
   [crates/fortuna-ledger/migrations/](../../crates/fortuna-ledger/migrations/).
   Schema is NOT a backup concern (migrations recreate it; `connect()`
   auto-migrates — ledger lib.rs); the ROWS are.
2. **`data/perishable/`** — the recorder's B0 market-data dataset
   (~4.6GB as of 2026-06-12, one directory per UTC day). It is
   append-only JSONL captured live and therefore UNRECOVERABLE if lost —
   the CLI code calls it "the sacred B0 dataset"
   ([crates/fortuna-cli/src/main.rs](../../crates/fortuna-cli/src/main.rs))
   and the verifier protocol pins "the perishable dataset is NEVER touched"
   ([docs/design/orchestration.md](../design/orchestration.md) §4b).
   **Never delete it**, including under disk pressure — delete build
   `target/` dirs instead ([troubleshooting.md](troubleshooting.md) §10).
3. **`.env` and `.keys/`** — NOT in git BY DESIGN (gitignored; secrets
   never in the repo — CLAUDE.md conventions). Git is therefore not a
   backup for them: the operator backs these up separately (password
   manager / secret store), or accepts re-provisioning from the venue and
   service consoles via [key-rotation-and-secrets.md](key-rotation-and-secrets.md).

Everything else (code, config shape, docs) is in git; build artifacts
(`target/`) are regenerable and explicitly disposable.

## 2. Interim manual backup (until §4 is built)

Postgres — `pg_dump` custom-format archive (read-only against the live DB;
syntax verified against pg_dump 15.14 on this machine):

```
mkdir -p data/backups
# REQUIREMENT: pg_dump major version must match the server (server is 16.x;
# a 15.x pg_dump aborts with "server version mismatch"). Check first:
#   psql "$DATABASE_URL" -tAc 'show server_version'   # 16.x
#   pg_dump --version                                  # must also be 16.x
# (Homebrew: use $(brew --prefix postgresql@16)/bin/pg_dump if PATH has 15.)
pg_dump --format=custom --file="data/backups/fortuna-$(date -u +%Y%m%dT%H%M%SZ).dump" "$DATABASE_URL"
```

`data/` is gitignored, so a dump there can never be committed (it contains
the audit trail — treat it with the same care as the live DB). It is still
on the SAME disk; copy it off-box to count as a backup.

Recorder dataset — plain copy, append-only source so rsync is safe to
re-run:

```
rsync -a data/perishable/ /Volumes/REPLACE-WITH-BACKUP-TARGET/fortuna-perishable/
```

(Destination is yours to choose — the placeholder is deliberate; there is
no sanctioned destination yet, see §4.)

## 3. Restore — into a FRESH database, never over the live one

**OPERATOR-JUDGMENT** — restore is the destructive direction. Preconditions:
you are restoring into a NEW database name; the daemon is not pointed at it;
you double-checked the `--dbname`. Never `pg_restore --clean` against the
live `fortuna` database.

```
createdb fortuna_restore_test
pg_restore --dbname=fortuna_restore_test "data/backups/<your-dump-file>.dump"
```

Two known frictions:

- `createdb` will fail as `fortuna_app` — that role cannot
  `CREATE DATABASE` (t41-completion-gate verdict, Commands note 1;
  [troubleshooting.md](troubleshooting.md) §4). Run `createdb` as a role
  with CREATEDB (the gate verdicts used the local
  `postgres://xavierbriggs@localhost:5432` superuser role).
- An untested backup is not a backup. Spec makes the restore drill a
  MONTHLY review item ("kill-switch test, Postgres backup restore drill" —
  docs/spec.md, Monthly review), and the daemon's monthly review routes
  that reminder to Slack #ops as an operator action (GAPS.md, T4.1/M2
  slice C2). Verify a restore actually produces queryable rows:

```
psql -d fortuna_restore_test -c "select count(*) from audit"
dropdb fortuna_restore_test
```

(`dropdb` of the TEST database only — also OPERATOR-JUDGMENT; check the
name before pressing enter.)

## 4. NOT YET BUILT — automated backup

Said plainly: **no automated backup procedure exists in this repository as
of `334612d`.** `scripts/` contains `killswitch-test.sh`, `replay.sh`,
`run-dst.sh`, and `check-protected-invariants.sh` — nothing backs anything up
on a schedule, nothing copies off-box, nothing rotates dumps, and no restore
has ever been drilled.

The spec names the target state — "Nightly backups on ITHACA plus offsite
copy" (docs/spec.md, Section 7 storage paragraph) — and the monthly restore
drill (docs/spec.md, Monthly review). Neither is implemented. The GAPS
ledger's only standing references are the monthly-review drill routing
(GAPS.md T4.1/M2 slice C2) and this runbook; the build item itself is
unledgered operator-queue work. Until it lands, §2 run by hand — and logged
in the soak log when run during the soak — is the entire backup story.
Do not let this section silently rot: when the automation lands, rewrite
this runbook around it.

## When to stop and escalate

- Disk failure or ENOSPC threatening `data/perishable/` → protect the
  dataset first (it is the only unrecoverable artifact besides the ledger);
  free space by deleting build targets, never data
  ([troubleshooting.md](troubleshooting.md) §10).
- A restore drill fails (dump unreadable, counts implausible) → treat every
  existing dump as suspect; take a fresh §2 dump immediately and diagnose
  before anything else.
- You are about to run any restore command containing the live database
  name `fortuna` → stop. Fresh name only.
