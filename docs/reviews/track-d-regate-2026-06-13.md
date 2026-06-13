# Re-Gate: track-d news-aggregation (D1-D5) — 2026-06-13

Base: main @ e85f92c  Head: track-d @ 3e4be0e
("fix(sources): CRITICAL SSRF host-pin bypass — unify on the WHATWG URL parser")
Verdict: **MERGE** (the Critical is cleared by reproduction-of-refusal; one Major
remains, honestly ledgered, operator-waivable; one scope caveat).
Protected crate touched: **no** (crates/fortuna-invariants: 0 files in main...HEAD).

Worktree: /tmp/fortuna-gtd detached @ 3e4be0e (clean, restored after mutation).
CARGO_TARGET_DIR=/tmp/fortuna-gate-target. Independence: read only
GATE-FINDINGS-LATEST.md from docs/reviews/.

---

## FIVE-LINE SUMMARY
1. The Critical SSRF is FIXED at the root cause: `host_of_https` is deleted; the pin
   decision and the reqwest connection now resolve the host through the SAME WHATWG
   parser (`reqwest::Url`), on the initial URL and every redirect hop; reqwest
   auto-redirect is disabled.
2. Reproduction-of-refusal achieved: the 4 named regression tests pass, MY 29 adversarial
   vectors (incl. 169.254.169.254, IDN homoglyph, punycode, double-@, trailing-dot,
   IPv6, %-encoded host, tab/newline smuggle, content-embedded URL) + on->off->on
   redirect chains ALL refuse off-pin, and reverting the fix turns them RED.
3. The Major (Layer-1 per-item validator) is STILL UNWIRED — built + unit-tested but
   zero production call site; a shape-drifted item ingests verbatim. Honestly ledgered
   as "D9 (the scheduler)"; it does not block the crate merge if the operator accepts.
4. Battery green: fmt clean, clippy -D warnings clean, 58/58 sources tests, no test
   weakening (one removed assertion was a wrong-WHATWG-model correction, replaced by
   stronger ones); DST core corpus 4+2000 = zero invariant violations.
5. Scope caveat for the operator: this is a PARTIAL Phase A — 2 of 4 adapters (NWS+RSS,
   no Calendar/GDELT), NO scheduler, NO drive() seam, NO registry rows/config entries
   yet. The crate is self-contained and not yet reachable from the daemon.

---

## A-E GRADES

- **A. THE SSRF FIX — PASS** (Critical cleared; pass/fail, reproduction-of-refusal met).
- **B. THE MAJOR (Layer-1 validator) — UNWIRED (Major stands; does not block merge).**
- **C. THE FULL UNIT (D1-D5) — PASS** (with the partial-Phase-A scope caveat).
- **D. BATTERY — PASS** (DST core corpus green; settlement/perp harnesses not re-run
  this session due to ENOSPC — unaffected by the additive, order-path-free diff).
- **E. DRIVE() SEAM — ABSENT** (no fortuna-live change; scheduler not wired; noted,
  not a blocker per the rubric).

---

## CRITERIA (fixed before reading the diff)

### A1 — host_of_https deleted; one WHATWG parser; no second hand parser: PASS
- `grep host_of_https crates/fortuna-sources/` => 0 occurrences (deleted everywhere).
- The pin check is one helper `canonical_https_host` (fetch.rs:114-127) using
  `reqwest::Url::parse(url).host_str()` — the SAME parser reqwest connects through
  (`reqwest::Url` re-exports the `url` crate). `HostPin::admits` (fetch.rs:84-96) and
  `HostPin::from_url` both call it; the public `FetchClient::fetch` validates the
  initial URL (fetch.rs:285) AND re-validates every redirect Location (fetch.rs:314)
  through it. The real transport disables reqwest auto-redirect
  (`redirect::Policy::none()`, fetch.rs:354), so there is no hidden second follow path.
- Flanking sweep: ALL reqwest usage is in fetch.rs; feed-rs is a pure parser over
  already-fetched bytes (rss.rs:101) and fetches nothing; no raw `.send()`/`Client::`
  outside fetch.rs. Every networked byte goes through HostPin. No second host parser.

