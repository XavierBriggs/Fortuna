//! Aeolus↔Kalshi bucket matching tests, driven by the RECORDED forecast
//! (`knyc_tmax.json`, μ≈87.35/σ≈1.90) + the recorded KXHIGHNY 2026-06-13 day-set
//! (the contract §2 table). Proves the partition telescopes to 1.0 and the
//! beliefs map 1:1 onto the tradeable buckets.

use fortuna_cognition::aeolus_buckets::{
    aeolus_bucket_beliefs, score_bucket_briers, BucketKind, WeatherBucket,
};
use fortuna_cognition::aeolus_forecast::{parse_response, AeolusForecast};

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/aeolus/knyc_tmax.json"
));

fn forecast() -> AeolusForecast {
    parse_response(FIXTURE).expect("recorded fixture parses")[0].clone()
}

/// The recorded KXHIGHNY 2026-06-13 active day-set — a COMPLETE, non-overlapping
/// partition (≤86 | [87,88] | [89,90] | [91,92] | [93,94] | ≥95).
fn demo_buckets() -> Vec<WeatherBucket> {
    let m = |t: &str| format!("KXHIGHNY-26JUN13-{t}");
    vec![
        WeatherBucket {
            market_key: m("T87"),
            kind: BucketKind::LessEq { threshold_f: 86 },
        },
        WeatherBucket {
            market_key: m("B87.5"),
            kind: BucketKind::InRange { lo_f: 87, hi_f: 88 },
        },
        WeatherBucket {
            market_key: m("B89.5"),
            kind: BucketKind::InRange { lo_f: 89, hi_f: 90 },
        },
        WeatherBucket {
            market_key: m("B91.5"),
            kind: BucketKind::InRange { lo_f: 91, hi_f: 92 },
        },
        WeatherBucket {
            market_key: m("B93.5"),
            kind: BucketKind::InRange { lo_f: 93, hi_f: 94 },
        },
        WeatherBucket {
            market_key: m("T94"),
            kind: BucketKind::GreaterEq { threshold_f: 95 },
        },
    ]
}

#[test]
fn bucket_beliefs_partition_sums_to_one_and_maps_1_to_1() {
    let fc = forecast();
    let buckets = demo_buckets();
    let drafts = aeolus_bucket_beliefs(&fc, &buckets);
    assert_eq!(
        drafts.len(),
        6,
        "one propose-only belief per discovered bucket"
    );

    for (d, b) in drafts.iter().zip(buckets.iter()) {
        assert_eq!(
            d.event_id,
            format!("aeolus:{}", b.market_key),
            "event_id ↔ market 1:1"
        );
        assert!(d.p > 0.0 && d.p < 1.0, "valid belief probability");
        assert_eq!(d.p, d.p_raw, "propose-only: no calibration here");
    }

    // THE INVARIANT (contract §4.1): a complete day-set telescopes to 1.0.
    let total: f64 = drafts.iter().map(|d| d.p).sum();
    assert!(
        (total - 1.0).abs() < 1e-9,
        "bucket p's must sum to 1 (got {total})"
    );

    // The mass sits where the forecast says: the [87,88] bucket dominates (μ≈87.3).
    let b8788 = drafts
        .iter()
        .find(|d| d.event_id.ends_with("B87.5"))
        .unwrap();
    assert!(
        b8788.p > 0.30,
        "the in-the-money bucket carries real mass: {}",
        b8788.p
    );
}

#[test]
fn bucket_brier_outcomes_are_per_kind_against_the_realized_high() {
    let fc = forecast();
    let buckets = demo_buckets();
    // Realized high 88 → ONLY the [87,88] bucket is satisfied (a partition).
    let scores = score_bucket_briers(&fc, &buckets, 88.0);
    assert_eq!(scores.len(), 6);
    assert!(
        scores
            .iter()
            .find(|s| s.market_key.ends_with("B87.5"))
            .unwrap()
            .outcome,
        "88 ∈ [87,88]"
    );
    assert!(
        !scores
            .iter()
            .find(|s| s.market_key.ends_with("T87"))
            .unwrap()
            .outcome,
        "88 ≰ 86"
    );
    assert!(
        !scores
            .iter()
            .find(|s| s.market_key.ends_with("T94"))
            .unwrap()
            .outcome,
        "88 ≱ 95"
    );
    assert_eq!(
        scores.iter().filter(|s| s.outcome).count(),
        1,
        "exactly one bucket resolves true"
    );
    for s in &scores {
        let o = if s.outcome { 1.0 } else { 0.0 };
        assert!(
            (s.brier - (s.p_fortuna - o).powi(2)).abs() < 1e-12,
            "brier = (p−outcome)²"
        );
    }
}

#[test]
fn propose_only_surface_and_empty_input() {
    let fc = forecast();
    let drafts = aeolus_bucket_beliefs(&fc, &demo_buckets());
    // I6: the belief surface is exactly the data-only field set — no exec fields.
    let v = serde_json::to_value(&drafts[0]).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "event_id",
            "evidence",
            "horizon",
            "p",
            "p_raw",
            "provenance"
        ]
    );
    // No buckets discovered → no beliefs.
    assert!(aeolus_bucket_beliefs(&fc, &[]).is_empty());
}
