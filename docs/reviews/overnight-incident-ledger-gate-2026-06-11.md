# Review: overnight-incident-ledger-gate — 2026-06-11
Base: b8fa0c8  Head: 16478bb (main tip at review time: 8544c3f)  Verdict: BLOCK
Protected crate touched: no (`git diff --stat b8fa0c8..16478bb -- crates/fortuna-invariants/` empty)

Part 2 of 2 (incident + ledger + fixture-content scope). Code/battery side covered by
the concurrent verifier; no battery or DST run here by design. All file inspection at
pinned worktree /tmp/fortuna-g2 (detached 16478bb). Independence: no docs/reviews file
read except history-rewrite-2026-06-11.md (in scope as the hash map).

## Criteria (fixed before reading the diff)

### S — Security incident
- S1 Incident entry complete/honest incl. rotation + finalization demands: **PASS** —
  GAPS.md:91-135 names both exposed files, root cause (`>>`-append corrupted `.keys/**`
  to `.keys/**data/` — VERIFIED verbatim in old tree: `git show 7b00ce6:.gitignore`
  last line is `.keys/**data/`), exposure bound (never pushed, machine-local),
  remediation chain, and TWO operator actions: (1) ROTATE both Kalshi keys ("treat as
  compromised per policy even though exposure was machine-local — the live key is also
  the I4 kill-switch credential") and (2) FINALIZE the purge with the exact command,
  "BEFORE any first push". Entry also honestly records (corrected-not-erased) that an
  earlier version falsely claimed finalization "VERIFIED" — fixed by 16478bb (F1d).
- S2 Purge effective on main: **PASS** — `git log main --oneline -- .keys/` empty;
  `git rev-list --objects main` has zero `.keys/`/`data/` paths; PEM sweep
  `git grep -lE "BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY" $(git rev-list main)` (99
  commits) → 3 unique paths, all benign: kalshi/auth.rs:75 (parser doc-comment),
  tests/kalshi_auth.rs (comments + redaction assertion `assert!(!dbg.contains("PRIVATE
  KEY"))`), polymarket research raw page (truncated doc placeholder `MIIEpAIBAAKCAQEA...`).
  refs/original/refs/heads/main exists → fc1d2f3 (old tip), correctly documented in
  GAPS + the rewrite record as the remaining in-.git exposure pending finalization.
  Old objects still hold both keys (`git ls-tree 7b00ce6 .keys/` lists both) — exactly
  as the entry discloses.
- S3 .gitignore + tracking state: **PASS** — `git check-ignore -v` matches `.keys/`
  (line 7) and `data/` (line 8); `od -c` shows all lines newline-terminated incl. new
  `.playwright-mcp/`; `git ls-files .keys data` empty; recorder still writing untracked
  output: data/perishable/2026-06-11/ holds 5 jsonl streams dated today.
- S4 Fixture secret sweep + redaction-by-construction: **PASS** — grep of the FULL
  36-char demo and prod key-id values (from .env, values never printed) over fixtures/:
  zero hits. Header sweep (KALSHI-ACCESS-KEY/SIGNATURE, authorization, bearer,
  set-cookie, cookie): one hit = a checklist note string ("KALSHI-ACCESS-SIGNATURE
  omitted"), benign. No base64 runs >=200 chars; no token field names. Recorder
  (crates/fortuna-venues/examples/record_kalshi_fixtures.rs) redacts by construction:
  REST meta serializes only {recorded_at, environment, host, method, path, status,
  auth-label, request_body, note} (lines 205-215); WS meta likewise (lines 1160-1173);
  request headers are built into the reqwest/tungstenite request objects and never
  serialized; response headers never recorded; demo hosts hardcoded; only demo env
  vars referenced.

### L — Ledger accuracy
- L1(i) degrade_alerts/CalibrationParamsRepo "overstatements corrected in ASSUMPTIONS":
  **STILL-FALSE** — the GAPS.md:80-82 claim is verbatim unchanged from b8fa0c8;
  `git diff b8fa0c8..16478bb -- ASSUMPTIONS.md` contains ONLY the F6 wall-clock entry;
  `grep -i degrade_alerts ASSUMPTIONS.md` → zero hits at head-of-range.
- L1(ii) "Polymarket source count corrected to 95 (erratum in the research doc)":
  **STILL-FALSE** — `git diff b8fa0c8..16478bb -- docs/research/venue/polymarket-us-2026-06-10/`
  empty; `grep -c "\b95\b" research.md` → 0; no "errat*" anywhere under that dir.
- L2 F1-F7 claim-vs-tree: **PASS for F1-F4, F6, F7; F5 unaccounted** —
  F1 verified via S1-S3 (incl. the F1d honesty correction). F2 verified against the
  fixture bodies themselves: nested (auth__bad_signature.json `{"error":{...}}`), flat
  (orders__numeric_field_types.json `{"code":"bad_request",...Go struct field...}`),
  bare (markets__limit_over_max.json `{"msg":"Parameter validation failed..."}`) —
  three shapes exactly as ledgered, corrected-not-erased in README finding 1 + GAPS.
  F3 verified against fixtures: cancel ack ts_ms 1781159364112 / reduced_by "1.00";
  GET meta recorded_at 1781159364471 (~359ms later) returns status "resting",
  remaining_count_fp "1.00"; recancel body `{"error":{"code":"not_found",...}}` —
  race real, poll-until-terminal requirement ledgered (README finding 16). F4 verified:
  README "Coverage statement (corrected...gate finding F4)" enumerates exceptions
  (STP maker unobserved, #20 vacuous empty book, #17 sub-items, settlement, voided,
  series fields, prod parity). F6 verified: ASSUMPTIONS.md:1187 entry present; chrono
  absent from crates/fortuna-recorder/Cargo.toml (grep exit 1). F7 verified: GAPS
  Phase B entry reads CONFIRMED with the prior two-state contradiction noted as
  corrected. F5 appears in NEITHER remediation commit message NOR any tracked ledger
  (grep GAPS/ASSUMPTIONS/BUILD_PLAN/README) — unverifiable within this review's
  read constraints (the defining gate verdict is excluded from my reading); noted,
  not invented as a defect.
- L3 docs/reviews integrity through the rewrite: **PASS** — `git log --diff-filter=M`
  and `--diff-filter=D` over the range on docs/reviews/: both empty. Blob-hash diff
  of `git ls-tree` b8fa0c8 vs 16478bb: all 12 pre-existing verdict blobs identical;
  only three ADDs (residue-closure-INDEPENDENT-gate-2026-06-10.md added in 825d144,
  history-rewrite map in ab810bf, perps-b0-b1-fixtures-gate in 6f34d86), each added
  once, never modified.
- L4 Hash-map spot-check (3): **PASS** — 7b00ce6→94d651a: old unreachable from main,
  subjects identical, delta = 26/26 paths all under .keys|data|.playwright-mcp;
  eb189cc→213e41f: old unreachable, subjects identical, zero non-purge-path deltas;
  0b8670d→1259388: old unreachable, subjects identical, trees byte-identical
  (`git diff` 0 bytes — keys already untracked at that point, as the map implies).

### F — Fixture session content
- F1 Inventory vs 27-item checklist: **PASS (honestly scoped)** — 58 capture files
  (56 .json + 2 .jsonl) + 59 metas incl. session manifest; manifest has exactly 60
  result rows, every stage maps to an artifact ("60 captures" claim consistent).
  Mapping: COVERED 18 (items 1,2,3,4,6,7,8,9,10,12,13,14,15,16,18,21,24,25),
  PARTIAL 6 (5 unauth-prod-half+rate-limit; 11 STP maker mode; 17 cursor
  stability/expired; 19 settlement cent-int half — settlements__page is `{"cursor":"",
  "settlements":[]}`, record pending market close; 22 series fee fields; 23 WS quiet
  market), MISSING 3 (20 empty-book vacuous, 26 prod parity, 27 maintenance window) —
  every PARTIAL/MISSING is named in README Known gaps / GAPS. High-stakes items:
  409-duplicate body COVERED (verbatim `order_already_exists`, nested shape); error
  catalog COVERED (3 wire shapes, counterexamples named); cancel-reconcile race
  COVERED live; fills cursor terminal COVERED (`"cursor":""` in fills__after_taker;
  stability/expired sub-items PARTIAL, ledgered); voided settlement MISSING (cannot
  be forced; ledgered); fee fields on fills COVERED (`fee_cost` 6dp dollars string
  "0.017500" = ceil of 0.07×P×(1−P), quadratic confirmed); WS snapshot/delta COVERED
  in BOTH use_yes_price states (frame-type counts: 1 snapshot + 2/4 deltas + subscribed
  each) but ZERO `trade` frames in either capture (quiet market) — trade message shape
  unobserved.
- F2 No premature paper/live clearance claim: **PASS** — BUILD_PLAN T4.2 unticked
  ("[ ] T4.2 POST-FIXTURE tranche"); GAPS states the adapter "is cleared for Sim
  development ONLY. Paper/live clearance requires operator-recorded fixtures..." and
  the new session entry explicitly lists "REMAINING for clearance (T4.2)". No text in
  the range claims clearance achieved.

## Findings
- [Major] FALSE CLOSURE persists (2nd consecutive gate): GAPS.md:80-82 still claims
  degrade_alerts/CalibrationParamsRepo "overstatements corrected in ASSUMPTIONS" —
  no such correction exists in ASSUMPTIONS.md at 16478bb (grep: zero degrade_alerts
  hits; range diff to ASSUMPTIONS = F6 entry only). Fix: correct-not-erase the GAPS
  sentence or land the actual ASSUMPTIONS correction.
- [Major] FALSE CLOSURE persists (2nd consecutive gate): same GAPS sentence claims
  "Polymarket source count corrected to 95 (erratum in the research doc)" — the
  research doc was untouched in the range, contains no "95" and no erratum. Fix:
  add the erratum or correct the claim.
- [Minor] WS `trade`-channel message shape UNOBSERVED in both captures (0 trade
  frames; quiet market). README discloses the frame composition and promises a
  busy-market capture, but the Known-gaps list does not name the trade shape
  explicitly while GAPS T1.1 lists "public `trade` messages" as required from this
  capture. Ledger it explicitly as a gap item.
- [Minor/observation] F5 is skipped in the F1-F7 remediation surface (commit
  1259388 message claims F1-F4,F6,F7; no F5 in any tracked ledger). If F5 required
  a tree change, it is unremediated; if it was no-action, that disposition should be
  ledgered where the others are.

OPERATOR ACTIONS the incident demands (verified present in GAPS.md:122-135):
1. ROTATE both Kalshi keys (demo + prod/live; the live key doubles as the I4
   kill-switch credential) at (demo.)kalshi.co Account & security → API Keys; place
   new PEMs at the .env paths.
2. FINALIZE the purge (irreversible, classifier-gated): drop refs/original + reflog
   expire --expire=now --all + gc --prune=now — BEFORE any first push. Until then the
   old key blobs remain reachable inside .git (verified: refs/original/refs/heads/main
   → fc1d2f3; `git ls-tree 7b00ce6 .keys/` lists both keys).

## Commands run (verbatim result lines)
- `git log main --oneline -- .keys/` → (empty)
- `git rev-list main | wc -l` → 99
- `git grep -lE "BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY" $(git rev-list main)` → 3 unique paths (auth.rs doc-comment, kalshi_auth.rs redaction test, polymarket doc placeholder)
- `git for-each-ref refs/original/` → fc1d2f30d9eb... refs/original/refs/heads/main
- `git check-ignore -v .keys/fortuna-key.txt data/perishable/x.json` → .gitignore:7 `.keys/`; .gitignore:8 `data/`
- `git ls-files .keys data` → (empty); `git rev-list --objects main | grep -iE "\.keys|^[0-9a-f]+ data/"` → (empty)
- grep fixtures for full 36-char demo/prod key-id values → exit 1 (no hits) both
- header/bearer/cookie sweep over fixtures/ → 1 hit, a checklist note string
- `git show 7b00ce6:.gitignore | tail -3` → last line `.keys/**data/` (root cause confirmed)
- `git diff b8fa0c8..16478bb -- ASSUMPTIONS.md` → F6 wall-clock entry ONLY
- `grep -i degrade_alerts ASSUMPTIONS.md` → exit 1; `grep -c "\b95\b" polymarket research.md` → 0
- `git log --diff-filter=M b8fa0c8..16478bb -- docs/reviews/` → (empty); ls-tree blob diff → 3 adds, 0 changes
- hash-map spot-checks: 3/3 old hashes unreachable from main; subjects match; deltas confined to purge paths (0b8670d→1259388 byte-identical)
- `grep chrono crates/fortuna-recorder/Cargo.toml` → exit 1 (F6 dep drop confirmed)
- manifest rows = 60; stages without artifact = []; WS frame types: yes={1 snapshot, 2 delta, 2 subscribed}, noleg={1 snapshot, 4 delta, 2 subscribed}, trade=0
- `git diff --stat b8fa0c8..16478bb -- crates/fortuna-invariants/` → (empty)