### A2 — the 4 named regression tests pass: PASS
`cargo test -p fortuna-sources --lib fetch::` => 20 passed, incl.
- fetch_refuses_backslash_ssrf_payload_as_initial_url ... ok
- fetch_refuses_backslash_ssrf_payload_in_redirect_location ... ok
- fetch_follows_on_pin_redirect_but_refuses_off_pin_redirect ... ok
- fetch_refuses_url_off_pin_before_touching_network ... ok
All assert REFUSAL through the public fetch path with an unscripted mock transport
(any transport call panics => the smuggled host is never connected to).

### A3 — adversarial extension (my own vectors): PASS — none reach off-pin
Scratch test (since deleted) drove HostPin::admits + FetchClient::fetch and asserted,
for every vector, that admits() agrees with reqwest's real connection host. 29 vectors
+ 2 redirect-chain tests, all green. The verdict table (verbatim below) proves:
every ADMIT connects to api.weather.gov; every off-pin host is REFUSED. Specifically
refused: `evil@`/`user:api.weather.gov@evil`/`@@evil`/`:443@evil` userinfo smuggles,
the backslash payload + `https:\\` variant, IDN homoglyph (`xn--pi-6kc.weather.gov`),
punycode confusable, suffix `api.weather.gov.evil.example.com`, trailing-dot
`api.weather.gov.`, IPv6 `[::1]`, cloud-metadata `169.254.169.254`, loopback
`127.0.0.1`, newline-smuggle, and a content-embedded URL (`see https://… for details`
=> connect_host None => REFUSED, never a fetch target). The on->off->on redirect chain
refuses at the off hop and the transport sees ONLY the on-pin URL. NO vector reached
an off-pin host.

### A4 — mutation (revert the fix) turns the regression tests RED: PASS
Locally reverted `canonical_https_host` to a naive split-on-`@`/`/` hand parser. Result:
- fetch_refuses_backslash_ssrf_payload_as_initial_url ... FAILED (reached the transport
  => SSRF reproduces under the mutation)
