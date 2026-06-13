//! THE operator-mandated "test with live data" for funding_forecast
//! (perp-strategies-and-scalar-claims §2.2/§2.3/§7; GAPS R1).
//!
//! Flow (end-to-end against the committed Kinetics recordings):
//!   1. Load `fixtures/kinetics-perps/funding__rates_estimate.json` (the
//!      recorded KXBTCPERP1 estimate + next_funding_time — the INPUT).
//!   2. Build a `PerpTick` from that recorded estimate + next_funding_time.
//!   3. Run `funding_forecast::on_event` -> take the emitted
//!      `PredictiveDistribution::Scalar` (the quantile fan).
//!   4. STREAM-parse `funding__rates_historical.json` (~436 KB array),
//!      folding it element-by-element (never holding all 3656 rows), to find
//!      the realized `rate` for `market_ticker == "KXBTCPERP1"` at the
//!      `funding_time` matching the estimate's `next_funding_time`.
//!   5. Score the fan with `CrpsPinballRule` against
//!      `RealizedOutcome::Scalar { value: realized }`; assert the score is
//!      FINITE + non-negative and the fan validated.
//!
//! HISTORICAL-WINDOW MATCH (the §7 honesty caveat, confirmed against the
//! committed fixtures): the estimate fixture (captured 2026-06-12T12:40Z)
//! targets next_funding_time = 2026-06-12T20:00:00Z, but the historical
//! archive's latest KXBTCPERP1 row is 2026-06-11T20:00:00Z — the realized
//! archive predates the estimate's target window by ~24 h, so there is NO
//! EXACT MATCH. This is the venue-data reality, not a test defect: the
//! recordings were captured a day apart. The test therefore scores against
//! the CLOSEST AVAILABLE realized rate (the most-recent historical
//! KXBTCPERP1 funding_time) and PRINTS the gap loudly, while still proving
//! the full pipeline (estimate -> forecast -> CRPS over real recorded
//! numbers). The exact-match path is exercised whenever a future re-capture
//! lands a historical row at the estimate's target window.

use fortuna_cognition::scoring::{
    CrpsPinballRule, PredictiveDistribution, RealizedOutcome, ScoringRule,
};
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, VenueId};
use fortuna_core::perp::{FundingObservation, PerpMarks, PerpPrice};
use fortuna_runner::funding_forecast::FundingForecast;
use fortuna_runner::{CoreHandle, Strategy};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::PathBuf;
use std::str::FromStr;

const MARKET: &str = "KXBTCPERP1";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kinetics-perps")
}

// ── the recorded estimate (the INPUT) ───────────────────────────────────────

#[derive(Debug, Deserialize)]
struct EstimateFixture {
    /// The capture instant of the estimate — the honest observation time.
    computed_time: String,
    funding_rate: f64,
    #[allow(dead_code)]
    mark_price: String,
    market_ticker: String,
    next_funding_time: String,
}

fn load_estimate() -> EstimateFixture {
    let path = fixtures_dir().join("funding__rates_estimate.json");
    let bytes = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read estimate fixture {}: {e}", path.display()));
    serde_json::from_str(&bytes)
        .unwrap_or_else(|e| panic!("parse estimate fixture {}: {e}", path.display()))
}

// ── the realized history (the CRPS TARGET), stream-folded ────────────────────

#[derive(Debug, Deserialize)]
struct HistoricalRow {
    funding_rate: f64,
    funding_time: String,
    market_ticker: String,
    #[allow(dead_code)]
    mark_price: String,
}

/// The result of folding the historical array for one market: the exact-match
/// realized rate (if a row at `target_time` exists) and, regardless, the
/// most-recent realized (rate, time) — the closest available fallback.
#[derive(Debug, Default)]
struct HistoricalMatch {
    exact: Option<f64>,
    latest: Option<(f64, String)>,
}

