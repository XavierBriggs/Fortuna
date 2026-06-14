//! F7 (Track-A half): the Aeolus↔Kalshi bucket seam — the venue-facing glue
//! that turns a recorded Kalshi temperature-bracket book into FORTUNA weather
//! beliefs + market-event edges.
//!
//! Three deterministic, pure helpers (no `Clock`, no IO, no panic):
//!
//! 1. [`station_series`] — the GROUNDED NWS-station → Kalshi-series map. Only
//!    `(KNYC, tmax) → KXHIGHNY` is confirmed against recorded data; other
//!    cities are intentionally absent until their station↔city pairing is
//!    confirmed (returning `None` is the conservative default).
//! 2. [`market_to_bucket`] — applies the grounded venue→`BucketKind` mapping
//!    (contract: `floor`/`cap` strikes → in-range/tail kinds). PURE kind
//!    derivation: it does NOT filter on market status; the active-only filter
//!    is the CALLER's concern, so a test can build buckets from a settled
//!    day-set. An absent/non-integer strike or unknown `strike_type` → `None`.
//! 3. [`aeolus_bucket_edges`] — runs the Track-E belief matcher
//!    ([`aeolus_bucket_beliefs`]) over a discovered bucket set and emits, 1:1
//!    and order-preserving, one auto-confirmed `Direct` [`EdgeProposal`] per
//!    belief draft.
//!
//! ## Why auto-confirm the edges here (spec §5.12 + I1/I6 reconciliation)
//!
//! These edges are EXACT-bucket matches: a single Kalshi market and a single
//! Aeolus bucket are the *same* `Direct` proposition by construction (the
//! belief's `event_id` IS `aeolus:{ticker}`). §5.12 reserves human-confirmed
//! edges for the cross-venue / multi-leg equivalence risk (the UMA failure
//! mode); a deterministic in-venue `Direct` 1:1 carries none of that risk, so
//! it is auto-confirmed (`confirmed_by = "discovery:auto"`). This does NOT
//! weaken any invariant: the belief stays propose-only (I6 — the model never
//! sized, timed, or routed it), and any resulting order still crosses the same
//! deterministic gate pipeline (I1) on the operator-gated `kalshi` venue. The
//! edge only makes the belief *tradeable*; it authorizes nothing on its own.
//!
//! NOTHING here is wired into `drive()` — the parent owns the live discovery
//! plug-in. This module is the recorded-fixture-tested machinery only.

use fortuna_cognition::aeolus_buckets::{aeolus_bucket_beliefs, BucketKind, WeatherBucket};
use fortuna_cognition::aeolus_forecast::{AeolusForecast, Variable};
use fortuna_cognition::beliefs::BeliefDraft;
use fortuna_cognition::events::{EdgeProposal, MappingType};
use fortuna_core::market::MarketId;
use fortuna_venues::kalshi::dto::KalshiMarket;

/// The signal `kind` the Aeolus source emits (the raw `aeolus.forecast/v2`
/// envelope, carried as untrusted DATA). The F7 live weather plug-in reads
/// exactly this kind from the signals ledger.
pub const AEOLUS_FORECAST_SIGNAL_KIND: &str = "aeolus.forecast";

/// The grounded NWS-station → Kalshi temperature-series map (contract: F7
/// discovery). The ONLY confirmed pairing is Central Park / KNYC → `KXHIGHNY`
/// (proved by `fixtures/kalshi/markets__high_temp.json`, whose rules name
/// "Central Park, New York"). Other cities (KORD/KAUS/…) are deliberately
/// omitted: their NWS-station↔Kalshi-city mapping is not yet confirmed against
/// recorded data, and an unconfirmed series is worse than no series. Any
/// non-`Tmax` variable returns `None` (no tmin series confirmed). Pure.
pub fn station_series(station: &str, variable: Variable) -> Option<&'static str> {
    match (station, variable) {
        ("KNYC", Variable::Tmax) => Some("KXHIGHNY"),
        // Other (station, tmax) pairs and ALL tmin: unconfirmed → None.
        _ => None,
    }
}

