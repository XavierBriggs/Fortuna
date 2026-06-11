# Review: ws-ingestion (INDEPENDENT, part 2 of 2) — 2026-06-10
Base: 9b244ee  Head: 10443c5  Scope: d1cafe1, 57ae240, 10443c5 + Polymarket research/GAPS reshape
Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no (`git diff 9b244ee...10443c5 -- crates/fortuna-invariants/` is empty)

Independence: no file under docs/reviews/ was read. Worktree /tmp/fortuna-wsgate (detached
at 10443c5), removed after review. Targeted tests only per split-verifier instruction; full
battery + DST owned by the concurrent verifier.

## Criteria (fixed before reading the diff)

- W1 Provenance (CLAUDE.md "never invent venue API behavior"): PASS — archive exists
  (docs/research/venue/kalshi-api-2026-06-10/raw/asyncapi.yaml, 3952 lines, + 46 page
  snapshots incl. websockets_*). Spot-mapped 8 parser behaviors to archived sources:
  (1) subscribe cmd channels+use_yes_price -> asyncapi.yaml:982-1028, 2122-2144 +
  raw/pages/getting_started_order_direction.md:74-93; (2) snapshot envelope/fields
  (type/sid/seq/msg, yes_dollars_fp/no_dollars_fp) -> asyncapi.yaml:1459-1476 verbatim
  example (the test fixture IS that example: FED-23DEC-T3.00, sid 2, seq 2, 0.0800/300.00)
  + schema 2605-2660; (3) delta price_dollars/delta_fp/side -> asyncapi.yaml:1486-1498
  verbatim; (4) trade yes_price_dollars/count_fp/taker_side -> asyncapi.yaml:1590-1602
  verbatim; (5) subscribed ack msg.channel/msg.sid -> asyncapi.yaml:1245-1253; (6)
  unsubscribed top-level sid -> asyncapi.yaml:1262-1268; (7) error code 6 "Already
  subscribed" -> asyncapi.yaml:42, 1391; (8) seq consistency doctrine ->
  asyncapi.yaml:2008-2013 ("checked if you want to guarantee you received all the
  messages"); strict prev+1 contiguity is an inference, LEDGERED via fixture checklist
  item 23 (research.md:929-930 "verify seq gap detection and re-subscribe flow") and the
  ASSUMPTIONS "Kalshi WS message layer is doc-derived and dial-gated" entry.
- W2 BookAssembler fail-closed: PASS — delta-before-snapshot refused
  (the_assembler_folds... ok); overdraw refused with no phantom level
  (an_overdrawn_delta_leaves_no_phantom_level_behind ok); negative snapshot qty FAILS
  LOUD (negative_snapshot_quantities_are_refused_not_swallowed ok; ask-side mirror
  verified by my scratch test, 4/4 ok); crossed assembled book refused — verified by my
  scratch tests via both crossed delta and crossed snapshot (render -> OrderBook::validate,
  book.rs:76) but NOT pinned by any committed test (Minor F1); per-sid gap persists until
  resync (a_seq_gap_keeps_reporting_until_resync ok: gap at 12, still reported at 13,
  fresh snapshot at seq 20 resyncs, delta 21 flows).
- W3 One-sided snapshot fix (57ae240): PASS — archived schema quote: "Optional - This
  key will not exist if there are no Yes offers in the orderbook" (asyncapi.yaml:2632-2636);
  one_sided_snapshots_parse_with_the_absent_side_empty ok (incl. fully-empty msg);
  one-sided books render (overdraw test renders empty-ask book) and PaperVenue::apply_book
  re-validates (validate only cross-checks when both sides present); no over-correction:
  all W2 refusals re-ran green after the fix, present-but-non-array side still refuses
  (my scratch test ok — claimed in the 57ae240 subject but unpinned, Minor F2).
- W4 Sub-cent rejection everywhere: PASS — all three price entries (snapshot levels,
  delta price, trade price) go through dto::parse_dollars_to_cents_exact (Decimal,
  exact-or-error); fractional counts via parse_count_integral (fract -> hard error).
  Tests: sub_cent_prices_and_fractional_counts_are_rejected ok (delta 0.965, delta_fp
  1.50, snapshot level 0.0825), sub_cent_trade_prices_are_rejected_too ok (0.365).
- W5 Replay seam honesty: PASS — fortuna-paper::feed_stream_event (lib.rs:770) routes
  to the SAME pre-existing entry points the harness uses: apply_book (lib.rs:153,
  re-validates) and apply_public_trade (lib.rs:183, refuses qty<=0 and price outside
  1..=99 — fail closed even if a raw RecordedStream bypasses the parser). Seam test
  a_recorded_stream_drives_paper_fills_through_the_assembler ok: touch at 50 NEVER fills,
  trade-through at 49 fills exactly 10 at OUR resting 50, is_maker, haircut floor(30/2)
  capped at our size.
- W6 No live network: PASS — zero tungstenite/connect_async/TcpStream/wss:// hits in
  any Cargo.toml, Cargo.lock, or src; kalshi/ws.rs contains no socket code at all
  (parse_frame(&str) + a serde_json command builder); the only network client is the
  pre-existing REST client.rs, UNCHANGED in this range; no test opens a socket
  (RecordedStream is an in-memory VecDeque). The dial is absent, exactly as claimed,
  ledgered fixture-gated in GAPS/ASSUMPTIONS.
- W7 10443c5 Minor closures: PASS — (a) negative qty loud: stream.rs:109-117/124-136 +
  test ok; (b) pinned behaviors: non_positive_trade_counts_are_refused ok (count_fp
  "0.00"); ws.rs:173 refuses qty<=0; GAPS fixture ref corrected to "research checklist
  item #20" (research.md:919-921 IS the no-leg-pricing item) with both flag states to
  record; ASSUMPTIONS records the assembled-book-validation posture ("will not be
  loosened on speculation"); (c) abort path = the runner mid-group all-or-nothing abort
  (added 57ae240): a_mid_group_gate_rejection_submits_nothing_and_releases_reservations
  — ran it: ok (1 passed). 3-leg group, third leg band-breach at 99: 1 proposal, >=1
  gate rejection, 0 orders submitted, fortuna_reserved_exposure_cents gauge asserted 0.
- W8 Polymarket reshape: PASS (count note) — research.md exactly 829 lines (claim
  exact). Raw archive ACTUAL: 95 files (80 page snapshots + 13 OpenAPI schema JSONs +
  llms.txt + web-sources.md index) vs claimed "96 archived sources" — off by one,
  immaterial. The three load-bearing findings present AND sourced in research.md:
  no client order id -> lines 464-466 (S8; institutional clordId S31, 409 ClOrdID S27);
  sub-cent ticks LIVE -> lines 404-409 ("A value of 0.005 means half-cent ticks" S7,
  first 0.5c market 2026-06-01, 0.25c in preprod S24); no retail sandbox -> lines
  640-653 ("Retail API: NO sandbox is documented", institutional preprod dummy funds
  S25/S29). GAPS.md frames it as OPERATOR DECISION REQUIRED (line ~209) with all three +
  fees confirmed. No code pretends otherwise: range diff over crates/ has zero polymarket
  hits; PolymarketUsStub still VenueError::FixtureGated (polymarket/mod.rs).
- W9 Test-weakening + mechanical sweep: PASS — full-range diff 9b244ee...10443c5: zero
  removed assert/test lines, zero new #[ignore], zero proptest case reductions, zero
  loosened tolerances; no unwrap/expect/panic!/todo! added in src of touched crates; no
  SystemTime/Instant/Utc::now in the three WS commits; no f64 near prices (Decimal +
  i64 cents only; StreamEvent prices are Cents, counts i64); BTreeMap only in assembler
  and parser (deterministic iteration); no GatedOrder construction outside fortuna-gates
  (the new place( site in 5a1581a consumes gate-emitted GatedOrders — primary review by
  the concurrent verifier); secrets sweep: hits are archived Polymarket doc prose only.

## Findings

- [Minor] F1: crossed-assembled-book refusal is enforced (render -> OrderBook::validate)
  but pinned by NO committed test at the assembler layer — reproduction: my scratch
  tests verifier_crossed_assembled_book_is_refused / verifier_crossed_snapshot_is_refused
  (both pass against HEAD; scratch file deleted). Implementer should add the pin to
  crates/fortuna-venues/tests/stream.rs and ledger in GAPS.md.
- [Minor] F2: 57ae240 subject claims "a side that is present but non-array still refuses"
  — true (my scratch verifier_present_but_non_array_side_refuses passes) but unpinned by
  any committed test.
- [Minor] F3: stream.rs:160 `let next = current + delta_contracts;` is an unchecked i64
  add in a venue-data path (house style bans panics in venue paths; debug-profile
  overflow panics). Only reachable with absurd venue counts that survive
  parse_count_integral; in release the wrap is always negative for two large positives,
  so it fails closed via the next<0 refusal. Recommend checked_add -> VenueError.
- [Minor] F4: ws.rs parses REQUIRED envelope fields leniently: missing "type" ->
  Ignored{frame_type:""} (verified by scratch test), missing sid/seq -> unwrap_or(0).
  All degradations land in Ignored/SeqGap (never a Stream event), so fail direction is
  closed, but the leniency on off-contract frames is unledgered.
- [Info] F5: commit message claims "96 archived sources"; actual raw/ file count is 95.
  research.md line count (829) is exact.

## Commands run (verbatim results)

- git diff 9b244ee...10443c5 -- crates/fortuna-invariants/  -> empty (protected crate untouched)
- cargo fmt --check  -> exit 0
- cargo clippy -p fortuna-venues -p fortuna-paper -p fortuna-runner --all-targets -- -D warnings
  -> "Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.24s" (clean)
- cargo test -p fortuna-venues --test stream
  -> "test result: ok. 12 passed; 0 failed; 0 ignored" (matches the "stream 12/12" claim)
- cargo test -p fortuna-paper
  -> "test result: ok. 12 passed; 0 failed" incl. a_recorded_stream_drives_paper_fills_through_the_assembler ... ok
- cargo test -p fortuna-runner --test sim_loop a_mid_group_gate_rejection
  -> "test result: ok. 1 passed; 0 failed; ...; 10 filtered out"
- scratch adversarial suite (4 tests: crossed-via-delta, crossed-snapshot, negative-ask-qty,
  negative-trade-count) -> "test result: ok. 4 passed; 0 failed" (file deleted after run)
- scratch suite 2 (non-array side, missing-type) -> "test result: ok. 2 passed; 0 failed" (deleted)
- DST / full workspace battery: NOT run here (explicitly owned by the concurrent part-1 verifier)