/// STREAM-parse the historical fixture, folding the `funding_rates` array
/// element-by-element. Only the running match state is retained — the full
/// 3656-row vector is never materialized (the operator's "stream-parse it,
/// find the matching window"). Implemented via a `DeserializeSeed` so serde
/// drives the parse incrementally from a buffered file reader.
fn stream_match_history(market: &str, target_time: &str) -> HistoricalMatch {
    use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
    use std::fmt;

    /// Folds the inner array without collecting it.
    struct RowsFold<'a> {
        market: &'a str,
        target_time: &'a str,
        acc: HistoricalMatch,
    }

    impl<'de> Visitor<'de> for RowsFold<'_> {
        type Value = HistoricalMatch;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a sequence of funding-rate rows")
        }
        fn visit_seq<A: SeqAccess<'de>>(mut self, mut seq: A) -> Result<Self::Value, A::Error> {
            // Pull one row at a time; keep only the running match state.
            while let Some(row) = seq.next_element::<HistoricalRow>()? {
                if row.market_ticker != self.market {
                    continue;
                }
                if row.funding_time == self.target_time {
                    self.acc.exact = Some(row.funding_rate);
                }
                // Track the lexicographically-greatest funding_time (the ISO8601
                // form sorts chronologically) = the most-recent realized.
                let newer = match &self.acc.latest {
                    None => true,
                    Some((_, t)) => row.funding_time.as_str() > t.as_str(),
                };
                if newer {
                    self.acc.latest = Some((row.funding_rate, row.funding_time));
                }
            }
            Ok(self.acc)
        }
    }

    /// Seeds the inner array fold from the `{"funding_rates": [...]}` wrapper.
    struct WrapperSeed<'a> {
        market: &'a str,
        target_time: &'a str,
    }

    impl<'de> DeserializeSeed<'de> for WrapperSeed<'_> {
        type Value = HistoricalMatch;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            struct WrapperVisitor<'a> {
                market: &'a str,
                target_time: &'a str,
            }
            impl<'de> Visitor<'de> for WrapperVisitor<'_> {
                type Value = HistoricalMatch;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("an object with a funding_rates array")
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                    let mut found: Option<HistoricalMatch> = None;
                    while let Some(key) = map.next_key::<String>()? {
                        if key == "funding_rates" {
                            // Drive the array fold as the value of this key.
                            struct ArraySeed<'a> {
                                market: &'a str,
                                target_time: &'a str,
                            }
                            impl<'de> DeserializeSeed<'de> for ArraySeed<'_> {
                                type Value = HistoricalMatch;
                                fn deserialize<D: Deserializer<'de>>(
                                    self,
                                    d: D,
                                ) -> Result<Self::Value, D::Error> {
                                    d.deserialize_seq(RowsFold {
                                        market: self.market,
                                        target_time: self.target_time,
                                        acc: HistoricalMatch::default(),
                                    })
                                }
                            }
                            found = Some(map.next_value_seed(ArraySeed {
                                market: self.market,
                                target_time: self.target_time,
                            })?);
                        } else {
                            // Skip any other top-level field.
                            let _ = map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                    Ok(found.unwrap_or_default())
                }
            }
            d.deserialize_map(WrapperVisitor {
                market: self.market,
                target_time: self.target_time,
            })
        }
    }

    let path = fixtures_dir().join("funding__rates_historical.json");
    let file = std::fs::File::open(&path)
        .unwrap_or_else(|e| panic!("open historical fixture {}: {e}", path.display()));
    let reader = BufReader::new(file);
    let mut de = serde_json::Deserializer::from_reader(reader);
    WrapperSeed {
        market,
        target_time,
    }
    .deserialize(&mut de)
    .unwrap_or_else(|e| panic!("stream-parse historical fixture {}: {e}", path.display()))
}

// ── the strategy harness (mirrors the unit test's) ───────────────────────────

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula = \"quadratic\"\neffective_date = \"2026-01-01\"\ntaker_coeff = \"0.07\"\nmaker_coeff = \"0.0175\"\n",
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

fn run_forecast(
    estimate: Decimal,
    obs_at: UtcTimestamp,
    next_funding: UtcTimestamp,
) -> PredictiveDistribution {
    let books = BTreeMap::new();
    let markets = BTreeMap::new();
    let fees = fee_model();
    let core = CoreHandle {
        now: obs_at,
        books: &books,
        markets: &markets,
        fee_model: &fees,
    };
    let ev = BusEvent {
        seq: 1,
        at: obs_at,
        origin: EventOrigin::External,
        payload: EventPayload::PerpTick {
            venue: VenueId::new("kinetics").unwrap(),
            market: MarketId::new(MARKET).unwrap(),
            marks: PerpMarks {
                venue_settlement: PerpPrice::from_dollars_exact(
                    Decimal::from_str("6.3332").unwrap(),
                )
                .unwrap(),
                conservative: None,
            },
            funding: FundingObservation {
                estimate,
                next_funding_time: next_funding,
                reference_price: PerpPrice::from_dollars_exact(
                    Decimal::from_str("6.3000").unwrap(),
                )
                .unwrap(),
                obs_at,
            },
        },
    };
    let mut s = FundingForecast::new().unwrap();
    let proposals = futures::executor::block_on(s.on_event(&ev, &core)).unwrap();
    assert!(proposals.is_empty(), "zero-capital: no proposal");
    let drafts = s.drain_scalar_beliefs();
    assert_eq!(
        drafts.len(),
        1,
        "one belief from the recorded estimate tick"
    );
    drafts.into_iter().next().unwrap().predictive
}

