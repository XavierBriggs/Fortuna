# Review: track-d news-aggregation Phase-A (D1-D4 code unit) — 2026-06-13

Base: 28065372 (parent of D1)  Head: 8fd2e2d (D4)  Verdict: **BLOCK**
Protected crate touched: **no** (crates/fortuna-invariants/ absent from the diff)
Code unit gated: D1-D4 only (fortuna-sources scaffold+config / FetchClient / Layer-1
validator + Layer-0 dossier / NwsSource). The two later doc commits on track-d's tip
(667baac Aeolus contract rev2, b28f596 handoff prompt) were correctly EXCLUDED — the
worktree is detached at 8fd2e2d.

Diff scope (git diff --stat 28065372...8fd2e2d): 19 files, +2638. New crate
crates/fortuna-sources/{config,error,fetch,lib,nws,validate}.rs; docs/research/sources/
{TEMPLATE.md, nws/dossier.md}; fixtures/sources/nws/{alerts_active,afd_list,afd_product,
error_400}.json + README; ASSUMPTIONS/BUILD_PLAN/GAPS/Cargo.toml/Cargo.lock additive.

## Criteria (fixed from the dispatch rubric + design §4.4 + spec 5.11, BEFORE the diff)

- **A. Never-invent-feed-behavior — PASS.** Every NwsSource parser branch maps to a
  recorded api.weather.gov capture. Spot-map (>=5): AlertsActive→`features[]`
  (alerts_active.json, a real geojson-ld FeatureCollection w/ VTEC, urn:oid, ontology
  URIs); AfdProducts→`@graph[]` (afd_list.json, real JSON-LD product summaries);
  error-envelope→no-array refusal (error_400.json, real problem+json); claimed-time
  alert→`properties.sent`; claimed-time afd→`issuanceTime`; non-JSON→parse error.
  AFD full-text two-hop is captured (afd_product.json) and ledgered as a deferred
  follow-up (GAPS §Track D), not invented. Evidence: nws.rs:122-188; tests
  parses_real_alert_featurecollection / parses_real_afd_product_list /
  an_error_envelope_is_a_fetch_error_not_a_signal (all green).

- **B. SSRF / URL discipline — FAIL (Critical, see finding 1).** https-only ✓
  (host_of_https rejects non-https; pin construction rejects http/ftp/no-host),
  initial-URL pinned before any token/network ✓ (fetch.rs:280), size cap ✓ (tested
  TooLarge), redirect hop cap ✓ (tested TooManyRedirects), timeout cap present
  (ReqwestFetchTransport timeout → FetchError::Timeout), content-embedded URLs are
  NEVER fetch targets ✓ (structurally: the only fetch inputs are self.url and the
  redirect Location; no payload field reaches the fetch path — grep-confirmed; scratch
  test content_embedded_url_is_never_admitted passed). BUT the redirect re-validation
  (the core §6 control) is BYPASSABLE: see finding 1.

- **C. Layer 0 admission — PASS.** docs/research/sources/nws/dossier.md scores the six
  §4.4 dimensions, cites authenticity evidence against the CANONICAL
  https://api.weather.gov verified from the official services-web-api documentation
  page (not a search hit), records ToS ("open data… we do not charge any fees", no
  scraping), proposes tier 9 with band rationale + consumption consequences. Config is
  fail-closed: a source absent/disabled in the registry is refused by the existing
  normalizer (signals.rs normalize_and_dedup RefusedUnregistered/RefusedDisabled). The
  dossier states the registry row is created at D9 scheduler-wiring time (ledgered).

- **D. Layer 1 structural validation — PARTIAL (Major, see finding 2).** Container-shape
  refusal present + tested (parse() rejects a payload missing the `features[]`/`@graph[]`
  array → SignalError::Fetch, never a silent emit). Timestamp-sanity (future-dated
  reject), stale-republication flag, per-tick volume envelope are all built in
  StructuralValidator and mutation-tested (rejects_future_dated_beyond_tolerance,
  flags_republished_content, enforces_per_tick_volume_envelope). HOWEVER: (i) per-ITEM
  schema validation (a `features` element that is a number / wrong-shape object /
  missing required `properties`) is NOT performed — such items are ingested verbatim as
  RawSignals (scratch-proven: garbage-features → 4 signals; missing-properties → 1
  signal); (ii) StructuralValidator is NOT wired to the adapter in this unit, so the
  absurd-value (year-9999 sent) path is not actually refused at ingest yet (scratch:
  adapter emits it; nws_claimed_time DOES extract it, so Layer 1 *could* reject once
  wired — D9). Non-fail-open (DATA-NOT-INSTRUCTIONS + I6 bound the blast radius), and
  wiring is genuinely D9 scope, but the per-item-validation absence is not explicitly
  ledgered. See finding 2.

