# Runbook: monthly kill-switch drill (I4)

**Who this is for:** the operator running the monthly kill-switch test, and
anyone deciding between `fortuna halt` and `fortuna kill` mid-incident.
**When to read it:** monthly (the drill), and before the soak's first
incident.
**Status:** accurate as of commit `334612d` (2026-06-12).

Why this exists: invariant I4 — the kill switch must work when nothing else
does. It "must not depend on the cognition runtime, the event loop,
Postgres, or any LLM provider being healthy" (CLAUDE.md I4; spec Section 3).
Spec pins the cadence: "Tested monthly" — and the daemon's monthly review
routes the drill reminder to Slack #ops as an operator action, I7
(GAPS.md, T4.1/M2 slice C2 entry).

Related: [halt-and-rearm.md](halt-and-rearm.md) ·
[soak-start.md](soak-start.md)

---

## 1. Run the drill

```
scripts/killswitch-test.sh
```

(Optionally pass a journal path: `scripts/killswitch-test.sh /tmp/my-drill.jsonl`;
the default is `/tmp/fortuna-killswitch-test-YYYYMMDD.jsonl` —
[scripts/killswitch-test.sh](../../scripts/killswitch-test.sh).)

What it does, and why it proves I4: the script runs

```
env -u DATABASE_URL cargo run -q -p fortuna-killswitch -- self-test --journal <path>
```

`DATABASE_URL` is DELIBERATELY UNSET — "the switch must never need it"
(script comment). `self-test` exercises the full freeze machinery against an
in-process sim venue: build a venue with live orders and positions, freeze,
verify zero open orders remain, print the report
([crates/fortuna-killswitch/src/main.rs](../../crates/fortuna-killswitch/src/main.rs)).
The switch's only state is its own flat-file journal — no Postgres, no
daemon, no Slack, no LLM (the spec Principle-9 exception, CLAUDE.md).

The script's own header says to run it with the main runtime DOWN and
Postgres optionally stopped — "the switch must not care." The self-test
itself touches neither (it is an in-process sim), so running it beside a
live soak daemon is harmless; but at least one drill per quarter should be
run with the runtime and Postgres actually down, because proving
independence under real outage conditions is the point of I4.
**OPERATOR-JUDGMENT** — stopping the runtime/Postgres for a full-dark drill
interrupts the soak (a restart re-fires reviews; note it in the soak log).
Precondition: not inside an unexplained-incident window.

## 2. What success and failure look like

Success (script output):

```
[killswitch-test] building and running self-test (journal: /tmp/…jsonl)
self-test OK: cancelled 2/2 orders, reported 1 positions; journal at /tmp/…jsonl
[killswitch-test] PASS — record this run in the ops log / audit.
```

(Output verified by an actual run at `334612d`, 2026-06-12.)

Failure: a non-zero exit, or the self-test's own complaint that cancelled ≠
seen or resting orders remain (killswitch main.rs). A failed drill is an
incident: the last line of defense does not work. Fix before anything else
resumes.

## 3. Record the result

The script says it itself: "record this run in the ops log / audit."
Concretely, as of `334612d`:

1. Append a line to `docs/reviews/soak-log.md` (the soak's log of record;
   create it if this is the first entry): date (UTC), journal path,
   PASS/FAIL, operator.
2. Keep the journal file — it is the drill's machine evidence.

## 4. When to use the REAL kill (`fortuna kill`) vs `fortuna halt`

| | `fortuna halt` | `fortuna kill` |
|---|---|---|
| What it is | durable gate flag; the runner stops NEW orders within ≤500ms ([halt-and-rearm.md](halt-and-rearm.md)) | execs the STANDALONE `fortuna-killswitch` binary to freeze (cancel everything working at the venue) — out-of-band, no Postgres ([crates/fortuna-cli/src/main.rs](../../crates/fortuna-cli/src/main.rs), `kill`) |
| Needs | Postgres up | nothing but the binary + (eventually) venue credentials |
| Use when | normal incident response; drills; anything where the daemon and DB are alive | the daemon or Postgres is unresponsive, or the venue is unreachable while orders are working — the `fortuna stop` timeout message names this exact case: "if the venue is unreachable use `fortuna kill`" (CLI A7 text) |
| Undo | `rearm` + restart (I2) | re-arm path unchanged; the kill journal records what it did |

**OPERATOR-JUDGMENT** — `fortuna kill` on a live system cancels every working
order at the venue. Precondition: you have decided trading must stop NOW and
the normal path is unavailable.

**Honest limitation as of `334612d`:** the live venue plug is NOT wired.
`fortuna kill` (which runs `fortuna-killswitch freeze`; `--flatten` runs the
`report` action) exits 3 with "no live adapter for venue … is wired yet" —
only `self-test` is functional (killswitch main.rs; GAPS.md "Kill-switch
live venue plug": `freeze --venue kalshi` stays unwired until the adapter
passes fixture confirmation, T4.2, and then gets its OWN credential pair,
`FORTUNA_KILLSWITCH_*` — see
[key-rotation-and-secrets.md](key-rotation-and-secrets.md)). During the Sim
soak this is acceptable: the sim daemon's orders die with the daemon, and
`fortuna halt` covers every drill scenario. Re-verify this section when T4.2
lands.

## When to stop and escalate

- Drill FAILS → incident. Nothing resumes until the switch passes again.
- Drill passes only WITH `DATABASE_URL` set, fails without → I4 violation;
  the implementation is wrong, not the test. Record in GAPS.md, stop.
- You reached for `fortuna kill` in earnest and got exit 3 → expected
  pre-T4.2 (see above); fall back to `fortuna halt` + venue-side manual
  cancellation, and escalate to the operator queue that T4.2 is now
  blocking incident response.
