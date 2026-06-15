//! A2d SLICE 3 part 2 tests: the funding-rates POLLER that FILLS
//! `funding_rates_historical` (`fortuna_live::funding_poller`; design
//! docs/design/perp-strategies-and-scalar-claims.md §9.1 + perps_openapi.yaml
//! line 887, the PUBLIC `GET /margin/funding_rates/historical`).
//!
//! Written FROM the prompt/spec text BEFORE the implementation (TDD). They cover,
//! adversarially:
//!   - the BACKFILL path: parse the REAL public capture and `poll_once` INSERTS
//!     all 100 finalized rates (report.inserted == 100, quarantined == 0); each
//!     is then readable via `repo.realized_rate`; `latest_funding_time` advanced
//!     for a sampled market;
//!   - IDEMPOTENCY: a SECOND `poll_once` over the same fixture inserts 0 and
//!     counts 100 dup hits (report.inserted == 0, skipped_dup == 100); the cursor
//!     (`latest_funding_time`) is UNCHANGED — the poller is safe to re-run;
//!   - the QUARANTINE path (UNTRUSTED DATA, spec 5.11): a crafted response with a
//!     non-finite funding_rate, an empty market_ticker, AND an unparseable
//!     funding_time has those entries quarantined+skipped (counted, alerted) while
//!     the well-formed siblings still insert — NO panic;
//!   - a whole-response FETCH FAILURE (the fetch trait returns Err): alert-and-
//!     continue — the report is flagged a fetch failure with the reason, NO panic,
//!     and nothing is written;
//!   - `next_funding_poll_at` boundary cases (the 04:00/12:00/20:00 set + the
//!     20:00 -> next-day-04:00 day rollover), PURE;
//!   - mark_price is stored VERBATIM (the inserted string == the fixture's, no
//!     numeric round-trip).
//!
//! Fixture-grounded: the realized rates are the REAL public capture at
//! docs/research/venue/kinetics-perps-2026-06-10/raw/live_prod_funding_hist_all.json
//! (100 records, 11 markets, 2026-06-06 -> 06-11, 8h cadence; the same file Part
//! 3's resolve/score tests pin).
//!
//! ## Mutation-check note (for a reviewer)
//!
//! Every assertion has teeth:
//!   - REMOVE the per-entry shape validation (the quarantine seam) and
//!     `quarantines_a_malformed_entry_and_keeps_the_siblings` reds: a NaN
//!     funding_rate / empty ticker / unparseable funding_time would either panic,
//!     insert garbage, or push the quarantine count to 0.
//!   - Use the venue's `funding_time` (not `now`) for `captured_at` and
//!     `backfill_inserts_every_fixture_row_with_verbatim_mark` reds: the asserted
//!     `captured_at == NOW_ISO` would fail (it would be the funding boundary).
//!   - DROP the `insert`-returns-`false`-on-dup idempotency reliance (e.g. count
//!     every entry as inserted) and `a_second_poll_is_idempotent` reds:
//!     inserted would be 100 again and skipped_dup 0 on the re-poll.
//!   - SWAP `next_funding_poll_at` to a non-strict boundary (`>=` not `>`) and
//!     `next_poll_is_strictly_after_now_at_a_boundary` reds (a poll sitting
//!     exactly on 12:00 would return 12:00, not 20:00).
//!   - Parse `mark_price` to f64 and back and `mark_price_is_stored_verbatim`
//!     reds (e.g. "6.2658" -> 6.2658 -> "6.2658" survives, but "7.8030" would
//!     drop the trailing zero -> "7.803").

use fortuna_core::clock::UtcTimestamp;
use fortuna_ledger::FundingRatesHistoricalRepo;
use fortuna_live::funding_poller::{
    next_funding_poll_at, poll_funding_rates_once, FundingHistFetch,
};
use fortuna_venues::kinetics::dto::{FundingRateHistoricalEntry, FundingRatesHistoricalResponse};
use fortuna_venues::VenueError;

