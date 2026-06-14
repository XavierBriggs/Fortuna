//! The cognition half of the weather "close-the-loop" bridge (source contract
//! `docs/design/aeolus-fortuna-source-contract.md` §5 Layer 3): PURE +
//! deterministic helpers the live `resolve_and_score_weather_beliefs`
//! orchestrator composes to grade an open Aeolus weather belief against the
//! realized NWS temperature.
//!
//! Three concerns, each fail-closed and replay-deterministic (no `Clock`, no IO,
//! no panic — the crate denies `unwrap`/`panic` off-test):
//!
//! 1. [`cli_serves_station`] — STATION ROUTING. Decide whether a persisted
//!    `nws.cli` product is the daily climate report for a belief's grading
//!    station, by its AWIPS product id (`CLINYC` ⟺ "NYC"). Routing to the WRONG
//!    station's report would mis-grade a belief, so the match is exact + fail-
//!    closed (no match ⇒ the belief stays OPEN, never graded by a stray product).
//! 2. [`parse_bracket_hint`] / [`realized_f_for`] — recover `(comparison,
//!    threshold)` from the producer-controlled `event_hint`, and pick the
//!    realized °F (daily MAX for `tmax`, MIN for `tmin`) off the grade.
//! 3. [`score_bracket`] — the per-belief resolver: Brier the belief's OWN
//!    persisted probability `p` against the realized 0/1 outcome.
//!
//! ## Why grade the PERSISTED `p` (not a re-derived μ/σ probability)
//!
//! The belief that was proposed and persisted carries FORTUNA's probability —
//! today `p == p_raw` (no weather calibration layer yet), but the design reserves
//! a downstream calibration step that would make `p ≠ p_raw`. Reliability must
//! score what we ACTUALLY believed, so the resolver reads `p` off the belief row
//! and Briers it — mirroring how the funding resolver reconstructs its score from
//! the persisted belief, never from the source signal. The realized °F is the
//! INDEPENDENT NWS grade (never Aeolus — the V4 self-grading caution); a grade
//! the bridge cannot place (no matching product, an ambiguous CLI, an unparseable
//! hint) returns `None` and the belief stays OPEN — never a fabricated outcome
//! (spec 5.12).

use crate::aeolus_forecast::{Comparison, Variable};
use crate::aeolus_reliability::bracket_outcome;
use crate::beliefs::brier_score;

/// The AWIPS product-id prefix every NWS CLI report carries (`CLI` + the
/// 3-letter climate-station id, e.g. `CLINYC` = Central Park, `CLITTD` =
/// Troutdale). The id is the authoritative station key — the issuing WFO
/// (`KPQR`) and any city name in prose are not.
const CLI_AWIPS_PREFIX: &str = "CLI";

/// True iff this CLI `product_text` is the daily climate report for the grading
/// station `nws_station_id` — i.e. it carries the AWIPS id `CLI{nws_station_id}`
/// (`CLINYC` ⟺ "NYC") as a whitespace-delimited token (NWS prints it on its own
/// header line). Matched as a whole TOKEN, case-insensitively — never a substring
/// — so a city name in the report body can never spuriously route a forecast to
/// the wrong product. Fail-closed: no token ⇒ `false` ⇒ the belief stays OPEN.
///
/// SINGLE-STATION assumption (ledgered in GAPS): one station per CLI product;
/// a multi-station office report is out of scope for this bridge.
pub fn cli_serves_station(product_text: &str, nws_station_id: &str) -> bool {
    if nws_station_id.is_empty() {
        return false;
    }
    let awips = format!("{CLI_AWIPS_PREFIX}{nws_station_id}");
    product_text
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case(&awips))
}

/// Recover `(comparison, threshold_f)` from a v1 bracket `event_hint`
/// (`knyc-2026-06-13-tmax-ge87` → `(Ge, 87)`; `…-lt87` → `(Lt, 87)`). The
/// comparison+threshold are the trailing `-`-delimited token. The `event_hint`
/// is producer-controlled, so this is treated as untrusted: any shape that is not
/// `ge<int>`/`lt<int>` — an `in_bracket`-style hint, a missing token, a
/// non-integer, or a NEGATIVE threshold (whose leading `-` splits the token) —
/// returns `None`, and the belief is SKIPPED (left OPEN), never mis-graded.
/// Negative daily-high/low brackets do not occur for the stations Aeolus
/// forecasts today (NYC summer highs/lows); the limitation is ledgered in GAPS.
pub fn parse_bracket_hint(event_hint: &str) -> Option<(Comparison, i64)> {
    let last = event_hint.rsplit('-').next()?;
    let (comparison, digits) = if let Some(d) = last.strip_prefix("ge") {
        (Comparison::Ge, d)
    } else if let Some(d) = last.strip_prefix("lt") {
        (Comparison::Lt, d)
    } else {
        return None;
    };
    let threshold_f: i64 = digits.parse().ok()?;
    Some((comparison, threshold_f))
}

/// The realized °F the belief's `variable` resolves against: the official daily
/// MAX for `tmax` brackets, the daily MIN for `tmin` — both straight off the
/// independent NWS grade (`high_f`/`low_f`), never derived.
pub fn realized_f_for(variable: Variable, high_f: i64, low_f: i64) -> f64 {
    match variable {
        Variable::Tmax => high_f as f64,
        Variable::Tmin => low_f as f64,
    }
}

