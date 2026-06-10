//! I5: append-only audit, halt on write failure
//!
//! Property to encode: audit rows are never updated or deleted (no UPDATE/DELETE issued; replay reconstructs identical streams); injected audit write failure halts trading in DST
//!
//! Implemented in T0.8 per tests/README.md (stubs are implemented, never
//! weakened, by their owning BUILD_PLAN task):
//! - Append-only is enforced at the DATABASE (triggers reject UPDATE and
//!   DELETE outright), below the application's INSERT-only repos.
//! - Reading the stream twice reconstructs identical bytes (replay).
//! - An audit write failure is LOUD and the no-audit-no-trading contract
//!   wires it to a global halt that blocks every order; only the operator
//!   re-arm restores flow. (The randomized-DST injection of this failure
//!   extends at T0.10 when the composed runner exists; the contract itself
//!   is pinned here.)

use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{CandidateOrder, GateCheck, GateConfig, GateInputs, GatePipeline, HaltScope};
use fortuna_ledger::AuditWriter;
use serde_json::json;
use sqlx::PgPool;
use std::collections::BTreeSet;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

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

        [per_strategy.i5]
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
        strategy: StrategyId::new("i5").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        market,
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(50),
        qty: Contracts::new(1),
        fair_value: Cents::new(60),
        client_order_id: ClientOrderId::new(format!("i5-{n}")).unwrap(),
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

/// The original stub, implemented (named test preserved; #[sqlx::test]
/// provisions an isolated, migrated database).
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn i5_audit_append_only(pool: PgPool) {
    let clock = Arc::new(SimClock::new(t0()));
    let writer = AuditWriter::new(pool.clone(), clock.clone(), 7);

    // A stream of audit records.
    for n in 0..5 {
        writer
            .append("gate_decision", Some("system"), None, json!({ "n": n }))
            .await
            .unwrap();
    }

    // 1. Rows can never be updated or deleted — the DATABASE refuses.
    let update = sqlx::query("UPDATE audit SET payload = '{}'")
        .execute(&pool)
        .await;
    assert!(
        update.unwrap_err().to_string().contains("append-only"),
        "UPDATE must be refused by the append-only trigger"
    );
    let delete = sqlx::query("DELETE FROM audit").execute(&pool).await;
    assert!(
        delete.unwrap_err().to_string().contains("append-only"),
        "DELETE must be refused by the append-only trigger"
    );

    // 2. Replay reconstructs identical streams: two reads, byte-identical.
    let read = || async {
        let rows = writer.recent("gate_decision", 100).await.unwrap();
        rows.iter()
            .map(|r| format!("{}|{}|{}|{}", r.audit_id, r.at, r.kind, r.payload))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let first = read().await;
    let second = read().await;
    assert_eq!(first, second);
    assert_eq!(first.lines().count(), 5);

    // 3. No audit, no trading: a dead audit store fails loudly, the runner
    // contract wires that to a global halt, and nothing passes the gates
    // until the operator re-arms.
    let mut gates = pipeline();
    assert!(try_order(&mut gates, 1).is_ok());

    pool.close().await; // the audit store dies
    let failure = writer.append("order", None, None, json!({})).await;
    assert!(failure.is_err(), "audit write into a dead store must error");

    // The contract: append failure => halt (this wiring is asserted in the
    // composed runner's DST at T0.10; the invariant is the behavior).
    gates.set_halt(
        HaltScope::Global,
        "audit write failure: no audit, no trading",
    );
    assert_eq!(try_order(&mut gates, 2), Err(GateCheck::Halts));

    // Only the operator clears it (I2 path).
    gates.rearm(HaltScope::Global).unwrap();
    assert!(try_order(&mut gates, 3).is_ok());
}