/// The REAL public capture (NEVER fabricated) — the same fixture Part 3 pins.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/research/venue/kinetics-perps-2026-06-10/raw/live_prod_funding_hist_all.json"
);

/// The poll time (NOT a venue funding_time): strictly after the newest fixture
/// boundary, so `captured_at` is provably distinct from any `funding_time`.
const NOW_ISO: &str = "2026-06-11T05:30:00.000Z";

/// A market with a known newest boundary in the fixture (its `latest_funding_time`
/// after a full backfill).
const SAMPLE_MARKET: &str = "KXBCHPERP";
const SAMPLE_LATEST_FT: &str = "2026-06-11T04:00:00Z";
const SAMPLE_LATEST_RATE: f64 = -0.0003971378687289;
const SAMPLE_LATEST_MARK: &str = "2.0115";
/// A fixture row whose verbatim mark carries a trailing zero a f64 round-trip
/// would silently drop ("7.8030" -> 7.803).
const TRAILING_ZERO_MARK: &str = "7.8030";
const TRAILING_ZERO_MARKET: &str = "KXLINKPERP";
const TRAILING_ZERO_FT: &str = "2026-06-11T04:00:00Z";

fn now() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(NOW_ISO).expect("NOW parses")
}

/// The canonical millisecond ISO form the store + the poller agree on
/// (`to_iso8601()` maps `…Z` -> `….000Z`). The poller normalizes the venue's
/// funding_time through this before insert, so the test reads back under it.
fn canon(iso: &str) -> String {
    UtcTimestamp::parse_iso8601(iso)
        .expect("fixture funding_time parses")
        .to_iso8601()
}

/// Load + parse the REAL fixture into the EXISTING venue DTO (reused, never
/// redefined) — exactly what the production fetch impl yields.
fn fixture_response() -> FundingRatesHistoricalResponse {
    let raw = std::fs::read_to_string(FIXTURE).expect("fixture readable");
    serde_json::from_str(&raw).expect("fixture parses into FundingRatesHistoricalResponse")
}

/// A scripted fetch handle: hands back a pre-built response (or an error) so the
/// unit runs with NO network and NO credentials (the creds-less seam).
struct ScriptedFetch(Result<FundingRatesHistoricalResponse, VenueError>);

#[async_trait::async_trait]
impl FundingHistFetch for ScriptedFetch {
    async fn fetch_all(&self) -> Result<FundingRatesHistoricalResponse, VenueError> {
        match &self.0 {
            Ok(resp) => Ok(FundingRatesHistoricalResponse {
                funding_rates: resp.funding_rates.clone(),
            }),
            Err(_) => Err(VenueError::Outage {
                venue: "kinetics".to_string(),
                reason: "scripted fetch failure".to_string(),
            }),
        }
    }
}

fn ok_fetch(resp: FundingRatesHistoricalResponse) -> ScriptedFetch {
    ScriptedFetch(Ok(resp))
}

fn err_fetch() -> ScriptedFetch {
    ScriptedFetch(Err(VenueError::Outage {
        venue: "kinetics".to_string(),
        reason: "scripted".to_string(),
    }))
}

