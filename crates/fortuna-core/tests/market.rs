//! T0.3 tests: shared market vocabulary types. Written from spec 5.2/5.3
//! before implementation.
//!
//! These are the identifier and quantity types shared by gates, venues, exec,
//! and state. They live in fortuna-core so that fortuna-venues -> fortuna-gates
//! -> fortuna-core forms a clean dependency chain (GatedOrder needs them
//! without depending on the venues crate).

use fortuna_core::ids::IdGen;
use fortuna_core::market::{
    notional, Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId,
};
use fortuna_core::money::{Cents, MoneyError};

// ---- string id newtypes ----

#[test]
fn string_ids_reject_empty_and_whitespace() {
    assert!(VenueId::new("kalshi").is_ok());
    assert!(VenueId::new("").is_err());
    assert!(VenueId::new("   ").is_err());
    assert!(MarketId::new("KXHIGHNY-26JUN09-B85.5").is_ok());
    assert!(MarketId::new("").is_err());
    assert!(StrategyId::new("mech_structural").is_ok());
    assert!(StrategyId::new("\t").is_err());
}

#[test]
fn string_ids_display_and_serde_as_plain_strings() {
    let v = VenueId::new("kalshi").unwrap();
    assert_eq!(v.to_string(), "kalshi");
    assert_eq!(serde_json::to_string(&v).unwrap(), "\"kalshi\"");
    let back: VenueId = serde_json::from_str("\"kalshi\"").unwrap();
    assert_eq!(back, v);
    // Deserializing an empty id fails (validation runs on deserialize too).
    assert!(serde_json::from_str::<VenueId>("\"\"").is_err());
}

// ---- side and action ----

#[test]
fn side_opposite_and_serde() {
    assert_eq!(Side::Yes.opposite(), Side::No);
    assert_eq!(Side::No.opposite(), Side::Yes);
    assert_eq!(serde_json::to_string(&Side::Yes).unwrap(), "\"yes\"");
    assert_eq!(serde_json::to_string(&Action::Buy).unwrap(), "\"buy\"");
}

// ---- contracts ----

#[test]
fn contracts_checked_arithmetic() {
    let a = Contracts::new(10);
    assert_eq!(a.raw(), 10);
    assert_eq!(a.checked_add(Contracts::new(5)), Ok(Contracts::new(15)));
    assert_eq!(a.checked_sub(Contracts::new(15)), Ok(Contracts::new(-5)));
    assert!(Contracts::new(i64::MAX)
        .checked_add(Contracts::new(1))
        .is_err());
}

#[test]
fn notional_is_price_times_quantity_checked() {
    assert_eq!(
        notional(Cents::new(37), Contracts::new(100)),
        Ok(Cents::new(3700))
    );
    assert_eq!(notional(Cents::new(37), Contracts::new(0)), Ok(Cents::ZERO));
    assert!(matches!(
        notional(Cents::new(i64::MAX), Contracts::new(2)),
        Err(MoneyError::Overflow { .. })
    ));
}

// ---- client order ids ----

#[test]
fn client_order_id_derives_deterministically_from_intent_id() {
    use fortuna_core::clock::UtcTimestamp;
    use fortuna_core::ids::IntentId;
    let mut g = IdGen::new(7);
    let intent = IntentId::new(
        g.next(UtcTimestamp::from_epoch_millis(1_000).unwrap())
            .unwrap(),
    );
    let a = ClientOrderId::from_intent(intent);
    let b = ClientOrderId::from_intent(intent);
    // Same intent => same client order id: crash resubmission is idempotent
    // by construction (spec 5.4).
    assert_eq!(a, b);
    assert_eq!(a.to_string(), format!("fortuna-{intent}"));
}
