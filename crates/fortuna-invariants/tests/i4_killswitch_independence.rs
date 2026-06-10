//! I4: kill switch independence
//!
//! Property to encode: the standalone killswitch binary freeze-and-cancels against the sim venue with Postgres stopped and the main runtime process killed
//!
//! Implemented in T0.9 per tests/README.md (stubs are implemented, never
//! weakened, by their owning BUILD_PLAN task). Three layers:
//! 1. STRUCTURAL: fortuna-killswitch's resolved dependency graph contains no
//!    Postgres client, no fortuna-ledger, no cognition — asserted
//!    mechanically from `cargo metadata` so a future dependency addition
//!    fails this invariant at CI time.
//! 2. OPERATIONAL: the BINARY runs its freeze-and-cancel self-test as a
//!    subprocess with DATABASE_URL scrubbed from the environment and no
//!    main runtime alive (this test process never starts one).
//! 3. BEHAVIORAL: the library freeze-and-cancel clears every open order on
//!    a sim venue even под cancel-ambiguity faults, touching no positions.

use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_killswitch::freeze_and_cancel;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::{FaultConfig, PlaceOrder, SimVenue};
use fortuna_venues::{Market, MarketStatus, PriceLevel, SettlementMeta};
use std::process::Command;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

fn sim_with_orders(faults: FaultConfig) -> (Arc<SimClock>, SimVenue) {
    let clock = Arc::new(SimClock::new(t0()));
    let s: FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
        "#,
    )
    .unwrap();
    let venue = SimVenue::new(
        VenueId::new("sim").unwrap(),
        clock.clone(),
        ScheduleFeeModel::new(vec![s]).unwrap(),
        faults,
        Cents::new(100_000),
    );
    let market = MarketId::new("I4-MKT").unwrap();
    venue.add_market(Market {
        id: market.clone(),
        venue: VenueId::new("sim").unwrap(),
        title: "i4".into(),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 0,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    });
    venue
        .set_book(
            &market,
            vec![PriceLevel {
                price: Cents::new(45),
                qty: Contracts::new(50),
            }],
            vec![PriceLevel {
                price: Cents::new(55),
                qty: Contracts::new(50),
            }],
        )
        .unwrap();
    for n in 0..5 {
        venue
            .place_raw(PlaceOrder {
                market: market.clone(),
                side: if n % 2 == 0 { Side::Yes } else { Side::No },
                action: Action::Buy,
                limit_price: Cents::new(30 + n),
                qty: Contracts::new(2),
                client_order_id: ClientOrderId::new(format!("i4-{n}")).unwrap(),
            })
            .unwrap();
    }
    (clock, venue)
}

/// The original stub, implemented.
#[test]
fn i4_killswitch_independence() {
    // ---- 1. STRUCTURAL: no Postgres/ledger/cognition in the dep graph ----
    let metadata = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(metadata.status.success(), "cargo metadata failed");
    let doc: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();

    // Resolve fortuna-killswitch's transitive NORMAL dependencies.
    let resolve = doc["resolve"]["nodes"].as_array().unwrap();
    let find_node = |id_substr: &str| {
        resolve
            .iter()
            .find(|n| n["id"].as_str().unwrap_or("").contains(id_substr))
    };
    let ks = find_node("fortuna-killswitch").expect("killswitch node in resolve graph");
    let mut to_visit: Vec<String> = vec![ks["id"].as_str().unwrap().to_string()];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = to_visit.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(node) = resolve.iter().find(|n| n["id"] == serde_json::json!(id)) {
            for dep in node["deps"].as_array().unwrap_or(&Vec::new()) {
                // Normal deps only: dev/build deps don't ship in the binary.
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

    // ---- 2. OPERATIONAL: the binary self-test with no DB, no runtime ----
    let journal = std::env::temp_dir().join(format!("fortuna-i4-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&journal);
    let run = Command::new(env!("CARGO"))
        .args([
            "run",
            "-q",
            "-p",
            "fortuna-killswitch",
            "--",
            "self-test",
            "--journal",
        ])
        .arg(&journal)
        .env_remove("DATABASE_URL") // Postgres is DOWN as far as it knows
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "kill-switch binary self-test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let journal_contents = std::fs::read_to_string(&journal).unwrap();
    assert!(journal_contents.contains("freeze_and_cancel_started"));
    assert!(journal_contents.contains("\"orders_cancelled\":2"));
    let _ = std::fs::remove_file(&journal);

    // ---- 3. BEHAVIORAL: freeze clears every order, even under faults ----
    let (clock, venue) = sim_with_orders(FaultConfig {
        cancel_timeout_not_cancelled_pm: 300, // ambiguity: retries must win
        ..FaultConfig::none(7)
    });
    let journal2 =
        std::env::temp_dir().join(format!("fortuna-i4-lib-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&journal2);
    // Bounded retries at the lib level are per-order; sweep until clear like
    // the operator would (re-running the switch is always safe).
    let mut report = None;
    for _ in 0..10 {
        let r = futures::executor::block_on(freeze_and_cancel(
            &venue,
            clock.as_ref() as &dyn Clock,
            &journal2,
        ))
        .unwrap();
        let done = venue.resting_orders().is_empty();
        report = Some(r);
        if done {
            break;
        }
    }
    assert!(
        venue.resting_orders().is_empty(),
        "freeze-and-cancel left orders resting after 10 sweeps"
    );
    let report = report.unwrap();
    assert_eq!(report.flatten_orders_placed, 0); // freeze touches no positions
    let _ = std::fs::remove_file(&journal2);
}
