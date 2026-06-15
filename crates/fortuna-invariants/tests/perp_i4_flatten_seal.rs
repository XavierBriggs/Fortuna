//! I1 + I4 (spec 5.15, T5.B8): the kill-switch perp FLATTEN seal.
//!
//! ADD-ONLY — this file adds pins; it weakens nothing. The kill-switch's perp
//! flatten places a close ONLY as a sealed `GatedPerpOrder` it obtained from the
//! real perp gate (I1: the switch sits on the consumer side of the seal — it
//! never constructs orders, it asks `evaluate_perp`). These pins encode that the
//! seal behaves correctly for the flatten path AND that wiring the seal
//! (fortuna-gates) into the kill-switch kept its I4 independence intact.
//!
//! - (a) a VALID reduce-only close (opposite the position, qty == |pos|, limit in
//!   band) SEALS with a complete PERP_ALL pass trail and `reduce_only()==true` —
//!   and it seals even at ZERO equity, because the reduce-only capital checks
//!   WAIVE (an exposure-reducing close can only improve headroom).
//! - (b) a SAME-DIRECTION "reduce-only" is not exposure-reducing → rejects at
//!   MarginHeadroom, never seals.
//! - (c) an OVERSIZED reduce-only (qty > |pos|) "would flip, not reduce" → rejects,
//!   never seals.
//! - (d) a reduce-only with NO position → rejects, never seals.
//! - (e) fortuna-killswitch's resolved dependency graph: the I4 forbidden set
//!   (sqlx / tokio-postgres / postgres / fortuna-ledger / fortuna-cognition) stays
//!   ABSENT, and fortuna-gates (the seal) is now PRESENT.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpPosition, PerpPrice};
use fortuna_gates::perp::{PerpCandidateOrder, PerpGateInputs, PERP_ALL};
use fortuna_gates::{GateCheck, GateConfig, GatePipeline, Verdict};
use std::collections::BTreeSet;
use std::process::Command;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T08:00:00.000Z").unwrap()
}

/// A gate configured for the flatten path: strategy `killswitch_flatten`, venue
/// `kinetics`, asset `KXBTCPERP`, a generous price band (so a small slippage
/// passes PriceSanity). Mirrors perp_i1's fixture.
fn pipeline() -> GatePipeline {
    let cfg: GateConfig = toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 100000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 100000000
        per_event_exposure_cents = 100000000
        require_event_mapping = false

        [per_strategy.killswitch_flatten]
        max_exposure_cents = 100000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 0

        [rate.kinetics]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000

        [perp.venues.kinetics]
        max_total_notional_cents = 10000000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_bps = 2000
        assumed_fee_bps = 12
        funding_drag_bps_per_window = 4
        min_liquidation_distance_bps = 100
        mm_safety_multiplier_pct = 130

        [perp.assets.KXBTCPERP]
        max_leverage_x10 = 50
        max_notional_cents = 5000000
        mm_curve = [[1000000, 500], [5000000, 800]]
        "#,
    )
    .unwrap();
    GatePipeline::new(cfg).unwrap()
}

fn long_position(qty: i64) -> PerpPosition {
    PerpPosition {
        market: MarketId::new("KXBTCPERP").unwrap(),
        qty: Contracts::new(qty),
        avg_entry: PerpPrice::new(60_000),
    }
}

/// A reduce-only close candidate — exactly what `freeze_cancel_perp_and_flatten`
/// builds per position (limit crosses the mark by a small slippage).
fn close_candidate(action: Action, qty: i64) -> PerpCandidateOrder {
    let mut g = IdGen::new(7);
    PerpCandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("killswitch_flatten").unwrap(),
        venue: VenueId::new("kinetics").unwrap(),
        market: MarketId::new("KXBTCPERP").unwrap(),
        action,
        reduce_only: true,
        limit_price: PerpPrice::new(62_500),
        qty: Contracts::new(qty),
        fair_value: PerpPrice::new(62_600),
        holding_windows: 1,
        client_order_id: ClientOrderId::new("ks-flat-1").unwrap(),
    }
}

fn inputs<'a>(
    account: &'a MarginAccountView,
    position: Option<&'a PerpPosition>,
    recent: &'a BTreeSet<String>,
) -> PerpGateInputs<'a> {
    PerpGateInputs {
        now: t0(),
        account,
        position,
        conservative_mark: PerpPrice::new(62_600),
        venue_open_notional_cents: Cents::ZERO,
        own_resting: &[],
        recent_client_order_ids: recent,
    }
}

// (a) ----------------------------------------------------------------------

