# Completion-state audit — 2026-06-13 (verifier-owned; AUTHORITATIVE remaining-items checklist)

Audited state: main @ 7431e5b (tree moved from c139386 mid-audit — three concurrent
commits 3b52bf0/e7a6005/7431e5b, ALL doc-only: `git diff c139386..7431e5b --stat --
crates/ scripts/ config/ Cargo.toml Cargo.lock` = empty; battery attributes to both).
Method: every claim below verified by command in this session; nothing inherited from
the operator draft or prior sessions.

MAIN BATTERY (this session, CARGO_TARGET_DIR=/tmp/fortuna-gate-target, true exit codes):
    cargo fmt --check                                      # exit 0
    cargo clippy --workspace --all-targets -- -D warnings  # exit 0
    cargo test --workspace                                 # TRUE exit 0; 110 suites "test result: ok"; zero FAILED lines
    scripts/run-dst.sh                                     # TRUE exit 0; "[dst] OK: 3 corpus + 2000 random seeds, zero invariant violations"
MAIN IS GREEN at 7431e5b.

## A. Track frontiers (exact, verified)

| ref | head | main..X | state |
|---|---|---|---|
| main | 7431e5b | — | GREEN (battery above) |
| track-b | 43b340b | 0 commits | FULLY MERGED (8966a00 final gate merge + 8c50945 orphaned-minors F-1/F-2; gates: track-b-cli-slice1, track-b-final ACCEPT-WITH-GAPS, cheap-tier minors gate) |
| track-c | 3c8b126 | 0 commits — ANCESTRY ONLY | merge a586b4a (a586b4a^2 = 3c8b126, i.e. ALL of track-c) was CONTENT-REVERTED at 19b3888. The perps plane is NOT in main's tree: `git grep max_leverage main` = 0 hits; no kinetics adapter source in `git ls-tree main`. Gates: track-c-perp-gates + track-c-final (BLOCK) + track-c-regate (ACCEPT-WITH-GAPS) all pre-merge. |
| track-d | d064688 | 0 commits | merged at e7a6005 (doc-only design: four-layer trust framework). NO track-d gate verdict exists; per e537e7e the framework's V-concerns become the gate rubric — the first track-d gate should note the design merge retroactively. |
| build/phase-0 | eab031e | 0 commits | historical, fully merged |

Commits anywhere NOT in main: exactly the 7 pre-history-rewrite shadows under
refs/original/refs/heads/main (= fc1d2f3; verified `git for-each-ref refs/original/`).
That is the pending PURGE FINALIZATION, not live work. No stray branches; worktrees
wt-b/wt-c/wt-d sit at their merged heads (wt-b carries untracked .sqlx litter only).

UN-GATED FRONTIER ON MAIN (after soak-go-gate head 8ea8a4d): two code commits —
1e5ff71 (fortuna-live gates-view spec number; in-message battery claim at 10k) and
c139386 (recorder example KALSHI_WS_SECS knob; verifier-authored) — plus the docs-set
commit 3b52bf0 whose standing gate verdict is BLOCK (re-gate not yet recorded, see F).
These fold into the next verifier firing. Nothing else anywhere is un-gated.

## B. BUILD_PLAN Phases 4–5 box-state truth

Ticked boxes spot-verified (they stand):
- T4.1 [x]: docs/reviews/t41-completion-gate-2026-06-12.md (BLOCK -> ADDENDUM line 250:
  "M1 remediation re-gate" cleared bidirectionally) + docs/reviews/
  soak-go-gate-2026-06-12.md (ACCEPT, 803/0/0, run-dst.sh 10000, M2 graded RESOLVED
  via slices B1-B4). STANDS.
- T4.4 [x]: docs/reviews/track-b-final-gate-2026-06-12.md (ACCEPT-WITH-GAPS; T4.4
  slices 2+3 in the gated range). STANDS. Residual operator action: one manual
  fortuna-cli.md §13 runbook execution before managed-lifecycle adoption.
- T5.0/T5.B0/T5.B1 [x]: stand on perps-b0-b1-remediation-regate-2026-06-11.md.

Unticked boxes, classified:
- T4.2 [ ] — decomposed in E. Mostly BUILDABLE-NOW (track A).
- T4.3 [ ] — decision package in D. R12 PASSED; two named remainders.
- Phase 4 EXIT — OPERATOR-GATED (soak start) then 7 days + verifier grading.
- T5.B2–B6 [ ] on main — BUILT AND GATED on track-c; ticks reverted with 19b3888.
  Restored mechanically by the re-merge (C). Classification: BUILDABLE-NOW
  (pool / operator-restarted track C) — the re-merge package, nothing more.
