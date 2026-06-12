//! T5.B4 slice-2 tests: the kinetics REST client vs the recorded
//! requests. FIXTURES-GATED at the TRANSPORT level: for each endpoint,
//! the client is driven with the recorded parameters, the mock transport
//! replays the recorded `(status, body)`, and the test asserts the
//! client issued EXACTLY the recorded request — method, path, query, and
//! JSON body all compared against the capture's `.meta.json`. Nothing
//! about the wire is invented.

use fortuna_core::perp::PerpPrice;
use fortuna_venues::kalshi::client::{MockKalshiTransport, RecordedCall};
use fortuna_venues::kinetics::client::{BookSide, CreateOrderRequest, KineticsClient, TimeInForce};
use fortuna_venues::VenueError;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kinetics-perps")
}

struct Recorded {
    method: String,
    /// Venue-relative path (the transport owns /trade-api/v2).
    path: String,
    query: Option<String>,
    body: Option<serde_json::Value>,
    status: u16,
    response: serde_json::Value,
}

fn recorded(name: &str) -> Recorded {
    let meta: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fixtures_dir().join(format!("{name}.meta.json"))).unwrap(),
    )
    .unwrap();
    let full_path = meta["path"].as_str().unwrap();
    let stripped = full_path.strip_prefix("/trade-api/v2").unwrap_or(full_path);
    let (path, query) = match stripped.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (stripped.to_string(), None),
    };
    Recorded {
        method: meta["method"].as_str().unwrap().to_string(),
        path,
        query,
        body: match &meta["request_body"] {
            serde_json::Value::Null => None,
            other => Some(other.clone()),
        },
        status: meta["status"].as_u64().unwrap() as u16,
        response: serde_json::from_str(
            &fs::read_to_string(fixtures_dir().join(format!("{name}.json"))).unwrap(),
        )
        .unwrap(),
    }
}

fn client_with(rec: &Recorded) -> (KineticsClient, Arc<MockKalshiTransport>) {
    let transport = Arc::new(MockKalshiTransport::new());
    transport.push_ok(rec.status, rec.response.clone());
    (KineticsClient::new(transport.clone()), transport)
}

