//! F5: identity-tuple dedup tests. Forecasts are built through the real
//! `parse_envelope` (the only constructor of the validated `AeolusForecast`), so
//! the dedup is exercised on genuinely-parsed values.

use fortuna_cognition::aeolus_dedup::dedup_forecasts;
use fortuna_cognition::aeolus_forecast::{parse_envelope, AeolusForecast, Variable};
use fortuna_core::clock::UtcTimestamp;

/// A minimal valid v2 envelope for a given identity + μ (built as JSON so it goes
/// through the strict parser, not a private constructor).
fn fc(station: &str, variable: &str, date: &str, run_at: &str, mu: f64) -> AeolusForecast {
    let body = serde_json::json!({
        "schema": "aeolus.forecast/v2",
        "station": station, "nws_station_id": station,
        "variable": variable, "units": "degF", "target_date": date,
        "run_at": run_at, "next_run_at": "2026-06-13T06:00:00+00:00",
        "valid_until": "2026-06-13T04:00:00+00:00",
        "distribution": {"family": "normal", "mu": mu, "sigma": 2.0, "model_version": "sar-semos-v1"},
        "skill": {"crps": 1.2, "crpss_vs_raw": null, "n_scored": 30, "window_days": 30, "as_of": "2026-06-12T00:00:00+00:00"},
        "resolution": {"authority": "nws_observed_high", "nws_station_id": station, "settles_after": "2026-06-14T10:00:00+00:00", "note": "x"},
        "brackets": [{"event_hint": format!("{station}-{date}-{variable}-ge85"), "threshold_f": 85, "comparison": "ge", "p": 0.5}]
    })
    .to_string();
    parse_envelope(&body).expect("template envelope parses")
}

fn ts(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(s).unwrap()
}

#[test]
fn newest_run_wins_within_a_slot_regardless_of_input_order() {
    let older = || {
        fc(
            "KNYC",
            "tmax",
            "2026-06-13",
            "2026-06-13T00:00:00+00:00",
            80.0,
        )
    };
    let newer = || {
        fc(
            "KNYC",
            "tmax",
            "2026-06-13",
            "2026-06-13T06:00:00+00:00",
            85.0,
        )
    };

    for input in [vec![older(), newer()], vec![newer(), older()]] {
        let out = dedup_forecasts(input);
        assert_eq!(out.len(), 1, "one slot collapses to one forecast");
        assert_eq!(out[0].run_at(), ts("2026-06-13T06:00:00+00:00"));
        assert!(
            (out[0].mu() - 85.0).abs() < 1e-9,
            "the newest run's μ survives"
        );
    }
}

#[test]
fn same_run_at_revision_resolves_to_the_later_received() {
    let first = fc(
        "KNYC",
        "tmax",
        "2026-06-13",
        "2026-06-13T00:00:00+00:00",
        80.0,
    );
    let revised = fc(
        "KNYC",
        "tmax",
        "2026-06-13",
        "2026-06-13T00:00:00+00:00",
        82.0,
    );
    let out = dedup_forecasts(vec![first, revised]);
    assert_eq!(out.len(), 1);
    assert!(
        (out[0].mu() - 82.0).abs() < 1e-9,
        "a same-run_at correction supersedes (later-received wins, contract §3)"
    );
}

#[test]
fn distinct_slots_all_survive_in_first_seen_order() {
    let run = "2026-06-13T00:00:00+00:00";
    let a = fc("KNYC", "tmax", "2026-06-13", run, 80.0);
    let b = fc("KNYC", "tmin", "2026-06-13", run, 60.0); // different variable
    let c = fc("KBOS", "tmax", "2026-06-13", run, 78.0); // different station
    let d = fc("KNYC", "tmax", "2026-06-14", run, 81.0); // different target_date
    let out = dedup_forecasts(vec![a, b, c, d]);
    assert_eq!(
        out.len(),
        4,
        "station / variable / target_date each define a distinct slot"
    );
    let stations: Vec<&str> = out.iter().map(|f| f.station()).collect();
    let vars: Vec<Variable> = out.iter().map(|f| f.variable()).collect();
    assert_eq!(
        stations,
        vec!["KNYC", "KNYC", "KBOS", "KNYC"],
        "first-seen order preserved"
    );
    assert_eq!(
        vars,
        vec![
            Variable::Tmax,
            Variable::Tmin,
            Variable::Tmax,
            Variable::Tmax
        ]
    );
}

#[test]
fn empty_and_single_are_identity() {
    assert!(dedup_forecasts(Vec::new()).is_empty());
    let out = dedup_forecasts(vec![fc(
        "KNYC",
        "tmax",
        "2026-06-13",
        "2026-06-13T00:00:00+00:00",
        80.0,
    )]);
    assert_eq!(out.len(), 1);
}

#[test]
fn many_runs_collapse_to_the_newest_irrespective_of_arrival_order() {
    let runs = [
        "2026-06-13T00:00:00+00:00",
        "2026-06-13T06:00:00+00:00",
        "2026-06-13T12:00:00+00:00",
        "2026-06-13T18:00:00+00:00",
    ];
    let mut v: Vec<AeolusForecast> = runs
        .iter()
        .enumerate()
        .map(|(i, r)| fc("KNYC", "tmax", "2026-06-13", r, 80.0 + i as f64))
        .collect();
    v.reverse(); // arrive newest-first
    let out = dedup_forecasts(v);
    assert_eq!(out.len(), 1);
    assert!(
        (out[0].mu() - 83.0).abs() < 1e-9,
        "the 18:00 run (μ=83) wins regardless of arrival order"
    );
    assert_eq!(out[0].run_at(), ts("2026-06-13T18:00:00+00:00"));
}
