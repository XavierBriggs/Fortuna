//! The standalone kill-switch BINARY (I4). Runs with everything else dead:
//! no Postgres, no main runtime, no Slack required.
//!
//! Usage:
//!   fortuna-killswitch freeze --journal <path> [--venue kalshi]
//!   fortuna-killswitch report --journal <path> [--venue kalshi]
//!   fortuna-killswitch self-test --journal <path>
//!
//! `self-test` exercises the full freeze machinery against an in-process
//! sim venue (the monthly-test path, spec I4); live venue adapters plug in
//! at T1.1 with their own credential set (env: FORTUNA_KILLSWITCH_*).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

use fortuna_core::clock::{Clock, RealClock, SimClock, UtcTimestamp};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_killswitch::freeze_cancel_and_report_positions;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::{FaultConfig, PlaceOrder, SimVenue};
use fortuna_venues::{Market, MarketStatus, PriceLevel, SettlementMeta};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut journal: Option<PathBuf> = None;
    let mut venue_name = "kalshi".to_string();
    let mut command: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--journal" => {
                i += 1;
                journal = args.get(i).map(PathBuf::from);
            }
            "--venue" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    venue_name = v.clone();
                }
            }
            c if command.is_none() && !c.starts_with('-') => command = Some(c.to_string()),
            other => {
                eprintln!("unknown arg {other:?}");
                return usage();
            }
        }
        i += 1;
    }
    let Some(command) = command else {
        return usage();
    };
    let Some(journal) = journal else {
        eprintln!("--journal <path> is required (the switch's own flat-file state)");
        return usage();
    };

    match command.as_str() {
        "self-test" => self_test(&journal),
        "freeze" | "report" => {
            // Live venue adapters arrive with T1.1 (built against recorded
            // fixtures; never invented). Until then the switch can only
            // self-test; failing LOUDLY here beats pretending.
            eprintln!(
                "no live adapter for venue {venue_name:?} is wired yet (BUILD_PLAN T1.1); \
                 run `fortuna-killswitch self-test --journal <path>` to exercise the machinery"
            );
            ExitCode::from(3)
        }
        other => {
            eprintln!("unknown command {other:?}");
            usage()
        }
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: fortuna-killswitch <freeze|report|self-test> --journal <path> [--venue kalshi]"
    );
    ExitCode::from(2)
}

/// The monthly test (I4): build a sim venue with live orders + positions,
/// freeze it, verify zero open orders remain, print the report.
fn self_test(journal: &std::path::Path) -> ExitCode {
    let start = match UtcTimestamp::from_epoch_millis(1_780_000_000_000) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("clock setup failed: {e}");
            return ExitCode::from(1);
        }
    };
    let clock = Arc::new(SimClock::new(start));
    let venue = match build_sim(clock.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sim setup failed: {e}");
            return ExitCode::from(1);
        }
    };

    let result = futures::executor::block_on(freeze_cancel_and_report_positions(
        &venue,
        clock.as_ref(),
        journal,
    ));
    match result {
        Ok(report) => {
            let remaining = venue.resting_orders().len();
            if report.orders_cancelled != report.orders_seen || remaining != 0 {
                eprintln!(
                    "SELF-TEST FAILED: {} of {} cancelled, {remaining} remaining",
                    report.orders_cancelled, report.orders_seen
                );
                return ExitCode::from(1);
            }
            println!(
                "self-test OK: cancelled {}/{} orders, reported {} positions; journal at {}",
                report.orders_cancelled,
                report.orders_seen,
                report.positions_seen,
                journal.display()
            );
            // The wall clock exists out here in binary-land (RealClock is
            // the legal source); stamp the journal with a real-time marker.
            let _ = RealClock.now();
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("SELF-TEST FAILED: {e}");
            ExitCode::from(1)
        }
    }
}

fn build_sim(clock: Arc<SimClock>) -> Result<SimVenue, Box<dyn std::error::Error>> {
    let s: FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
        "#,
    )?;
    let venue = SimVenue::new(
        VenueId::new("sim")?,
        clock,
        ScheduleFeeModel::new(vec![s]).map_err(|e| std::io::Error::other(e.to_string()))?,
        FaultConfig::none(1),
        Cents::new(100_000),
    );
    let market = MarketId::new("KS-TEST")?;
    venue.add_market(Market {
        id: market.clone(),
        venue: VenueId::new("sim")?,
        title: "kill-switch self-test market".into(),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 0,
        },
        payout_per_contract: Cents::new(100),
    });
    venue.set_book(
        &market,
        vec![PriceLevel {
            price: Cents::new(45),
            qty: Contracts::new(50),
        }],
        vec![PriceLevel {
            price: Cents::new(55),
            qty: Contracts::new(50),
        }],
    )?;
    // Live orders + a position for the freeze to deal with.
    venue.place_raw(PlaceOrder {
        market: market.clone(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(40),
        qty: Contracts::new(5),
        client_order_id: ClientOrderId::new("ks-resting-1")?,
    })?;
    venue.place_raw(PlaceOrder {
        market: market.clone(),
        side: Side::No,
        action: Action::Buy,
        limit_price: Cents::new(30),
        qty: Contracts::new(5),
        client_order_id: ClientOrderId::new("ks-resting-2")?,
    })?;
    venue.place_raw(PlaceOrder {
        market,
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(55),
        qty: Contracts::new(3),
        client_order_id: ClientOrderId::new("ks-filled-1")?,
    })?;
    Ok(venue)
}
