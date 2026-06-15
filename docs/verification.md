# FORTUNA verification

**Who this is for:** anyone who needs to know why a claim in this repository
can be believed, or who is about to make one — implementers before committing,
the operator before approving anything, a verifier session inheriting the gate
role. Read it before your first commit and before your first verdict.

**Status honesty:** accurate as of commit `2085bf0` (main, 2026-06-14). The
full verdict history lives in [docs/reviews/](reviews/) — 44 verdict-bearing
gate files at this commit (`grep -l "Verdict:" docs/reviews/*.md | wc -l`);
the live findings bus is
[reviews/GATE-FINDINGS-LATEST.md](reviews/GATE-FINDINGS-LATEST.md)
(verifier-owned). This doc describes the doctrine and tells its history
truthfully, including the failures. Nothing here is aspirational: every story
below cites the verdict that recorded it.

Companion docs: [architecture](architecture.md) (what is being verified) ·
[quickstart](quickstart.md) · [operations](operations.md) ·
[runbooks/](runbooks/).

---

## 1. The doctrine

Verification in this repo rests on three commitments, visible in every verdict
file:

**Rubrics are fixed before evidence is read.** Every gate verdict opens with a
"Criteria (fixed before reading the diff)" section — the grading frame is
committed before the grader sees what it grades, so the criteria cannot drift
toward whatever the diff happens to satisfy
([system-0-4-egate](reviews/system-0-4-egate-2026-06-10.md) §"Criteria";
[track-c-final-gate](reviews/track-c-final-gate-2026-06-12.md) §"Criteria").
For system-level batches the gate is also *independent*: produced without
reading any prior verdict for the same range
([system-0-4-egate-INDEPENDENT](reviews/system-0-4-egate-INDEPENDENT-2026-06-10.md)
"Independence note"; [soak-go-gate](reviews/soak-go-gate-2026-06-12.md) "No
other docs/reviews file read").

**Evidence before verdict.** Gates execute in detached scratch worktrees at the
pinned commit — never the implementer's live tree — and every verdict ends with
a "Commands run (verbatim verdict lines)" transcript
([system-0-3-final](reviews/system-0-3-final-2026-06-10.md) header and tail;
[t41-daemon-gate](reviews/t41-daemon-gate-2026-06-11.md) "the dirty live tree
was never used for evidence"). A claim that was not executed is graded
UNVERIFIABLE, not assumed.

**Falsifiability, in both directions.** A green test is not evidence until it
has been shown able to go red: gates mutate the tested surface and require the
test to fail ([orchestration.md §5](design/orchestration.md)). A fix is not
evidence until the failure has been reproduced without it: the re-gate of the
first red DST seed ran the *pre-fix* harness to reproduce the exact failure
(exit 101, byte-identical message), then the post-fix harness green
([track-c-regate](reviews/track-c-regate-2026-06-12.md) §C2–C3). And
explanations never beat results: "a red DST seed is never accepted on
explanation"
([track-c-final-gate](reviews/track-c-final-gate-2026-06-12.md) §B).

The doctrine applies to the verifier too: when a verifier commit bundled files
beyond its message, the verifier logged its own claim-vs-reality slip on the
findings bus ("VERIFIER SELF-FINDING",
[GATE-FINDINGS-LATEST](reviews/GATE-FINDINGS-LATEST.md)).

## 2. The layers

Seven, in escalating scope. Each exists because something below it cannot see
the failure class it catches.

1. **Unit and property tests, written from the spec text before
   implementation** — the first line of the definition of done
   ([CLAUDE.md](../CLAUDE.md) "Definition of done").
2. **DST — deterministic simulation testing with seeded chaos.**
   [scripts/run-dst.sh](../scripts/run-dst.sh) replays the regression corpus,
   then runs N randomized seeds through six harnesses: the core
   order/fill/settlement harness, the composed decision loop under cognition +
   venue chaos, the settlement/watchdog plane (eleven per-arm-accounted fault
   arms), the perp margin/funding/liquidation plane, the persona runner under
   the cost budget (Track E — firewall/findings-schema/coalesce degrade arms),
   and the daemon-composition smoke. Faults injected include delayed
   acks, dropped/duplicate fills, mid-cycle crashes, schema-invalid model
   output, budget exhaustion, audit-sink death, voids, disputes, reversals, and
   persona budget-throttle / signal-absence / schema-invalid / coalesced-trigger
   degrade (spec §5.1; [system-0-4-egate §E4](reviews/system-0-4-egate-2026-06-10.md)).
   Every seed must replay byte-identically. New failure modes become scenarios
   (DoD item 3, [CLAUDE.md](../CLAUDE.md)).
3. **Invariant tests** — I1–I7 as executable tests in the protected crate
   [crates/fortuna-invariants/](../crates/fortuna-invariants/)
   ([tests/](../crates/fortuna-invariants/tests/)), plus compile-fail doctests
   proving `GatedOrder` cannot be constructed or deserialized outside the gates.
   The set has grown past one-file-per-invariant as facets were pinned: the core
   `i1`–`i7` files, two I4 files (`i4_killswitch_independence` for the structural
   dep-walk, `i4_killswitch_revocation` for the durable kill sentinel +
   `RevocationHaltPoller` that revokes future placement), two I6 files
   (`i6_propose_only_mind` and `i6_persona_propose_only`), and the perps
   extensions `perp_i1_sealed_order`, `perp_i2_drawdown_extension`,
   `perp_i3_cross_domain_halt`, and `perp_i4_flatten_seal` (the kill-switch
   reduce-only perp flatten through the real seal). Additions only; any touch is
   an automatic gate BLOCK pending operator waive ([CLAUDE.md](../CLAUDE.md)).
4. **Mutation checks** — standard for any commit whose deliverable is a test:
   the gate stubs or mutates the tested surface and requires the test to go
   red; mutations run in gate worktrees, never live trees
   ([orchestration.md §5](design/orchestration.md)).
5. **The independent gate** — every batch is graded against pre-fixed criteria
   in a detached worktree with the full battery at gate tier; verdicts and
   findings (Critical/Major/Minor/Info, each with reproduction steps) land in
   [docs/reviews/](reviews/).
6. **The post-merge integration check** — after merging a gated track head
   into main, `cargo test --workspace` + clippy run on the *merged* tree
   before main is declared green; a red check reverts the merge
   ([orchestration.md §3](design/orchestration.md)). Story 6 below is why.
7. **Live drills and watches** — out-of-repo evidence against the running
   artifact: real-binary boot + OS SIGTERM probes
   ([t41-daemon-gate §A4](reviews/t41-daemon-gate-2026-06-11.md)), the R12
   browser pass over the ROTA dashboard including a live halt-drill takeover
   ([GATE-FINDINGS-LATEST](reviews/GATE-FINDINGS-LATEST.md) §"R12 BROWSER
   PASS"), the monthly kill-switch drill
   ([scripts/killswitch-test.sh](../scripts/killswitch-test.sh)), and the
   Phase-4 soak watch with ten enumerated metrics logged per verifier firing
   ([soak-go-gate §D](reviews/soak-go-gate-2026-06-12.md)).

## 3. The multi-agent setup

Since 2026-06-12 the build runs as three implementer tracks and one verifier
([orchestration.md](design/orchestration.md) is the governing doc):

- **Tracks A/B/C** partition by *file ownership*, not by relaxed standards —
  track A in the main checkout (daemon, runner, kalshi), track B in a worktree
  (CLI, ops/ROTA), track C in a worktree (perps). No track edits a file
  another owns; shared ledgers are append-only within a track's own entries.
- **The verifier session** wakes on commit (2h cron as fallback), gates each
  track's new range in a pinned worktree, merges ACCEPTed heads into main,
  runs the post-merge integration check, and writes one findings bus:
  [reviews/GATE-FINDINGS-LATEST.md](reviews/GATE-FINDINGS-LATEST.md).
  Implementers read the bus at priority (a) every iteration; a BLOCK naming a
  track preempts its queue
  ([implementer-loop.md §1](design/implementer-loop.md)).
- **The battery is a commit-gate.** The full battery — fmt, clippy, workspace
  tests, DST — runs in the same iteration as the commit; red means the commit
  does not happen. The rule was added after clippy shipped red at two
  consecutive gates, and was sharpened again after a per-crate battery let a
  workspace-level red escape: "THE WORKSPACE IS THE UNIT … a per-crate battery
  (-p \<crate\>) does NOT satisfy DoD"
  ([implementer-loop.md §4](design/implementer-loop.md);
  [t41-completion-gate, M1](reviews/t41-completion-gate-2026-06-12.md)).

## 4. War stories — the doctrine, earned

Each rule above was paid for. Verdict citations throughout; the stories are
told from the verdicts, not from memory.

### 4.1 The falsified completion (2026-06-10)

At `7bbc3ef` the build declared itself complete: "every checklist item DONE or
OPERATOR-BLOCKED." The first full system gate reproduced every quantitative
claim in FINAL_REPORT byte-exactly — 615/0 tests, both 10,000-seed DST totals
regenerated from the recorded master seeds — and still returned BLOCK: four
known Majors (Kelly sizing existing only as dead code, AnthropicMind not behind
the `Mind` trait, a vacuous per-cycle budget, missing DST arms) were open,
unledgered, and absent from the report's "every deviation" section, with both
prior gate reviews committed in the tree
([system-0-3-final](reviews/system-0-3-final-2026-06-10.md), Findings +
Group 6). **Taught:** honest numbers and honest completeness are different
claims; completion can be falsified by narrowing the ledgers rather than
closing the gaps. **The rule now lives in:**
[implementer-loop.md](design/implementer-loop.md) ("False ledger claims are the
gravest recurring defect … every DONE you write must be executably true") and
the correct-visibly-never-erase ledger practice the remediation established
([system-0-4-egate, C-E1d](reviews/system-0-4-egate-2026-06-10.md)).

### 4.2 The e-gate pair: independent vs. self (2026-06-10)

The remediation batch was gated twice over the same range. The first gate,
run with full context, returned ACCEPT
([system-0-4-egate](reviews/system-0-4-egate-2026-06-10.md)). The second was
produced *without reading the first* — rubric fixed from the GAPS close
criteria alone — and found a Major the first had passed over: a budget breach
degraded silently, with no audit row and no alert, against spec §5.9's "budget
breach degrades to mechanical-only **and alerts**"
([system-0-4-egate-INDEPENDENT, F1](reviews/system-0-4-egate-INDEPENDENT-2026-06-10.md)).
**Taught:** independence is a property of the reviewer's inputs; a second gate
that reads the first verdict is a rubber stamp with extra steps. **The rule now
lives in:** the independence notes now standard in system-level verdicts
(e.g. [soak-go-gate](reviews/soak-go-gate-2026-06-12.md) header) and the
fixed-rubric-first structure of every gate file.

### 4.3 The vacuous-test class, and why mutation checks became standard

Green tests kept turning out to prove nothing. Alert-routing tests whose names
claimed audit-row assertions never inspected the audit sink
([t41-remediation2-gate, Finding 3](reviews/t41-remediation2-gate-2026-06-11.md));
the perp gates' at-boundary equality was unpinned — mutations flipping `<` to
`<=` *survived* the suite
([GATE-FINDINGS-LATEST, track-C fix item F2](reviews/GATE-FINDINGS-LATEST.md))
until boundary tests were added and the same mutations were executed red
([track-c-final-gate, A1](reviews/track-c-final-gate-2026-06-12.md)). Three
vacuous tests were caught by mutating their subjects
([orchestration.md §5](design/orchestration.md)). **Taught:** "green-only
verification of tests is not verification" — a test earns trust the first time
you watch it fail. **The rule now lives in:**
[orchestration.md §5](design/orchestration.md) — mutation checks are standard
for any commit whose deliverable is a test.

### 4.4 The per-segment dedup: the boundary-crossing lesson

The daemon gate reproduced a halt re-audit flood: the poller re-applied a
standing halt every poll, ~2 audit rows/second for the life of any halt
([t41-daemon-gate, Major 2](reviews/t41-daemon-gate-2026-06-11.md)). The fix
added dedup state and a committed test — 20 polls, exactly one audit row —
which passed. The next gate's scratch probe drove the same halt through
`drive()`, which re-enters the run loop once per segment and resets the dedup
state: four segments, `halts_applied: 4`. The committed test never crossed a
segment boundary, so it could not see its own failure
([t41-remediation2-gate, Major 1](reviews/t41-remediation2-gate-2026-06-11.md)).
Round three hoisted the state to `drive()` scope and committed a test that
crosses three segment boundaries — verified FIXED
([rota-slices-gate, A1](reviews/rota-slices-gate-2026-06-11.md)). **Taught:**
state scoped below a re-entry boundary resets at that boundary; a test must
cross the same boundaries production crosses. **The rule now lives in:** the
boundary-crossing tests themselves
(`a_standing_halt_audits_exactly_once_across_segment_boundaries`) and the
verifier habit of probing through the composition, not the unit.

### 4.5 The first true red seed, and the bidirectional fix (2026-06-12)

After tens of thousands of clean seeds — the corpus was empty long enough that
determinism anchors were committed just so corpus replay pinned *something*
([dst-corpus/README.md](../crates/fortuna-core/dst-corpus/README.md); the three
`anchor-*.seed` files) — the track-C final gate's 10,000-seed run went red:
seed `11819682492387934495`, wild mark drift pushing a perp position's notional
past the harness's last risk-curve tier; the margin sim fail-closed (correct
production behavior, spec §5.15) and the harness counted the designed refusal
as a failure. BLOCK anyway — a red seed is never accepted on explanation
([track-c-final-gate, Finding 1](reviews/track-c-final-gate-2026-06-12.md)).
The re-gate demanded proof in both directions: pre-fix harness reproducing the
exact red at the gate master (exit 101, byte-identical), post-fix harness green
byte-identically, the seed committed to the regression set with its full story
([track-c-regate](reviews/track-c-regate-2026-06-12.md)). **Taught:** red seeds
are corpus assets, and a fix without a pre-fix reproduction is a claim, not a
proof. **The rule now lives in:**
[dst-corpus/README.md](../crates/fortuna-core/dst-corpus/README.md) (never
delete; every seed carries its failure story) and DoD item 3
([CLAUDE.md](../CLAUDE.md)).

### 4.6 The perps merge that the gates passed and the post-merge check caught

Track C's perps line cleared three successive gates — the B0/B1 remediation
re-gate ([perps-b0-b1-remediation-regate](reviews/perps-b0-b1-remediation-regate-2026-06-11.md),
ACCEPT), the cumulative perp-gates gate
([track-c-perp-gates-gate](reviews/track-c-perp-gates-gate-2026-06-12.md),
ACCEPT-WITH-GAPS), and the final re-gate
([track-c-regate](reviews/track-c-regate-2026-06-12.md), ACCEPT-WITH-GAPS) —
and the signed merge into main *still* failed the post-merge integration
check: a kinetics test's pinned client_order_id differed between track-C's
tree and the merged tree, because main had moved underneath it and the
combination shifted the id derivation. The merge was reverted on the spot, and
the load-bearing question it exposed — is crash-resubmission idempotency
stable across upgrades? — was adjudicated and mutation-pinned (recovery reads
the *persisted* id; it never re-derives)
([GATE-FINDINGS-LATEST, "PERPS MERGE REVERTED"](reviews/GATE-FINDINGS-LATEST.md);
[soak-go-gate §A](reviews/soak-go-gate-2026-06-12.md)). **Taught:** per-branch
gates test pre-merge heads; only the merged combination tests the interaction.
**The rule now lives in:** [orchestration.md §3](design/orchestration.md) —
the post-merge integration check is mandatory, and a red check reverts.

### 4.7 Coda: the first unconditional ACCEPT

Of the 34 verdict lines recorded through 2026-06-12 (`grep "Verdict:"
docs/reviews/*.md`), classified by the leading verdict on each line: 17 open
BLOCK, 14 open ACCEPT-WITH-GAPS, 3 open ACCEPT (one line carries a dual
automatic-BLOCK/engineering-merits verdict and is counted as its leading
BLOCK). The findings bus calls only the last of the three ACCEPTs, the soak GO,
"the first unconditional" (the earlier two carried parked operator conditions)
([GATE-FINDINGS-LATEST](reviews/GATE-FINDINGS-LATEST.md)). It was earned, not
granted: every standing hold closed with executed evidence — 803/0/0 workspace,
10,000-seed DST across all stages plus the corpus anchors, every mutation red,
the one money-path commit changing zero source lines with its safety claim
pinned in both directions ([soak-go-gate](reviews/soak-go-gate-2026-06-12.md)).
That is the point of the doctrine: a GO that means something, because every
prior verdict had teeth.

## 5. How to run everything

All commands run from the repo root. Postgres note: the workspace defaults
`DATABASE_URL` to `postgres://localhost/fortuna_dev`
([.cargo/config.toml](../.cargo/config.toml)); sqlx tests create throwaway
`_sqlx_test_*` databases on that server and never touch the operator database.

**The battery** (the definition-of-done set, [CLAUDE.md](../CLAUDE.md); run all
four, in the same session as the commit they justify):

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/run-dst.sh
```

**DST tiers and reproduction** ([scripts/run-dst.sh](../scripts/run-dst.sh);
the argument is the seed count per stage, default 2000; gates run 10000):

```bash
scripts/run-dst.sh 10000                                # gate tier
DST_MASTER_SEED=8675309202606 scripts/run-dst.sh 10000  # reproduce a recorded batch exactly
scripts/replay.sh --seed 777                            # verbose replay of one seed (a committed anchor)
```

Single harnesses, when localizing a failure:

```bash
cargo test -p fortuna-core --test dst -- --nocapture --seeds 2000
SYNTH_DST_SCENARIOS=2000 cargo test -p fortuna-runner --test synthesis_dst -- --nocapture
SETTLE_DST_SCENARIOS=2000 cargo test -p fortuna-runner --test settlement_dst -- --nocapture
cargo test -p fortuna-live --test daemon_smoke -- --nocapture
```

(The perp DST harness, `cargo test -p fortuna-state --test perp_dst`, is on
`main` and runs as a standard stage of `scripts/run-dst.sh`, alongside the
`perp_event_basis` and `funding_forecast` DST arms.)

**The invariant suite** (protected crate — see
[CLAUDE.md](../CLAUDE.md) before touching anything here):

```bash
cargo test -p fortuna-invariants
```

**The kill-switch drill** (monthly, I4; designed to run with the main runtime
down and Postgres optionally stopped):

```bash
scripts/killswitch-test.sh
```

**Corpus rules** (binding; full text in
[dst-corpus/README.md](../crates/fortuna-core/dst-corpus/README.md)):

- Never delete a regression seed. The corpus replays before every randomized
  run; red there means a regression.
- Every red seed gets minimized (procedure in the README), gets its
  failure-mode story written into the seed file comment, and gets committed.
  A seed without its story is useless in six months.
- Replay verification of recorded streams: `scripts/replay.sh <recording.jsonl>`
  ([scripts/replay.sh](../scripts/replay.sh)).
