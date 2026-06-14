//! F7: world-forward match tests, driven by the RECORDED fixture
//! (`fixtures/sources/aeolus/knyc_tmax.json`) so the synthesis is exercised on
//! genuinely-parsed data — including the `nws_station_id` ≠ `station` case.

use fortuna_cognition::aeolus_forecast::{parse_response, Authority, Comparison, Variable};
use fortuna_cognition::aeolus_match::match_forecast;

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/aeolus/knyc_tmax.json"
));

#[test]
fn match_synthesizes_the_predicted_family_from_the_recorded_forecast() {
    let fc = &parse_response(FIXTURE).expect("recorded fixture parses")[0];
    let fam = match_forecast(fc);

    // Identity + provenance carried from the forecast.
    assert_eq!(fam.station, "KNYC");
    assert_eq!(fam.variable, Variable::Tmax);
    assert_eq!(fam.target_date, "2026-06-13");
    assert_eq!(fam.model_version, "sar-semos-v1");

    // The grading station is the OFFICIAL one and is DISTINCT from the Aeolus
    // station here ("NYC" vs "KNYC") — never inferred, taken from resolution.*.
    assert_eq!(fam.nws_station_id, "NYC");
    assert_eq!(fam.resolution_authority, Authority::NwsObservedHigh);

    // One scoreable event per bracket, in order, keyed aeolus:{event_hint}.
    assert_eq!(fam.events.len(), 14);
    let first = &fam.events[0];
    assert_eq!(first.event_id, "aeolus:knyc-2026-06-13-tmax-ge81");
    assert_eq!(first.event_hint, "knyc-2026-06-13-tmax-ge81");
    assert_eq!(first.threshold_f, 81);
    assert_eq!(first.comparison, Comparison::Ge);
    assert!((first.p_aeolus - 0.9998401961079686).abs() < 1e-12);

    // Thresholds ascend 81..94 in order; every event id is aeolus-namespaced.
    let thresholds: Vec<i64> = fam.events.iter().map(|e| e.threshold_f).collect();
    assert_eq!(thresholds, (81..=94).collect::<Vec<_>>());
    assert!(fam
        .events
        .iter()
        .all(|e| e.event_id.starts_with("aeolus:") && !e.event_hint.is_empty()));
}
