# Review: perps-b0-b1-remediation-regate — 2026-06-11

Base: 935517a (= old 3e0d34f, the BLOCKed head, mapped per docs/reviews/history-rewrite-2026-06-11.md)
Head: 1485d98 (HEAD moved twice during review — 6f34d86, 1485d98, both docs-only; all code graded is in 935517a..3eaa5e4)
Verdict: **BLOCK** (one Major; narrow — see Findings; every other criterion PASS)
Protected crate touched: **no** (`git diff 935517a..HEAD -- crates/fortuna-invariants/` = 0 bytes)

Reviewer: FORTUNA verifier (independent context; implementer rationale not read).
Rubric fixed before opening the diff: prior verdict F1–F7 (docs/reviews/perps-b0-b1-fixtures-gate-2026-06-11.md),
the re-gate brief's claim list, honesty checks on the post-BLOCK diff (T4.1 explicitly NOT graded for
completion), fortuna-review mechanical checklist. Old hashes mapped via the history-rewrite record before
any comparison.

## Criteria (fixed before reading the diff)

### C-F1 — keys (was Critical)
- F1a .gitignore repaired: **PASS** — lines 7–9 `.keys/`, `data/`, `.playwright-mcp/`, every line
  newline-terminated (`sed -n l`: all `$`; `tail -c1` = 0x0a); corrupt `.keys/**data/` gone;
  `git check-ignore -v` matches all three paths to those exact lines.
- F1b untracked at HEAD: **PASS** — `git ls-files .keys/ .playwright-mcp/ data/` = 0 paths. Keys remain
  on disk (mode 0600) for the operator, correctly ignored.
