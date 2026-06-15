//! I4: kill switch REVOKES order-placing capability (open audit C2).
//!
//! Property to encode: spec I4 (spec.md:43) requires the kill path to "flatten
//! or freeze all positions AND revoke order-placing capability". The freeze /
//! flatten paths cancel resting risk (covered by i4_killswitch_independence,
//! which this file does NOT touch); THIS file pins the SECOND half — a durable
//! kill sentinel the standalone switch WRITES and the runtime CONSUMES as a
//! global halt that blocks FUTURE placement until the operator clears it
//! out-of-band and restarts (I2).
//!
//! ADDITIONS-ONLY (PROTECTED CRATE, CLAUDE.md): a brand-new file. It does NOT
//! modify, weaken, rename, or delete any existing test — in particular
//! i4_killswitch_independence.rs (whose structural dep-walk over
//! fortuna-killswitch's NORMAL deps is unaffected: this test reaches the
//! RevocationHaltPoller through fortuna-live as a DEV-dependency).
//!
//! Four assertions:
//!  (a) write_revocation => is_revoked()==true; a RevocationHaltPoller over a
//!      trivial no-halt inner poller reports the standing revocation halt.
//!  (b) that halt reason, applied to a GatePipeline via set_halt(Global, ...),
//!      makes EVERY candidate order evaluate to Err(GateCheck::Halts) (the
//!      pipeline()/try_order fixture shape from i5_audit_append_only.rs).
//!  (c) survives restart: a FRESH RevocationHaltPoller over the SAME path still
//!      reports the halt (the sentinel is durable across process boundaries).
//!  (d) clear_revocation => is_revoked()==false => the wrapper delegates to its
//!      inner (Ok(None)); a freshly-rearmed pipeline accepts an order again.

use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{CandidateOrder, GateCheck, GateConfig, GateInputs, GatePipeline, HaltScope};
use fortuna_killswitch::{clear_revocation, is_revoked, revocation_path, write_revocation};
use fortuna_live::run_loop::{HaltPoller, RevocationHaltPoller};
use std::collections::BTreeSet;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

/// A unique temp dir per test so the sentinel never collides across the suite's
/// parallel binaries (the i4-independence test uses temp_dir + pid for the same
/// reason; a per-test subdir is even stricter).
fn unique_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fortuna-i4-revoke-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- the i5 gate fixture shape (pipeline + try_order), reused verbatim ----

struct ZeroFees;
impl FeeModel for ZeroFees {
    fn fee(
        &self,
        _role: FillRole,
        _price: Cents,
        _qty: Contracts,
        _category: Option<&str>,
        _at: UtcTimestamp,
    ) -> Result<Cents, FeeError> {
        Ok(Cents::ZERO)
    }
}

fn pipeline() -> GatePipeline {
    let cfg: GateConfig = toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 1000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 1000000
        per_event_exposure_cents = 1000000
        require_event_mapping = false

        [per_strategy.i4]
        max_exposure_cents = 1000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 0

        [rate.sim]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000
        "#,
    )
    .unwrap();
    GatePipeline::new(cfg).unwrap()
}

fn try_order(p: &mut GatePipeline, n: u64) -> Result<(), GateCheck> {
    let market = MarketId::new("M").unwrap();
    let book = OrderBook {
        market: market.clone(),
        as_of: t0(),
        yes_bids: vec![PriceLevel {
            price: Cents::new(49),
            qty: Contracts::new(1000),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(51),
            qty: Contracts::new(1000),
        }],
    };
    let fees = ZeroFees;
    let recent = BTreeSet::new();
    let mut g = IdGen::new(n + 1);
    let intent = IntentId::new(g.next(t0()).unwrap());
    let candidate = CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("i4").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        market,
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(50),
        qty: Contracts::new(1),
        fair_value: Cents::new(60),
        client_order_id: ClientOrderId::new(format!("i4-{n}")).unwrap(),
    };
    let inputs = GateInputs {
        now: t0(),
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: Some(&book),
        last_trade_price: None,
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    match p.evaluate(&candidate, &inputs).gated {
        Ok(_) => Ok(()),
        Err(rej) => Err(rej.check),
    }
}

/// A trivial inner poller that NEVER halts (Ok(None)). The wrapper's behavior is
/// therefore entirely driven by the sentinel: Some(reason) only when revoked,
/// and a clean delegation to this Ok(None) once the sentinel is cleared.
struct NoHaltPoller;
impl HaltPoller for NoHaltPoller {
    async fn poll(&mut self) -> Result<Option<String>, String> {
        Ok(None)
    }
}

