# Review: audit-tail-fix-gate (rota-slices remediation + T4.3 slice 4) — 2026-06-11
Base: 75f4782  Head: 2e54c18  Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no (`git diff --name-only 75f4782..2e54c18 | grep -c fortuna-invariants` = 0)

Reviewed at COMMIT 2e54c18 in a detached worktree (/tmp/fortuna-g7); the dirty
working tree was not graded. Independence: no docs/reviews/ file read except
GATE-FINDINGS-LATEST.md (the remediation rubric). Rubric fixed before opening
the diff: findings items 1-5 + rota-dashboard.md AMENDMENTS + BUILD_PLAN T4.3.

## Criteria (fixed before reading the diff)

### A. Remediation (GATE-FINDINGS-LATEST items 1-5)
- A1 cursorless audit-tail = LATEST page, comment aligned, pagination test incl.
  absent-cursor (findings item 1, MAJOR): **PASS** — evidence:
  - Code: `audit_tail_page` (rota.rs) — absent cursor => `ORDER BY audit_id DESC
    LIMIT $1` then `rows.reverse()` (latest page, ASC); present cursor =>
    `audit_id > $1 ORDER BY audit_id ASC` (forward). Doc comment states exactly
    this and cites the F1 defect. Limit clamped [1,500].
  - Committed test ran green: `audit_tail_cursorless_returns_the_latest_page_
    not_the_oldest` (#[sqlx::test], includes the absent-cursor case + an
    explicit regression guard `assert_ne!(page[0].0, oldest)`), plus
    `audit_tail_empty_table_is_an_empty_page_not_an_error`.
  - Verifier scratch reproduction (prior-gate shape, HTTP level, worktree-only
    scratch test, 250 old rows then newest): cursorless GET returned the newest
    100 ENDING at the newest id with `next_after` = newest id (NOT the oldest
    page); full forward walk from the empty cursor via next_after was LOSSLESS
    (251/251 ids, in order, no gaps/dupes); a row inserted after the poll was
    returned by the next poll with `after=<prev next_after>` (the live-tail
    property F1 broke). Note: pagination is forward-only by design — there is
    no backward cursor; "history" is reachable losslessly from `after=` empty.
    ROTA_SHELL polls cursorless (rota.rs:322) => live tail now correct.
- A2 favicon 404 (item 2): **OPEN (carried Minor)** — no favicon route or 204
  stub: `grep -rn favicon crates/ assets/` = 0 hits; route table = /rota + 6
  API routes only. Implementer explicitly ledgered F2 as STILL OPEN in GAPS.md
  (this range). Not claimed fixed by the commit; carried, not waved through.
- A3 compile-time audit query or ledgered exception (item 3): **PASS (ledgered)**
  — ASSUMPTIONS.md gained the entry "ROTA audit-tail query is runtime sqlx, not
  compile-time query!" (rationale: single read-only query, schema-pinned by
  migration, avoids .sqlx build coupling; revisit at R7). The claimed
  #[sqlx::test] coverage exists and ran green this session.
- A4 DailyScheduler restart-fire / cumulative-counters labeling / drive()-level
  digest assertion (item 4): **OPEN (carried Minor)** — no scheduler file in the
  range diff; GAPS.md tracks F4 as STILL OPEN. Not fixed, not yet ledgered as a
  deliberate behavior — only tracked as open work.
- A5 ASSUMPTIONS dead-man contradiction (item 5): **OPEN (carried Minor)** —
  contradiction confirmed live at head: GAPS.md:142-143 "No ASSUMPTIONS
  exception is needed" vs ASSUMPTIONS.md:1220 "The dead-man pinger task reads
  SystemTime::now() at the IO edge" (stale post-RealClock wording). GAPS tracks
  F5 as STILL OPEN.

### B. Slice 4 (ee7ab9d) vs rota-dashboard.md (§5 streams + amendments)
- B1 metadata-only, no content leak: **PASS** — only fs APIs in src/rota.rs are
  `read_dir` (line 115) and `fs::metadata`; view fields are stream name (file
  stem), last_capture_age_secs, size_bytes, healthy. No file content is opened
  or read; no captured venue payload can flow through ROTA. Committed test also
  pins rows_today/key_count ABSENT (deferred, never faked).
- B2 healthy threshold (design §5: age < 120): **PASS** — `age.map(|a| a < 120)`;
  test asserts fresh ~5s => healthy:true and stale 200s => healthy:false with
  age >= 120. Green.
- B3 missing dir / empty day => degraded not 500: **PASS** — `Err(_) =>
  Vec::new()` => empty array; tests: missing-dir => empty array (unit),
  capability-absent => recorder key OMITTED (handler test), merge test green.
- B4 bounded scan: **PASS (with observation)** — flat, non-recursive
  `read_dir` of one day dir. Verifier scratch test: 10,000 .jsonl files +
  a nested dir containing a .jsonl => exactly 10,000 entries (nested NOT
  walked) in 79.7ms. No explicit entry cap exists, but the dir is written
  only by the recorder (server-side, ~a dozen streams/day) and is not
  client-influenceable. Observation, not a finding.
- B5 path traversal: **PASS** — date prefix comes from the server-side
  snapshot generated_at; stream names come from server-side read_dir. The
  streams handler takes no client input at all; the only Query extractor in
  rota.rs is AuditQuery{after,limit} and `after` is only ever a bound SQL
  parameter, never a path component.