// ── backfill: every fixture row inserted, verbatim mark, cursor advanced ──────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn backfill_inserts_every_fixture_row_with_verbatim_mark(pool: sqlx::PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());
    let resp = fixture_response();
    assert_eq!(
        resp.funding_rates.len(),
        100,
        "the real capture has 100 rows"
    );

    let report = poll_funding_rates_once(&ok_fetch(resp), &repo, now())
        .await
        .expect("poll_once succeeds");

    assert!(!report.fetch_failed, "a successful fetch is not flagged");
    assert_eq!(report.fetched, 100, "all 100 fixture rows fetched");
    assert_eq!(report.inserted, 100, "all 100 inserted on a fresh store");
    assert_eq!(report.skipped_dup, 0, "nothing was a duplicate on backfill");
    assert_eq!(report.quarantined, 0, "the real capture is all well-formed");

    // Each is readable at its (market, canonical funding_time).
    let got = repo
        .realized_rate(SAMPLE_MARKET, &canon(SAMPLE_LATEST_FT))
        .await
        .expect("realized_rate")
        .expect("the sampled row was inserted");
    assert_eq!(
        got, SAMPLE_LATEST_RATE,
        "the funding_rate round-trips as the venue's f64"
    );

    // The cursor advanced to the sampled market's newest boundary (canonical form).
    let latest = repo
        .latest_funding_time(SAMPLE_MARKET)
        .await
        .expect("latest_funding_time")
        .expect("the sampled market has rows");
    assert_eq!(
        latest,
        canon(SAMPLE_LATEST_FT),
        "latest_funding_time is the newest captured boundary"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn mark_price_is_stored_verbatim(pool: sqlx::PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());
    poll_funding_rates_once(&ok_fetch(fixture_response()), &repo, now())
        .await
        .expect("poll_once");

    // The mark string is stored byte-for-byte (a f64 round-trip would drop the
    // trailing zero "7.8030" -> "7.803"). Read it back raw.
    let mark: String = sqlx::query_scalar(
        "SELECT mark_price FROM funding_rates_historical \
         WHERE market_ticker = $1 AND funding_time = $2",
    )
    .bind(TRAILING_ZERO_MARKET)
    .bind(canon(TRAILING_ZERO_FT))
    .fetch_one(&pool)
    .await
    .expect("mark row present");
    assert_eq!(
        mark, TRAILING_ZERO_MARK,
        "mark_price is the venue's verbatim string, no numeric round-trip"
    );
    // And the sampled mark too, for good measure.
    let sample_mark: String = sqlx::query_scalar(
        "SELECT mark_price FROM funding_rates_historical \
         WHERE market_ticker = $1 AND funding_time = $2",
    )
    .bind(SAMPLE_MARKET)
    .bind(canon(SAMPLE_LATEST_FT))
    .fetch_one(&pool)
    .await
    .expect("sample mark row present");
    assert_eq!(
        sample_mark, SAMPLE_LATEST_MARK,
        "sampled mark stored verbatim"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn captured_at_is_the_poll_time_not_the_funding_time(pool: sqlx::PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());
    poll_funding_rates_once(&ok_fetch(fixture_response()), &repo, now())
        .await
        .expect("poll_once");

    // captured_at is the POLL time (now), never the venue's funding_time. (The
    // sampled row's funding_time is 2026-06-11T04:00:00Z; NOW is 05:30.)
    let captured_at: String = sqlx::query_scalar(
        "SELECT captured_at FROM funding_rates_historical \
         WHERE market_ticker = $1 AND funding_time = $2",
    )
    .bind(SAMPLE_MARKET)
    .bind(canon(SAMPLE_LATEST_FT))
    .fetch_one(&pool)
    .await
    .expect("row present");
    assert_eq!(
        captured_at, NOW_ISO,
        "captured_at is clock.now(), distinct from the funding boundary"
    );
}

// ── idempotency: a second poll inserts nothing, the cursor is unchanged ───────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn a_second_poll_is_idempotent(pool: sqlx::PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());

    let first = poll_funding_rates_once(&ok_fetch(fixture_response()), &repo, now())
        .await
        .expect("first poll");
    assert_eq!(first.inserted, 100, "first poll backfills all 100");

    let latest_after_first = repo
        .latest_funding_time(SAMPLE_MARKET)
        .await
        .expect("latest after first")
        .expect("rows exist");

    // Re-poll the SAME fixture: ON CONFLICT DO NOTHING ⇒ every insert returns
    // false, so 0 inserted and 100 dup hits — the poller is safe to re-run.
    let second = poll_funding_rates_once(&ok_fetch(fixture_response()), &repo, now())
        .await
        .expect("second poll must not error on existing rows");
    assert_eq!(second.fetched, 100, "the second fetch still sees 100");
    assert_eq!(second.inserted, 0, "nothing newly inserted on a re-poll");
    assert_eq!(second.skipped_dup, 100, "all 100 were duplicates");
    assert_eq!(second.quarantined, 0, "still no quarantine");

    let latest_after_second = repo
        .latest_funding_time(SAMPLE_MARKET)
        .await
        .expect("latest after second")
        .expect("rows exist");
    assert_eq!(
        latest_after_first, latest_after_second,
        "an idempotent re-poll does not move the cursor"
    );
}