- fetch_refuses_backslash_ssrf_payload_in_redirect_location ... FAILED
- pin_refuses_backslash_authority_parser_differential ... FAILED
- pin_admits_same_host_https_only ... FAILED (naive parser doesn't case-fold)
My adversarial test also went RED. Then restored: `git diff` on fetch.rs is EMPTY
(byte-identical), and all 4 tests are GREEN again. The regression suite is load-bearing.

### B — Layer-1 per-item validator wired into the ingest path: FAIL (Major, unchanged)
- `StructuralValidator::assess` has ZERO production call sites: grep shows it called
  ONLY inside validate.rs's own unit tests (lines 201-297). `nws_claimed_time` /
  `rss_claimed_time` (its inputs) are referenced ONLY in their modules' tests. There is
  NO scheduler in fortuna-sources, and NO drive() seam in fortuna-live.
- MUTATION (my scratch test, since deleted) through the REAL NwsSource::fetch:
  - a malformed top-level body (no `features[]`) IS refused by the adapter's parse guard
    (whole-body, nws.rs:130-141) — a partial structural control exists;
  - a shape-DRIFTED item inside a valid `features[]` (missing `properties`, carrying
    attacker `JUNK`) is EMITTED as a RawSignal verbatim — NOT refused-and-quarantined
    per-item. The independently-constructed validator WOULD reject a future-dated
    variant, but the ingest path never calls it.
- Honestly ledgered (GAPS.md:2204-2228): the validator belongs in the ingestion
  scheduler ("D9"), which does not exist yet; adapters stay dumb (spec 5.11). This is
  the architecturally-correct placement per design §4.2/§4.4. Per the rubric, the Major
  stands and is reported; it does not block the crate merge if the operator accepts the
  deferral (note: the adapters are not yet reachable from drive() either — see E).

### C — fixtures-traceable, fail-closed, Layer-0, data-not-instructions, Clock, no
f64, no unwrap/panic, no secrets: PASS (scope caveat below)
- Fixtures-first (>=5 behaviors -> fixtures/sources/): 6 real fixtures loaded via
  include_bytes!; behaviors: NWS alerts parse, NWS AFD-list parse, NWS error-envelope
  refusal, RSS-2.0 parse, Atom parse, malformed-feed-as-error (>=7 mapped). READMEs
  document real 2026-06-13 captures + re-record commands; trimming is honest
  ("every other byte is as the API returned it"); no invented feed behavior.
- Fail-closed config (config.rs): `#[serde(deny_unknown_fields)]` (typo => hard error);
  "no model in the ingestion path" enforced at validation (enabled + extraction=model
  => error, line 215-220); enabled non-buildable kinds refused; https-only even when
  disabled; enabled-source completeness; zero-budget rejected; trust-cap 0..=10; window
  times require trailing Z (point-in-time). 30 config/validate/fetch unit assertions.
- Layer-0 dossiers exist for all 3 admitted sources (nws, rss_fed_press, rss_sec_edgar)
  + TEMPLATE; the NWS dossier scores all six dimensions with CITED, dated evidence from
  real NWS API docs — a genuine vetting dossier, not a stub.
- Data-not-instructions: adapters pass payloads through untouched (nws.rs:144-149,
  rss.rs:116-139); feed-rs parses already-fetched bytes and follows no links; URLs in
  content are data, never fetch targets (proven in A3).
- Injected Clock: `received_at = self.clock.now()` in both adapters; the politeness
  limiter and validator read injected time; `grep SystemTime::now|Instant::now|Utc::now`
  => 0 in the crate.
- No f64/f32 in the crate (IO/text crate, no money). No unwrap/expect/panic/todo/
  unimplemented in production code (53 unwrap/expect, ALL in test modules). No secrets
  (only GCRA "token" terminology; feeds need no auth; UA is a public politeness contact).
- HashSet (validate.rs recent_set) is used only for O(1) membership, never ordered
  iteration; ordering uses VecDeque + BTreeMap (deterministic). No nondeterminism leak.
- SCOPE CAVEAT: design Phase A (§10) lists 4 adapters (Calendar, NWS, RSS, GDELT) +
  scheduler. This unit ships only NWS + RSS adapters and the (unwired) validator. No
  Calendar/GDELT, no scheduler, no registry rows/config entries. This is a partial
  Phase A; the operator should know Phase A is not complete on merge.

### D — fmt; clippy -D warnings; tests; test-weakening sweep; protected crate: PASS
- `cargo fmt --check` clean (exit 0).
- `cargo clippy -p fortuna-sources --all-targets -- -D warnings` clean (exit 0).
- `cargo test -p fortuna-sources` => 58 passed, 0 failed, 0 ignored (grew from the prior
  47 with the SSRF regression additions; none ignored).
- Test-weakening sweep over the WHOLE main...HEAD diff: 0 removed assertions, 0 new
  #[ignore], 0 proptest case-count reductions (the only added `proptest!` is the NEW
  politeness property; the only `.timeout()` is the reqwest client builder). The diff is
  +4111/-0 (purely additive crate). Within the fix commit, one assertion was changed:
  `https:///nopath` `.is_err()` was removed because WHATWG resolves it to host `nopath`
  (verified empirically), so it was a wrong-hand-parser-model assertion; it was replaced
  by two STRONGER genuinely-hostless assertions (`https://`, `not a url`). Net fix-commit
  test delta: +4 tests, 1 strengthened, 0 weakened.
- Protected crate crates/fortuna-invariants/: 0 files touched.
- fortuna-cognition: 0 files touched; the Source trait (the rubric contract) is unchanged
  — track-d implemented against the existing trait.

### E — drive() seam: ABSENT
track-d touches 0 fortuna-live files; no flagged drive() seam landed. The ingestion
scheduler is not wired into the daemon. Per the rubric this is noted, not a blocker for
the crate merge. It corroborates B: the validator cannot be wired until the scheduler
exists, and neither has landed.

---

## FINDINGS