/// The single issued call must equal the recording.
fn assert_call_matches(transport: &MockKalshiTransport, rec: &Recorded, name: &str) {
    let calls = transport.calls();
    assert_eq!(calls.len(), 1, "{name}: expected exactly one call");
    let expected = RecordedCall {
        method: rec.method.clone(),
        path: rec.path.clone(),
        query: rec.query.clone(),
        body: rec.body.clone(),
    };
    assert_eq!(calls[0], expected, "{name}: issued request != recording");
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

// ---- reads ----

#[test]
fn reads_issue_exactly_the_recorded_requests() {
    // (fixture, closure issuing the equivalent client call)
    type Call = Box<dyn Fn(&KineticsClient) -> Result<(), VenueError>>;
    let cases: Vec<(&str, Call)> = vec![
        (
            "exchange__status",
            Box::new(|c| block_on(c.exchange_status()).map(drop)),
        ),
        (
            "auth__margin_enabled_ok",
            Box::new(|c| block_on(c.margin_enabled()).map(drop)),
        ),
        (
            "auth__margin_balance",
            Box::new(|c| block_on(c.balance(false)).map(drop)),
        ),
        (
            "balance__compute_available",
            Box::new(|c| block_on(c.balance(true)).map(drop)),
        ),
        (
            "account__limits_perps",
            Box::new(|c| block_on(c.account_limits_perps()).map(drop)),
        ),
        (
            "markets__list",
            Box::new(|c| block_on(c.markets()).map(drop)),
        ),
        (
            "markets__single",
            Box::new(|c| block_on(c.market("KXBTCPERP1")).map(drop)),
        ),
        (
            "orderbook__depth5",
            Box::new(|c| block_on(c.orderbook("KXBTCPERP1", 5, None)).map(drop)),
        ),
        (
            "orderbook__agg_010",
            Box::new(|c| block_on(c.orderbook("KXBTCPERP1", 5, Some("0.10"))).map(drop)),
        ),
        (
            "orders__filter_resting",
            Box::new(|c| block_on(c.list_orders(Some("resting"), Some(10))).map(drop)),
        ),
        (
            "orders__list_all",
            Box::new(|c| block_on(c.list_orders(None, Some(10))).map(drop)),
        ),
        (
            "orders__get_after_create",
            Box::new(|c| block_on(c.get_order("c445aeac-f95b-4c96-8086-faacebfd300d")).map(drop)),
        ),
        (
            "fills__after_open",
            Box::new(|c| block_on(c.fills(Some("KXBTCPERP1"), Some(10))).map(drop)),
        ),
        (
            "positions__open",
            Box::new(|c| block_on(c.positions()).map(drop)),
        ),
        (
            "risk__account",
            Box::new(|c| block_on(c.risk_account()).map(drop)),
        ),
        (
            "risk__parameters",
            Box::new(|c| block_on(c.risk_parameters()).map(drop)),
        ),
        (
            "risk__notional_limit",
            Box::new(|c| block_on(c.notional_risk_limit()).map(drop)),
        ),
        (
            "fees__tiers",
            Box::new(|c| block_on(c.fee_tiers()).map(drop)),
        ),
        (
            "funding__rates_estimate",
            Box::new(|c| block_on(c.funding_estimate("KXBTCPERP1")).map(drop)),
        ),
        (
            "funding__rates_historical",
            Box::new(|c| {
                block_on(c.funding_rates_historical(Some("KXBTCPERP1"), Some(5))).map(drop)
            }),
        ),
        (
            "funding__history_baseline",
            Box::new(|c| block_on(c.funding_history("2026-06-05", "2026-06-13")).map(drop)),
        ),
        (
            "groups__get",
            Box::new(|c| block_on(c.get_group("400e176b-0022-40bb-8248-f188e6d4f409")).map(drop)),
        ),
        (
            "groups__list",
            Box::new(|c| block_on(c.list_groups()).map(drop)),
        ),
    ];
    for (name, call) in cases {
        let rec = recorded(name);
        let (client, transport) = client_with(&rec);
        call(&client).unwrap_or_else(|e| panic!("{name}: client call failed: {e}"));
        assert_call_matches(&transport, &rec, name);
    }
}

// ---- order writes (request bodies must equal the recordings) ----

#[test]
fn create_order_gtc_matches_recording() {
    let rec = recorded("orders__create_gtc");
    let (client, transport) = client_with(&rec);
    let resp = block_on(client.create_order(&CreateOrderRequest {
        ticker: "KXBTCPERP1".into(),
        side: BookSide::Bid,
        price: PerpPrice::new(53_829),
        count: 1,
        client_order_id: "99845c0f-725c-4a4a-8955-a95a30e58072".into(),
        time_in_force: TimeInForce::GoodTillCanceled,
        post_only: Some(false),
        reduce_only: None,
        order_group_id: None,
    }))
    .unwrap();
    assert_eq!(resp.order_id, "c445aeac-f95b-4c96-8086-faacebfd300d");
    assert_call_matches(&transport, &rec, "orders__create_gtc");
}

#[test]
fn create_order_ioc_omits_post_only_like_the_recording() {
    let rec = recorded("orders__funding_position_ioc");
    let (client, transport) = client_with(&rec);
    block_on(client.create_order(&CreateOrderRequest {
        ticker: "KXBTCPERP1".into(),
        side: BookSide::Bid,
        price: PerpPrice::new(63_616),
        count: 1,
        client_order_id: "76f7796b-ab18-4de1-adba-58c86400b2dd".into(),
        time_in_force: TimeInForce::ImmediateOrCancel,
        post_only: None,
        reduce_only: None,
        order_group_id: None,
    }))
    .unwrap();
    assert_call_matches(&transport, &rec, "orders__funding_position_ioc");
}

#[test]
fn create_order_in_group_carries_group_id() {
    let rec = recorded("orders__create_in_group");
    let (client, transport) = client_with(&rec);
    block_on(client.create_order(&CreateOrderRequest {
        ticker: "KXBTCPERP1".into(),
        side: BookSide::Bid,
        price: PerpPrice::new(53_829),
        count: 1,
        client_order_id: "a4e0fe1c-84ae-424b-a662-38f2802d6871".into(),
        time_in_force: TimeInForce::GoodTillCanceled,
        post_only: Some(false),
        reduce_only: None,
        order_group_id: Some("400e176b-0022-40bb-8248-f188e6d4f409".into()),
    }))
    .unwrap();
    assert_call_matches(&transport, &rec, "orders__create_in_group");
}

#[test]
fn reduce_only_rejection_maps_with_raw_code_first() {
    let rec = recorded("orders__reduce_only_gtc");
    let (client, transport) = client_with(&rec);
    let err = block_on(client.create_order(&CreateOrderRequest {
        ticker: "KXBTCPERP1".into(),
        side: BookSide::Ask,
        price: PerpPrice::new(63_816),
        count: 1,
        client_order_id: "61f14161-f3df-4a20-88f5-c41645f0b480".into(),
        time_in_force: TimeInForce::GoodTillCanceled,
        post_only: None,
        reduce_only: Some(true),
        order_group_id: None,
    }))
    .unwrap_err();
    let VenueError::Rejected { reason } = &err else {
        panic!("expected Rejected, got {err:?}");
    };
    assert!(
        reason.starts_with("invalid_order"),
        "raw code first: {reason}"
    );
    assert_call_matches(&transport, &rec, "orders__reduce_only_gtc");
}

#[test]
fn duplicate_client_order_id_maps_to_rejected_with_code() {
    let rec = recorded("orders__duplicate_client_order_id");
    let (client, _) = client_with(&rec);
    let err = block_on(client.create_order(&CreateOrderRequest {
        ticker: "KXBTCPERP1".into(),
        side: BookSide::Bid,
        price: PerpPrice::new(53_829),
        count: 1,
        client_order_id: "99845c0f-725c-4a4a-8955-a95a30e58072".into(),
        time_in_force: TimeInForce::GoodTillCanceled,
        post_only: Some(false),
        reduce_only: None,
        order_group_id: None,
    }))
    .unwrap_err();
    let VenueError::Rejected { reason } = &err else {
        panic!("expected Rejected, got {err:?}");
    };
    assert!(reason.starts_with("order_already_exists"), "{reason}");
}

#[test]
fn amend_decrease_cancel_match_recordings() {
    let rec = recorded("orders__amend_price");
    let (client, transport) = client_with(&rec);
    block_on(client.amend_order(
        "5208305e-eecb-4c59-ac5c-115a047de9d7",
        "KXBTCPERP1",
        BookSide::Bid,
        PerpPrice::new(53_779),
        1,
    ))
    .unwrap();
    assert_call_matches(&transport, &rec, "orders__amend_price");

    let rec = recorded("orders__decrease_reduce_by");
    let (client, transport) = client_with(&rec);
    let resp = block_on(client.decrease_order("e4d339ff-8be1-45d6-b19b-eef5c8cb5a74", 1)).unwrap();
    assert_eq!(resp.remaining_count, "1.00");
    assert_call_matches(&transport, &rec, "orders__decrease_reduce_by");

    let rec = recorded("orders__cancel");
    let (client, transport) = client_with(&rec);
    let resp = block_on(client.cancel_order("c445aeac-f95b-4c96-8086-faacebfd300d")).unwrap();
    assert_eq!(resp.reduced_by, "1.00");
    assert_call_matches(&transport, &rec, "orders__cancel");
}

#[test]
fn cancel_of_unknown_order_maps_to_not_found() {
    let rec = recorded("cleanup__leftover_0");
    let (client, _) = client_with(&rec);
    let err = block_on(client.cancel_order("82c52513-39ed-4008-9aa0-73546b956f7a")).unwrap_err();
    assert!(matches!(err, VenueError::NotFound { .. }), "{err:?}");
}

// ---- groups / subaccounts / transfers ----

#[test]
fn group_lifecycle_matches_recordings() {
    let gid = "400e176b-0022-40bb-8248-f188e6d4f409";
    let rec = recorded("groups__create");
    let (client, transport) = client_with(&rec);
    let resp = block_on(client.create_group(10)).unwrap();
    assert_eq!(resp.order_group_id, gid);
    assert_call_matches(&transport, &rec, "groups__create");

    let rec = recorded("groups__update_limit");
    let (client, transport) = client_with(&rec);
    block_on(client.update_group_limit(gid, 5)).unwrap();
    assert_call_matches(&transport, &rec, "groups__update_limit");

    for (name, op) in [
        ("groups__trigger", "trigger"),
        ("groups__reset", "reset"),
        ("groups__delete", "delete"),
    ] {
        let rec = recorded(name);
        let (client, transport) = client_with(&rec);
        match op {
            "trigger" => block_on(client.trigger_group(gid)).map(drop).unwrap(),
            "reset" => block_on(client.reset_group(gid)).map(drop).unwrap(),
            _ => block_on(client.delete_group(gid)).map(drop).unwrap(),
        }
        assert_call_matches(&transport, &rec, name);
    }
}

#[test]
fn subaccount_create_always_sends_empty_json_body() {
    // Finding 7: a body-less POST is rejected with invalid_content_type,
    // so the client ALWAYS sends {} — this is the one deliberate
    // divergence from the recorded request_body (recorded as null by the
    // sanitizer; the accepted wire request carried {}).
    let rec = recorded("subaccounts__create");
    let (client, transport) = client_with(&rec);
    let resp = block_on(client.create_subaccount()).unwrap();
    assert_eq!(resp.subaccount_number, 6);
    let calls = transport.calls();
    assert_eq!(calls[0].method, "POST");
    assert_eq!(calls[0].path, "/portfolio/margin/subaccounts");
    assert_eq!(calls[0].body, Some(serde_json::json!({})));
}

#[test]
fn transfers_match_recordings_and_idempotency_dup_maps_rejected() {
    let rec = recorded("subaccounts__transfer_first");
    let (client, transport) = client_with(&rec);
    block_on(client.subaccount_transfer("c25a36af-2eb3-4cc1-a971-f98055bd7c6b", 0, 6, 1)).unwrap();
    assert_call_matches(&transport, &rec, "subaccounts__transfer_first");

    let rec = recorded("subaccounts__transfer_duplicate");
    let (client, _) = client_with(&rec);
    let err = block_on(client.subaccount_transfer("c25a36af-2eb3-4cc1-a971-f98055bd7c6b", 0, 6, 1))
        .unwrap_err();
    let VenueError::Rejected { reason } = &err else {
        panic!("expected Rejected, got {err:?}");
    };
    assert!(reason.starts_with("transfer_already_applied"), "{reason}");

    let rec = recorded("transfer__intra_exchange");
    let (client, transport) = client_with(&rec);
    let resp = block_on(client.intra_exchange_transfer("event_contract", "margined", 100)).unwrap();
    assert_eq!(resp.transfer_id, "ac64adcf-16bb-4ff7-8d04-19c2811647d1");
    assert_call_matches(&transport, &rec, "transfer__intra_exchange");
}

// ---- auth-shaped failures pass through the error mapping ----

#[test]
fn bad_signature_401_maps_to_rejected_with_auth_code() {
    let rec = recorded("auth__bad_signature");
    let (client, _) = client_with(&rec);
    let err = block_on(client.balance(false)).unwrap_err();
    let VenueError::Rejected { reason } = &err else {
        panic!("expected Rejected, got {err:?}");
    };
    assert!(reason.starts_with("authentication_error"), "{reason}");
}

#[test]
fn bare_msg_400_maps_to_rejected() {
    let rec = recorded("funding__history_no_params");
    let (client, _) = client_with(&rec);
    // Drive the client through a path that COULD produce this venue
    // response (the recorded probe omitted the params deliberately; the
    // typed client cannot, so replay it against the same endpoint).
    let err = block_on(client.funding_history("2026-06-05", "2026-06-13")).unwrap_err();
    assert!(matches!(err, VenueError::Rejected { .. }), "{err:?}");
}
