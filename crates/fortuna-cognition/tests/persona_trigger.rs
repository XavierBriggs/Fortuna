//! Track E E.3b: the persona trigger layer (design §7). Tests from the design:
//! signal-driven matching (a persona fires only on kinds it reads), the
//! fire-once-per-period cadences (generalizing DailyScheduler), and the
//! per-(persona, region) serialization/debounce — duplicate/concurrent triggers
//! coalesce into ONE in-flight run (the §8/§15 "coalesced re-triggers → one run").

use fortuna_cognition::persona_trigger::{
    persona_region_key, Cadence, CadenceError, CadenceScheduler, PersonaTriggerGate,
    PersonaTriggerSpec,
};
use fortuna_cognition::signals::TriggerDecision;
use fortuna_core::clock::UtcTimestamp;

fn at(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).unwrap()
}

// ---- signal-driven matching (design §7) ----

#[test]
fn a_persona_fires_only_on_signal_kinds_it_reads() {
    let spec = PersonaTriggerSpec {
        persona_id: "meteorologist".to_string(),
        reads_signal_kinds: vec![
            "aeolus.forecast".to_string(),
            "nws.observed_high".to_string(),
        ],
        cadences: vec![],
    };
    assert!(spec.fires_on_signal("aeolus.forecast"));
    assert!(spec.fires_on_signal("nws.observed_high"));
    assert!(
        !spec.fires_on_signal("rss.headline"),
        "a kind the persona does not read never triggers it"
    );
}

#[test]
fn cadence_validate_rejects_an_out_of_range_daily_hour() {
    // A typo like hour: 24 would silently never fire — reject it at config-load.
    assert_eq!(
        Cadence::DailyAtHourUtc { hour: 24 }.validate(),
        Err(CadenceError::HourOutOfRange(24))
    );
    assert!(Cadence::DailyAtHourUtc { hour: 0 }.validate().is_ok());
    assert!(Cadence::DailyAtHourUtc { hour: 23 }.validate().is_ok());
    assert!(Cadence::EveryHours { hours: 0 }.validate().is_ok()); // clamped at use
}

#[test]
fn persona_region_keys_do_not_collide_on_ambiguous_components() {
    // A naive "::" join maps both of these to "a::b::c"; the unit separator keeps
    // the two distinct (persona, region) pairs apart.
    assert_ne!(
        persona_region_key("a", "b::c"),
        persona_region_key("a::b", "c")
    );
    assert_eq!(
        persona_region_key("meteo", "knyc"),
        persona_region_key("meteo", "knyc")
    );
}

// ---- cadences: fire once per period (design §7) ----

#[test]
fn every_hours_cadence_fires_once_per_window() {
    let mut sched = CadenceScheduler::new();
    let c = Cadence::EveryHours { hours: 6 };
    // First tick in the window fires.
    assert!(sched.due("meteo::r", &c, at("2026-06-12T00:00:00.000Z")));
    // Later in the SAME 6h window: not due.
    assert!(!sched.due("meteo::r", &c, at("2026-06-12T03:00:00.000Z")));
    assert!(!sched.due("meteo::r", &c, at("2026-06-12T05:59:00.000Z")));
    // The next 6h window: due again.
    assert!(sched.due("meteo::r", &c, at("2026-06-12T06:00:00.000Z")));
}

#[test]
fn daily_cadence_waits_for_its_hour_then_fires_once_per_day() {
    let mut sched = CadenceScheduler::new();
    let c = Cadence::DailyAtHourUtc { hour: 5 };
    // Before 05:00 UTC: not eligible.
    assert!(!sched.due("meteo::r", &c, at("2026-06-12T04:30:00.000Z")));
    // On/after 05:00 UTC: fires once.
    assert!(sched.due("meteo::r", &c, at("2026-06-12T05:30:00.000Z")));
    // Later the same day: not due again.
    assert!(!sched.due("meteo::r", &c, at("2026-06-12T18:00:00.000Z")));
    // Next day after the hour: due.
    assert!(sched.due("meteo::r", &c, at("2026-06-13T05:01:00.000Z")));
}

#[test]
fn cadence_keys_are_independent() {
    let mut sched = CadenceScheduler::new();
    let c = Cadence::EveryHours { hours: 6 };
    assert!(sched.due("a::r1", &c, at("2026-06-12T00:00:00.000Z")));
    // A different (persona, region) key fires independently in the same window.
    assert!(sched.due("a::r2", &c, at("2026-06-12T00:30:00.000Z")));
    assert!(!sched.due("a::r1", &c, at("2026-06-12T00:30:00.000Z")));
}

// ---- serialization + debounce: coalesce into ONE in-flight run (design §8) ----

#[test]
fn concurrent_triggers_for_one_region_coalesce_into_one_run() {
    let mut gate = PersonaTriggerGate::new(0);
    let now = at("2026-06-12T05:00:00.000Z");

    // First request fires.
    assert_eq!(gate.request("meteo", "knyc", now), TriggerDecision::Fire);
    gate.begin("meteo", "knyc");

    // While it runs, duplicate/concurrent requests are absorbed (one run).
    assert_eq!(
        gate.request("meteo", "knyc", now),
        TriggerDecision::CoalescedInFlight
    );
    assert_eq!(
        gate.request("meteo", "knyc", now),
        TriggerDecision::CoalescedInFlight
    );

    // Completion reports how many coalesced (audited, never silent).
    let coalesced = gate.complete("meteo", "knyc", now);
    assert_eq!(coalesced, 2);
}

#[test]
fn a_request_within_the_debounce_window_is_coalesced() {
    let mut gate = PersonaTriggerGate::new(60_000); // 60s debounce
    let t0 = at("2026-06-12T05:00:00.000Z");
    assert_eq!(gate.request("meteo", "knyc", t0), TriggerDecision::Fire);
    gate.begin("meteo", "knyc");
    gate.complete("meteo", "knyc", t0);

    // 30s later — inside the debounce window: coalesced.
    assert_eq!(
        gate.request("meteo", "knyc", at("2026-06-12T05:00:30.000Z")),
        TriggerDecision::CoalescedDebounce
    );
    // 61s later — outside: fires again.
    assert_eq!(
        gate.request("meteo", "knyc", at("2026-06-12T05:01:01.000Z")),
        TriggerDecision::Fire
    );
}

#[test]
fn different_regions_do_not_serialize_against_each_other() {
    let mut gate = PersonaTriggerGate::new(0);
    let now = at("2026-06-12T05:00:00.000Z");
    assert_eq!(gate.request("meteo", "knyc", now), TriggerDecision::Fire);
    gate.begin("meteo", "knyc");
    // A DIFFERENT region for the same persona fires independently.
    assert_eq!(gate.request("meteo", "kbos", now), TriggerDecision::Fire);
}
