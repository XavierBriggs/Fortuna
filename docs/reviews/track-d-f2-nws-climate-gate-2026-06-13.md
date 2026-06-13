# Track D — F2 NwsClimateSource (observed daily-extreme grader): gate

Date: 2026-06-13. Target: track-d @ b190bc2. Files: `crates/fortuna-sources/src/
nws_climate.rs` (+396), fixtures (cli_list.json + cli_product.json), research
dossier (+132), lib.rs (+2 export), BUILD_PLAN/GAPS. Self-gate (focused;
untrusted-data/SSRF surface, disk-light vs track-E's concurrent cold build).

## VERDICT: ACCEPT-SLICE — gate-clean; factory-wiring is the ledgered residual

The NWS CLI grader (the observed daily high/low that resolves weather forecasts)
is a disciplined two-hop adapter. Built + tested, not yet factory-wired (dormant
until registered — the established build-then-wire pattern).

## Risk classes (the untrusted-data/SSRF surface where the Critical SSRF was caught)
- **SSRF — inherited-clean.** Uses the SSRF-fixed `FetchClient`/`HostPin`
  (nws_climate.rs:28,36,97,128); rolls NO host parsing of its own. The product URL
  is constructed from the CLI list's `@id` (or `https://api.weather.gov/products/{id}`
  fallback, :176) and fetched THROUGH the pin — an injected `@id` is refused off-pin.
  Pinned to api.weather.gov.
- **Untrusted-parse — safe, no panic.** `parse_list`/`parse_product` (:160,:184) use
  `serde_json::from_slice(...).map_err(...)` + `ok_or_else` + `?` — no production
  `unwrap`/`expect`/`panic!` (all such are in `#[cfg(test)]` ≥253). A malformed/
  doctored product is SKIPPED-and-retried (`if let Ok(signal) = parse_product(...)`,
  `_ => continue`, :131-138), never crashing or failing the whole poll. The raw
  `productText` is carried as quoted JSON DATA (:187-194), never executed (spec 5.11).
- **Politeness.** Conditional fetch (etag/last-modified → 304 yields empty, :100-101);
  `max_new_per_tick` cap (:117); per-product dedup via `seen` (:120,134).

## Battery (track-d warm target, SQLX_OFFLINE)
- `cargo fmt -p fortuna-sources --check` → FMT_OK
- `cargo clippy -p fortuna-sources --all-targets -- -D warnings` → Finished, 0 warnings
- `cargo test -p fortuna-sources` → 94 lib + 5 DST passed, 0 failed; the 6 nws_climate
  tests pass incl. `two_hop_fetch_emits_cli_signals_and_dedups_next_tick`,
  `list_304_yields_no_signals`, `parses_a_cli_product_with_raw_text_and_report_date`.
- `git diff main...HEAD -- crates/fortuna-invariants` empty (protected crate untouched).
- Fixtures-first (cli_list + cli_product), research-dossier-grounded; mock transport in
  tests (no live socket).

## Residual (ledgered, not a defect)
NwsClimateSource is exported (lib.rs) but NOT registered in the factory (`factory.rs`
has no nws.cli case) — so it is built-but-not-config-reachable. Once factory-wired, its
output flows through the scheduler's D9 Layer-1 validator like any source. Until then
it is dormant (the same build-then-wire staging as D9 validator / D10 drive seam). Gate
the factory-wiring commit when it lands. (f64 for temperatures is forecast-quantity, not
money — house-style compliant, like the existing weather signals.)
