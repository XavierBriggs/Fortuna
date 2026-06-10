//! Veto scaffolding tests (spec Section 6, mech_extremes item 2; BUILD_PLAN
//! T1.3). The veto is REDUCE-ONLY: it can suppress or shrink a sized
//! candidate, never add or grow. Every assessment is auditable and every
//! suppressed/shrunk quantity is counterfactually scorable at settlement.
//!
//! Written BEFORE src/veto.rs per the repository TDD doctrine.

use fortuna_cognition::veto::{
    counterfactual_pnl, FillAssumption, KeepBps, StubVetoMind, VetoCandidate, VetoError, VetoMind,
    VetoVerdict,
};
use fortuna_core::book::{FeeModel, FillRole};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId, Side, StrategyId};
use fortuna_core::money::Cents;
use proptest::prelude::*;

fn candidate(market: &str, qty: i64) -> VetoCandidate {
    VetoCandidate {
        strategy: StrategyId::new("mech_extremes").unwrap(),
        market: MarketId::new(market).unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(92),
        fair_value: Cents::new(95),
        qty: Contracts::new(qty),
        yes_bid: Some(Cents::new(91)),
        yes_ask: Some(Cents::new(93)),
        category: None,
        thesis: "favorite-longshot fade".to_string(),
        as_of: UtcTimestamp::from_epoch_millis(1_750_000_000_000).unwrap(),
    }
}

/// Flat 1c/contract fee model for counterfactual math tests.
#[derive(Debug)]
struct FlatFee;
impl FeeModel for FlatFee {
    fn fee(
        &self,
        _role: FillRole,
        _price: Cents,
        qty: Contracts,
        _category: Option<&str>,
        _at: UtcTimestamp,
    ) -> Result<Cents, fortuna_core::book::FeeError> {
        Ok(Cents::new(qty.raw()))
    }
}

// ---------------------------------------------------------------- KeepBps

/// Reduce-only by construction: a shrink factor of zero (that is a
/// suppress, say so explicitly) or of 100%+ (that is allow-or-grow) cannot
/// be expressed at all.
#[test]
fn keep_bps_rejects_zero_and_full_or_more() {
    assert!(KeepBps::new(0).is_err());
    assert!(KeepBps::new(10_000).is_err());
    assert!(KeepBps::new(10_001).is_err());
    assert!(KeepBps::new(u16::MAX).is_err());
    assert!(KeepBps::new(1).is_ok());
    assert!(KeepBps::new(9_999).is_ok());
}

#[test]
fn keep_bps_apply_floors_toward_zero() {
    let half = KeepBps::new(5_000).unwrap();
    assert_eq!(half.apply(Contracts::new(10)).raw(), 5);
    // floor: 3 * 0.3333 = 0.9999 -> 0 contracts kept.
    let third = KeepBps::new(3_333).unwrap();
    assert_eq!(third.apply(Contracts::new(3)).raw(), 0);
    // 1 contract at 99.99% keeps 0 (floor), never rounds up to 1.
    let most = KeepBps::new(9_999).unwrap();
    assert_eq!(most.apply(Contracts::new(1)).raw(), 0);
}

proptest! {
    /// THE reduce-only property: no expressible shrink can ever yield more
    /// contracts than went in, and the result is never negative.
    #[test]
    fn shrink_never_grows(qty in 0i64..2_000_000, bps in 1u16..10_000) {
        let keep = KeepBps::new(bps).unwrap();
        let out = keep.apply(Contracts::new(qty));
        prop_assert!(out.raw() <= qty);
        prop_assert!(out.raw() >= 0);
    }
}

// ------------------------------------------------------------- serdeness

