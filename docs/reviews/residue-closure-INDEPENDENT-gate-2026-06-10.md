# Review: residue-closure (independent gate) — 2026-06-10
Base: 10443c5  Head: b8fa0c8  Verdict: BLOCK
Protected crate touched: no (git diff 10443c5...b8fa0c8 -- crates/fortuna-invariants/ = 0 lines)

Range: 1f63068 (.env.example), c2c123c (credentials verified live, operator),
eab031e (two INDEPENDENT gate verdicts recorded + .gitignore .keys/),
b8fa0c8 (9-of-11 residue closure). Reviewed in detached worktree
/tmp/fortuna-resgate; implementer rationale not read; docs/reviews/ read only
for the S4 tamper check (name-status, no content).

## Criteria (fixed before reading the diff)

### S — secrets
- S1 range-diff secret sweep: PASS — patterns (xoxb/xapp/sk-ant/PEM/AKIA/base64>=60/
  hex32/secret-keywords) hit only placeholders `sk-ant-REPLACE-ME`, `xoxb-REPLACE-ME`
  in .env.example and prose; c2c123c diff read line-by-line: no key material, no
  channel IDs, no URLs with tokens.
- S2 placeholder-only .env.example; .env/.keys untracked+ignored: PASS —
  every value is REPLACE-ME/placeholder; `git check-ignore -v` matches `.env`
  (.gitignore:3 `*.env`) and `.keys/test.pem` (.gitignore:7 `.keys/**`);
  `git ls-files .keys/` = 0 tracked files.
