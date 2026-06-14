//! F7 station→series map (`aeolus_venue::station_series`) — every mapping is
//! grounded in a RECORDED Kalshi `rules_primary` that names the grading station
//! EXPLICITLY (captured read-only 2026-06-14; see
//! `docs/research/sources/kalshi-temperature-stations.md`). This test pins the
//! grounded set AND the conservative `None` for everything the recorded rules do
//! not pin to a precise station.

use fortuna_cognition::aeolus_forecast::Variable;
use fortuna_live::aeolus_venue::station_series;

#[test]
fn maps_every_explicitly_named_high_temp_station() {
    // Each rule names the station precisely → unambiguous ICAO → mapped.
    assert_eq!(station_series("KNYC", Variable::Tmax), Some("KXHIGHNY")); // Central Park, New York
    assert_eq!(station_series("KAUS", Variable::Tmax), Some("KXHIGHAUS")); // Austin Bergstrom
    assert_eq!(station_series("KMDW", Variable::Tmax), Some("KXHIGHCHI")); // Chicago Midway
    assert_eq!(station_series("KLAX", Variable::Tmax), Some("KXHIGHLAX")); // Los Angeles Airport
    assert_eq!(station_series("KMIA", Variable::Tmax), Some("KXHIGHMIA")); // Miami International
    assert_eq!(station_series("KPHL", Variable::Tmax), Some("KXHIGHPHIL")); // Philadelphia International
}

#[test]
fn maps_the_one_daily_low_aeolus_emits() {
    // Aeolus forecasts KNYC tmin (knyc_tmin.json); NYC's NWS CLI station is
    // Central Park (KNYC), matching KXLOWTNYC.
    assert_eq!(station_series("KNYC", Variable::Tmin), Some("KXLOWTNYC"));
}

#[test]
fn returns_none_for_city_named_rules_not_pinned_to_a_station() {
    // The rule names only a CITY (Denver, Atlanta, Boston, …), so the exact NWS
    // CLI station is not pinned by the contract → conservative None.
    assert_eq!(station_series("KDEN", Variable::Tmax), None); // "Denver, CO"
    assert_eq!(station_series("KATL", Variable::Tmax), None); // "Atlanta"
    assert_eq!(station_series("KBOS", Variable::Tmax), None); // "Boston"
    assert_eq!(station_series("KLAS", Variable::Tmax), None); // "Las Vegas"
    assert_eq!(station_series("KSEA", Variable::Tmax), None); // "Seattle"
    assert_eq!(station_series("KSFO", Variable::Tmax), None); // "San Francisco"
}

#[test]
fn returns_none_for_ambiguous_multi_airport_metros() {
    // "Dallas" / "Washington DC" / "Houston" have multiple major airports — the
    // rule does not say which, so no entry is invented.
    assert_eq!(station_series("KDFW", Variable::Tmax), None);
    assert_eq!(station_series("KDAL", Variable::Tmax), None);
    assert_eq!(station_series("KDCA", Variable::Tmax), None);
    assert_eq!(station_series("KIAH", Variable::Tmax), None);
}

#[test]
fn the_variable_is_part_of_the_key() {
    // A station mapped for tmax is NOT mapped for tmin unless its low series is
    // separately grounded (only NYC is). KAUS/KMDW/KLAX/… have no grounded low.
    assert_eq!(station_series("KAUS", Variable::Tmin), None);
    assert_eq!(station_series("KMDW", Variable::Tmin), None);
    assert_eq!(station_series("KLAX", Variable::Tmin), None);
    assert_eq!(station_series("KMIA", Variable::Tmin), None);
    assert_eq!(station_series("KPHL", Variable::Tmin), None);
}

#[test]
fn returns_none_for_unknown_stations() {
    assert_eq!(station_series("KZZZ", Variable::Tmax), None);
    assert_eq!(station_series("", Variable::Tmax), None);
    assert_eq!(station_series("NYC", Variable::Tmax), None); // the nws_station_id, not the station code
}
