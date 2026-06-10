//! The standalone kill switch (I4). Spec 5.4 exemption, Section 8, I4.
//!
//! INDEPENDENCE IS STRUCTURAL: this crate depends only on fortuna-core and
//! fortuna-venues. No Postgres, no fortuna-ledger, no cognition runtime, no
//! event loop, no Slack. It must function when everything else is dead —
//! including the database (spec Principle 9 exception: its own state is a
//! flat journal file).
//!
//! Default action: FREEZE-AND-CANCEL (cancel every open order; touch no
//! positions). Emergency flatten is best-effort taker exits WITHOUT the
//! flatten planner (spec 5.4: "the standalone kill-switch process cannot
//! depend on the planner; emergency flatten through it is best-effort
//! without cost estimation, an accepted emergency cost").
//!
//! Every action appends one JSON line to a local journal file (flat-file
//! state, fsync'd) so the operator can reconstruct what the switch did even
//! with the audit store down. Tested monthly per I4 (script in T0.9).

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

use fortuna_core::clock::Clock;
use fortuna_venues::{Venue, VenueError};
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KillSwitchError {
    #[error("kill-switch journal write failed: {0}")]
    Journal(#[from] std::io::Error),
    #[error("venue error: {0}")]
    Venue(#[from] VenueError),
}

/// What the switch did, per order/position, plus the summary.
#[derive(Debug, Serialize)]
pub struct KillReport {
    pub action: &'static str,
    pub orders_seen: usize,
    pub orders_cancelled: usize,
    pub orders_cancel_failed: usize,
    pub positions_seen: usize,
    pub flatten_orders_placed: usize,
    pub flatten_orders_failed: usize,
    pub at: String,
}

/// Freeze-and-cancel: cancel EVERY open order at the venue, retrying each
/// once on ambiguity. Touches no positions. The default and safest action.
pub async fn freeze_and_cancel(
    venue: &dyn Venue,
    clock: &dyn Clock,
    journal_path: &Path,
) -> Result<KillReport, KillSwitchError> {
    journal_line(
        journal_path,
        &serde_json::json!({
            "event": "freeze_and_cancel_started",
            "venue": venue.id().to_string(),
            "at": clock.now().to_iso8601(),
        }),
    )?;

    let open = venue.open_orders().await?;
    let mut cancelled = 0usize;
    let mut failed = 0usize;
    for order in &open {
        let mut ok = false;
        for _ in 0..2 {
            match venue.cancel(&order.venue_order_id).await {
                Ok(()) | Err(VenueError::NotFound { .. }) => {
                    ok = true;
                    break;
                }
                Err(_) => continue, // retry once; ambiguity resolved below
            }
        }
        if ok {
            cancelled += 1;
        } else {
            failed += 1;
            journal_line(
                journal_path,
                &serde_json::json!({
                    "event": "cancel_failed",
                    "venue_order_id": order.venue_order_id.to_string(),
                    "at": clock.now().to_iso8601(),
                }),
            )?;
        }
    }

    let report = KillReport {
        action: "freeze_and_cancel",
        orders_seen: open.len(),
        orders_cancelled: cancelled,
        orders_cancel_failed: failed,
        positions_seen: 0,
        flatten_orders_placed: 0,
        flatten_orders_failed: 0,
        at: clock.now().to_iso8601(),
    };
    journal_line(journal_path, &serde_json::to_value(&report)?)?;
    Ok(report)
}

/// Best-effort emergency flatten: freeze-and-cancel first, then report the
/// open positions for the operator. ACTUAL position exits go through the
/// venue as plain orders; the kill switch deliberately does NOT construct
/// orders itself — placing requires a `GatedOrder` (I1), and the emergency
/// path's job is to stop the bleeding (cancel resting risk) and surface
/// state, not to trade. Position exits in an emergency are venue-UI or
/// CLI-confirmed actions by the operator. (ASSUMPTIONS.md, T0.9.)
pub async fn freeze_cancel_and_report_positions(
    venue: &dyn Venue,
    clock: &dyn Clock,
    journal_path: &Path,
) -> Result<KillReport, KillSwitchError> {
    let mut report = freeze_and_cancel(venue, clock, journal_path).await?;
    let positions = venue.positions().await?;
    report.positions_seen = positions.len();
    for p in &positions {
        journal_line(
            journal_path,
            &serde_json::json!({
                "event": "open_position",
                "market": p.market.to_string(),
                "yes": p.yes,
                "no": p.no,
                "cost_cents": p.cost.raw(),
                "at": clock.now().to_iso8601(),
            }),
        )?;
    }
    Ok(report)
}

/// One JSON line, appended and fsync'd: the switch's own flat-file state.
fn journal_line(path: &Path, value: &serde_json::Value) -> Result<(), KillSwitchError> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(value)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

impl From<serde_json::Error> for KillSwitchError {
    fn from(e: serde_json::Error) -> Self {
        KillSwitchError::Journal(std::io::Error::other(e))
    }
}