// ── quarantine: malformed entries skipped+counted, siblings still insert ──────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn quarantines_a_malformed_entry_and_keeps_the_siblings(pool: sqlx::PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());

    // A crafted response (UNTRUSTED DATA, spec 5.11): two WELL-FORMED rows framing
    // three NON-CONFORMING ones — a non-finite rate, an empty ticker, and an
    // unparseable funding_time. The good two must insert; the bad three quarantine.
    let resp = FundingRatesHistoricalResponse {
        funding_rates: vec![
            FundingRateHistoricalEntry {
                market_ticker: "KXBTCPERP".to_string(),
                funding_rate: -0.000_12,
                funding_time: "2026-06-11T04:00:00Z".to_string(),
                mark_price: "6.2658".to_string(),
            },
            // BAD: NaN rate (non-finite ⇒ quarantine).
            FundingRateHistoricalEntry {
                market_ticker: "KXETHPERP".to_string(),
                funding_rate: f64::NAN,
                funding_time: "2026-06-11T04:00:00Z".to_string(),
                mark_price: "1.6541".to_string(),
            },
            // BAD: empty market_ticker (⇒ quarantine).
            FundingRateHistoricalEntry {
                market_ticker: String::new(),
                funding_rate: -0.000_20,
                funding_time: "2026-06-11T04:00:00Z".to_string(),
                mark_price: "1.0000".to_string(),
            },
            // BAD: unparseable funding_time (⇒ quarantine).
            FundingRateHistoricalEntry {
                market_ticker: "KXSOLPERP".to_string(),
                funding_rate: -0.000_30,
                funding_time: "not-a-timestamp".to_string(),
                mark_price: "6.5245".to_string(),
            },
            FundingRateHistoricalEntry {
                market_ticker: "KXXRPPERP".to_string(),
                funding_rate: -0.000_27,
                funding_time: "2026-06-11T04:00:00Z".to_string(),
                mark_price: "1.1198".to_string(),
            },
        ],
    };

    let report = poll_funding_rates_once(&ok_fetch(resp), &repo, now())
        .await
        .expect("poll_once must NOT panic on malformed entries");

    assert!(!report.fetch_failed, "the fetch itself succeeded");
    assert_eq!(report.fetched, 5, "five entries arrived");
    assert_eq!(
        report.inserted, 2,
        "only the two well-formed entries inserted"
    );
    assert_eq!(
        report.quarantined, 3,
        "the three non-conforming entries quarantined"
    );
    assert_eq!(report.skipped_dup, 0, "none was a duplicate");
    assert_eq!(
        report.quarantine_alerts.len(),
        3,
        "each quarantine emits a structured alert reason for the caller to route"
    );

    // The good siblings are durably present; the bad ones wrote nothing.
    assert!(
        repo.realized_rate("KXBTCPERP", &canon("2026-06-11T04:00:00Z"))
            .await
            .expect("realized")
            .is_some(),
        "the leading well-formed entry inserted"
    );
    assert!(
        repo.realized_rate("KXXRPPERP", &canon("2026-06-11T04:00:00Z"))
            .await
            .expect("realized")
            .is_some(),
        "the trailing well-formed entry inserted"
    );
    assert!(
        repo.realized_rate("KXSOLPERP", &canon("2026-06-11T04:00:00Z"))
            .await
            .expect("realized")
            .is_none(),
        "the unparseable-funding_time entry was quarantined, not inserted"
    );
    assert!(
        repo.latest_funding_time("KXETHPERP")
            .await
            .expect("latest")
            .is_none(),
        "the NaN-rate entry was quarantined, not inserted"
    );
}