/// Derive the [`WeatherBucket`] a Kalshi temperature-bracket market represents,
/// applying the grounded venue→kind mapping (contract: the only logic F7
/// writes besides discovery):
///
/// - `strike_type == "between"` → `InRange { lo_f: floor, hi_f: cap }`
///   (Kalshi "87° to 88°" ⇒ floor=87, cap=88; both inclusive).
/// - `strike_type == "greater"` → `GreaterEq { threshold_f: floor + 1 }`
///   (">94" means the integer high ≥ 95).
/// - `strike_type == "less"`    → `LessEq { threshold_f: cap - 1 }`
///   ("<87" means the integer high ≤ 86).
///
/// `market_key` is the raw ticker (`m.ticker`). A market missing the strike
/// field it needs, carrying a non-integer (price-series) strike, or with an
/// unknown `strike_type` returns `None` — never a panic, never a fabricated
/// bucket.
///
/// PURE kind derivation by design: this does NOT inspect `m.status`. The
/// active-only filter belongs to the CALLER (live discovery passes only active
/// markets; a test may pass a settled day-set to exercise the construction).
pub fn market_to_bucket(m: &KalshiMarket) -> Option<WeatherBucket> {
    let kind = match m.strike_type.as_deref()? {
        "between" => BucketKind::InRange {
            lo_f: m.floor_strike_int()?,
            hi_f: m.cap_strike_int()?,
        },
        "greater" => BucketKind::GreaterEq {
            threshold_f: m.floor_strike_int()?.checked_add(1)?,
        },
        "less" => BucketKind::LessEq {
            threshold_f: m.cap_strike_int()?.checked_sub(1)?,
        },
        // "structured" / "custom" / anything else: not a temperature bucket.
        _ => return None,
    };
    Some(WeatherBucket {
        market_key: m.ticker.clone(),
        kind,
    })
}

/// Match a forecast against a discovered bucket set and emit, 1:1 and
/// order-preserving, the belief drafts (via the Track-E
/// [`aeolus_bucket_beliefs`]) paired with one auto-confirmed `Direct`
/// [`EdgeProposal`] each.
///
/// Each draft's `event_id` is `aeolus:{market_key}` (the Kalshi ticker); the
/// edge recovers that ticker via `strip_prefix("aeolus:")` to build the
/// `MarketId`. A `MarketId::new` failure (impossible for a real, non-empty
/// ticker) skips just that edge — the matching belief is still returned, but
/// no malformed edge is emitted, and nothing panics. The returned vectors are
/// therefore zipped 1:1 ONLY in the (universal) case where every ticker is a
/// valid `MarketId`; a skipped edge makes `edges.len() < drafts.len()`.
///
/// Empty / no-bucket input yields `(vec![], vec![])`. Pure; never panics.
pub fn aeolus_bucket_edges(
    fc: &AeolusForecast,
    buckets: &[WeatherBucket],
) -> (Vec<BeliefDraft>, Vec<EdgeProposal>) {
    let drafts = aeolus_bucket_beliefs(fc, buckets);
    let mut edges = Vec::with_capacity(drafts.len());
    for draft in &drafts {
        // The matcher guarantees event_id == "aeolus:{ticker}"; recover the
        // ticker. A draft that somehow lacks the prefix is skipped (defensive).
        let Some(market_key) = draft.event_id.strip_prefix("aeolus:") else {
            continue;
        };
        // A real ticker is a valid MarketId; a (theoretical) empty key skips
        // the edge rather than panicking (no unwrap in a money-adjacent path).
        let Ok(market) = MarketId::new(market_key) else {
            continue;
        };
        edges.push(EdgeProposal {
            market,
            venue: "kalshi".to_string(),
            event_id: draft.event_id.clone(),
            mapping: MappingType::Direct,
            confidence: 1.0,
            proposed_by: "aeolus_bucket_match".to_string(),
            confirmed_by: Some("discovery:auto".to_string()),
        });
    }
    (drafts, edges)
}
