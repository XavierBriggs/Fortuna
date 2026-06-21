//! WS4 W1 — golden-JSON contract tests for the chain-view the UI session renders.
//! Asserts (1) a fully-populated chain round-trips by value, (2) optional stages are OMITTED from
//! the JSON when absent (clean shape for the UI) yet still round-trip, and (3) the WS3 `validation`
//! field accepts any forward-declared JSON shape.

use fortuna_ops::chain_view::*;
use serde_json::json;

fn full_chain() -> ChainView {
    ChainView {
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
        validation: Some(
            json!({ "pbo": 0.03, "spa_p_c": 0.02, "family_n_trials": 48, "verdict": "Go" }),
        ),
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

#[test]
fn validation_accepts_forward_declared_json() {
    // WS3's ValidationRun isn't built yet; the contract must accept any JSON shape it later commits.
    let mut cv = full_chain();
    cv.validation = Some(json!({ "anything": ["WS3", "shape"], "nested": { "ok": true } }));
    let s = serde_json::to_string(&cv).unwrap();
    let back: ChainView = serde_json::from_str(&s).unwrap();
    assert_eq!(cv, back);
}