// ── fetch failure: alert-and-continue, no panic, nothing written ──────────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn a_whole_response_fetch_failure_alerts_and_continues(pool: sqlx::PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());

    // The fetch trait returns Err: the unit reports a fetch failure (with the
    // reason) and writes NOTHING — never a panic.
    let report = poll_funding_rates_once(&err_fetch(), &repo, now())
        .await
        .expect("a fetch failure is alert-and-continue, NOT an Err/panic");

    assert!(report.fetch_failed, "the report flags the fetch failure");
    assert_eq!(report.fetched, 0, "nothing was fetched");
    assert_eq!(report.inserted, 0, "nothing inserted");
    assert_eq!(
        report.quarantined, 0,
        "no per-entry quarantine on a whole-fetch failure"
    );
    assert!(
        report.fetch_alert.as_ref().is_some_and(|r| !r.is_empty()),
        "the fetch-failure alert carries a non-empty reason for the caller to route"
    );

    // The store is untouched.
    assert!(
        repo.latest_funding_time(SAMPLE_MARKET)
            .await
            .expect("latest")
            .is_none(),
        "a fetch failure writes nothing to the store"
    );
}

// ── next_funding_poll_at: the 04/12/20 boundaries + the day rollover (PURE) ────

fn ts(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).expect("iso parses")
}

#[test]
fn next_poll_is_the_next_8h_boundary_after_now() {
    // Just before 04:00 ⇒ 04:00.
    assert_eq!(
        next_funding_poll_at(ts("2026-06-11T03:59:59.000Z")),
        ts("2026-06-11T04:00:00.000Z"),
        "03:59:59 -> 04:00"
    );
    // Between 04:00 and 12:00 ⇒ 12:00.
    assert_eq!(
        next_funding_poll_at(ts("2026-06-11T05:30:00.000Z")),
        ts("2026-06-11T12:00:00.000Z"),
        "05:30 -> 12:00"
    );
    // Between 12:00 and 20:00 ⇒ 20:00.
    assert_eq!(
        next_funding_poll_at(ts("2026-06-11T15:00:00.000Z")),
        ts("2026-06-11T20:00:00.000Z"),
        "15:00 -> 20:00"
    );
}

#[test]
fn next_poll_is_strictly_after_now_at_a_boundary() {
    // EXACTLY on a boundary ⇒ the NEXT boundary (strictly-after, never the same
    // instant — a `>=` mutant returns 12:00 here and reds).
    assert_eq!(
        next_funding_poll_at(ts("2026-06-11T12:00:00.000Z")),
        ts("2026-06-11T20:00:00.000Z"),
        "exactly 12:00 -> 20:00 (strictly after)"
    );
    assert_eq!(
        next_funding_poll_at(ts("2026-06-11T04:00:00.000Z")),
        ts("2026-06-11T12:00:00.000Z"),
        "exactly 04:00 -> 12:00 (strictly after)"
    );
}

#[test]
fn next_poll_rolls_over_from_2000_to_next_day_0400() {
    // After 20:00 (the last boundary of the day) ⇒ next-day 04:00.
    assert_eq!(
        next_funding_poll_at(ts("2026-06-11T20:00:00.000Z")),
        ts("2026-06-12T04:00:00.000Z"),
        "exactly 20:00 -> next-day 04:00"
    );
    assert_eq!(
        next_funding_poll_at(ts("2026-06-11T23:45:00.000Z")),
        ts("2026-06-12T04:00:00.000Z"),
        "23:45 -> next-day 04:00"
    );
    // Just after midnight (00:30) ⇒ the SAME day's 04:00, not a rollover.
    assert_eq!(
        next_funding_poll_at(ts("2026-06-12T00:30:00.000Z")),
        ts("2026-06-12T04:00:00.000Z"),
        "00:30 -> same-day 04:00"
    );
    // Month rollover: after 20:00 on the last day of June ⇒ July 1st 04:00.
    assert_eq!(
        next_funding_poll_at(ts("2026-06-30T21:00:00.000Z")),
        ts("2026-07-01T04:00:00.000Z"),
        "month rollover 06-30T21:00 -> 07-01T04:00"
    );
}