- T5.B7 [ ] — genuinely unbuilt (rung-0 perps strategies; fee-trap rule binding).
  BUILDABLE only AFTER the re-merge. Track-C ownership. Three track-c-regate Minors
  ride with it (corpus-placement doc, designed-arm pin tightening, invariant-3
  fail-noisy call site).
- T5.B8 [ ] — genuinely unbuilt (kill-switch perps flatten, margin/funding telemetry,
  funding-regime ROTA panel). After re-merge; the ROTA panel mounts per the T4.5
  cross-reference (implementer-loop-track-c.md:36) — coordinate ownership.
- DESIGN GAPS in the plan itself:
  (1) T4.5 is cited by orchestration.md:14, track-a-completion-queue.md:45-47, and
      GAPS.md:1273 ("operator also slotted BUILD_PLAN T4.5 (ROTA v1.1 deferred
      panels)") — but NO T4.5 ENTRY EXISTS in BUILD_PLAN.md
      (`git grep T4.5 main -- BUILD_PLAN.md` = 0 hits). The entry (scope + TEST RULE)
      must be written before T4.5 work starts. DESIGN-DECISION-PENDING (operator
      wording; track A can transcribe).
  (2) Phase 5 has NO EXIT line (Phase 4's is BUILD_PLAN:522; Phase 5 ends at T5.B8).
      DESIGN-DECISION-PENDING (operator; B7's I7 forward-validation is the de facto
      floor).

## C. Perps re-merge package — precise state

Moved since the revert diagnosis: ONLY c25b368 (track A, on main) — the exec
adjudication. Core ruled UPGRADE-SAFE (crash recovery reads the persisted
client_order_id from the journal fold, never re-derives; mutation-proven pin
`crash_recovery_adopts_a_resting_order_via_its_persisted_client_order_id`,
crates/fortuna-exec/tests/manager.rs). Disposition: option-2 (fix the TEST) is
correct and is track-C domain.

NOT moved (track-c head unchanged at 3c8b126 = the reverted merge's second parent):
1. Kinetics client-id test fix — NOT DONE. track-c
   crates/fortuna-venues/tests/kinetics_adapter.rs:153-168
   (`place_maps_gated_order_to_the_recorded_create_request`) still pins the
   tree-state-dependent expectation (wire body asserted verbatim against the recorded
   meta including the client id); the merged-tree id-shift WILL recur. Fix per
   c25b368: derive the expected id through the same derivation path (or seed IdGen by
   construction), compare the wire body modulo the derived id.
2. 2x leverage cap — NOT BUILT anywhere. operator-decisions-2026-06-12.md item 4
   (approved 21:30 UTC). track-c has the per-asset `max_leverage_x10` gate machinery
   + venue-curve check, but: no committed [perp] 2x config entry (test fixtures use
   50 and 200 = 5x/20x), no min(config, venue curve) boundary pin at 2.01x-refused /
   1.99x-passes, no ASSUMPTIONS I7-style-loosening note.

RE-MERGE PROCEDURE (standing signatures remain valid: waive batch 5 + F1 disposition,
operator-decisions items 1-2; GATE-FINDINGS: "Operator signatures (batch 5 + F1)
REMAIN VALID; the merge re-runs after the fix + re-gate"):
  a. NOTE: `git merge track-c` is now a NO-OP (full ancestry). The restore is
     `git revert 19b3888` (revert-of-revert) on a branch off main — this restores the
     perps plane INCLUDING the protected-crate pure additions (covered by the standing
     batch-5 waive; the touch still auto-flags — cite the waive in the verdict) and
     restores the T5.B2-B6 ticks.
  b. Land fix 1 + build item 2 on that branch (the three regate Minors may ride or
     stay ledgered for B7).
  c. FULL re-gate at 10k: fmt / clippy -D warnings / cargo test --workspace /
     run-dst.sh 10000 (+ PERP_DST_SCENARIOS=10000), detached gate worktree,
     CARGO_TARGET_DIR=/tmp/fortuna-gate-target.
  d. Merge to main; POST-MERGE INTEGRATION CHECK on merged main (cargo test
     --workspace + clippy minimum) MUST explicitly include the previously-failing
     kinetics test (kinetics_adapter.rs place_maps_...) — the combination is what
     failed last time.
Estimated: 1-2 implementer iterations + 1 verifier firing.

## D. T4.3 tick decision package (operator)

SHIPPED + GATED: slices 1-7 incl. money view SIM-ONLY subset per R6 with honest NULLs
for floating/total (money-view-gate-2026-06-12.md ACCEPT-WITH-GAPS); slices 8-9
cognition view R7 + presentation layer + wheel/cornucopia logo (track-b-final-gate
ACCEPT-WITH-GAPS); gates-view real spec number (1e5ff71); R12 BROWSER PASS PASSED
2026-06-12 ~10:25Z (GATE-FINDINGS R12 section; screenshots verified on disk:
docs/reviews/rota-visual/rota-r12-{desktop-clean,halt-takeover,mobile-390}.png).

REMAINING (the only two): (1) full §5 money model (mark-loop floating + per-strategy
attribution) — operator/design call by the box's own wording; (2) audit-recents
queries (recent_rejections / recent_watchdog — confirmed absent:
fortuna-live/tests/views.rs:116,145 assert the fields are none, "LATER slices").

THE DECISION, precisely: tick T4.3 NOW if and only if both remainders are explicitly
re-scoped into the (to-be-written) T4.5 ROTA v1.1 entry — which the operator already
slotted for exactly this purpose (GAPS.md:1273) — with sub-checkboxes, M2-precedent
style (disclosed, never silently dropped). Otherwise the box stays unticked until
they land. Recommendation: re-scope + tick; every in-scope R-criterion of the
authoritative design doc has executed gate evidence.

## E. T4.2 decomposition (owner: TRACK A; sequencing already pinned in
##    docs/design/track-a-completion-queue.md item 2)

| component | class | grounds | est |
|---|---|---|---|
| (i) WS dial (signed handshake, keep-alive, redial+resubscribe, seq-gap resync) | BUILDABLE-NOW | ws__*.jsonl fixtures EXIST (101 on both flag states, ~10s ping cadence, snapshot+deltas); redial test cases = the LEDGERED venue failures (reset-without-close, 502-on-reconnect; fixtures/kalshi/README.md 2026-06-13 entry). Mock transport only. | 2-3 iter |
| (ii) recorded-stream replay -> PaperVenue (both mech strategies) | SPLIT | book/delta replay BUILDABLE-NOW (fixtures exist); TRADE-THROUGH portion OPERATOR-GATED — the public trade frame was NEVER captured (venue-side recapture failure ledgered); never fabricate trade fixtures. | 2 iter now + 1 post-capture |
| (iii) adapter paper/live clearance validation | BUILDABLE-NOW | 27-item checklist (docs/research/venue/kalshi-api-2026-06-10/research.md) vs fixtures/kalshi/, each item PASS/FAIL/UNCOVERABLE with fixture cited; 7-8 known gaps stay ledgered. OUTPUT IS A CLEARANCE RECORD THE OPERATOR SIGNS; venue=kalshi stays refused until that signature. | 1-2 iter + operator signature |
| (iv) kill-switch KalshiVenue plug | BUILDABLE-NOW (code) / OPERATOR (creds+live drill) | mock transport; I4 dependency rules absolute. FORTUNA_KILLSWITCH_* pair should come from the ROTATED keys (G.2). | 1 iter |
| (v) Slack Socket Mode listener | BUILDABLE-NOW (code) / OPERATOR (app token) | research contract + mock transport; kill REQUESTS only, re-arms stay CLI-only. | 1-2 iter |

## F. Small items

- M3 REARM NOTICES — OPEN, now POOL-RELEASED TO TRACK A (track-a-completion-queue.md
  item 1 supersedes the GAPS boundary-block). Verified missing: zero hits for the
  pending-restart notice in fortuna-cli/fortuna-ops on main. MUST land BEFORE the
  first soak halt drill. 1 iteration.
- DOCS SET — landed at 3b52bf0 mid-audit claiming the docs-gate BLOCK findings fixed.
  Spot-checks confirm all three Majors fixed in the claimed direction
  (backup-restore.md:58-61 version requirement; FINAL_REPORT.md:207 "THE LIVING
  PROCEDURE is docs/runbooks/soak-start.md"; operations.md:453-455 "has NOT been
  started"). BUT the standing recorded verdict (docs/reviews/2026-06-12-docs-gate.md)
  is BLOCK and NO RE-GATE IS RECORDED. VERIFIER: execute the cheap re-gate (re-run
  the failing pg_dump version-matched; verify the §5/§3 edits; sweep the Minors) and
  record the addendum. Until then main carries BLOCK-verdict material.
- NEWS / TRACK-D PHASE A — design fully merged (cc099b4 + d064688 via e7a6005;
  db0ce00 Mind-tiering precision). Phase A binding scope: Layers 0-2 (admission
  dossiers, structural validation, corroboration), fixtures FIRST under
  fixtures/sources/ (DOES NOT EXIST yet — zero code started), NO model in the
  ingestion path, drive() seam = ONE flagged minimal commit in fortuna-live (track A
  accommodates on rebase, never rewrites). BUILDABLE-NOW by track D. Est 4-8 iter.
- BUS ITEMS UNOWNED: none remaining. F-1/F-2 closed (8c50945); M3 re-owned (track A);
  perps package owned (pool / restarted track C); #2 composition-root pool-distinct
  guard deferred-with-rationale (rides the next main.rs change — track A backlog,
  completion-queue item 4); three track-c regate Minors ride with B7; T4.3 remainders
  pend the operator call (D).
- HYGIENE: wt-b untracked .sqlx cache files; main untracked .claude/ralph-loop.local.md;
  T4.5 dangling references (B.1); recorder single-writer rule — do NOT kill pid 79813.

## G. OPERATOR-ONLY actions, final (procedure pointers exact)

1. START THE 7-DAY SOAK (Phase-4 EXIT critical path; everything else parallels):
   docs/runbooks/soak-start.md (committed at 3b52bf0; runbook-governs) — release
   build, .env (DATABASE_URL, FORTUNA_SLACK_BOT_TOKEN + 5 channels,
   FORTUNA_DEADMAN_URL, ANTHROPIC_API_KEY or [cognition] allow_stub_mind), then
   `./target/release/fortuna-live config/fortuna.toml`. CAUTION: managed
   `fortuna start` REFUSES while the unmanaged recorder runs (pid 79813, up 1d18h —
   by design); use the raw start per the gate contract
   (soak-go-gate-2026-06-12.md lines ~202-215). Soak-watch arms at the verifier's
   first post-start firing -> docs/reviews/soak-log.md.
2. KEYS ROTATION: GAPS.md:1546-1551 — rotate BOTH Kalshi keys ((demo.)kalshi.co
   Account & security -> API Keys), place new PEMs at the .env paths; runbook
   docs/runbooks/key-rotation-and-secrets.md. Also provisions the
   FORTUNA_KILLSWITCH_* pair for E(iv).
3. PURGE FINALIZATION (irreversible; BEFORE any first push): exact command at
   GAPS.md:1552-1558. Verified still pending this session:
   refs/original/refs/heads/main = fc1d2f3; 7 pre-rewrite commits reachable.
4. FIXTURE RECORDING SESSION(S) (one credential fix, two sessions, back-to-back —
   GAPS.md:1791-1799; runbook docs/runbooks/fixture-recording.md):
   (a) Kalshi EVENT API: the public WS `trade` frame (RETRY guidance ledgered:
   busy market, 180-300s windows x N — fixtures/kalshi/README.md 2026-06-13 entry,
   after the venue-side reset/502 failure), STP `maker` mode, two-sided REST
   orderbook (#20), settlement re-poll. (b) Kinetics residue as needed.
5. KINETICS PROD PARITY POST-FEES: re-check /margin/fee_tiers once the $0 promo ends
   (GAPS.md:1779 "from the June 11 release (re-check then)") + prod-parity
   re-record before first live use (kalshi README gaps #26/#27 pattern). The Sim
   gates re-run when fees activate (fee-trap rule, T5.B7 text).
6. T4.3 TICK DECISION + full §5 money-model design call (D).
7. ADAPTER CLEARANCE SIGNATURE once E(iii) produces the record (the venue=kalshi
   flip is yours alone).
8. SLACK APP TOKEN for E(v).
9. T4.4: one manual fortuna-cli.md §13 runbook execution before adopting the managed
   lifecycle (track-b-final-gate residual).
10. PG ROLE: grant CREATEDB to fortuna_app or accept the documented workaround
    (t41-completion-gate operator item; docs/runbooks/troubleshooting.md).
11. DISK: approve the 35GB main-target cargo-clean idle window (21Gi free at audit
    time — above the 10GB floor).
12. PROMOTIONS LADDER (I7; operator-only forever): post-soak forward-validation
    review of mech_structural / mech_extremes / synthesis Sim records; perps
    strategies additionally bound by amendment C ("promo-$0 never justifies GO").
13. PLAN TEXT: supply the BUILD_PLAN T4.5 entry wording + a Phase-5 EXIT line (B).

## MASTER CHECKLIST (priority order)

| # | item | owner | class | pointer | est |
|---|---|---|---|---|---|
| 1 | Start the soak | OPERATOR | operator-gated | G.1 / soak-start.md | minutes + 7d clock |
| 2 | Docs RE-GATE (convert the standing BLOCK on landed 3b52bf0) | VERIFIER | gate pending | F / 2026-06-12-docs-gate.md | 1 firing (cheap) |
| 3 | Perps re-merge package (kinetics test fix + 2x cap -> 10k re-gate -> revert-of-revert -> post-merge check incl. the kinetics test) | pool / track-C | buildable-now | C | 1-2 iter + 1 firing |
| 4 | M3 rearm notices (CLI + ROTA) — before first halt drill | track A | buildable-now | F / completion-queue item 1 | 1 iter |
| 5 | Main-tail gate (1e5ff71 + c139386 fold into next firing) | VERIFIER | gate pending | A | rides #2's firing |
| 6 | T4.2 (i) WS dial -> (ii) book replay -> (iii) clearance record -> (iv) killswitch plug -> (v) Slack listener | track A | buildable-now (iii/iv/v end at operator signatures/tokens) | E | 7-10 iter total |
| 7 | T4.3 tick decision (re-scope two remainders to T4.5) | OPERATOR | decision | D | one decision |
| 8 | Write BUILD_PLAN T4.5 entry + Phase-5 EXIT line | OPERATOR wording (track A transcribes) | design-decision | B | 0.5 iter |
| 9 | Track-D Phase A (fixtures/sources/ first; Layers 0-2; one flagged drive() seam) | track D | buildable-now | F | 4-8 iter |
| 10 | Keys rotation + purge finalization | OPERATOR | operator-gated | G.2-G.3 | minutes |
| 11 | Trade-frame + residual captures (sessions) | OPERATOR | operator-gated (venue recovered) | G.4 | 1 session |
| 12 | T4.5 panels (after T4.2) | track A | blocked on #6+#8 | completion-queue item 3 | 2-4 iter |
| 13 | T5.B7 strategies + 3 riding Minors; then T5.B8 ops | track C | blocked on #3 | B | 5-8 iter |
| 14 | Composition-root pool-distinct guard | track A | deferred (rides next main.rs change) | GAPS #2 | 0.5 iter |
| 15 | Kinetics prod-parity post-fees; promotions ladder | OPERATOR | post-soak / post-promo | G.5, G.12 | — |

## Corrections to the operator's draft plan

1. "track-b needs landing" — STALE/FALSE. Fully merged: 8966a00 (final, gated
   ACCEPT-WITH-GAPS) + 8c50945 (orphaned minors); `git log main..track-b` = 0.
2. "R12 browser pass pending" — STALE/FALSE. PASSED 2026-06-12 ~10:25Z
   (GATE-FINDINGS-LATEST.md R12 section; rota-r12-* screenshots on disk).
3. Do NOT read `main..track-c = 0` as "perps landed" — the merge was content-reverted
   (19b3888); the perps plane is ABSENT from main's tree. Ancestry masks it.
4. The kinetics client-id finding is adjudicated (c25b368, exec side DONE +
   regression pin) but the track-C TEST FIX IS NOT BUILT; the 2x leverage cap is
   operator-approved but NOT BUILT anywhere.
5. T4.1 M2 is RESOLVED (reviews built + gate-graded; soak-go B1-B4) — no waive
   decision remains there.
6. The docs-gate verdict EXISTS and is BLOCK; the fix commit has since landed
   (3b52bf0) — the open item is the RE-GATE record, not the verdict.
7. T4.5 is referenced in three governing docs but has NO BUILD_PLAN entry; Phase 5
   has no EXIT line.
8. M3 is still open and is now track-A-owned (pool release superseded the
   boundary-block).

Protected crate: UNTOUCHED on main this audit (`git diff` of the gated ranges; the
perps invariant additions sit behind the revert and re-enter under standing waive
batch 5). This audit modified no code, tests, config, or ledgers — this file only.
