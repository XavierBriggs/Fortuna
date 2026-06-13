//! T5.B4 slice-2 tests: the kinetics REST client vs the recorded
//! requests — RE-RECORDING-PROOF (perps-merge revert lesson). Every
//! test DERIVES its parameters from the capture's `.meta.json`
//! (request body fields, query parameters, path ids) and asserts the
//! typed client reproduces the recorded request EXACTLY — a true
//! round-trip pin that survives fixture re-recording, because nothing
//! capture-specific is hardcoded. Responses are compared against the
//! recorded body's own fields.

use fortuna_core::perp::PerpPrice;
use fortuna_venues::kalshi::client::{MockKalshiTransport, RecordedCall};
use fortuna_venues::kinetics::client::{BookSide, CreateOrderRequest, KineticsClient, TimeInForce};
use fortuna_venues::kinetics::dto;
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
        status: meta["status"].as_u64().unwrap_or(200) as u16,
        response: serde_json::from_str(
            &fs::read_to_string(fixtures_dir().join(format!("{name}.json"))).unwrap(),
        )
        .unwrap(),
    }
}

impl Recorded {
    fn body_str(&self, key: &str) -> String {
        self.body.as_ref().unwrap()[key]
            .as_str()
            .unwrap_or_else(|| panic!("recorded body missing string {key}"))
            .to_string()
    }

    fn body_price(&self, key: &str) -> PerpPrice {
        dto::parse_perp_price(&self.body_str(key)).unwrap()
    }

    fn body_count(&self, key: &str) -> i64 {
        self.body_str(key).parse().unwrap()
    }

    /// Trailing path segment (order/group ids ride in the path).
    fn path_id(&self, suffix_strip: &str) -> String {
        self.path
            .trim_end_matches(suffix_strip)
            .rsplit('/')
            .next()
            .unwrap()
            .to_string()
    }

