//! WS4 W1 — golden-JSON contract tests for the chain-view the UI session renders.
//! Asserts (1) a fully-populated chain round-trips by value, (2) optional stages are OMITTED from
//! the JSON when absent (clean shape for the UI) yet still round-trip, and (3) the WS3
//! `validation` field accepts the REAL `fortuna_backtest::sweep::ValidationRun` shape (not a
//! placeholder) and round-trips it losslessly. This pins the WS3→WS4 contract wire shape for the UI.

use fortuna_backtest::sweep::{RecalMethod, SelectedConfig, TrialSpace, ValidationRun};
use fortuna_ops::chain_view::*;
use fortuna_scoring::GoDecision;

fn real_validation_run() -> ValidationRun {
    // A minimal but real ValidationRun — all fields populated with representative
    // values that round-trip through serde_json::Value without loss.
    ValidationRun {
        run_id: "01SWEEPTEST0000000000AABB".to_string(),
        scope: "weather:KNYC:tmax".to_string(),
        producer: None,
        trial_space: TrialSpace {
            calibration_windows: vec![30, 60],
            recal_methods: vec![RecalMethod::Platt],
            scopes: vec!["weather:KNYC:tmax".to_string()],
            go_thresholds: vec![0.5],
        },
        n_trials: 2,
        family_n_trials: 4,
        selected_config: Some(SelectedConfig {
            calibration_window: 30,
            recal_method: RecalMethod::Platt,
            go_threshold: 0.5,
        }),
        brier_edge: 0.042,
        brier_pbo: 0.03,
        brier_spa_p: 0.02,
        clv_edge: 0.011,
        clv_pbo: 0.08,
        clv_spa_p: 0.12,
        effective_n: 18.0,
        mintrl_ok: true,
        sharpe_dsr: 0.71,
        verdict: GoDecision::Go,
        computed_at: "2026-06-22T12:00:00.000Z".to_string(),
    }
}

fn full_chain() -> ChainView {
    let vr = real_validation_run();
    ChainView {
        event: EventRef {
            event_linkage: "weather:NYC:tmax:2026-06-23#ge87".to_string(),
            category: "temperature_ny".to_string(),
            scope: "weather:KNYC:tmax".to_string(),
            target_date: "2026-06-23".to_string(),
            // NOTE: market_ticker maps to market_id — the events table has no separate
            // ticker column. See GAPS.md: "events table has no ticker column".
            market_ticker: "KXHIGHTNY-26JUN23-B87.5".to_string(),
        },
        safety: SafetyPills {
            execution_mode: "paper_ledger".to_string(),
            order_mutation_enabled: false,
            book_freshness_secs: Some(42),
        },
        signals: vec![SignalRef {
            source: "aeolus".to_string(),
            kind: "aeolus.forecast".to_string(),
            at: "2026-06-23T04:00:00.000Z".to_string(),
            summary: "mu=72.68 sigma=2.71".to_string(),
        }],
        producers: vec![
            ProducerBelief {
                producer_id: "aeolus".to_string(),
                producer_type: "ScalarProducer".to_string(),
                mind_id: None,
                mind_version: None,
                p_raw: 0.38,
                p_cal: Some(0.41),
                rationale: None,
                belief_at: "2026-06-23T05:00:00.000Z".to_string(),
                score: Some(BeliefScore {
                    status: "resolved".to_string(),
                    outcome: Some(0.0),
                    brier: Some(0.1681),
                    clv_bps: Some(64.0),
                }),
            },
            ProducerBelief {
                producer_id: "meteorologist".to_string(),
                producer_type: "Mind".to_string(),
                mind_id: Some("meteorologist".to_string()),
                mind_version: Some(5),
                p_raw: 0.31,
                p_cal: Some(0.33),
                rationale: Some("approaching warm front; convective cap risk".to_string()),
                belief_at: "2026-06-23T05:00:00.000Z".to_string(),
                // CLV is market-level: identical to aeolus's because they share the bracket.
                score: Some(BeliefScore {
                    status: "resolved".to_string(),
                    outcome: Some(0.0),
                    brier: Some(0.1089),
                    clv_bps: Some(64.0),
                }),
            },
        ],
        proposal: Some(ProposalRef {
            market: "KXHIGHTNY-26JUN23-B87.5".to_string(),
            side: "no".to_string(),
            max_price_cents: 11,
            size: 7,
            thesis: "envelope + AFD cap below bracket".to_string(),
            belief_ref: "01BELIEF...".to_string(),
            urgency: "passive".to_string(),
        }),
        gate: Some(GateResult {
            decision: "accept".to_string(),
            checks: vec![GateCheck {
                name: "drawdown_halt".to_string(),
                passed: true,
                detail: None,
            }],
        }),
        fill: Some(FillRef {
            price_cents: 11,
            qty: 7,
            orders: 0,
            at: "2026-06-23T05:01:00.000Z".to_string(),
        }),
        settlement: Some(SettlementRef {
            outcome: 0.0,
            realized_pnl_cents: -77,
            settled_at: "2026-06-24T16:00:00.000Z".to_string(),
            resolution_source: "nws_cli".to_string(),
        }),
        scorecard: None, // the WS2 Scorecard composes here; its own serialization is WS2-tested.
        validation: Some(serde_json::to_value(&vr).expect("ValidationRun must serialize")),
    }
}

