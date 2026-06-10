//! T2.1: canonical events, market-event edges, benchmark snapshot
//! scheduling, and CLV scoring (spec 5.12 + 5.5).
//!
//! Doctrine under test:
//! - Event lifecycle transitions are LEGAL-OR-ERROR (5.13 reference
//!   model): created -> active -> resolution_pending ->
//!   resolved_provisional -> resolved_final; provisional may excurse to
//!   disputed and return (or reverse); dead(voided|source_lost|mutated)
//!   reachable from any PRE-final state only.
//! - Edge confidence tiers gate usage: an unconfirmed edge NEVER
//!   satisfies a strategy that demands human confirmation (a wrong
//!   equivalence edge turns an arb into an unhedged position).
//! - Deterministic edge checks score proposals (resolution source match,
//!   horizon match) — the model proposes, arithmetic disposes.
//! - Snapshots: T-24h/T-1h/T-5m fire ONCE per (market,event,kind) when
//!   due, never before; on_trade is unscheduled.
//! - CLV uses the LATEST LIQUID pre-benchmark snapshot; illiquid or
//!   post-benchmark snapshots produce NO CLV, never fake CLV.
//!
//! Written BEFORE src/events.rs per the repository TDD doctrine.

use fortuna_cognition::events::{
    clv_bps, deterministic_edge_score, due_snapshots, CanonicalEvent, EdgeCheckInputs,
    EdgeProposal, EdgeTier, EventStatus, LiquidityPolicy, MappingType, SnapshotKind, SnapshotPoint,
    TakenKey,
};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, Side};
use fortuna_core::money::Cents;
use std::collections::BTreeSet;

fn t(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).unwrap()
}

fn benchmark() -> UtcTimestamp {
    t("2026-06-20T18:00:00.000Z")
}

fn event() -> CanonicalEvent {
    CanonicalEvent {
        event_id: "evt-1".to_string(),
        statement: "Team A beats Team B on 2026-06-20".to_string(),
        resolution_criteria: "official final score".to_string(),
        resolution_source: "league.example".to_string(),
        horizon: Some(t("2026-06-21T00:00:00.000Z")),
        benchmark_at: benchmark(),
        category: "sports".to_string(),
        status: EventStatus::Created,
        unscoreable: false,
    }
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

// ------------------------------------------------------------- lifecycle

#[test]
fn lifecycle_legal_path_and_dispute_excursion() {
    let mut e = event();
    e.transition(EventStatus::Active).unwrap();
    e.transition(EventStatus::ResolutionPending).unwrap();
    e.transition(EventStatus::ResolvedProvisional).unwrap();
    e.transition(EventStatus::Disputed).unwrap();
    e.transition(EventStatus::ResolvedProvisional).unwrap();
    e.transition(EventStatus::ResolvedFinal).unwrap();
    assert_eq!(e.status, EventStatus::ResolvedFinal);

    // Final is terminal: nothing leaves it.
    assert!(e.transition(EventStatus::Active).is_err());
    assert!(e.mark_dead("voided").is_err());
}

#[test]
fn lifecycle_illegal_jumps_error() {
    let mut e = event();
    assert!(e.transition(EventStatus::ResolvedFinal).is_err());
    assert!(e.transition(EventStatus::Disputed).is_err());
    e.transition(EventStatus::Active).unwrap();
    assert!(e.transition(EventStatus::ResolvedProvisional).is_err());
}

#[test]
fn dead_reachable_from_pre_final_only_with_valid_reason() {
    let mut e = event();
    e.transition(EventStatus::Active).unwrap();
    e.mark_dead("source_lost").unwrap();
    assert_eq!(e.status, EventStatus::Dead);
    assert!(
        e.transition(EventStatus::Active).is_err(),
        "dead is terminal"
    );

    let mut e2 = event();
    assert!(
        e2.mark_dead("because reasons").is_err(),
        "reason vocabulary is closed"
    );
}

// ----------------------------------------------------------------- edges

fn proposal(confirmed: bool) -> EdgeProposal {
    EdgeProposal {
        market: mkt("KXTEAM-A"),
        venue: "kalshi".to_string(),
        event_id: "evt-1".to_string(),
        mapping: MappingType::Direct,
        confidence: 0.9,
        proposed_by: "model:stub".to_string(),
        confirmed_by: if confirmed {
            Some("operator:xavier".to_string())
        } else {
            None
        },
    }
}

#[test]
fn tier_gating_unconfirmed_never_satisfies_confirmed_requirement() {
    let unconfirmed = proposal(false);
    let confirmed = proposal(true);
    assert_eq!(unconfirmed.tier(), EdgeTier::Proposed);
    assert_eq!(confirmed.tier(), EdgeTier::Confirmed);

    assert!(confirmed.tier().satisfies(EdgeTier::Proposed));
    assert!(confirmed.tier().satisfies(EdgeTier::Confirmed));
    assert!(unconfirmed.tier().satisfies(EdgeTier::Proposed));
    assert!(
        !unconfirmed.tier().satisfies(EdgeTier::Confirmed),
        "multi-leg/cross-venue strategies demand human confirmation"
    );
}

#[test]
fn deterministic_checks_score_source_and_horizon_match() {
    let e = event();
    // Market metadata agreeing on both source and horizon scores 1.0.
    let good = deterministic_edge_score(&EdgeCheckInputs {
        event_resolution_source: &e.resolution_source,
        market_resolution_source: "league.example",
        event_horizon: e.horizon,
        market_close_at: Some(t("2026-06-20T23:00:00.000Z")),
        horizon_tolerance_ms: 24 * 3_600_000,
    });
    assert_eq!(good, 1.0);

    // Source mismatch is the UMA-style failure mode: hard zero.
    let bad_source = deterministic_edge_score(&EdgeCheckInputs {
        event_resolution_source: &e.resolution_source,
        market_resolution_source: "tweets",
        event_horizon: e.horizon,
        market_close_at: Some(t("2026-06-20T23:00:00.000Z")),
        horizon_tolerance_ms: 24 * 3_600_000,
    });
    assert_eq!(bad_source, 0.0);

    // Horizon outside tolerance halves the score (still reviewable).
    let bad_horizon = deterministic_edge_score(&EdgeCheckInputs {
        event_resolution_source: &e.resolution_source,
        market_resolution_source: "league.example",
        event_horizon: e.horizon,
        market_close_at: Some(t("2026-07-15T00:00:00.000Z")),
        horizon_tolerance_ms: 24 * 3_600_000,
    });
    assert_eq!(bad_horizon, 0.5);
}

// ------------------------------------------------------------- scheduler

#[test]
fn scheduled_snapshots_fire_once_when_due_never_before() {
    let markets = vec![mkt("KXTEAM-A")];
    let mut taken: BTreeSet<TakenKey> = BTreeSet::new();

    // 30h out: nothing due.
    let due = due_snapshots(
        "evt-1",
        benchmark(),
        &markets,
        t("2026-06-19T12:00:00.000Z"),
        &taken,
    );
    assert!(due.is_empty());

    // 23h out: t24h due (and only it).
    let due = due_snapshots(
        "evt-1",
        benchmark(),
        &markets,
        t("2026-06-19T19:00:00.000Z"),
        &taken,
    );
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].kind, SnapshotKind::T24h);
    for d in &due {
        taken.insert(d.key());
    }

    // Same instant again: dedup'd.
    let due = due_snapshots(
        "evt-1",
        benchmark(),
        &markets,
        t("2026-06-19T19:00:00.000Z"),
        &taken,
    );
    assert!(due.is_empty());

    // 30 minutes out: t1h AND t5m windows both open; t24h already taken.
    let due = due_snapshots(
        "evt-1",
        benchmark(),
        &markets,
        t("2026-06-20T17:56:00.000Z"),
        &taken,
    );
    let kinds: Vec<SnapshotKind> = due.iter().map(|d| d.kind).collect();
    assert_eq!(kinds, vec![SnapshotKind::T1h, SnapshotKind::T5m]);

    // Past benchmark: NOTHING fires (post-event windows are excluded).
    let mut taken2 = BTreeSet::new();
    let due = due_snapshots(
        "evt-1",
        benchmark(),
        &markets,
        t("2026-06-20T19:00:00.000Z"),
        &taken2,
    );
    assert!(
        due.is_empty(),
        "post-benchmark scheduled snapshots are noise"
    );
    taken2.insert((String::new(), mkt("x"), SnapshotKind::T5m)); // silence unused warnings
}