#[test]
fn killswitch_revocation_halts_then_clears_and_survives_restart() {
    let dir = unique_dir("full");
    let journal = dir.join("killswitch.jsonl");
    let sentinel = revocation_path(&journal);
    // revocation_path is the KILLSWITCH_REVOKED sibling of the journal.
    assert_eq!(sentinel, dir.join("KILLSWITCH_REVOKED"));
    assert!(!is_revoked(&sentinel), "starts un-revoked");

    let clock = Arc::new(SimClock::new(t0()));

    // ---- (a) write_revocation => is_revoked(); wrapper reports the halt ----
    write_revocation(&sentinel, clock.as_ref() as &dyn Clock, "freeze_and_cancel").unwrap();
    assert!(is_revoked(&sentinel), "write_revocation sets the sentinel");

    let mut poller = RevocationHaltPoller {
        revocation_file: sentinel.clone(),
        inner: NoHaltPoller,
    };
    let reason = match futures::executor::block_on(poller.poll()) {
        Ok(Some(r)) => r,
        other => panic!("revoked poller must report a standing halt, got {other:?}"),
    };
    assert!(
        reason.contains("revocation") && reason.contains("I4"),
        "the halt reason names the I4 revocation: {reason:?}"
    );

    // ---- (b) the reason, applied Global, halts EVERY candidate order ----
    let mut gates = pipeline();
    assert!(try_order(&mut gates, 1).is_ok(), "un-halted gate accepts");
    gates.set_halt(HaltScope::Global, reason);
    // Every candidate now evaluates to Halts (try a few distinct ones).
    for n in 2..6 {
        assert_eq!(
            try_order(&mut gates, n),
            Err(GateCheck::Halts),
            "candidate {n} must be refused with GateCheck::Halts while revoked"
        );
    }

    // ---- (c) survives "restart": a FRESH wrapper over the SAME path halts ----
    // (the sentinel is durable on disk; a new process boots straight into the
    // halt because the loop polls before it ticks.)
    let mut fresh = RevocationHaltPoller {
        revocation_file: sentinel.clone(),
        inner: NoHaltPoller,
    };
    match futures::executor::block_on(fresh.poll()) {
        Ok(Some(r)) => assert!(r.contains("I4"), "restart still halts: {r:?}"),
        other => panic!("a fresh wrapper over a set sentinel must halt, got {other:?}"),
    }

    // ---- (d) clear_revocation => not revoked => wrapper delegates (Ok(None)) ----
    clear_revocation(&sentinel).unwrap();
    assert!(
        !is_revoked(&sentinel),
        "clear_revocation removes the sentinel"
    );
    // clear is idempotent — a second clear on the already-clear state is Ok.
    clear_revocation(&sentinel).unwrap();

    let mut rearmed = RevocationHaltPoller {
        revocation_file: sentinel.clone(),
        inner: NoHaltPoller,
    };
    assert_eq!(
        futures::executor::block_on(rearmed.poll()),
        Ok(None),
        "a cleared sentinel makes the wrapper delegate to the (no-halt) inner"
    );

    // The operator's out-of-band re-arm (I2 path, mirroring i5): the SAME gates
    // that were revocation-halted in (b) are re-armed and accept an order again —
    // order-placing capability returns only after clear + re-arm (a human action).
    gates.rearm(HaltScope::Global).unwrap();
    assert!(
        try_order(&mut gates, 9).is_ok(),
        "after clear + re-arm, order-placing capability returns"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// write_revocation is idempotent: a kill that fires twice re-truncates to a
/// fresh line, and the sentinel stays present (its PRESENCE is the halt). Pins
/// the lib contract independent of the wrapper.
#[test]
fn write_revocation_is_idempotent() {
    let dir = unique_dir("idem");
    let journal = dir.join("j.jsonl");
    let sentinel = revocation_path(&journal);
    let clock = Arc::new(SimClock::new(t0()));

    write_revocation(&sentinel, clock.as_ref() as &dyn Clock, "freeze_and_cancel").unwrap();
    write_revocation(&sentinel, clock.as_ref() as &dyn Clock, "flatten_perps").unwrap();
    assert!(is_revoked(&sentinel), "still revoked after a second write");

    // Exactly one JSON line (truncate, not append), and it records the LAST verb.
    let contents = std::fs::read_to_string(&sentinel).unwrap();
    assert_eq!(
        contents.lines().count(),
        1,
        "write_revocation truncates: one line, not appended"
    );
    assert!(
        contents.contains("flatten_perps") && contents.contains("revoked_at"),
        "the sentinel records the revoking verb + timestamp: {contents:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