/// Score one open bracket belief: parse its `event_hint`, decide the realized
/// 0/1 outcome (`ge t ⟺ realized ≥ t`, `lt t ⟺ realized < t` — the SAME rule F9
/// uses, reused from [`bracket_outcome`] so the live resolver and the scorecard
/// can never drift), and Brier the belief's OWN persisted probability `p` against
/// it. Returns `(outcome, brier)`, or `None` when the hint is unparseable or
/// `in_bracket` ⇒ the belief is skipped (left OPEN), never mis-scored.
pub fn score_bracket(event_hint: &str, p: f64, realized_f: f64) -> Option<(bool, f64)> {
    let (comparison, threshold_f) = parse_bracket_hint(event_hint)?;
    let outcome = bracket_outcome(threshold_f, comparison, realized_f)?;
    Some((outcome, brier_score(p, outcome)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    // Recorded NWS CLI captures (2026-06-14): Troutdale (AWIPS `CLITTD`) and
    // Pago Pago (AWIPS `CLIPPG`). The grader's own tests pin the temperatures;
    // here they ground the STATION-ROUTING matcher in real product text.
    const TROUTDALE: &str =
        include_str!("../../../fixtures/sources/nws_climate/cli_product_troutdale.json");
    const PAGO: &str = include_str!("../../../fixtures/sources/nws_climate/cli_product_pago.json");

    fn product_text(fixture: &str) -> String {
        let v: Value = serde_json::from_str(fixture).unwrap();
        v["productText"].as_str().unwrap().to_string()
    }

    // --- station routing -------------------------------------------------

    #[test]
    fn cli_serves_its_own_awips_station_and_no_other() {
        let troutdale = product_text(TROUTDALE);
        // CLITTD serves TTD (case-insensitive), not NYC and not Pago's PPG.
        assert!(cli_serves_station(&troutdale, "TTD"));
        assert!(cli_serves_station(&troutdale, "ttd"));
        assert!(!cli_serves_station(&troutdale, "NYC"));
        assert!(!cli_serves_station(&troutdale, "PPG"));

        let pago = product_text(PAGO);
        assert!(cli_serves_station(&pago, "PPG"));
        assert!(!cli_serves_station(&pago, "TTD"));
    }

    #[test]
    fn station_match_is_a_whole_token_not_a_substring() {
        // A city name in prose ("...THE TROUTDALE OR CLIMATE SUMMARY...") must
        // not route, and the AWIPS prefix alone is not a station.
        let troutdale = product_text(TROUTDALE);
        assert!(!cli_serves_station(&troutdale, "TROUTDALE"));
        assert!(!cli_serves_station(&troutdale, ""));
        // A synthetic NYC report routes to NYC (the live grading station).
        let nyc = "\n000\nCDUS41 KOKX 141000\nCLINYC\n\nCLIMATE SUMMARY FOR JUNE 13 2026\nMAXIMUM 88\nMINIMUM 70\n";
        assert!(cli_serves_station(nyc, "NYC"));
        assert!(!cli_serves_station(nyc, "TTD"));
    }

    // --- hint parsing ----------------------------------------------------

    #[test]
    fn parses_ge_and_lt_hints() {
        assert_eq!(
            parse_bracket_hint("knyc-2026-06-13-tmax-ge87"),
            Some((Comparison::Ge, 87))
        );
        assert_eq!(
            parse_bracket_hint("knyc-2026-06-13-tmin-lt60"),
            Some((Comparison::Lt, 60))
        );
    }

    #[test]
    fn rejects_unparseable_or_unsupported_hints() {
        assert_eq!(parse_bracket_hint("knyc-2026-06-13-tmax-in87-88"), None);
        assert_eq!(parse_bracket_hint("knyc-2026-06-13-tmax-eq87"), None);
        assert_eq!(parse_bracket_hint("knyc-2026-06-13-tmax-ge"), None);
        assert_eq!(parse_bracket_hint("knyc-2026-06-13-tmax-geX"), None);
        assert_eq!(parse_bracket_hint(""), None);
        // Negative threshold: the leading '-' splits the token ⇒ conservatively
        // skipped (never mis-graded), per the documented limitation.
        assert_eq!(parse_bracket_hint("kxxx-2026-01-01-tmin-ge-5"), None);
    }

    // --- realized °F selection ------------------------------------------

    #[test]
    fn realized_picks_high_for_tmax_low_for_tmin() {
        assert_eq!(realized_f_for(Variable::Tmax, 91, 50), 91.0);
        assert_eq!(realized_f_for(Variable::Tmin, 91, 50), 50.0);
    }

    // --- per-belief scoring (Brier of the PERSISTED p) ------------------

    #[test]
    fn scores_a_bracket_against_the_persisted_probability() {
        // realized high 91: ge87 is TRUE (91≥87), brier = (p−1)².
        let p = 0.6719055375922601;
        let (outcome, brier) = score_bracket("knyc-2026-06-13-tmax-ge87", p, 91.0).unwrap();
        assert!(outcome);
        assert!((brier - (p - 1.0).powi(2)).abs() < 1e-12);

        // ge92 is FALSE (91<92), brier = (p−0)² = p².
        let p2 = 0.02;
        let (o2, b2) = score_bracket("knyc-2026-06-13-tmax-ge92", p2, 91.0).unwrap();
        assert!(!o2);
        assert!((b2 - p2 * p2).abs() < 1e-12);
    }

    #[test]
    fn lt_outcome_uses_strict_less_than() {
        // lt60 against a realized low of 50: 50 < 60 ⇒ TRUE.
        let (o, _) = score_bracket("knyc-2026-06-13-tmin-lt60", 0.5, 50.0).unwrap();
        assert!(o);
        // lt60 against 60: 60 < 60 is FALSE.
        let (o2, _) = score_bracket("knyc-2026-06-13-tmin-lt60", 0.5, 60.0).unwrap();
        assert!(!o2);
    }

    #[test]
    fn unparseable_hint_scores_nothing() {
        assert_eq!(score_bracket("knyc-2026-06-13-tmax-eq87", 0.5, 91.0), None);
    }
}