#[test]
fn full_chain_round_trips_by_value() {
    let cv = full_chain();
    let s = serde_json::to_string(&cv).expect("serialize");
    let back: ChainView = serde_json::from_str(&s).expect("deserialize");
    assert_eq!(cv, back, "the full chain must round-trip without loss");
}

#[test]
fn head_to_head_carries_both_producers_with_clv() {
    // The showpiece: both producers present with brier (differentiator) + clv (market-level/shared).
    let cv = full_chain();
    assert_eq!(cv.producers.len(), 2);
    let v = serde_json::to_value(&cv).unwrap();
    let producers = v["producers"].as_array().unwrap();
    for p in producers {
        assert!(
            p["score"]["brier"].is_number(),
            "each producer carries a brier"
        );
        assert!(
            p["score"]["clv_bps"].is_number(),
            "each producer carries clv (market-level)"
        );
    }
    // CLV is shared because they share the bracket — same value, not two independent measurements.
    assert_eq!(
        producers[0]["score"]["clv_bps"],
        producers[1]["score"]["clv_bps"]
    );
}

#[test]
fn minimal_chain_omits_absent_stages_yet_round_trips() {
    let cv = ChainView {
        event: EventRef {
            event_linkage: "weather:NYC:tmax:2026-06-23#ge87".to_string(),
            category: "temperature_ny".to_string(),
            scope: "weather:KNYC:tmax".to_string(),
            target_date: "2026-06-23".to_string(),
            market_ticker: "KXHIGHTNY-26JUN23-B87.5".to_string(),
        },
        safety: SafetyPills {
            execution_mode: "paper_ledger".to_string(),
            order_mutation_enabled: false,
            book_freshness_secs: None,
        },
        signals: vec![],
        producers: vec![],
        proposal: None,
        gate: None,
        fill: None,
        settlement: None,
        scorecard: None,
        validation: None,
    };
    let v = serde_json::to_value(&cv).unwrap();
    // Required stages present:
    for key in ["event", "safety", "signals", "producers"] {
        assert!(v.get(key).is_some(), "required key {key} must be present");
    }
    // Absent optional stages OMITTED (clean shape for the UI):
    for key in [
        "proposal",
        "gate",
        "fill",
        "settlement",
        "scorecard",
        "validation",
    ] {
        assert!(
            v.get(key).is_none(),
            "absent optional key {key} must be omitted"
        );
    }
    // ...and a chain at minimal maturity still round-trips:
    let back: ChainView = serde_json::from_value(v).expect("deserialize minimal");
    assert_eq!(cv, back);
}

/// Pins the REAL ValidationRun wire shape for the UI — must be a real
/// `fortuna_backtest::sweep::ValidationRun`, not a placeholder JSON.
/// Asserts:
///   - `brier_pbo` is a number (was fictional "pbo" in prior placeholder)
///   - `brier_spa_p` is a number (was fictional "spa_p_c" in prior placeholder)
///   - `verdict` round-trips (must serialize as the GoDecision string)
///   - `family_n_trials` is an integer
///   - the full value round-trips into ChainView without loss
#[test]
fn validation_carries_real_validation_run_fields_and_round_trips() {
    let vr = real_validation_run();
    let vr_value = serde_json::to_value(&vr).expect("ValidationRun must serialize");

    // Pin the fields the UI session uses — these are REAL ValidationRun fields,
    // not the prior fictional pbo/spa_p_c names.
    assert!(
        vr_value["brier_pbo"].is_number(),
        "brier_pbo must be a number in the wire shape"
    );
    assert!(
        vr_value["brier_spa_p"].is_number(),
        "brier_spa_p must be a number in the wire shape"
    );
    assert!(
        vr_value["family_n_trials"].is_number(),
        "family_n_trials must be a number"
    );
    // verdict must be the GoDecision string representation (snake_case via serde rename_all).
    let verdict_str = vr_value["verdict"]
        .as_str()
        .expect("verdict must be a string");
    assert!(
        verdict_str == "go" || verdict_str == "no_go" || verdict_str == "insufficient",
        "verdict must be a GoDecision snake_case variant, got: {verdict_str}"
    );

    // The full ChainView with this validation must round-trip losslessly.
    let mut cv = ChainView {
        event: EventRef {
            event_linkage: "weather:NYC:tmax:2026-06-23#ge87".to_string(),
            category: "temperature_ny".to_string(),
            scope: "weather:KNYC:tmax".to_string(),
            target_date: "2026-06-23".to_string(),
            market_ticker: "KXHIGHTNY-26JUN23-B87.5".to_string(),
        },
        safety: SafetyPills {
            execution_mode: "paper_ledger".to_string(),
            order_mutation_enabled: false,
            book_freshness_secs: None,
        },
        signals: vec![],
        producers: vec![],
        proposal: None,
        gate: None,
        fill: None,
        settlement: None,
        scorecard: None,
        validation: None,
    };
    cv.validation = Some(vr_value.clone());

    let serialized = serde_json::to_string(&cv).expect("ChainView must serialize");
    let back: ChainView = serde_json::from_str(&serialized).expect("ChainView must deserialize");
    assert_eq!(
        cv, back,
        "ChainView with real ValidationRun must round-trip losslessly"
    );
    // The validation value must survive the round-trip intact.
    assert_eq!(
        back.validation.as_ref().unwrap()["brier_pbo"],
        vr_value["brier_pbo"],
        "brier_pbo must survive ChainView round-trip"
    );
}