- F1c branch history purged: **PASS for .keys/ and data/** — `git log main -- .keys/` = 0 commits;
  `git log main -- data/` = 0 commits; tree sweep over every commit on main: zero trees contain `.keys/`
  or `data/`. **.playwright-mcp/ residue → Minor (finding 2)** — 19 blobs (~202 KB) survive in the trees
  of the unrewritten prefix a4c9071..e464780 (the filter range `7b00ce6^..HEAD` starts where the KEYS
  entered, not where the litter entered). Executed secret sweep over all 19 surviving blobs: 0 raw
  pattern hits (PEM/api_key/secret/token/password/bearer). No ledger claims playwright was purged from
  ALL history; the rewrite doc quotes the exact filter command. Hygiene, not security.
- F1d GAPS incident entry: **FAIL → Major (finding 1)** — entry exists ("SECURITY INCIDENT 2026-06-11",
  GAPS.md from line 91) and is substantively complete (both keys named incl. the kill-switch .env mapping;
  root cause `echo "data/" >>` onto no-trailing-newline `.keys/**`; exposure bound never-pushed; process
  fix; OPERATOR rotation action with key-page locations). BUT the REMEDIATION sentence asserts two things
  contradicted by execution: line 105 lists "(filter-branch + reflog expire + gc)" as the performed
  mechanism — reflog expire and gc were NOT run (operator-gated by design); lines 108–109 claim "purge
  VERIFIED (no key blobs reachable from any ref)" — false: `git for-each-ref refs/original/` →
  refs/original/refs/heads/main = fc1d2f3; `git merge-base --is-ancestor 7b00ce6 refs/original/refs/heads/main`
  → yes; `git show 7b00ce6:.keys/fortuna-key.txt` → `-----BEGIN RSA PRIVATE KEY-----`. The GAPS operator
  queue (line 111) lists rotation only — the finalization decision exists only in the rewrite doc.
- F1e hash-map doc accurate: **PASS** — spot-checks 94d651a ("B0: fortuna-recorder…") and 935517a
  ("gitignore data/…") match by subject AND by content: `git diff --name-only 7b00ce6 94d651a` and
  `3e0d34f 935517a` contain ONLY purged-prefix paths (0 non-purged paths) — the rewrite is content-exact.
- F1f known-open items ledgered as operator-gated: rotation **PASS** (GAPS line 111, prominent);
  finalization **ledgered correctly in the rewrite doc** ("FINALIZATION … is OPERATOR-GATED … the key
  blobs remain recoverable inside .git") **but anti-ledgered in GAPS** (claimed done+verified) — folded
  into finding 1. The pending state itself is BY DESIGN per the brief and is NOT a finding.

### C-F2 — error shapes (was Major): **PASS**
README finding 1 enumerates THREE shapes — (a) nested 17/19 4xx, (b) flat OpenAPI naming
orders__numeric_field_types (`code:"bad_request"`, Go-unmarshal details), (c) bare `{"msg"}` naming
markets__limit_over_max — with the correction visible, original claim quoted as "falsified by this set's
own captures" (corrected-not-erased). GAPS Kalshi session text (lines 181–187) matches verbatim in substance.

### C-F3 — stale read (was Major): **PASS**
README finding 16: DELETE acked 200 at ts_ms 1781159364112; GET ~360 ms later `status:"resting"`,
`remaining_count_fp:"1.00"`, last_update_time unchanged; re-cancel 404. ADAPTER REQUIREMENT: poll until
terminal with bounded backoff; 404-on-recancel = proof-of-canceled. Cites all three fixture pairs.
GAPS (lines 186–189) carries the same with "gate F3 — checklist #15's highest-stakes item".

### C-F4 — coverage overstatements (was Minor): **PASS**
README Known gaps now opens with the corrected coverage statement ("EXCEPT the items below — 'covered'
without this list was an overstatement") and lists #11 STP `maker` UNOBSERVED, #20 vacuous empty-book
(`{"orderbook":{}}`), #17 sub-items (cursor stability across inserts; expired cursor). GAPS line 178–181
scopes the session the same way ("EXCEPT the ledgered exceptions … gate finding F4").

### C-F5 — repo litter (was Minor): **PASS** (residue = finding 2)
Playwright untracked at HEAD (F1b); `git log main -- data/` = 0 (transit purged — old ad89942's successor
f551d84 tree is clean; old 576d826 pruned-empty exactly as the map records).

### C-F6 — wall-clock ledger + chrono (was Minor): **PASS**
ASSUMPTIONS.md lines 1187–1194: capture tools use wall clock at the IO edge, gate finding F6 cited, scope
reasoned (timestamps ARE the data; logic halves pure). `chrono` absent from crates/fortuna-recorder/Cargo.toml
(deps: anyhow, reqwest, serde_json, tokio); `grep -rn chrono crates/fortuna-recorder/` = 0 hits.

### C-F7 — GAPS Kinetics staleness (was Minor): **PASS**
"Phase B: CONFIRMED by the operator 2026-06-11" with the correction noted in place: "(This entry previously
said 'awaiting confirmation' after confirmation had landed — one ledger held two states; corrected per gate
finding F7.)"

### C-NEW — post-BLOCK diff honesty (T4.1 completion explicitly NOT graded)
- fortuna-live tests never read real env: **PASS** — `grep -rn 'std::env\|env::var\|dotenv'` over the crate:
  only main.rs:16 (args) and main.rs:25 (vars) — the binary edge; lib and tests are pure over injected
  BTreeMaps with synthetic values; tests/boot.rs header states the kickoff pitfall explicitly.
- Secret redaction test-asserted: **PASS** — `required_env_never_displays_secret_values` formats Debug and
  asserts the API key, Slack token, and DB URL are absent (Secret Debug = `<REDACTED>`; values never enter
  BootError variants — var names + placeholder marks only).
- Binary refuses to pretend to run: **PASS** — main.rs bails "the runtime composition is not wired yet
  (T4.1 in progress…)"; stub-mind degrade requires explicit `allow_stub_mind` opt-in; venue=kalshi refuses
  pending T4.2 fixture clearance (test-pinned).
- No T4.1-done claim anywhere: **PASS** — BUILD_PLAN line 274 `[ ]`; repo-wide done-claim sweep = 0 hits;
  commit subject "T4.1 increment … (req 1, 7 partial)".
- config/fortuna.example.toml [daemon]: **PASS** — lines 89–93 (venue="sim", halt_poll_ms=500 pin,
  127.0.0.1 metrics bind); example parses (test daemon_toml_parses_the_committed_example, executed green).
- Operator content committed (BUILD_PLAN T4.3/T4.4 amendments + overnight directive, docs/design/fortuna-cli.md):
  **PASS** (present, coherent; verbatim-ness against the operator's source is unverifiable from artifacts —
  noted, non-gating since the operator authored it). Note (non-gating, in-progress code): main.rs doc comment
  says the operator's .env "is loaded EXPLICITLY here" but the binary loads nothing — it reads the ambient
  env and fails closed when unsourced (the error hint says to source .env). Behavior conservative; wording loose.
- 11 boot tests claimed: **PASS** — exactly 11 ran green this session. Red-first authorship is unverifiable
  from a single commit; not gating (rationale is ignored by policy).

### C-MECH (all executed this session)
- fmt --check: PASS (EXIT=0). clippy --workspace --all-targets -D warnings: PASS (EXIT=0).
- cargo test --workspace: PASS — 95 suites, **676 passed, 0 failed, 0 ignored**, EXIT=0 (matches the
  expected ~676 = 665 prior + 11 boot).
- scripts/run-dst.sh 200: PASS — core "OK: 0 corpus + 200 random seeds, zero invariant violations"
  (master seed 1781169228731); synthesis-dst 200 scenarios ok (538 orders, 854 proposals, 2826 cognition
  failures); settlement-dst 200 scenarios ok (36 discrepancies, 54 watchdog rows, 36 halts); EXIT=0.
- Test-weakening sweep over 935517a..1485d98: PASS — 0 deleted asserts, no `#[ignore]`, no case reductions.
- Secrets sweep over added lines: PASS — 5 hits, all prose mentions of the incident in docs; no key material.
- place()/GatedOrder: PASS — 4 hits are `.replace(` in boot tests + verdict prose; no venue/exec code added.

## Findings

1. **[Major] GAPS security-incident entry asserts a purge verification that is false at HEAD.**
   GAPS.md line 105 lists "(filter-branch + reflog expire + gc)" as the executed remediation mechanism and
   lines 108–109 claim "purge VERIFIED (no key blobs reachable from any ref)". Executed evidence: reflog
   expire and gc were never run; `refs/original/refs/heads/main` (fc1d2f3) exists; old 7b00ce6 is reachable
   from it; `git show 7b00ce6:.keys/fortuna-key.txt` returns the PEM. The pending-finalization STATE is
   by design and correctly ledgered in docs/reviews/history-rewrite-2026-06-11.md — but GAPS, the document
   the operator queue lives in, states the opposite, and its OPERATOR ACTION REQUIRED (line 111) omits the
   finalization decision entirely. This is the F2/F3 defect class (honest artifacts, false summary layer)
   and the F7 defect class (one ledger, two states) applied to a Critical incident's security claim — the
   exact text was written in the pre-purge surface commit (1259388) and never reconciled after the actual
   purge ran narrower than described. Marginal exposure today is nil (the same user already has .keys/ on
   disk), but the ledger now asserts a destruction that has not happened; a future clone/push decision made
   on GAPS alone would be made on false premises. Fix is textual: correct the mechanism sentence, restate
   verification as "no key blobs reachable from main; recoverable via refs/original until operator-approved
   finalization", and add the finalization approve/decline to the GAPS operator queue.
   — reproduction: `git for-each-ref refs/original/`; `git merge-base --is-ancestor 7b00ce6
   refs/original/refs/heads/main && echo yes`; `git show 7b00ce6:.keys/fortuna-key.txt | head -1`;
   GAPS.md lines 103–111 vs docs/reviews/history-rewrite-2026-06-11.md lines 14–19.

2. **[Minor] .playwright-mcp/ blobs survive in the unrewritten history prefix and are main-reachable
   forever.** 19 files (~202 KB: 9 console logs + 10 page snapshots) persist in the trees of
   a4c9071..e464780 because the filter range `7b00ce6^..HEAD` begins where the keys entered, two-plus
   commits after the litter entered (`git log main -- .playwright-mcp/` → 4 commits: 94d651a [deletion],
   4213f11, 825d144, a4c9071). Executed secret sweep across all 19 blobs: zero raw pattern hits — litter,
   not exposure. Note for the operator's finalization decision: these blobs are reachable from main itself,
   so dropping refs/original + gc will NOT remove them; removal requires extending the filter range to
   a4c9071^ (a second rewrite). No remediation claim is contradicted (no document asserts playwright was
   purged from all history; the rewrite doc quotes the exact command and range). Ledger and decide.

## What survives this BLOCK

Everything except two sentences in GAPS: F1a/b/c(keys,data)/e/f-rotation, F2–F7, all honesty checks, and
the full mechanical suite PASS with executed evidence. The fortuna-live increment is honest, pure-injected,
redaction-asserted, and fail-closed. Re-gate after the GAPS correction should be minutes: re-verify the
incident entry text against `git for-each-ref refs/original/` and the operator queue contains the
finalization item; optionally ledger finding 2.

## Commands run (verbatim results, trimmed to verdict lines)

```
cargo fmt --check                                          → FMT_EXIT=0
cargo clippy --workspace --all-targets -- -D warnings      → CLIPPY_EXIT=0
cargo test --workspace                                     → 95 suites: 676 passed; 0 failed; 0 ignored; TEST_EXIT=0
  (Running tests/boot.rs … running 11 tests … 11 ok — incl. required_env_never_displays_secret_values)
scripts/run-dst.sh 200                                     → DST_EXIT=0
  [dst] OK: 0 corpus + 200 random seeds, zero invariant violations (master seed 1781169228731)
  [synthesis-dst] 200 scenario(s) … ok. 1 passed   [settlement-dst] 200 scenario(s) … ok. 1 passed
git ls-files .keys/ .playwright-mcp/ data/                 → 0 paths
git log main --oneline -- .keys/                           → 0 commits
git log main --oneline -- data/                            → 0 commits
git log main --oneline -- .playwright-mcp/                 → 4 commits (a4c9071, 825d144, 4213f11, 94d651a)
git for-each-ref refs/original/                            → fc1d2f3… refs/original/refs/heads/main
git show 7b00ce6:.keys/fortuna-key.txt | head -1           → -----BEGIN RSA PRIVATE KEY-----
git diff --name-only 7b00ce6 94d651a | grep -vE '^\.keys/|^\.playwright-mcp/|^data/' → empty (content-exact rewrite)
git diff 935517a..HEAD -- crates/fortuna-invariants/       → 0 bytes
secret sweep, 19 surviving playwright blobs (e464780)      → 0 raw hits over 202,025 bytes
```

---

## Addendum — micro re-grade of fix commit 16478bb (2026-06-11, later same day)

Scope: finding 1 (F1d, Major) and finding 2 (Minor) only. Fix head: 16478bb (GAPS.md
only). This addendum SUPERSEDES the overall verdict above for the remediation batch.

**Verdict: ACCEPT** (remediation batch as a whole). Protected crate touched: no.

### Criteria (fixed by the re-grade brief before reading the fix)

- A1 incident entry truthful (branch-only / finalization NOT run / old objects
  reachable via refs/original / overstatement corrected-not-erased): **PASS** —
  GAPS.md now states "the BRANCH history rewritten via filter-branch", "FINALIZATION
  HAS NOT RUN: the refs/original backup ref and reflogs still REACH THE OLD OBJECTS
  (... `git show <old-hash>:.keys/...` works by design until finalization)", names the
  classifier denial, and quotes the prior false claim in place: "The earlier text here
  claimed 'reflog expire + gc ... VERIFIED' — that was the plan, written ahead of the
  denial and not reconciled; corrected, not erased." Every claim in the corrected text
  was re-executed and is true at HEAD (A4). The 08:00Z→08:30Z hash-epoch change aligns
  GAPS with history-rewrite-2026-06-11.md ("executed 2026-06-11 ~08:30Z"). The reflog
  half of the reachability claim was independently verified, not taken on faith:
  `git rev-list --walk-reflogs main | grep -c 7b00ce6` → 1.
- A2 operator queue carries finalization as its own decision with the exact command,
  distinct from rotation: **PASS** — "OPERATOR ACTIONS REQUIRED (two distinct
  decisions)": 1. ROTATE (full prior detail preserved); 2. FINALIZE THE PURGE
  (irreversible; classifier-gated) with the verbatim command
  `git for-each-ref --format='%(refname)' refs/original/ | xargs -n1 git update-ref -d
  && git reflog expire --expire=now --all && git gc --prune=now`, the
  reachable-until-run warning, and "Do this BEFORE any first push". Command is the
  canonical finalization sequence and matches the rewrite doc's description.
- A3 playwright pre-batch residue ledgered (finding 2): **PASS** — "Pre-batch
  .playwright-mcp blobs (zero-secret browser logs) also remain in older history; their
  purge is optional and folds into the same finalization decision." Matches this
  review's executed secret sweep (0 hits over 19 blobs).
- A4 ground truth re-executed this session: **PASS** —
  `git for-each-ref refs/original/` → fc1d2f3 refs/original/refs/heads/main;
  `git show 7b00ce6:.keys/fortuna-key.txt | head -1` → `-----BEGIN RSA PRIVATE KEY-----`
  (reachable: the corrected ledger is TRUE, the old ledger was false);
  `git merge-base --is-ancestor 7b00ce6 fc1d2f3` → yes;
  `git log main --oneline -- .keys/` → 0 commits (branch clean).
- A5 fix-commit scope: **PASS** — `git show --stat 16478bb` → GAPS.md only
  (+33/−14). Secrets sweep of the added lines: paths and a placeholder
  `<old-hash>` only; no key material.

### Battery

Not re-run, per brief: the full battery ran green at 1485d98 (fmt/clippy/676 tests/DST
200+corpus, recorded above). Commits since: f9b18e1 (docs/design/fortuna-cli.md only)
and 16478bb (GAPS.md only) — `git show --stat` on both confirms zero code paths
touched; the 1485d98 results carry to 16478bb.

### Non-gating note

GAPS says the playwright purge "folds into the same finalization decision"; true as a
decision, but the queue-item-2 COMMAND alone will not remove those blobs — they are
reachable from main itself, and removal needs a second rewrite extending the filter
range to a4c9071^ (this file, finding 2). An operator weighing item 2 should read
finding 2 first. Not a false statement in GAPS, so not a finding; recorded here for
the operator's benefit.

### Disposition of the two open operator items

Rotation and finalization remain OPEN by design (operator-gated; spec/CLAUDE.md: never
simulate the human). They are queue items, not gaps in the work; they do not gate this
ACCEPT.