- [Critical — CLEARED] Parser-differential SSRF host-pin bypass (prior BLOCK). Fixed at
  root cause (single WHATWG parser, auth==connection, redirects re-validated, reqwest
  auto-redirect off). Reproduction-of-refusal: 4 named tests + 29 adversarial vectors +
  redirect chains all refuse off-pin; mutation turns them RED; restore byte-identical.
  Reproduction: see Commands (A2/A3/A4 blocks).
- [Major — STANDS] Layer-1 per-item structural validator is unwired (no production call
  site; no scheduler). A shape-drifted item from the pinned host ingests verbatim
  instead of refuse-and-quarantine. Honestly ledgered as D9. Reproduction: grep showing
  assess() called only in validate.rs tests; my scratch test
  shape_drifted_item_is_emitted_not_quarantined passed (item emitted, not quarantined).
  Operator-waivable for the crate merge (the crate is not yet reachable from drive()).
- [Minor — note] Partial Phase A: 2 of 4 designed adapters, no scheduler, no drive()
  seam, no registry/config rows. Self-contained, all green, no order paths. Operator
  should merge knowing Phase A is incomplete. Ledgered in GAPS (D9, Layer-4 floors,
  AFD second hop).
- [Minor — note] DST settlement/perp harnesses not re-run this session (ENOSPC during
  fortuna-ops compile; disk hit 120Mi then recovered). The DST CORE corpus completed
  GREEN before the ENOSPC (4 corpus + 2000 random, zero violations). The settlement/perp
  harnesses are unaffected by the additive, order-path-free track-d diff, and main was
  certified GREEN on full run-dst by the bus. Not a code defect.

---

## ADVERSARIAL VECTOR VERDICT TABLE (A3, verbatim; pin = api.weather.gov)

ADMIT    connect_host=Some("api.weather.gov")        :: https://API.WEATHER.GOV/x
ADMIT    connect_host=Some("api.weather.gov")        :: https://api.weather%2egov/x
ADMIT    connect_host=Some("api.weather.gov")        :: https://%61pi.weather.gov/x
REFUSE   connect_host=Some("evil.example.com")       :: https://api.weather.gov@evil.example.com/x
REFUSE   connect_host=Some("evil.example.com")       :: https://user:api.weather.gov@evil.example.com/x
REFUSE   connect_host=Some("evil.example.com")       :: https://api.weather.gov@@evil.example.com/x
ADMIT    connect_host=Some("api.weather.gov")        :: https://evil.example.com@api.weather.gov/x
REFUSE   connect_host=Some("evil.example.com")       :: https://evil.example.com\@api.weather.gov/x
ADMIT    connect_host=Some("api.weather.gov")        :: https://api.weather.gov\@evil.example.com/x
REFUSE   connect_host=Some("evil.example.com")       :: https:\\evil.example.com\@api.weather.gov/x
REFUSE   connect_host=Some("xn--pi-6kc.weather.gov") :: https://аpi.weather.gov/x   (Cyrillic homoglyph)
REFUSE   connect_host=None                           :: https://xn--pi-7md.weather.gov/x
REFUSE   connect_host=Some("api.weather.gov.evil.example.com") :: https://api.weather.gov.evil.example.com/x
REFUSE   connect_host=Some("api.weather.gov.")       :: https://api.weather.gov./x
REFUSE   connect_host=Some("[::1]")                  :: https://[::1]/x
REFUSE   connect_host=Some("169.254.169.254")        :: https://169.254.169.254/latest/meta-data/
REFUSE   connect_host=Some("127.0.0.1")              :: https://127.0.0.1/x
ADMIT    connect_host=Some("api.weather.gov")        :: https://api.weather\t.gov/x   (tab stripped by WHATWG => pin)
REFUSE   connect_host=Some("api.weather.gov.evil.example.com") :: https://api.weather.gov\n.evil.example.com/x
REFUSE   connect_host=Some("evil.example.com")       :: https://api.weather.gov:443@evil.example.com/x
REFUSE   connect_host=None                           :: see https://api.weather.gov/x for details

Every ADMIT connects to the pinned host; every REFUSE has a non-pin (or None) connect
host. No vector reached an off-pin host. The `\t` admit is safe: WHATWG strips the tab
identically on both the authorization and connection side (the no-differential property).

---

