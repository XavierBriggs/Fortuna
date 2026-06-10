//! T3.4: the Polymarket US adapter slot (fixtures-gated).
//!
//! No Polymarket fixtures exist (fixtures/ has kalshi only) and venue
//! API behavior is NEVER invented (CLAUDE.md). The adapter is therefore
//! a STUB behind the Venue trait: constructible (the composition can
//! name the slot), but every operation fails LOUDLY with a fixture-gate
//! error — it never fabricates market data, never accepts an order,
//! never panics. GAPS.md records the exact unblock (operator-recorded
//! fixtures + a venue research loop).

use fortuna_core::market::{MarketId, VenueId, VenueOrderId};
use fortuna_venues::polymarket::PolymarketUsStub;
use fortuna_venues::{Cursor, MarketFilter, Venue, VenueError};

#[test]
fn stub_is_constructible_and_names_its_venue() {
    let stub = PolymarketUsStub::new().unwrap();
    assert_eq!(stub.id(), VenueId::new("polymarket_us").unwrap());
}

#[test]
fn every_operation_fails_loudly_with_the_fixture_gate() {
    let stub = PolymarketUsStub::new().unwrap();
    let gated = |result: Result<(), VenueError>| {
        let err = result.unwrap_err();
        assert!(
            matches!(&err, VenueError::FixtureGated { venue } if venue == "polymarket_us"),
            "expected the fixture gate, got: {err}"
        );
        let msg = err.to_string();
        assert!(msg.contains("fixture"), "the error explains itself: {msg}");
    };

    futures::executor::block_on(async {
        gated(stub.markets(MarketFilter::default()).await.map(|_| ()));
        gated(stub.book(&MarketId::new("PM-X").unwrap()).await.map(|_| ()));
        gated(
            stub.cancel(&VenueOrderId::new("pm-1").unwrap())
                .await
                .map(|_| ()),
        );
        gated(stub.positions().await.map(|_| ()));
        gated(stub.open_orders().await.map(|_| ()));
        gated(stub.balance().await.map(|_| ()));
        gated(stub.fills_since(Cursor::start()).await.map(|_| ()));
        gated(stub.settlements_since(Cursor::start()).await.map(|_| ()));
        // `place` is exercised through the trait in the composition; the
        // stub refuses everything else, and a GatedOrder cannot even be
        // constructed outside the gate pipeline, so the order path is
        // doubly closed here.
    });

    // The fee model is the one thing a stub must NOT fake: fees feed
    // sizing and gates (a fabricated zero fee would inflate every edge).
    // The stub's model REFUSES every computation.
    let fee = stub.fee_model();
    let err = fee
        .fee(
            fortuna_core::book::FillRole::Maker,
            fortuna_core::money::Cents::new(50),
            fortuna_core::market::Contracts::new(1),
            None,
            fortuna_core::clock::UtcTimestamp::parse_iso8601("2026-06-11T00:00:00.000Z").unwrap(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("polymarket"), "{err}");
}