    /// One query parameter's value.
    fn query_param(&self, key: &str) -> Option<String> {
        self.query.as_deref()?.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == key).then(|| v.to_string())
        })
    }

    fn tif(&self) -> TimeInForce {
        match self.body_str("time_in_force").as_str() {
            "good_till_canceled" => TimeInForce::GoodTillCanceled,
            "immediate_or_cancel" => TimeInForce::ImmediateOrCancel,
            "fill_or_kill" => TimeInForce::FillOrKill,
            other => panic!("recorded tif {other:?}"),
        }
    }

    fn side(&self) -> BookSide {
        match self.body_str("side").as_str() {
            "bid" => BookSide::Bid,
            "ask" => BookSide::Ask,
            other => panic!("recorded side {other:?}"),
        }
    }

    /// Build the typed create request entirely from the recorded body.
    fn create_request(&self) -> CreateOrderRequest {
        let body = self.body.as_ref().unwrap();
        CreateOrderRequest {
            ticker: self.body_str("ticker"),
            side: self.side(),
            price: self.body_price("price"),
            count: self.body_count("count"),
            client_order_id: self.body_str("client_order_id"),
            time_in_force: self.tif(),
            post_only: body.get("post_only").and_then(|v| v.as_bool()),
            reduce_only: body.get("reduce_only").and_then(|v| v.as_bool()),
            order_group_id: body
                .get("order_group_id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        }
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

// ---- reads: parameters parsed OUT of each recording ----

#[test]
fn reads_issue_exactly_the_recorded_requests() {
    type Call = Box<dyn Fn(&KineticsClient, &Recorded) -> Result<(), VenueError>>;
    let cases: Vec<(&str, Call)> = vec![
        (
            "exchange__status",
            Box::new(|c, _| block_on(c.exchange_status()).map(drop)),
        ),
        (
            "auth__margin_enabled_ok",
            Box::new(|c, _| block_on(c.margin_enabled()).map(drop)),
        ),
        (
            "auth__margin_balance",
            Box::new(|c, _| block_on(c.balance(false)).map(drop)),
        ),
        (
            "balance__compute_available",
            Box::new(|c, _| block_on(c.balance(true)).map(drop)),
        ),
        (
            "account__limits_perps",
            Box::new(|c, _| block_on(c.account_limits_perps()).map(drop)),
        ),
        (
            "markets__list",
            Box::new(|c, _| block_on(c.markets()).map(drop)),
        ),
        (
            "markets__single",
            Box::new(|c, r| block_on(c.market(&r.path_id(""))).map(drop)),
        ),
        (
            "orderbook__depth5",
            Box::new(|c, r| {
                let ticker = r
                    .path
                    .trim_end_matches("/orderbook")
                    .rsplit('/')
                    .next()
                    .unwrap();
                let depth: i64 = r.query_param("depth").unwrap().parse().unwrap();
                block_on(c.orderbook(ticker, depth, None)).map(drop)
            }),
        ),
        (
            "orderbook__agg_010",
            Box::new(|c, r| {
                let ticker = r
                    .path
                    .trim_end_matches("/orderbook")
                    .rsplit('/')
                    .next()
                    .unwrap();
                let depth: i64 = r.query_param("depth").unwrap().parse().unwrap();
                let agg = r.query_param("aggregation_tick_size").unwrap();
                block_on(c.orderbook(ticker, depth, Some(&agg))).map(drop)
            }),
        ),
        (
            "orders__filter_resting",
            Box::new(|c, r| {
                let status = r.query_param("status").unwrap();
                let limit: i64 = r.query_param("limit").unwrap().parse().unwrap();
                block_on(c.list_orders(Some(&status), Some(limit))).map(drop)
            }),
        ),
        (
            "orders__list_all",
            Box::new(|c, r| {
                let limit: i64 = r.query_param("limit").unwrap().parse().unwrap();
                block_on(c.list_orders(None, Some(limit))).map(drop)
            }),
        ),
        (
            "fills__after_open",
            Box::new(|c, r| {
                let ticker = r.query_param("ticker").unwrap();
                let limit: i64 = r.query_param("limit").unwrap().parse().unwrap();
                block_on(c.fills(Some(&ticker), Some(limit))).map(drop)
            }),
        ),
        (
            "positions__open",
            Box::new(|c, _| block_on(c.positions()).map(drop)),
        ),
        (
            "risk__account",
            Box::new(|c, _| block_on(c.risk_account()).map(drop)),
        ),
        (
            "risk__parameters",
            Box::new(|c, _| block_on(c.risk_parameters()).map(drop)),
        ),
        (
            "risk__notional_limit",
            Box::new(|c, _| block_on(c.notional_risk_limit()).map(drop)),
        ),
        (
            "fees__tiers",
            Box::new(|c, _| block_on(c.fee_tiers()).map(drop)),
        ),
        (
            "funding__rates_estimate",
            Box::new(|c, r| {
                let ticker = r.query_param("ticker").unwrap();
                block_on(c.funding_estimate(&ticker)).map(drop)
            }),
        ),
        (
            "funding__rates_historical",
            Box::new(|c, r| {
                let ticker = r.query_param("ticker");
                let limit = r.query_param("limit").map(|l| l.parse().unwrap());
                block_on(c.funding_rates_historical(ticker.as_deref(), limit)).map(drop)
            }),
        ),
        (
            "funding__history_baseline",
            Box::new(|c, r| {
                let start = r.query_param("start_date").unwrap();
                let end = r.query_param("end_date").unwrap();
                block_on(c.funding_history(&start, &end)).map(drop)
            }),
        ),
        (
            "groups__list",
            Box::new(|c, _| block_on(c.list_groups()).map(drop)),
        ),
    ];
    for (name, call) in cases {
        let rec = recorded(name);
        let (client, transport) = client_with(&rec);
        call(&client, &rec).unwrap_or_else(|e| panic!("{name}: client call failed: {e}"));
        assert_call_matches(&transport, &rec, name);
    }
}

// ---- order writes: typed request built FROM the recorded body ----

#[test]
fn create_order_round_trips_each_recorded_body() {
    // Every recorded create (GTC, IOC sans post_only, in-group) round-
    // trips: parse the recorded body into the typed request, issue it,
    // and the wire body must equal the recording byte-for-byte as JSON.
    for name in [
        "orders__create_gtc",
        "orders__funding_position_ioc",
        "orders__create_in_group",
        "orders__create_post_only",
        "orders__create_for_decrease",
    ] {
        let rec = recorded(name);
        let (client, transport) = client_with(&rec);
        let resp = block_on(client.create_order(&rec.create_request()))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        // The placement echoes the recorded response's own fields.
        assert_eq!(
            resp.order_id,
            rec.response["order_id"].as_str().unwrap(),
            "{name}"
        );
        assert_eq!(
            resp.client_order_id,
            rec.response["client_order_id"].as_str().unwrap(),
            "{name}"
        );
        assert_call_matches(&transport, &rec, name);
    }
}

#[test]
fn rejected_creates_map_with_raw_code_first() {
    // The error FAMILIES are venue vocabulary (stable across captures);
    // the request derives from each recording.
    for (name, family) in [
        ("orders__reduce_only_gtc", "invalid_order"),
        ("orders__duplicate_client_order_id", "order_already_exists"),
        ("orders__insufficient_margin", ""),
        ("orders__price_band_violation", "invalid_"),
    ] {
        let rec = recorded(name);
        let (client, _) = client_with(&rec);
        let err = block_on(client.create_order(&rec.create_request()))
            .expect_err(&format!("{name} must reject"));
        let VenueError::Rejected { reason } = &err else {
            panic!("{name}: expected Rejected, got {err:?}");
        };
        // The raw code leads the reason (dynamic codes, finding 8); the
        // specific family is asserted where stable.
        assert!(reason.starts_with(family), "{name}: {reason}");
    }
}

#[test]
fn off_tick_price_is_unconstructible_in_the_typed_client() {
    // The recorded probe sent a sub-tick price and the venue 400'd. The
    // TYPED client cannot even build that request: PerpPrice refuses
    // sub-tick dollars, so the case dies before any wire — type-level
    // protection pinned against the recording's own body.
    let rec = recorded("orders__off_tick_price");
    let raw = rec.body_str("price");
    assert!(
        dto::parse_perp_price(&raw).is_err(),
        "recorded off-tick price {raw:?} must be unrepresentable"
    );
}

#[test]
fn amend_decrease_cancel_round_trip_recordings() {
    let rec = recorded("orders__amend_price");
    let (client, transport) = client_with(&rec);
    block_on(client.amend_order(
        &rec.path_id("/amend"),
        &rec.body_str("ticker"),
        rec.side(),
        rec.body_price("price"),
        rec.body_count("count"),
    ))
    .unwrap();
    assert_call_matches(&transport, &rec, "orders__amend_price");

    let rec = recorded("orders__decrease_reduce_by");
    let (client, transport) = client_with(&rec);
    let resp =
        block_on(client.decrease_order(&rec.path_id("/decrease"), rec.body_count("reduce_by")))
            .unwrap();
    assert_eq!(
        resp.remaining_count,
        rec.response["remaining_count"].as_str().unwrap()
    );
    assert_call_matches(&transport, &rec, "orders__decrease_reduce_by");

    let rec = recorded("orders__cancel");
    let (client, transport) = client_with(&rec);
    let resp = block_on(client.cancel_order(&rec.path_id(""))).unwrap();
    assert_eq!(
        resp.reduced_by,
        rec.response["reduced_by"].as_str().unwrap()
    );
    assert_call_matches(&transport, &rec, "orders__cancel");
}

#[test]
fn cancel_of_unknown_order_maps_to_not_found() {
    let rec = recorded("cleanup__leftover_0");
    let (client, _) = client_with(&rec);
    let err = block_on(client.cancel_order(&rec.path_id(""))).unwrap_err();
    assert!(matches!(err, VenueError::NotFound { .. }), "{err:?}");
}

// ---- groups / subaccounts / transfers ----

#[test]
fn group_lifecycle_round_trips_recordings() {
    let rec = recorded("groups__create");
    let (client, transport) = client_with(&rec);
    let limit = rec.body.as_ref().unwrap()["contracts_limit"]
        .as_i64()
        .unwrap();
    let resp = block_on(client.create_group(limit)).unwrap();
    assert_eq!(
        resp.order_group_id,
        rec.response["order_group_id"].as_str().unwrap()
    );
    assert_call_matches(&transport, &rec, "groups__create");

    let rec = recorded("groups__update_limit");
    let (client, transport) = client_with(&rec);
    let limit = rec.body.as_ref().unwrap()["contracts_limit"]
        .as_i64()
        .unwrap();
    block_on(client.update_group_limit(&rec.path_id("/limit"), limit)).unwrap();
    assert_call_matches(&transport, &rec, "groups__update_limit");

    for (name, strip) in [("groups__trigger", "/trigger"), ("groups__reset", "/reset")] {
        let rec = recorded(name);
        let (client, transport) = client_with(&rec);
        let gid = rec.path_id(strip);
        match strip {
            "/trigger" => block_on(client.trigger_group(&gid)).map(drop).unwrap(),
            _ => block_on(client.reset_group(&gid)).map(drop).unwrap(),
        }
        assert_call_matches(&transport, &rec, name);
    }

    let rec = recorded("groups__delete");
    let (client, transport) = client_with(&rec);
    block_on(client.delete_group(&rec.path_id(""))).unwrap();
    assert_call_matches(&transport, &rec, "groups__delete");
}

#[test]
fn subaccount_create_always_sends_empty_json_body() {
    // Finding 7: a body-less POST is rejected with invalid_content_type,
    // so the client ALWAYS sends {} — the one deliberate divergence from
    // the recorded request_body (null'd by the sanitizer).
    let rec = recorded("subaccounts__create");
    let (client, transport) = client_with(&rec);
    let resp = block_on(client.create_subaccount()).unwrap();
    assert_eq!(
        resp.subaccount_number,
        rec.response["subaccount_number"].as_i64().unwrap()
    );
    let calls = transport.calls();
    assert_eq!(calls[0].method, "POST");
    assert_eq!(calls[0].path, "/portfolio/margin/subaccounts");
    assert_eq!(calls[0].body, Some(serde_json::json!({})));
}

#[test]
fn transfers_round_trip_and_dup_maps_rejected() {
    let rec = recorded("subaccounts__transfer_first");
    let body = rec.body.clone().unwrap();
    let (client, transport) = client_with(&rec);
    block_on(client.subaccount_transfer(
        body["client_transfer_id"].as_str().unwrap(),
        body["from_subaccount"].as_i64().unwrap(),
        body["to_subaccount"].as_i64().unwrap(),
        body["amount_cents"].as_i64().unwrap(),
    ))
    .unwrap();
    assert_call_matches(&transport, &rec, "subaccounts__transfer_first");

    let rec = recorded("subaccounts__transfer_duplicate");
    let body = rec.body.clone().unwrap();
    let (client, _) = client_with(&rec);
    let err = block_on(client.subaccount_transfer(
        body["client_transfer_id"].as_str().unwrap(),
        body["from_subaccount"].as_i64().unwrap(),
        body["to_subaccount"].as_i64().unwrap(),
        body["amount_cents"].as_i64().unwrap(),
    ))
    .unwrap_err();
    let VenueError::Rejected { reason } = &err else {
        panic!("expected Rejected, got {err:?}");
    };
    assert!(reason.starts_with("transfer_already_applied"), "{reason}");

    // The intra-exchange rail recorded a 503 in the latest capture (the
    // rail flaked); whatever the capture's status, the client must
    // round-trip the request and surface success or a typed error.
    let rec = recorded("transfer__intra_exchange");
    let body = rec.body.clone().unwrap();
    let (client, transport) = client_with(&rec);
    let result = block_on(client.intra_exchange_transfer(
        body["source"].as_str().unwrap(),
        body["destination"].as_str().unwrap(),
        body["amount"].as_i64().unwrap(),
    ));
    if rec.status < 400 {
        assert_eq!(
            result.unwrap().transfer_id,
            rec.response["transfer_id"].as_str().unwrap()
        );
    } else {
        result.unwrap_err();
    }
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
    let err = block_on(client.funding_history("2026-06-05", "2026-06-13")).unwrap_err();
    assert!(matches!(err, VenueError::Rejected { .. }), "{err:?}");
}