- **E. Data-not-instructions (5.11) — PASS.** The payload is the raw JSON object passed
  through untouched (nws.rs:121 "this adapter interprets nothing"; payload: item.clone()).
  Nothing in the NWS path is executed/eval'd/used as a fetch target or command — grep
  confirmed no payload field flows to fetch; the item only becomes a RawSignal.payload
  (Value) for the downstream normalizer. The adapter is dumb (fetch→envelope).

- **F. Fail-closed config/registry — PASS.** config.rs is fail-closed throughout:
  deny_unknown_fields, unknown kind/extraction rejected, non-https url rejected even when
  disabled, model-extraction requires a trust cap and is refused when enabled (Phase A
  no-model), scrape/mcp refused when enabled, zero rate_budget rejected, enabled-source
  completeness enforced. Trust tier 0..=10 enforced (TrustTier::new + extraction cap
  0..=10). Politeness budget never exceeded: proptest limiter_never_exceeds_the_refill_
  bound over arbitrary non-decreasing schedules (GCRA, integer-exact, Clock-driven).

- **G. Hygiene — PASS.** Injected Clock only — the sole `.now()` sites are
  clock.now()/self.clock.now() (fetch.rs:285, nws.rs:90); zero SystemTime/Instant/
  Utc::now. No f32/f64 anywhere in the crate. No unwrap/expect/panic in non-test code
  (typed SignalError/SourcesError/FetchError). No secrets (UA in transport, not config;
  no KEY/TOKEN/SECRET literals). No drive() seam landed in this unit — correctly deferred
  to D9 (ledgered); nothing to flag.

- **H. Battery — PASS.** fmt --check EXIT 0; clippy -p fortuna-sources --all-targets
  -D warnings EXIT 0; cargo test -p fortuna-sources = 47 passed / 0 failed / 0 ignored.
  fortuna-cognition (dep) compiles green; existing Source trait consumed UNCHANGED.
  Test-weakening sweep across the whole diff: no #[ignore] added, no proptest case-count
  reduction, no deleted/loosened assertions (purely additive new crate). Protected crate
  untouched.

## Findings

