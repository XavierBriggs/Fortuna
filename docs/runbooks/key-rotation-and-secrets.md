# Runbook: key rotation and secrets handling

**Who this is for:** the operator provisioning, rotating, or auditing
credentials — and anyone about to touch `.gitignore`.
**When to read it:** before provisioning any credential; immediately after
any suspicion a secret reached git, a log, or a config file.
**Status:** accurate as of commit `334612d` (2026-06-12). The finalization
checklist below is STILL PENDING — verified this date: `refs/original/`
still exists in this repository.

The rule, once: **committed = compromised = rotate.** A secret that has ever
been in a git object is treated as leaked, even if the repo never left the
machine. The 2026-06-11 incident below is why this is policy and not
paranoia.

Related: [demo-bringup.md](demo-bringup.md) (umbrella bring-up) ·
[soak-start.md](soak-start.md) (env contract) ·
[fixture-recording.md](fixture-recording.md) (demo credential pair) ·
[kill-switch-drill.md](kill-switch-drill.md) (the kill-switch pair)

---

## 1. The env-name contract

Secrets live ONLY in env vars — never in the repo, never in config TOML,
never in logs or audit payloads (CLAUDE.md conventions). The committed
shape is [.env.example](../../.env.example); the real `.env` is gitignored
and `chmod 600`. Names, as read by the code today:

| Name | Read by |
|---|---|
| `DATABASE_URL` | sqlx, the CLI, the daemon |
| `ANTHROPIC_API_KEY` | the daemon (absent ⇒ StubMind; the env key IS the cognition feature flag) |
| `FORTUNA_SLACK_BOT_TOKEN` + the five `FORTUNA_SLACK_CHANNEL_*` ids | the daemon |
| `FORTUNA_DEADMAN_URL` | the daemon |

Reserved for the post-fixture live composition (provision once, two
SEPARATE pairs — .env.example "reserved" section):

| Name | Pair |
|---|---|
| `KALSHI_API_KEY_ID` / `KALSHI_PRIVATE_KEY_PATH` | the trading runtime |
| `FORTUNA_KILLSWITCH_KALSHI_API_KEY_ID` / `FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH` | the kill switch — **its OWN credential pair; it must never share keys with the runtime** (.env.example; GAPS.md kill-switch entry). A runtime-key revocation must not disarm I4, and vice versa. |

The fixture recorders read a third, demo-only pair —
`KALSHI_API_DEMO_KEY_ID` / `KALSHI_DEMO_PRIVATE_KEY_PATH` — and reference no
production names at all
([crates/fortuna-venues/examples/record_kalshi_fixtures.rs](../../crates/fortuna-venues/examples/record_kalshi_fixtures.rs)
safety rails).

Two enforcement details worth knowing at 3am: boot REFUSES placeholder
values (`replace`, `changeme`, `your-`, `<`, empty — boot.rs
`PLACEHOLDER_MARKS`), and the daemon redacts secret values in all
Debug/Display output (boot.rs `Secret`). Env is read at BOOT — any rotation
of a daemon-consumed secret takes effect only at the next daemon restart
([crates/fortuna-live/src/main.rs](../../crates/fortuna-live/src/main.rs)
gathers env once).

## 2. Kalshi key rotation procedure

**OPERATOR-JUDGMENT** — rotation invalidates the old credential immediately;
anything signed with it (a mid-flight fixture session, a running live
composition once one exists) breaks. Precondition: nothing is mid-session
on that key.

1. Generate the replacement on the venue: demo keys at demo.kalshi.co,
   prod keys at kalshi.co — Account & security → API Keys (GAPS.md
   SECURITY INCIDENT entry, operator action 1).
2. Save the downloaded PEM OUTSIDE the repo (e.g. `~/keys/`), `chmod 600`.
3. Point the `.env` PATH variable at the new PEM and update the matching
   key-id variable. Which pair you touch depends on role: trading
   (`KALSHI_*`), kill switch (`FORTUNA_KILLSWITCH_*`), or demo recording
   (`KALSHI_*_DEMO_*`). Rotate pairs independently — that is why they are
   separate.