### C. Battery at 2e54c18
- fmt: PASS — `cargo fmt --check` clean.
- clippy: PASS — `cargo clippy --workspace --all-targets -- -D warnings`
  finished clean (24.89s).
- workspace tests: PASS — `cargo test --workspace` (DATABASE_URL=
  postgres://localhost/fortuna_dev): every suite "test result: ok", 0 failed
  (~716 passed incl. the 5 new rota tests; prior gate counted 711).
- invariants per-test: PASS — 13/13 named invariant tests + 3 doctests green
  (i1 x2, i2 x2, i3, i4, i5, i6 x3, i7 x3).
- DST: PASS at the DEFAULT corpus (2000) — this range is READ-PATH ONLY
  (dashboard fs-scan, audit-tail read query, docs/ledgers); zero money-path
  contact, so the task-review default applies. The 10,000-seed bar remains
  for money-path changes and phase gates. `scripts/run-dst.sh` exit 0:
  - `[dst] OK: 0 corpus + 2000 random seeds, zero invariant violations`
  - `[synthesis-dst] master seed 1781213359200 -> 2000 scenario(s)` ... ok
  - `[settlement-dst] master seed 1781213381182 -> 2000 scenario(s)`,
    11/11 arms exercised ... ok
  - daemon_smoke 2/2 ok.
  Note: the regression corpus directory contains only README.md — true at
  base AND head (unchanged since T0.4); pre-existing, not a range regression.
- Mechanical sweep over the range diff: PASS — no SystemTime::now/Instant::now/
  Utc::now added outside tests (file-mtime duration_since(UNIX_EPOCH) is not a
  wall read); no unwrap/expect/panic in added src code (all hits are test
  code); paths explicitly sorted (no unordered-map iteration); no place(/
  GatedOrder constructs; no secret literals (grep hits were docs/reviews text).
- Test-weakening sweep: PASS — tests/rota.rs diff purely additive; no
  #[ignore], no proptest case reductions, no deleted asserts in code (minus-
  line assert grep hits are GATE-FINDINGS-LATEST.md prose only).
- Protected crate: untouched (0 files).

## Findings
- [Minor] NEW, reproduced: malformed `generated_at` makes scan_recorder render
  FAKED-FRESH, not degraded — `UtcTimestamp::parse_iso8601(...).unwrap_or(0)`
  => now_ms=0 => `(0 - mtime).max(0)/1000` = age 0 => healthy:true.
  Reproduction (worktree scratch test): `scan_recorder(&base,
  "2026-06-11Tgarbage")` => `{"healthy":true,"last_capture_age_secs":0}`.
  Input is daemon-controlled (always a real clock read today), so Minor — but
  the degraded-never-faked doctrine says a parse failure should render
  unhealthy/null. Implementer should ledger or flip the unwrap_or direction.
- [Minor, carried] F2 favicon 404 still open (no route, no 204 stub) — tracked
  in GAPS by the implementer; due by the T4.3 R12 gate (it is that gate's
  zero-console-errors criterion).
- [Minor, carried] F4 DailyScheduler boot-fire + cumulative-vs-day digest
  labeling + missing drive()-level digest assertion — still open, tracked.
- [Minor, carried] F5 ASSUMPTIONS/GAPS dead-man contradiction — still present
  verbatim at head (GAPS.md:142 vs ASSUMPTIONS.md:1220), tracked.
- [Observation] scan_recorder has no entry cap (flat scan measured 79.7ms at
  10k files — acceptable; server-side dir only). [Observation] dst-corpus/
  holds zero recorded regression seeds (pre-existing since T0.4).

## Verdict rationale
The one MAJOR (F1) is verified fixed by execution: committed sqlx tests green
plus an independent HTTP-level reproduction of the prior gate's shape
(cursorless => newest page; lossless forward walk; live-tail pickup). F3 is
properly ledgered with test coverage. Slice 4 conforms to the design on all
five criteria (metadata-only confirmed by sweep + tests; threshold, degraded
states, boundedness, and no client-controlled paths all evidenced). Battery
fully green at the read-path corpus default. The remaining items are four
Minors (three carried and already tracked as open in GAPS, one new with
reproduction) — none rises to BLOCK. ACCEPT-WITH-GAPS; the implementer owes
the new Minor a GAPS entry or a one-line fix in the next iteration.

## Commands run (verbatim verdict lines)
- `cargo fmt --check` => clean (no output)
- `cargo clippy --workspace --all-targets -- -D warnings` => `Finished
  \`dev\` profile [unoptimized + debuginfo] target(s) in 24.89s`
- `cargo test --workspace` => all suites `test result: ok. ... 0 failed`
- `cargo test -p fortuna-ops --test rota` => `test result: ok. 9 passed; 0
  failed` (incl. audit_tail_cursorless_returns_the_latest_page_not_the_oldest)
- `cargo test -p fortuna-invariants` => 13 tests + 3 doctests, all ok
- `scripts/run-dst.sh` (default 2000) => exit 0;
  `[dst] OK: 0 corpus + 2000 random seeds, zero invariant violations`;
  synthesis 2000 ok (seed 1781213359200); settlement 2000 ok, 11/11 arms
  (seed 1781213381182); daemon_smoke 2/2 ok
- Verifier scratch (worktree-only, removed with it): HTTP cursorless-latest +
  lossless-walk + live-tail repro => ok; 10k-file scan => 79.7ms, 10,000
  entries, nested dir not walked; malformed generated_at => faked-fresh
  (the new Minor's reproduction)