// ------------------------------------------------------------------ CLV

fn snap(at: &str, bid: Option<i64>, ask: Option<i64>, qty: i64) -> SnapshotPoint {
    SnapshotPoint {
        at: t(at),
        best_bid: bid.map(Cents::new),
        best_ask: ask.map(Cents::new),
        bid_qty: if bid.is_some() { qty } else { 0 },
        ask_qty: if ask.is_some() { qty } else { 0 },
    }
}

fn policy() -> LiquidityPolicy {
    LiquidityPolicy {
        min_touch_qty: 5,
        max_spread_cents: 10,
    }
}

#[test]
fn clv_uses_latest_liquid_pre_benchmark_snapshot() {
    let snaps = vec![
        snap("2026-06-19T18:00:00.000Z", Some(40), Some(43), 50),
        snap("2026-06-20T17:00:00.000Z", Some(48), Some(51), 50), // latest liquid
        snap("2026-06-20T17:58:00.000Z", Some(60), Some(95), 50), // too wide
        snap("2026-06-20T19:00:00.000Z", Some(70), Some(72), 50), // post-benchmark
    ];
    // Bought YES at 44c; benchmark mid moved to (48+51)/2 = 49.5 -> we
    // beat the close by 5.5c on a 44c entry = +1250 bps.
    let clv = clv_bps(Cents::new(44), Side::Yes, benchmark(), &snaps, &policy()).unwrap();
    assert_eq!(clv, 1_250);

    // A NO position at 56c against the same books: NO mid = 100 - 49.5 =
    // 50.5; entry 56 beat by... mid moved AGAINST the NO holder: clv
    // negative ((50.5 - 56) / 56).
    let clv_no = clv_bps(Cents::new(56), Side::No, benchmark(), &snaps, &policy()).unwrap();
    assert!(clv_no < 0);
}

#[test]
fn clv_is_none_when_no_liquid_pre_benchmark_snapshot_exists() {
    // One-sided, undersized, too-wide, and post-benchmark snapshots only.
    let snaps = vec![
        snap("2026-06-20T16:00:00.000Z", Some(40), None, 50), // one-sided
        snap("2026-06-20T16:30:00.000Z", Some(40), Some(43), 2), // undersized
        snap("2026-06-20T17:00:00.000Z", Some(30), Some(70), 50), // too wide
        snap("2026-06-20T19:00:00.000Z", Some(48), Some(50), 50), // post-benchmark
    ];
    assert_eq!(
        clv_bps(Cents::new(44), Side::Yes, benchmark(), &snaps, &policy()),
        None,
        "stale or one-sided books produce NO CLV rather than fake CLV"
    );
}