- **[Critical] SSRF: redirect re-validation is bypassable via backslash-authority
  confusion (parser differential).** `HostPin::admits` uses a hand-rolled `host_of_https`
  (fetch.rs:103-122) that splits the authority on `/ ? #` only and takes
  `rsplit('@').next()` as the host. For `https://evil.example.com\@api.weather.gov/x` it
  resolves host = `api.weather.gov` → ADMITS. But reqwest resolves URLs with the WHATWG
  `url` crate (v2.5.8, confirmed in the tree), which treats `\` as `/`, so the authority
  ends at the backslash → real host = `evil.example.com`. The redirect path
  (fetch.rs:304-316) re-validates the server-supplied `Location` with `admits` and then
  hands the RAW string to `transport.get()`; with `ReqwestFetchTransport` that string
  connects to evil.example.com — the §6/rubric-B redirect control fails OPEN.
  Reproduction (scratch tests, run this session, NOT committed, protected crate
  untouched):
    1. admits("https://evil.example.com\\@api.weather.gov/x") == Ok, while
       reqwest::Client::get(same).build().url().host_str() == Some("evil.example.com").
    2. End-to-end through the public FetchClient::fetch: a 302 with that Location is
       followed; the transport's recorded fetch targets were
       ["https://api.weather.gov/start", "https://evil.example.com\\@api.weather.gov/x"]
       and the off-pin body returned as Ok(Fetched).
  Reachable surface: the redirect Location is origin/MITM-controlled — exactly the
  threat the re-validation exists to stop. Fix direction (NOT applied — verifier reports,
  never fixes): resolve the host with the same WHATWG parser reqwest uses (url::Url) and
  compare host_str, or reject any URL whose authority contains a backslash / forbidden
  host char before admitting. A red SSRF finding on the canonical injection surface
  cannot be resolved by accepting an explanation — implementation must change.

- **[Major] Layer-1 schema conformance is incomplete for this unit: per-item structural
  validation absent and StructuralValidator unwired to the adapter.** Design §4.4 Layer 1
  lists "schema conformance" as an ingest-time property; the rubric (D) asks a
  shape-drifted / missing-required-field / absurd-value NWS payload to be REFUSED. The
  adapter refuses only on the CONTAINER shape (no top-level array); each array element is
  emitted verbatim with no field validation, and the absurd-value (future-dated) defense
  lives in StructuralValidator which this unit does not call (wiring deferred to D9).
  Reproduction (scratch, this session): a `features` array of [number, string, null,
  wrong-object] → 4 ingested signals; an alert with no `properties` → 1 signal; an alert
  with sent=9999-01-01 → emitted by the adapter (nws_claimed_time extracts 253370764800000
  so Layer 1 would catch it once wired). NOT fail-open downstream (payload is opaque
  Value; nothing executes it; I6/I1 bound blast radius), and the D9 wiring deferral IS
  ledgered — but the per-ITEM-validation absence is not explicitly called out in GAPS.
  Disposition: tighten the GAPS entry to name the per-item-schema gap, and ensure D9
  routes every RawSignal through StructuralValidator (it is built and tested for this).

## Commands run (verbatim results)

- `cargo fmt -p fortuna-sources -- --check` → FMT_EXIT=0
- `cargo clippy -p fortuna-sources --all-targets -- -D warnings` → CLIPPY_EXIT=0
  (Finished, zero warnings)
- `cargo test -p fortuna-sources` → "test result: ok. 47 passed; 0 failed; 0 ignored;
  0 measured; 0 filtered out" (config 13, fetch 15 + 2 proptests, nws 9, validate 8)
- `cargo build -p fortuna-sources` (warm) → Finished, BUILD_EXIT=0
- `cargo test -p fortuna-cognition --lib` → ok (0 lib unit tests; crate compiles, Source
  trait surface unchanged and consumed by fortuna-sources)
- git diff --name-only 28065372...8fd2e2d | grep invariant → UNTOUCHED
- git diff --name-only 28065372...8fd2e2d | grep cognition → UNTOUCHED
- Mechanical sweeps: unwrap/expect/panic in non-test code → NONE; SystemTime/Instant/
  Utc::now → NONE (only clock.now); f32/f64 → NONE; secrets → NONE; HashSet .iter()
  into ordering in validate.rs → NONE (membership-only; FIFO order via VecDeque)
- Scratch SSRF probes (4 files under crates/fortuna-sources/tests/, REMOVED after; +url
  dev-dep reverted; worktree restored to pristine 8fd2e2d, `git status` empty):
    * admits_never_disagrees_with_whatwg_host_resolution → FAILED on
      "https://evil.example.com\\@api.weather.gov/x" (admitted off-pin)
    * backslash_smuggle_admitted_but_resolves_offpin → CONFIRMED via reqwest builder
    * redirect_to_backslash_smuggle_is_followed_offpin → CONFIRMED end-to-end
    * shape-drift/missing-field/absurd-value → characterized (Major above)

## Merge recommendation: DO-NOT-MERGE

One Critical SSRF fail-open on the canonical injection surface gates the unit. The
remediation is small (host comparison via the WHATWG parser, or backslash/forbidden-char
rejection) and should land with a redirect-smuggle regression test (feed a backslash-
authority Location, assert OffPin refusal). The Major (Layer-1 per-item schema gap +
adapter wiring) should be tightened in GAPS and closed at D9. Everything else (Layer 0
dossier, fail-closed config, dumb adapter, hygiene, battery, fixtures-first discipline,
protected crate, no test weakening) is gate-clean. Re-gate after the SSRF fix; the
post-merge integration check (new workspace crate) applies on the eventual ACCEPT.