## COMMANDS RUN (verbatim verdict lines)

# A1 — host_of_https deleted, single parser
$ grep -rn host_of_https crates/fortuna-sources/      => (no output) exit 1
$ grep -rn "reqwest::Url::parse" crates/fortuna-sources/src/fetch.rs
  fetch.rs:116:  reqwest::Url::parse(url).map_err(...)   # the one parser

# A2 — named regression tests
$ cargo test -p fortuna-sources --lib fetch::
  test result: ok. 20 passed; 0 failed; 0 ignored; ... 38 filtered out
  (incl. all 4 named fetch_refuses_* / fetch_follows_on_pin_* tests)

# A3 — adversarial vectors + redirect chains (scratch, since deleted)
  running 3 tests
  test redirect_chain_on_off_on_is_refused_at_the_off_hop ... ok
  test redirect_location_with_backslash_smuggle_is_refused_in_a_chain ... ok
  test adversarial_host_vectors_agree_with_connection_host ... ok   (29 vectors)
  test result: ok. 3 passed; 0 failed

# A4 — mutation (naive parser) => RED, then restore
  test fetch::tests::fetch_refuses_backslash_ssrf_payload_as_initial_url ... FAILED
  test fetch::tests::fetch_refuses_backslash_ssrf_payload_in_redirect_location ... FAILED
  test fetch::tests::pin_refuses_backslash_authority_parser_differential ... FAILED
  test result: FAILED. 16 passed; 4 failed
  $ git diff --stat -- .../fetch.rs   => (empty: byte-identical restore)
  (re-run after restore) test result: ok. 20 passed; 0 failed

# B — validator unwired
$ grep -rn "\.assess(" crates/fortuna-sources/   => only validate.rs test lines
$ grep -rn StructuralValidator crates/ (outside validate.rs) => only lib.rs pub use
  (scratch) test shape_drifted_item_is_emitted_not_quarantined ... ok
            test malformed_body_is_refused_by_parse_guard ... ok

# D — battery
$ cargo fmt --check                                          => exit 0 (clean)
$ cargo clippy -p fortuna-sources --all-targets -- -D warnings => Finished; exit 0
$ cargo test -p fortuna-sources
  test result: ok. 58 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ scripts/run-dst.sh
  [dst] regression corpus: 4 seed(s)
  [dst] master seed 1781327507550 -> 2000 random scenario(s)
  [dst] OK: 4 corpus + 2000 random seeds, zero invariant violations
  (then ENOSPC during fortuna-ops compile of the settlement/perp harnesses — disk,
   not a failure; those harnesses are unaffected by the order-path-free track-d diff)

# E — drive() seam
$ git diff --name-only main...HEAD -- crates/fortuna-live/   => (empty)

# protected crate
$ git diff --name-only main...HEAD -- crates/fortuna-invariants/   => 0 files

---

## MERGE CALL: **MERGE**

The reproduced Critical that gated the prior BLOCK is cleared by reproduction-of-refusal
(not by explanation): the fix is the root-cause parser unification the bus prescribed,
the named regression tests are load-bearing (mutation-proven), and my independent
adversarial battery (29 vectors + redirect chains, including cloud-metadata, IDN,
punycode, and content-embedded URLs) reaches no off-pin host. fmt/clippy/58 tests/DST
core corpus are green with no test weakening, and the protected crate and the Source
trait are untouched.

The one remaining Major (Layer-1 validator unwired) is honestly ledgered as the
scheduler item (D9), is architecturally-correctly placed there, and does not block the
crate merge by itself — the crate adds no order paths and is not yet reachable from the
daemon. The operator should merge with eyes open to the scope caveat: this is a PARTIAL
Phase A (2 of 4 adapters, no scheduler, no drive() seam, no registry/config rows), and
the Layer-1 refuse-and-quarantine guarantee is NOT yet live on any ingest path. The
NEXT track-d iteration must be the scheduler (D9) that wires the validator and the
drive() seam, after which a full Phase-A gate (Calendar+GDELT+scheduler+DST scenarios)
applies.

Reviewer: FORTUNA verifier · Date: 2026-06-13