// ── the live-data CRPS test ──────────────────────────────────────────────────

#[test]
fn funding_forecast_scores_finite_crps_against_recorded_realized_rate() {
    // 1. The recorded estimate (the INPUT).
    let est = load_estimate();
    assert_eq!(
        est.market_ticker, MARKET,
        "fixture is the KXBTCPERP1 estimate"
    );
    let estimate = Decimal::from_str(&format!("{:.16}", est.funding_rate))
        .unwrap_or_else(|e| panic!("estimate funding_rate {} -> Decimal: {e}", est.funding_rate));
    let next_funding = UtcTimestamp::parse_iso8601(&est.next_funding_time).unwrap();
    // The observation time IS the fixture's recorded capture instant
    // (`computed_time`, truncated to ms by the parser) — the honest obs_at. The
    // estimate is a mid-window snapshot: capture (12:40Z) predates
    // next_funding_time (20:00Z), so remaining > 0 -> a non-degenerate fan.
    let obs_at = UtcTimestamp::parse_iso8601(&est.computed_time).unwrap();

    // 2-3. Forecast -> the scalar fan.
    let predictive = run_forecast(estimate, obs_at, next_funding);
    predictive
        .validate()
        .expect("the live forecast fan must validate");
    let fan = match &predictive {
        PredictiveDistribution::Scalar { quantiles, .. } => quantiles.clone(),
        other => panic!("expected Scalar, got {other:?}"),
    };

    // 4. STREAM-parse the historical fixture for the realized rate at the
    //    estimate's target window (exact), and the closest-available fallback.
    let matched = stream_match_history(MARKET, &est.next_funding_time);
    let (realized, realized_time, exact) = match matched.exact {
        Some(r) => (r, est.next_funding_time.clone(), true),
        None => {
            let (r, t) = matched
                .latest
                .clone()
                .expect("historical fixture has at least one KXBTCPERP1 row");
            (r, t, false)
        }
    };

    // 5. Score with CRPS (mean pinball) against the realized rate.
    let outcome = RealizedOutcome::Scalar { value: realized };
    let rule = CrpsPinballRule;
    let crps = rule
        .score(&predictive, &outcome)
        .expect("CRPS over a validated scalar fan + scalar outcome must succeed");

    // The §7 assertions: a finite, non-negative score (CRPS/pinball is a
    // proper, non-negative loss).
    assert!(crps.is_finite(), "CRPS must be finite, got {crps}");
    assert!(crps >= 0.0, "CRPS is a non-negative loss, got {crps}");

    // What the live data showed (printed for the operator record).
    println!("[funding_forecast live-data CRPS]");
    println!("  recorded estimate (funding_rate) : {}", est.funding_rate);
    println!(
        "  estimate next_funding_time       : {}",
        est.next_funding_time
    );
    println!("  forecast quantile fan (q -> v)   :");
    for q in &fan {
        println!("    {:>4} -> {:.10}", q.q, q.v);
    }
    if exact {
        println!("  realized rate (EXACT window match): {realized} @ {realized_time}");
    } else {
        println!(
            "  NO exact historical row at the estimate's target window \
             ({}); the historical archive's latest KXBTCPERP1 row is {realized_time} \
             (~24h earlier — recordings captured a day apart, see file header).",
            est.next_funding_time
        );
        println!("  realized rate (CLOSEST available) : {realized} @ {realized_time}");
    }
    println!("  CRPS (mean pinball)              : {crps:.10}");

    // The realized rate is a finite recorded number (a real venue funding rate).
    assert!(realized.is_finite());
}

#[test]
fn the_exact_window_is_absent_in_the_committed_archive() {
    // Pin the documented data reality so a future re-capture that DOES land the
    // target window flips this test red and prompts removing the fallback note.
    // (Honesty guard: the live-data test's fallback path is justified by THIS
    // fact, recorded executably.)
    let est = load_estimate();
    let matched = stream_match_history(MARKET, &est.next_funding_time);
    assert!(
        matched.exact.is_none(),
        "a historical row now EXISTS at the estimate's target window ({}); \
         update funding_forecast_live_data to score the exact match and drop \
         the closest-available fallback narrative.",
        est.next_funding_time
    );
    // The fallback always has SOMETHING to score against.
    assert!(
        matched.latest.is_some(),
        "the historical archive must carry at least one KXBTCPERP1 row"
    );
}