#[test]
fn a_valid_reduce_only_close_seals_with_a_full_pass_trail() {
    let mut gates = pipeline();
    // ZERO equity ON PURPOSE: the reduce-only capital waivers mean a close still
    // seals (margin headroom "can only improve") — never blocked by equity.
    let account = MarginAccountView::compute(Cents::ZERO, &[], Cents::ZERO).unwrap();
    let pos = long_position(5);
    let recent = BTreeSet::new();
    let out = gates.evaluate_perp(
        &close_candidate(Action::Sell, 5), // Sell closes a LONG; qty == |pos|
        &inputs(&account, Some(&pos), &recent),
    );
    let sealed = out.gated.expect("a valid reduce-only long-close must seal");
    assert!(sealed.reduce_only(), "the sealed close carries reduce_only");
    assert_eq!(sealed.action(), Action::Sell);
    assert_eq!(sealed.qty(), Contracts::new(5));
    assert_eq!(
        out.records.len(),
        PERP_ALL.len(),
        "a seal carries the COMPLETE PERP_ALL trail"
    );
    for (rec, expected) in out.records.iter().zip(PERP_ALL) {
        assert_eq!(rec.check, expected);
        assert!(
            matches!(rec.verdict, Verdict::Pass),
            "{:?} must Pass on a sealed close",
            rec.check
        );
    }
}

// (b) ----------------------------------------------------------------------

#[test]
fn a_same_direction_reduce_only_rejects_at_margin_headroom_and_never_seals() {
    let mut gates = pipeline();
    let account = MarginAccountView::compute(Cents::ZERO, &[], Cents::ZERO).unwrap();
    let pos = long_position(5);
    let recent = BTreeSet::new();
    // Buy on a LONG = the position's OWN direction = not exposure-reducing.
    let out = gates.evaluate_perp(
        &close_candidate(Action::Buy, 5),
        &inputs(&account, Some(&pos), &recent),
    );
    let rej = out
        .gated
        .expect_err("a same-direction reduce-only must NOT seal");
    assert_eq!(rej.check, GateCheck::MarginHeadroom);
    let last = out.records.last().unwrap();
    assert_eq!(last.check, GateCheck::MarginHeadroom);
    assert!(matches!(last.verdict, Verdict::Reject));
}

// (c) ----------------------------------------------------------------------

#[test]
fn an_oversized_reduce_only_would_flip_and_never_seals() {
    let mut gates = pipeline();
    let account = MarginAccountView::compute(Cents::ZERO, &[], Cents::ZERO).unwrap();
    let pos = long_position(5);
    let recent = BTreeSet::new();
    // Opposite direction (Sell), but qty 6 > |pos| 5 → "would flip, not reduce".
    let out = gates.evaluate_perp(
        &close_candidate(Action::Sell, 6),
        &inputs(&account, Some(&pos), &recent),
    );
    let rej = out
        .gated
        .expect_err("an oversized reduce-only (would flip) must NOT seal");
    assert_eq!(rej.check, GateCheck::MarginHeadroom);
}

// (d) ----------------------------------------------------------------------

#[test]
fn a_reduce_only_with_no_position_never_seals() {
    let mut gates = pipeline();
    let account = MarginAccountView::compute(Cents::ZERO, &[], Cents::ZERO).unwrap();
    let recent = BTreeSet::new();
    let out = gates.evaluate_perp(
        &close_candidate(Action::Sell, 5),
        &inputs(&account, None, &recent), // no position
    );
    let rej = out
        .gated
        .expect_err("a reduce-only with no position must NOT seal");
    assert_eq!(rej.check, GateCheck::MarginHeadroom);
}

// (e) ----------------------------------------------------------------------

#[test]
fn killswitch_dep_graph_keeps_i4_forbidden_absent_and_now_includes_the_gate_seal() {
    // Replicate the i4_killswitch_independence structural walk: resolve
    // fortuna-killswitch's transitive NORMAL deps from `cargo metadata`.
    let metadata = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(metadata.status.success(), "cargo metadata failed");
    let doc: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let resolve = doc["resolve"]["nodes"].as_array().unwrap();
    let ks = resolve
        .iter()
        .find(|n| {
            n["id"]
                .as_str()
                .unwrap_or("")
                .contains("fortuna-killswitch")
        })
        .expect("killswitch node in resolve graph");
    let mut to_visit: Vec<String> = vec![ks["id"].as_str().unwrap().to_string()];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = to_visit.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(node) = resolve.iter().find(|n| n["id"] == serde_json::json!(id)) {
            for dep in node["deps"].as_array().unwrap_or(&Vec::new()) {
                let kinds = dep["dep_kinds"].as_array().cloned().unwrap_or_default();
                let normal = kinds.iter().any(|k| k["kind"].is_null()) || kinds.is_empty();
                if normal {
                    if let Some(pkg) = dep["pkg"].as_str() {
                        to_visit.push(pkg.to_string());
                    }
                }
            }
        }
    }
    // The I4 forbidden set stays ABSENT (adding fortuna-gates introduced none of it).
    let forbidden = [
        "sqlx",
        "tokio-postgres",
        "postgres",
        "fortuna-ledger",
        "fortuna-cognition",
    ];
    for id in &seen {
        for bad in forbidden {
            assert!(
                !id.contains(bad),
                "I4 violated: fortuna-killswitch's dependency graph contains {bad:?} ({id})"
            );
        }
    }
    // And the SEAL is wired: fortuna-gates is now a normal dependency.
    assert!(
        seen.iter().any(|id| id.contains("fortuna-gates")),
        "the perp flatten seals through fortuna-gates, so it MUST be in the dep graph"
    );
}