- S3 live-exercise record clean + operator-gated design respected: PASS —
  record (GAPS.md "Operator-blocked: credentials", c2c123c) claims: Postgres
  migrated, 23 relations owned by fortuna_app, connection verified as app role;
  ONE Anthropic claude-haiku smoke call returning "FORTUNA smoke OK", 16 in / 8 out
  tokens (the GAPS-recommended first exercise; tight CostBudget is implied by "the
  recommended" phrasing + token counts, not separately evidenced); Slack bot
  `fortuna` auth.test ok + test post in ALL FIVE channels; FORTUNA_DEADMAN_URL set,
  deliberately NOT pinged (arming rationale recorded); Kalshi NOT exercised — key id
  set, demo-environment confirmation explicitly remaining, killswitch pair later.
  No live trading. No secret material in the record.
- S4 eab031e adds-only under docs/reviews/: PASS — `git log --diff-filter=M
  10443c5..b8fa0c8 -- docs/reviews/` empty; `git show --name-status eab031e` = A for
  both verdict files (M only .gitignore, GAPS.md).

### R — the 9 claimed closures
- R1 mind-spend in gauge block: PASS — runner.rs metrics_export: the metric now sits
  in the second tuple block emitting `counter: false` (first block emits
  `counter: true`). No type-asserting test exists (presence-only,
  sim_loop.rs:359); noted Minor.
- R2 assembler checked_add fail-closed: PASS — stream.rs:160-167 `checked_add` else
  `Err(VenueError::Invalid{..resync})`; verified by reproduction: verifier scratch
  test (snapshot qty i64::MAX-1 + delta i64::MAX) refuses cleanly in debug profile,
  1 passed. No committed regression test; noted Minor.
- R3 strict envelope parsing + provenance: PASS — ws.rs refuses missing/empty type
  tag and missing sid/seq on book frames (`sid_seq` helper); pinned by
  `frames_without_type_or_book_ids_are_refused` (ran: ok). Provenance: archived
  AsyncAPI (docs/research/venue/kalshi-api-2026-06-10) marks `type`, `sid`, `seq`
  `required: true` on BOTH orderbook_snapshot (raw/pages/websockets_orderbook-updates.md
  lines 76-92) and orderbook_delta (lines 284-301), and every server-message
  envelope schema in raw/asyncapi.yaml lists `required: - type` (lines 2309-2665
  region); yes_dollars_fp/no_dollars_fp documented `required: false` and the parser
  keeps absent-side legal, unknown TYPED frames still map to Ignored — strictness
  cannot reject documented-valid frames.
- R4 crossed-book + non-array-side refusal tests: PASS — committed in
  crates/fortuna-venues/tests/stream.rs; ran:
  `crossed_assembled_books_and_non_array_sides_are_refused ... ok`.
- R5 staggered mock, reverse completion, leg-order results: PASS —
  `out_of_order_completion_still_reports_and_journals_in_leg_order ... ok`;
  the mock yields M1=6/M2=3/M3=1 forcing completion M3,M2,M1; outcome venue ids
  assert [ov-2, ov-1, ov-0] (slot order = input order) and every intent journals
  Acked. Journal-stream order is pinned transitively (Phase 2 of
  submit_group_concurrent, manager.rs:485-486, journals sequentially in leg order),
  not by direct stream comparison — acceptable, noted.
- R6 Arm::MultiLegGroup in the standard battery at corpus scale: PASS — ARMS
  const is [Arm; 11] in settlement_dst.rs (run-dst.sh stage 4, standard battery);
  full corpus run at 10000:
  `[settlement-dst] arms {SettleClean: 933, SettleThenCorrect: 915, Void: 929,
  Dispute: 884, VenueMismatch: 951, CanonicalDivergence: 892, OrphanScan: 860,
  AuditDeath: 903, WideBook: 881, Overdue: 925, MultiLegGroup: 927}; 1843
  discrepancies, 2669 watchdog rows, 1854 halts` — MultiLegGroup hit 927 times,
  zero violations. Arm seeds ack-delay/api-error/reject faults and asserts
  orders_submitted <= 2 per two-leg group.
- R7 settlement_voids/reversals post-state asserts: PASS — settlement_loop.rs
  adds `counters().settlement_reversals == 1` and `settlement_voids == 1`
  post-tick asserts; pre-state ==0 assert preserved (refactored inline, semantics
  identical); suite ran 12 passed.
- R8 ASSUMPTIONS corrections visible: FAIL (half) — Kelly legs[0] note IS added
  (ASSUMPTIONS.md +3 lines, visible). But the claimed
  "degrade_alerts/CalibrationParamsRepo not-yet-live overstatements corrected in
  ASSUMPTIONS" (GAPS CLOSED list + commit subject body) does NOT exist: the range
  diff for ASSUMPTIONS.md contains ONLY the Kelly note; the degrade-alert passage
  is byte-identical at 10443c5 and HEAD and still reads present-tense ("The live
  composition diffs counters per scrape and routes through SlackRouter") while
  GAPS REMAINING admits the scrape consumer and live call site do not exist.
- R9 Polymarket 96->95 erratum in the research doc: FAIL — the range diff never
  touches docs/research/; `grep -ri erratum docs/research/` = no hits; research.md
  contains no "95"/"96" source count and `git log --all -S"96 archived"` shows it
  never did. The 96->95 correction is real and visible in GAPS.md (not silent),
  but GAPS now asserts twice that an erratum is "noted in the doc" /
  "in the research doc" — a correction that does not exist where claimed.

### D — battery and sweeps
- D1 cargo fmt --check: PASS (exit 0).
- D2 cargo clippy --workspace --all-targets -- -D warnings: PASS (Finished, no
  warnings).
- D3 cargo test --workspace: PASS — 657 passed / 0 failed / 0 ignored across 88
  test binaries, exit 0 (matches the commit's claimed 657).
- D4 fortuna-invariants per-test: PASS — i1 (2), i2 (2), i3 (1), i4 (1, 10.57s),
  i5 (1), i6 (3), i7 (3) + 3 doctests, all ok, 0 failed.
- D5 scripts/run-dst.sh 10000: PASS —
  `[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations` (master
  seed 1781156458450); `[synthesis-dst] master seed 1781156857434 -> 10000
  scenario(s) ... totals: 26897 orders, 41173 proposals, 131015 cognition
  failures, 116718 beliefs ... ok (93.02s)`; settlement stage per-arm line quoted
  at R6, ok (16.49s); `DST exit: 0`. Observation: the regression corpus directory
  holds only a README (0 seeds) — pre-existing since T0.4, not a range defect.
- D6 mechanical + test-weakening sweep (range diff): PASS — no #[ignore], no
  proptest case reductions; the single deleted assert line
  (`assert_eq!(voids_before, 0, ...)`) is re-stated inline with identical
  semantics and a STRONGER ==1 post-state added; no SystemTime/Instant/Utc::now,
  no f64/f32 added in src (pattern hits are verdict-file prose); new
  unwrap/panic only in test files; seq_by_sid is a BTreeMap (deterministic); new
  `place(` site is the test mock taking `fortuna_gates::GatedOrder`; no GatedOrder
  construction outside fortuna-gates.
- D7 protected crate: PASS — untouched (0-line diff).

## Findings
- [Major] False closure claim in the ledger: GAPS CLOSED list says the
  degrade_alerts/CalibrationParamsRepo not-yet-live overstatements were
  "corrected in ASSUMPTIONS"; no such correction exists in the range
  (ASSUMPTIONS.md diff = Kelly note only; degrade passage byte-identical at base
  and head and still present-tense about a live composition GAPS itself lists as
  REMAINING). Reproduction: `git diff 10443c5...b8fa0c8 -- ASSUMPTIONS.md` +
  byte-compare of `git show 10443c5:ASSUMPTIONS.md` lines 44-56 vs HEAD.
- [Major] False closure claim in the ledger: GAPS asserts a Polymarket
  source-count erratum is "noted in the (research) doc"; the doc was not modified
  in the range, contains no erratum and no 96/95 count, and never contained one
  (`grep -ri erratum docs/research/` empty; `git log --all -S"96 archived" --
  docs/research/venue/polymarket-us-2026-06-10/research.md` empty). The visible
  96->95 correction lives only in GAPS.md itself.
- [Minor] Assembler overflow refusal (R2) has no committed regression test —
  behavior verified by a verifier scratch test (refuses, no debug-profile panic).
- [Minor] Mind-spend gauge type (R1) has no type-asserting test; only a
  name-presence assert (sim_loop.rs:359).

Remediation for the Majors is a small ledger-correction commit: either make the
two claimed corrections for real (ASSUMPTIONS not-yet-live wording; an erratum
note in the research doc) or restate the GAPS CLOSED entries to say where the
corrections actually live. All seven code/test closures are genuine; secrets
posture is clean; the full battery is green.

## Commands run (verbatim results)
- cargo fmt --check -> exit 0
- DATABASE_URL=postgres://localhost/fortuna_dev cargo clippy --workspace
  --all-targets -- -D warnings -> "Finished `dev` profile ... in 23.54s" (clean)
- cargo test --workspace -> 88 result lines, "total passed: 657 total failed: 0",
  exit 0
- cargo test -p fortuna-invariants -> 13 tests + 3 doctests, all ok
- scripts/run-dst.sh 10000 ->
  "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations"
  "[synthesis-dst] totals: 26897 orders, 41173 proposals, 131015 cognition
   failures, 116718 beliefs" / "test result: ok. 1 passed ... 93.02s"
  "[settlement-dst] arms {SettleClean: 933, SettleThenCorrect: 915, Void: 929,
   Dispute: 884, VenueMismatch: 951, CanonicalDivergence: 892, OrphanScan: 860,
   AuditDeath: 903, WideBook: 881, Overdue: 925, MultiLegGroup: 927}; 1843
   discrepancies, 2669 watchdog rows, 1854 halts" / "DST exit: 0"
- targeted: frames_without_type_or_book_ids_are_refused ok;
  crossed_assembled_books_and_non_array_sides_are_refused ok;
  out_of_order_completion_still_reports_and_journals_in_leg_order ok;
  settlement_loop 12 passed (incl. both post-state asserts)
- scratch reproduction (R2): scratch_delta_overflow_refuses_and_leaves_book_untouched
  -> ok (then deleted; never committed)
