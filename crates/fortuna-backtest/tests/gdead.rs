//! G-DEAD integrity gate tests (S3).
//!
//! G-DEAD (spec §5) prevents false-negative survivorship bias: a producer must
//! not silently look good by dropping the markets it did badly on.
//!
//! Specifically `enforce_gdead` checks:
//! (a) **Coverage:** every manifest-engaged market appears in the scored set.
//! (b) **Voided/NO present:** voided markets and NO-resolved (outcome==0)
//!     markets must be in the scored set — they are the ones most likely to be
//!     quietly dropped to inflate results.
//!
//! Markets NOT in the manifest (legitimately un-forecast) must NOT trigger a
//! violation (false-positive guard).

use fortuna_backtest::manifest::enforce_gdead;
use fortuna_backtest::manifest::{EngagedMarket, GDeadViolation, ScoredRow, UniverseManifest};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn manifest(entries: &[(&str, bool, bool)]) -> UniverseManifest {
    UniverseManifest {
        engaged: entries
            .iter()
            .map(|(linkage, resolved, voided)| EngagedMarket {
                event_linkage: linkage.to_string(),
                resolved: *resolved,
                voided: *voided,
            })
            .collect(),
    }
}

fn scored(entries: &[(&str, f64, bool)]) -> Vec<ScoredRow> {
    entries
        .iter()
        .map(|(linkage, outcome, voided)| ScoredRow {
            event_linkage: linkage.to_string(),
            outcome: *outcome,
            voided: *voided,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// gdead_voided_present (LOAD-BEARING)
//
// A manifest with a voided market:
//   - If `scored` OMITS it → GDeadViolation
//   - If `scored` INCLUDES it → Ok
//
// This is the primary G-DEAD clause: the scorer must not silently drop voided
// markets (they're the ones most often dropped to inflate apparent performance).
// ---------------------------------------------------------------------------

#[test]
fn gdead_voided_present_omit_is_violation() {
    let m = manifest(&[("event://forecast/A/2026-01-01", false, true)]);
    // The voided market is OMITTED from the scored set.
    let s = scored(&[]);
    let result = enforce_gdead(&s, &m);
    assert!(
        result.is_err(),
        "omitting a voided engaged market must be a G-DEAD violation"
    );
    match result.unwrap_err() {
        GDeadViolation::DroppedMarkets(linkages) => {
            assert!(
                linkages.contains(&"event://forecast/A/2026-01-01".to_string()),
                "violation must name the dropped linkage"
            );
        }
    }
}

#[test]
fn gdead_voided_present_included_is_ok() {
    let m = manifest(&[("event://forecast/A/2026-01-01", false, true)]);
    // The voided market IS present in the scored set.
    let s = scored(&[("event://forecast/A/2026-01-01", 0.0, true)]);
    assert!(
        enforce_gdead(&s, &m).is_ok(),
        "a voided market present in scored must not be a violation"
    );
}

// ---------------------------------------------------------------------------
// gdead_no_resolved_present
//
// A NO-resolved market (outcome == 0.0, resolved=true, voided=false) must be
// in the scored set. Omitting it is a violation (classic survivorship: the
// producer forecast YES and it resolved NO — easy to quietly drop).
// ---------------------------------------------------------------------------

#[test]
fn gdead_no_resolved_present_omit_is_violation() {
    // outcome=0 means NO, resolved=true, voided=false
    let m = manifest(&[("event://forecast/B/2026-01-01", true, false)]);
    // Scored set contains the market but we want to test the NO-resolved path.
    // The market IS in the manifest but OMITTED from scored → violation.
    let s = scored(&[]);
    let result = enforce_gdead(&s, &m);
    assert!(
        result.is_err(),
        "omitting a NO-resolved engaged market must be a G-DEAD violation"
    );
}

#[test]
fn gdead_no_resolved_present_included_is_ok() {
    let m = manifest(&[("event://forecast/B/2026-01-01", true, false)]);
    // Include the NO-resolved market (outcome=0.0) in scored.
    let s = scored(&[("event://forecast/B/2026-01-01", 0.0, false)]);
    assert!(
        enforce_gdead(&s, &m).is_ok(),
        "a NO-resolved market present in scored must not be a violation"
    );
}

// ---------------------------------------------------------------------------
// gdead_coverage_equals_manifest
//
// Any engaged market dropped from the scored set — regardless of its
// resolution status — is a violation.
// ---------------------------------------------------------------------------

#[test]
fn gdead_coverage_dropped_engaged_is_violation() {
    let m = manifest(&[
        ("event://forecast/C/YES", true, false), // YES-resolved, present
        ("event://forecast/D/YES", true, false), // YES-resolved, DROPPED
    ]);
    let s = scored(&[
        ("event://forecast/C/YES", 1.0, false),
        // "event://forecast/D/YES" is intentionally absent
    ]);
    let result = enforce_gdead(&s, &m);
    assert!(
        result.is_err(),
        "a dropped engaged market must be a G-DEAD violation"
    );
    match result.unwrap_err() {
        GDeadViolation::DroppedMarkets(linkages) => {
            assert!(
                linkages.contains(&"event://forecast/D/YES".to_string()),
                "violation must name the dropped market"
            );
            assert!(
                !linkages.contains(&"event://forecast/C/YES".to_string()),
                "present market must not appear in the violation"
            );
        }
    }
}

#[test]
fn gdead_coverage_all_present_is_ok() {
    let m = manifest(&[
        ("event://forecast/E/2026-01-01", true, false),
        ("event://forecast/F/2026-01-01", true, false),
    ]);
    let s = scored(&[
        ("event://forecast/E/2026-01-01", 1.0, false),
        ("event://forecast/F/2026-01-01", 1.0, false),
    ]);
    assert!(enforce_gdead(&s, &m).is_ok());
}

// ---------------------------------------------------------------------------
// gdead_unforecast_market_not_false_positive
//
// A market that the producer NEVER engaged (not in the manifest) must NOT
// trigger a G-DEAD violation, even if it appears in the scored set or not.
// The key property: a legitimate non-forecast is not survivorship.
// ---------------------------------------------------------------------------

#[test]
fn gdead_unforecast_market_not_false_positive() {
    // Manifest only has one engaged market.
    let m = manifest(&[("event://forecast/G/engaged", true, false)]);
    // Scored set contains the engaged market PLUS an extra un-manifested market.
    let s = scored(&[
        ("event://forecast/G/engaged", 1.0, false),
        ("event://forecast/X/not-in-manifest", 0.0, false),
    ]);
    // The extra un-manifested market must NOT cause a violation.
    assert!(
        enforce_gdead(&s, &m).is_ok(),
        "a market not in the manifest must not trigger a G-DEAD false positive"
    );
}

#[test]
fn gdead_empty_manifest_empty_scored_is_ok() {
    // Edge case: no engaged markets → nothing to check → Ok.
    let m = manifest(&[]);
    let s = scored(&[]);
    assert!(enforce_gdead(&s, &m).is_ok());
}

#[test]
fn gdead_empty_manifest_extra_scored_is_ok() {
    // Edge case: no engaged markets, but scored has entries → no violation.
    let m = manifest(&[]);
    let s = scored(&[("event://forecast/Z/extra", 1.0, false)]);
    assert!(enforce_gdead(&s, &m).is_ok());
}

// ---------------------------------------------------------------------------
// Multiple violations accumulate
// ---------------------------------------------------------------------------

#[test]
fn gdead_multiple_drops_all_reported() {
    let m = manifest(&[
        ("event://forecast/H/drop1", false, true), // voided, dropped
        ("event://forecast/I/drop2", true, false), // NO-resolved, dropped
        ("event://forecast/J/kept", true, false),  // present
    ]);
    let s = scored(&[("event://forecast/J/kept", 1.0, false)]);
    let result = enforce_gdead(&s, &m);
    assert!(result.is_err());
    match result.unwrap_err() {
        GDeadViolation::DroppedMarkets(linkages) => {
            assert_eq!(linkages.len(), 2, "both dropped markets should be reported");
            assert!(linkages.contains(&"event://forecast/H/drop1".to_string()));
            assert!(linkages.contains(&"event://forecast/I/drop2".to_string()));
        }
    }
}