/// Verdicts and candidates land in append-only audit rows; their
/// serialization must round-trip stably.
#[test]
fn verdict_and_candidate_serde_round_trip() {
    let verdicts = vec![
        VetoVerdict::Allow,
        VetoVerdict::Shrink {
            keep: KeepBps::new(2_500).unwrap(),
            reason: "thin book".to_string(),
        },
        VetoVerdict::Suppress {
            reason: "headline risk".to_string(),
        },
    ];
    for v in verdicts {
        let json = serde_json::to_string(&v).unwrap();
        let back: VetoVerdict = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
    let c = candidate("KXTEST-A", 25);
    let json = serde_json::to_string(&c).unwrap();
    let back: VetoCandidate = serde_json::from_str(&json).unwrap();
    assert_eq!(c, back);
}

/// Deserializing a Shrink with an out-of-range keep_bps must FAIL: the
/// checked constructor cannot be bypassed through serde.
#[test]
fn shrink_deserialization_rejects_out_of_range_bps() {
    for bad in [0u32, 10_000, 60_000] {
        let json = format!(r#"{{"shrink":{{"keep":{bad},"reason":"x"}}}}"#);
        assert!(
            serde_json::from_str::<VetoVerdict>(&json).is_err(),
            "keep_bps {bad} must not deserialize"
        );
    }
}

// ------------------------------------------------------------- stub mind

#[tokio::test]
async fn stub_allow_all_allows_everything_at_zero_cost() {
    let stub = StubVetoMind::allow_all();
    for m in ["KXA", "KXB"] {
        let a = stub.assess(&candidate(m, 10)).await.unwrap();
        assert_eq!(a.verdict, VetoVerdict::Allow);
        assert_eq!(a.cost_cents, 0);
    }
}

/// Scripted verdicts are keyed by market; unscripted markets default to
/// Allow (the veto's null action is not interfering).
#[tokio::test]
async fn stub_scripted_returns_scripted_verdict_per_market() {
    let stub = StubVetoMind::scripted(vec![
        (
            MarketId::new("KXSUPPRESS").unwrap(),
            VetoVerdict::Suppress {
                reason: "scripted".to_string(),
            },
        ),
        (
            MarketId::new("KXSHRINK").unwrap(),
            VetoVerdict::Shrink {
                keep: KeepBps::new(5_000).unwrap(),
                reason: "scripted".to_string(),
            },
        ),
    ]);
    let s = stub.assess(&candidate("KXSUPPRESS", 10)).await.unwrap();
    assert!(matches!(s.verdict, VetoVerdict::Suppress { .. }));
    let h = stub.assess(&candidate("KXSHRINK", 10)).await.unwrap();
    assert!(matches!(h.verdict, VetoVerdict::Shrink { .. }));
    let a = stub.assess(&candidate("KXOTHER", 10)).await.unwrap();
    assert_eq!(a.verdict, VetoVerdict::Allow);
}

/// Same stub, same inputs, same outputs — the stub must be deterministic
/// (it is the DST stand-in for the real mind).
#[tokio::test]
async fn stub_is_deterministic_across_runs() {
    let mk = || {
        StubVetoMind::scripted(vec![(
            MarketId::new("KXX").unwrap(),
            VetoVerdict::Shrink {
                keep: KeepBps::new(1_234).unwrap(),
                reason: "det".to_string(),
            },
        )])
    };
    let one = mk();
    let two = mk();
    for m in ["KXX", "KXY", "KXX"] {
        let a = one.assess(&candidate(m, 7)).await.unwrap();
        let b = two.assess(&candidate(m, 7)).await.unwrap();
        assert_eq!(a.verdict, b.verdict);
        assert_eq!(a.cost_cents, b.cost_cents);
    }
}

/// The failing stub exercises the runner's veto-error path (provider down).
#[tokio::test]
async fn stub_failing_returns_provider_error() {
    let stub = StubVetoMind::failing("injected outage");
    let err = stub.assess(&candidate("KXA", 5)).await.unwrap_err();
    assert!(matches!(err, VetoError::Provider { .. }));
}

// --------------------------------------------------- counterfactual math

/// Suppressed BUY YES at 92c, market settles YES (payout 100c/contract):
/// the removed 10 contracts would have made 10*(100-92) = 80c gross minus
/// 10c maker fee = 70c. The veto FORFEITED 70c (positive hypothetical).
#[test]
fn counterfactual_buy_yes_winner() {
    let c = candidate("KXA", 10);
    let pnl = counterfactual_pnl(
        &c,
        Contracts::new(10),
        Side::Yes,
        Cents::new(100),
        &FlatFee,
        FillAssumption::FilledAtLimit,
    )
    .unwrap();
    assert_eq!(pnl.raw(), 70);
}

/// Same suppressed BUY YES, market settles NO: the removed contracts would
/// have lost 10*92 + 10 fee = 930c. The veto AVOIDED a 930c loss
/// (negative hypothetical = good veto).
#[test]
fn counterfactual_buy_yes_loser() {
    let c = candidate("KXA", 10);
    let pnl = counterfactual_pnl(
        &c,
        Contracts::new(10),
        Side::No,
        Cents::new(100),
        &FlatFee,
        FillAssumption::FilledAtLimit,
    )
    .unwrap();
    assert_eq!(pnl.raw(), -930);
}

/// BUY NO mirrors in NO-space: suppressed BUY NO at 95c, settles NO ->
/// would have made 5c/contract minus fee.
#[test]
fn counterfactual_buy_no_winner() {
    let mut c = candidate("KXA", 10);
    c.side = Side::No;
    c.limit_price = Cents::new(95);
    let pnl = counterfactual_pnl(
        &c,
        Contracts::new(10),
        Side::No,
        Cents::new(100),
        &FlatFee,
        FillAssumption::FilledAtLimit,
    )
    .unwrap();
    // 10*(100-95) - 10 = 40
    assert_eq!(pnl.raw(), 40);
}

/// A suppressed SELL is an exit that did not happen: selling YES at 92c
/// before a NO settlement would have SAVED 92c/contract (the lot went to
/// zero). Hypothetical = +910 (92*10 - 10 fee): the veto forfeited that
/// exit.
#[test]
fn counterfactual_sell_yes_before_loss() {
    let mut c = candidate("KXA", 10);
    c.action = Action::Sell;
    let pnl = counterfactual_pnl(
        &c,
        Contracts::new(10),
        Side::No,
        Cents::new(100),
        &FlatFee,
        FillAssumption::FilledAtLimit,
    )
    .unwrap();
    assert_eq!(pnl.raw(), 910);
}

/// Selling YES at 92c when YES settles at 100c would have COST 8c/contract
/// plus fee: hypothetical = -(8*10) - 10 = -90. Vetoing that sell was good.
#[test]
fn counterfactual_sell_yes_before_win() {
    let mut c = candidate("KXA", 10);
    c.action = Action::Sell;
    let pnl = counterfactual_pnl(
        &c,
        Contracts::new(10),
        Side::Yes,
        Cents::new(100),
        &FlatFee,
        FillAssumption::FilledAtLimit,
    )
    .unwrap();
    assert_eq!(pnl.raw(), -90);
}

/// Zero removed contracts (an Allow, or a shrink that removed nothing)
/// scores exactly zero.
#[test]
fn counterfactual_zero_removed_is_zero() {
    let c = candidate("KXA", 10);
    let pnl = counterfactual_pnl(
        &c,
        Contracts::new(0),
        Side::Yes,
        Cents::new(100),
        &FlatFee,
        FillAssumption::FilledAtLimit,
    )
    .unwrap();
    assert_eq!(pnl.raw(), 0);
}

/// Scoring MORE contracts than the candidate ever had would fabricate an
/// audit record; the scorer must refuse, never extrapolate.
#[test]
fn counterfactual_removed_beyond_candidate_is_an_error() {
    let c = candidate("KXA", 10);
    let res = counterfactual_pnl(
        &c,
        Contracts::new(11),
        Side::Yes,
        Cents::new(100),
        &FlatFee,
        FillAssumption::FilledAtLimit,
    );
    assert!(matches!(
        res,
        Err(VetoError::RemovedExceedsCandidate { .. })
    ));
}