4. Reload (`set -a && source .env && set +a`) and restart whatever consumes
   the rotated pair.
5. The recorded fixture set is unaffected by rotation — fixtures contain no
   key material by construction
   ([fixture-recording.md](fixture-recording.md)).

Slack token and `ANTHROPIC_API_KEY` rotation is the same pattern minus the
PEM: swap the value in `.env`, restart the daemon.

## 3. The 2026-06-11 incident — why all of this is written down

Full record: GAPS.md "SECURITY INCIDENT 2026-06-11" and
[docs/reviews/history-rewrite-2026-06-11.md](../reviews/history-rewrite-2026-06-11.md).
Summary:

- **What:** both Kalshi PEM private keys (`.keys/fortuna-demo-v1.txt`,
  `.keys/fortuna-key.txt` — the latter mapped by `.env` to BOTH the trading
  and the kill-switch key paths) were tracked in git from the B0 commit
  until same-day remediation.
- **Root cause:** an `echo "data/" >> .gitignore` onto a file whose last
  line `.keys/**` had no trailing newline corrupted the pattern to
  `.keys/**data/`, un-ignoring `.keys/`; the next `git add -A` swept the
  keys in.
- **Exposure bound:** the repo has never been pushed; the key material
  never left this machine.
- **Remediation done:** `.gitignore` repaired; keys and runtime data
  untracked at HEAD; branch history rewritten via `git filter-branch`
  (old→new hash map in the history-rewrite doc — hashes cited in documents
  dated before 2026-06-11T08:30Z are pre-rewrite).

### The finalization steps that are STILL PENDING (operator checklist)

Quoted from GAPS.md "OPERATOR ACTIONS REQUIRED (two distinct decisions)":

> 1. ROTATE both Kalshi keys (treat as compromised per policy even though
>    exposure was machine-local — the live key is also the I4 kill-switch
>    credential): demo + prod key pages at (demo.)kalshi.co Account &
>    security -> API Keys; place new PEMs at the paths .env names; the
>    fixture set is unaffected (recorded with the demo key, which you may
>    rotate independently).
> 2. FINALIZE THE PURGE (irreversible; classifier-gated to you): run
>    `git for-each-ref --format='%(refname)' refs/original/ | xargs -n1 git
>    update-ref -d && git reflog expire --expire=now --all && git gc
>    --prune=now` from the repo root (or tell the agent "finalize the
>    purge" to run it with your authorization). Until this runs, the old
>    key blobs remain reachable inside .git via the backup ref. Do this
>    BEFORE any first push of this repository, whatever else happens.

**OPERATOR-JUDGMENT** — both items. Item 2 irreversibly destroys git
recovery data (`refs/original`, all reflogs, unreachable objects);
preconditions: you do not need to recover anything from pre-rewrite
history, and you accept that verification of full unreachability happens
only AFTER it runs (GAPS.md). It is the mandatory gate before this repo's
first push.

Check whether finalization has happened (read-only, safe):

```
git for-each-ref --format='%(refname)' refs/original/
```

Non-empty output (today: `refs/original/refs/heads/main`) = still pending.
Empty = finalized.

## 4. Process rules distilled from the incident

- Never append to `.gitignore` with `>>`. Edit with anchored tooling and
  verify afterwards (GAPS.md PROCESS FIX):

  ```
  git check-ignore -v .keys .env data
  git status --ignored --short | head -n 20
  ```

  `.keys`, `.env`, and `data` must each resolve to an ignore rule.
- `.keys/` and `.env` are NOT in git BY DESIGN; there is no off-repo backup
  of them either unless you make one — see
  [backup-restore.md](backup-restore.md).
- Before committing anything that touched `fixtures/`, run the secrets
  sweep in [fixture-recording.md](fixture-recording.md) §5.

## When to stop and escalate

- Any secret value appears in `git diff`, a log line, an audit payload, or
  a fixture → stop, rotate that credential (rule above), then trace how it
  got there and ledger it in GAPS.md before resuming.
- You are about to push this repository anywhere and step 3's check still
  prints a ref → STOP. Finalization first (GAPS.md: "Do this BEFORE any
  first push of this repository, whatever else happens").
