//! The standalone kill switch (I4). Spec 5.4 exemption, Section 8, I4.
//!
//! INDEPENDENCE IS STRUCTURAL: this crate depends only on fortuna-core,
//! fortuna-venues, and fortuna-gates (the perp-flatten SEAL — itself I4-clean:
//! fortuna-core + thiserror + serde, NONE of the forbidden set). No Postgres,
//! no fortuna-ledger, no cognition runtime, no event loop, no Slack. It must
//! function when everything else is dead — including the database (spec
//! Principle 9 exception: its own state is a flat journal file).
//!
//! Actions:
//! - FREEZE-AND-CANCEL (the default, EVENT contracts): cancel every open order;
//!   touch no positions. The switch constructs NO event-contract orders —
//!   position exits are operator venue-UI/CLI flows ([`freeze_and_cancel`],
//!   [`freeze_cancel_and_report_positions`]).
//! - PERP FLATTEN (spec 5.15, [`freeze_cancel_perp_and_flatten`]): cancel every
//!   open perp order, then close each non-flat position with a REDUCE-ONLY IOC
//!   that crosses the live book — best-effort taker exits WITHOUT the flatten
//!   planner (spec 5.4: "the standalone kill-switch process cannot depend on the
//!   planner; emergency flatten through it is best-effort without cost
//!   estimation, an accepted emergency cost"). Each close is still a SEALED
//!   `GatedPerpOrder` from the real perp gate (I1) — the switch is a CONSUMER of
//!   the seal, never a constructor.
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
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpPrice};
use fortuna_gates::perp::{PerpCandidateOrder, PerpGateInputs};
use fortuna_gates::{GateConfig, GatePipeline};
use fortuna_venues::kinetics::adapter::KineticsAdapter;
use fortuna_venues::kinetics::client::TimeInForce;
use fortuna_venues::kinetics::dto;
use fortuna_venues::{Venue, VenueError};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KillSwitchError {
    #[error("kill-switch journal write failed: {0}")]
    Journal(#[from] std::io::Error),
    #[error("venue error: {0}")]
    Venue(#[from] VenueError),
    /// A fixed setup constant (the flatten strategy id) could not be built —
    /// refuse BEFORE any venue call (fail-closed; never panic on a constant).
    #[error("kill-switch setup error: {0}")]
    Setup(String),
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
    /// Perp positions left un-flattened because no live price could be crossed
    /// (no book / empty side / overflow) — never an un-priced order. 0 on the
    /// freeze (event-contract) path.
    pub flatten_orders_skipped_no_price: usize,
    /// Perp closes the gate refused (should be ~0 for an honest reduce-only —
    /// e.g. a slippage > price-band PriceSanity self-reject). 0 on the freeze path.
    pub flatten_orders_rejected_by_gate: usize,
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
        flatten_orders_skipped_no_price: 0,
        flatten_orders_rejected_by_gate: 0,
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

// ===========================================================================
// I4 REVOCATION SENTINEL (open audit C2 / GAPS "I4 revocation gap").
//
// Spec I4 (spec.md:43) requires the kill path to "flatten or freeze all
// positions AND revoke order-placing capability". The freeze/flatten paths
// above cancel resting risk; these functions add the SECOND half — a DURABLE
// kill sentinel the switch WRITES and the runtime CONSUMES as a global halt
// that blocks FUTURE order placement until the operator clears it out-of-band.
//
// INDEPENDENCE PRESERVED: std::fs only — no Postgres, no cognition, no event
// loop (the same flat-file posture as `journal_line`; spec Principle 9
// exception). The sentinel is a sibling of the journal so the operator manages
// ONE directory. Consumption lives in fortuna-live's RevocationHaltPoller
// (which polls BEFORE every tick), so a present sentinel halts the gate before
// any order — including the first poll after a restart ("boots revoked").
// ===========================================================================

/// The kill-sentinel path: a sibling of the journal named `KILLSWITCH_REVOKED`.
/// A journal with no parent (a bare filename) sits in the current directory, so
/// the sentinel does too — `unwrap_or`, NEVER `unwrap()` (no panic on a path).
pub fn revocation_path(journal: &Path) -> PathBuf {
    journal
        .parent()
        .unwrap_or(Path::new("."))
        .join("KILLSWITCH_REVOKED")
}

/// WRITE the revocation sentinel: create + truncate + fsync ONE JSON line
/// `{"revoked_at": <iso8601>, "by": <verb>}`. Idempotent — a kill that fires
/// twice simply re-truncates to a fresh line (the file's PRESENCE is the halt;
/// its contents are the audit trail of who revoked and when). fsync so the
/// halt survives a crash the instant the switch returns (durable revocation).
pub fn write_revocation(path: &Path, clock: &dyn Clock, by: &str) -> Result<(), KillSwitchError> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    let line = serde_json::to_string(&serde_json::json!({
        "revoked_at": clock.now().to_iso8601(),
        "by": by,
    }))?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

/// CLEAR the revocation sentinel — the operator's out-of-band re-arm
/// prerequisite (spec Section 8: kill-switch reversal is CLI-only). Idempotent:
/// a missing file is `Ok(())` (clearing an already-clear state succeeds), so the
/// operator can run it safely whether or not a kill fired.
pub fn clear_revocation(path: &Path) -> Result<(), KillSwitchError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(KillSwitchError::Journal(e)),
    }
}

/// Is order-placing capability revoked? The sentinel's PRESENCE is the halt —
/// a read-only check the runtime poller makes before every tick. Deliberately
/// total (no error): an unreadable parent dir reports "not revoked", but the
/// poller is layered OVER the durable PgHaltPoller, so a real halt is never lost
/// to a transient FS hiccup here.
pub fn is_revoked(path: &Path) -> bool {
    path.exists()
}

impl From<serde_json::Error> for KillSwitchError {
    fn from(e: serde_json::Error) -> Self {
        KillSwitchError::Journal(std::io::Error::other(e))
    }
}

/// The kill-switch's Kalshi credentials, loaded ENV-ONLY and FAIL-CLOSED. The
/// switch keeps a SEPARATE credential pair from the trading runtime (spec I4:
/// the switch must function when everything else is dead), so these are its own
/// `FORTUNA_KILLSWITCH_KALSHI_*` env vars — never read from config and never
/// logged.
pub struct KalshiCreds {
    pub api_key_id: String,
    /// The PEM TEXT (read from the file at `_PRIVATE_KEY_PATH`), never the path.
    pub private_key_pem: String,
    pub base_url: String,
}

/// Hand-written so the private key NEVER reaches a log line / panic message /
/// audit payload via `{:?}` (no secrets in logs — CLAUDE.md). The key id and
/// base URL are non-secret identifiers and are shown.
impl std::fmt::Debug for KalshiCreds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KalshiCreds")
            .field("api_key_id", &self.api_key_id)
            .field("private_key_pem", &"[redacted]")
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// Validate the three `FORTUNA_KILLSWITCH_KALSHI_*` inputs and read the private
/// key file — FAIL-CLOSED. A missing or empty value is a hard error naming the
/// ENV VAR (never its value). The base URL is REQUIRED, never defaulted: the
/// switch must not cancel against the wrong environment (prod vs demo must be an
/// explicit operator choice). Pure (no env access) so it is exhaustively
/// testable; `main` reads the env and the venue is built only after this passes.
pub fn load_kalshi_creds(
    api_key_id: Option<String>,
    private_key_path: Option<String>,
    base_url: Option<String>,
) -> Result<KalshiCreds, String> {
    fn require(value: Option<String>, var: &str) -> Result<String, String> {
        match value {
            Some(v) if !v.trim().is_empty() => Ok(v),
            _ => Err(format!("{var} is required (env-only, fail-closed)")),
        }
    }
    let api_key_id = require(api_key_id, "FORTUNA_KILLSWITCH_KALSHI_API_KEY_ID")?;
    let private_key_path = require(
        private_key_path,
        "FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH",
    )?;
    let base_url = require(base_url, "FORTUNA_KILLSWITCH_KALSHI_BASE_URL")?;
    let private_key_pem = std::fs::read_to_string(&private_key_path).map_err(|e| {
        format!("cannot read FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH ({private_key_path}): {e}")
    })?;
    if private_key_pem.trim().is_empty() {
        return Err(
            "the private key at FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH is empty".to_string(),
        );
    }
    Ok(KalshiCreds {
        api_key_id,
        private_key_pem,
        base_url,
    })
}

/// Load the kill-switch's SEPARATE Kinetics (perps) credential pair — ENV-ONLY,
/// FAIL-CLOSED, mirroring [`load_kalshi_creds`] (I4: the switch keeps its own
/// creds, never the runtime's). Its own `FORTUNA_KILLSWITCH_KINETICS_*` vars.
/// Pure (no env access); `main` reads the env and builds the venue only after
/// this passes. Reuses [`KalshiCreds`] (generic id/pem/base_url).
pub fn load_kinetics_creds(
    api_key_id: Option<String>,
    private_key_path: Option<String>,
    base_url: Option<String>,
) -> Result<KalshiCreds, String> {
    fn require(value: Option<String>, var: &str) -> Result<String, String> {
        match value {
            Some(v) if !v.trim().is_empty() => Ok(v),
            _ => Err(format!("{var} is required (env-only, fail-closed)")),
        }
    }
    let api_key_id = require(api_key_id, "FORTUNA_KILLSWITCH_KINETICS_API_KEY_ID")?;
    let private_key_path = require(
        private_key_path,
        "FORTUNA_KILLSWITCH_KINETICS_PRIVATE_KEY_PATH",
    )?;
    let base_url = require(base_url, "FORTUNA_KILLSWITCH_KINETICS_BASE_URL")?;
    let private_key_pem = std::fs::read_to_string(&private_key_path).map_err(|e| {
        format!(
            "cannot read FORTUNA_KILLSWITCH_KINETICS_PRIVATE_KEY_PATH ({private_key_path}): {e}"
        )
    })?;
    if private_key_pem.trim().is_empty() {
        return Err(
            "the private key at FORTUNA_KILLSWITCH_KINETICS_PRIVATE_KEY_PATH is empty".to_string(),
        );
    }
    Ok(KalshiCreds {
        api_key_id,
        private_key_pem,
        base_url,
    })
}

/// Load + validate the gate config from `FORTUNA_KILLSWITCH_GATE_CONFIG_PATH`,
/// FAIL-CLOSED (mirror [`load_kalshi_creds`]). A missing / unreadable /
/// unparseable / `validate()`-failing config REFUSES before any venue call;
/// the error names the ENV VAR (and the path), never the config contents.
///
/// The operator's TOML MUST carry `[per_strategy.killswitch_flatten]`,
/// `[perp.venues.<venue_id>]`, `[perp.assets.<each-ticker>]`, and
/// `[rate.<venue_id>]` — without them the perp `SizeSanity`/`PriceSanity` checks
/// fail-closed and EVERY close is gate-rejected (a no-op cancel-only flatten,
/// surfaced as `flatten_orders_rejected_by_gate`). Pure (takes the path value).
pub fn load_gate_config(path: Option<String>) -> Result<GateConfig, String> {
    let path = match path {
        Some(p) if !p.trim().is_empty() => p,
        _ => {
            return Err(
                "FORTUNA_KILLSWITCH_GATE_CONFIG_PATH is required (env-only, fail-closed)"
                    .to_string(),
            )
        }
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read FORTUNA_KILLSWITCH_GATE_CONFIG_PATH ({path}): {e}"))?;
    let config: GateConfig = toml::from_str(&text).map_err(|e| {
        format!("FORTUNA_KILLSWITCH_GATE_CONFIG_PATH ({path}) did not parse as a gate config: {e}")
    })?;
    config
        .validate()
        .map_err(|e| format!("gate config at FORTUNA_KILLSWITCH_GATE_CONFIG_PATH ({path}): {e}"))?;
    Ok(config)
}

/// The IOC limit that crosses the live touch, rounding the slippage AGAINST us.
/// Closing a LONG (Sell, hit the bid): `limit = bid − ceil(bid·bps/10_000)`.
/// Closing a SHORT (Buy, lift the ask): `limit = ask + ceil(ask·bps/10_000)`.
/// `None` on a degenerate (negative) product or ANY arithmetic overflow ⇒ the
/// caller SKIPS the position (never a fabricated / un-priced order). Pure.
fn crossed_close_limit(action: Action, touch: PerpPrice, slippage_bps: i64) -> Option<PerpPrice> {
    let product = touch.raw().checked_mul(slippage_bps)?;
    if product < 0 {
        return None; // a negative touch or bps is degenerate — skip, never guess.
    }
    // ceil(product / 10_000) for product >= 0.
    let slip = PerpPrice::new(product.checked_add(9_999)? / 10_000);
    match action {
        Action::Sell => touch.checked_sub(slip).ok(),
        Action::Buy => touch.checked_add(slip).ok(),
    }
}

/// PERP FLATTEN (spec 5.15, T5.B8): cancel EVERY open perp order, then close
/// each non-flat position with a REDUCE-ONLY IOC that crosses the live book —
/// every close a SEALED `GatedPerpOrder` through the real perp gate (I1: the
/// switch sits on the consumer side of the seal; it constructs a candidate and
/// asks the gate, exactly like the trading path). Best-effort + fail-closed: no
/// panic anywhere; a per-position failure (no price / gate-reject / place-error)
/// is JOURNALED and the sweep CONTINUES. Only a journal-write failure aborts.
///
/// I4 holds: no Postgres, no cognition, no event loop; the adapter carries the
/// switch's OWN credential pair; `evaluate_perp` is pure. The margin view is
/// balance-only (the reduce-only capital checks waive, so the gate never reads
/// `account.equity` on this path — an honest, sufficient view; no fabricated
/// marks/positions). `conservative_mark` = the touch we cross, so
/// `|limit − mark| = slippage`; if `slippage_bps` exceeds the config price-band
/// the gate self-rejects `PriceSanity` (counted, never fatal).
pub async fn freeze_cancel_perp_and_flatten(
    adapter: &KineticsAdapter,
    gates: &mut GatePipeline,
    venue_id: &VenueId,
    slippage_bps: i64,
    clock: &dyn Clock,
    journal: &Path,
) -> Result<KillReport, KillSwitchError> {
    // Build the fixed flatten strategy id ONCE, fail-closed BEFORE any venue
    // call (never panic on a constant).
    let strategy = StrategyId::new("killswitch_flatten")
        .map_err(|e| KillSwitchError::Setup(format!("killswitch_flatten strategy id: {e}")))?;

    journal_line(
        journal,
        &serde_json::json!({
            "event": "flatten_started",
            "venue": venue_id.to_string(),
            "slippage_bps": slippage_bps,
            "at": clock.now().to_iso8601(),
        }),
    )?;

    // ---- CANCEL-ALL open perp orders (retry once; NotFound == cancelled) ----
    let mut orders_seen = 0usize;
    let mut cancelled = 0usize;
    let mut cancel_failed = 0usize;
    match adapter.client().list_orders(None, Some(100)).await {
        Ok(listing) => {
            orders_seen = listing.orders.len();
            for o in &listing.orders {
                let mut ok = false;
                for _ in 0..2 {
                    match adapter.cancel(&o.order_id).await {
                        Ok(_) | Err(VenueError::NotFound { .. }) => {
                            ok = true;
                            break;
                        }
                        Err(_) => continue,
                    }
                }
                if ok {
                    cancelled += 1;
                } else {
                    cancel_failed += 1;
                    journal_line(
                        journal,
                        &serde_json::json!({
                            "event": "cancel_failed",
                            "order_id": o.order_id,
                            "at": clock.now().to_iso8601(),
                        }),
                    )?;
                }
            }
        }
        Err(e) => journal_line(
            journal,
            &serde_json::json!({
                "event": "flatten_list_orders_failed",
                "reason": e.to_string(),
                "at": clock.now().to_iso8601(),
            }),
        )?,
    }

    // ---- balance-only margin view (gate-irrelevant on reduce-only; honest) ----
    let balance = match adapter.client().balance(false).await {
        // settled_funds is a DOLLAR string; floor to cents (never overstate
        // cash). parse_dollars (VenueError) and from_dollars_floor (MoneyError)
        // have different error types, so chain through Option, not `?`.
        Ok(b) => match dto::parse_dollars(&b.settled_funds)
            .ok()
            .and_then(|d| Cents::from_dollars_floor(d).ok())
        {
            Some(c) => c,
            None => {
                journal_line(
                    journal,
                    &serde_json::json!({
                        "event": "flatten_balance_unparsed",
                        "at": clock.now().to_iso8601(),
                    }),
                )?;
                Cents::ZERO
            }
        },
        Err(e) => {
            journal_line(
                journal,
                &serde_json::json!({
                    "event": "flatten_balance_failed",
                    "reason": e.to_string(),
                    "at": clock.now().to_iso8601(),
                }),
            )?;
            Cents::ZERO
        }
    };
    // compute(&[]) is infallible (no PnL math); the manual fallback is the same
    // balance-only view, so this never panics and never fabricates a position.
    let account =
        MarginAccountView::compute(balance, &[], Cents::ZERO).unwrap_or(MarginAccountView {
            balance,
            unrealized: Cents::ZERO,
            pending_funding: Cents::ZERO,
            equity: balance,
            unmarked_flag: false,
        });

    // ---- per-position REDUCE-ONLY close ----
    let mut positions_seen = 0usize;
    let mut placed = 0usize;
    let mut order_failed = 0usize;
    let mut skipped_no_price = 0usize;
    let mut rejected_by_gate = 0usize;
    // Local, no-cognition id source seeded from the (injected) clock.
    let mut ids = IdGen::new(clock.now().epoch_millis().max(0) as u64);
    let empty_recent_ids: BTreeSet<String> = BTreeSet::new();

    match adapter.positions().await {
        Err(e) => journal_line(
            journal,
            &serde_json::json!({
                "event": "flatten_positions_failed",
                "reason": e.to_string(),
                "at": clock.now().to_iso8601(),
            }),
        )?,
        Ok(positions) => {
            for kp in &positions {
                let pos = &kp.position;
                if pos.is_flat() {
                    continue;
                }
                positions_seen += 1;

                // Cross the LIVE book: a long closes by SELLing into the bid; a
                // short closes by BUYing the ask. No book / empty side / overflow
                // ⇒ journal + skip (never an un-priced order).
                let (action, touch) = {
                    let book = match adapter
                        .client()
                        .orderbook(pos.market.as_str(), 0, None)
                        .await
                    {
                        Ok(b) => b,
                        Err(_) => {
                            journal_line(
                                journal,
                                &serde_json::json!({
                                    "event": "flatten_skipped_no_price",
                                    "market": pos.market.to_string(),
                                    "reason": "orderbook fetch failed",
                                    "at": clock.now().to_iso8601(),
                                }),
                            )?;
                            skipped_no_price += 1;
                            continue;
                        }
                    };
                    let side = if pos.is_long() {
                        book.orderbook.best_bid()
                    } else {
                        book.orderbook.best_ask()
                    };
                    match side {
                        Ok(Some((price, _qty))) => {
                            let action = if pos.is_long() {
                                Action::Sell
                            } else {
                                Action::Buy
                            };
                            (action, price)
                        }
                        _ => {
                            journal_line(
                                journal,
                                &serde_json::json!({
                                    "event": "flatten_skipped_no_price",
                                    "market": pos.market.to_string(),
                                    "reason": "empty book side",
                                    "at": clock.now().to_iso8601(),
                                }),
                            )?;
                            skipped_no_price += 1;
                            continue;
                        }
                    }
                };

                let Some(limit) = crossed_close_limit(action, touch, slippage_bps) else {
                    journal_line(
                        journal,
                        &serde_json::json!({
                            "event": "flatten_skipped_no_price",
                            "market": pos.market.to_string(),
                            "reason": "limit-price overflow",
                            "at": clock.now().to_iso8601(),
                        }),
                    )?;
                    skipped_no_price += 1;
                    continue;
                };

                // qty = |position|, opposite the position (validated reduce-only).
                let Some(qty_abs) = pos.qty.raw().checked_abs() else {
                    journal_line(
                        journal,
                        &serde_json::json!({
                            "event": "flatten_skipped_no_price",
                            "market": pos.market.to_string(),
                            "reason": "position size overflow",
                            "at": clock.now().to_iso8601(),
                        }),
                    )?;
                    skipped_no_price += 1;
                    continue;
                };

                // Mint the intent id locally (no cognition); a (near-impossible)
                // id error is a per-position failure, never a panic/abort.
                let intent_id = match ids.next(clock.now()) {
                    Ok(ulid) => IntentId::new(ulid),
                    Err(e) => {
                        journal_line(
                            journal,
                            &serde_json::json!({
                                "event": "flatten_order_failed",
                                "market": pos.market.to_string(),
                                "reason": format!("intent id mint: {e}"),
                                "at": clock.now().to_iso8601(),
                            }),
                        )?;
                        order_failed += 1;
                        continue;
                    }
                };

                let candidate = PerpCandidateOrder {
                    intent_id,
                    strategy: strategy.clone(),
                    venue: venue_id.clone(),
                    market: pos.market.clone(),
                    action,
                    reduce_only: true,
                    limit_price: limit,
                    qty: Contracts::new(qty_abs),
                    fair_value: touch,
                    holding_windows: 1,
                    client_order_id: ClientOrderId::from_intent(intent_id),
                };
                let inputs = PerpGateInputs {
                    now: clock.now(),
                    account: &account,
                    position: Some(pos),
                    conservative_mark: touch,
                    venue_open_notional_cents: Cents::ZERO,
                    own_resting: &[],
                    recent_client_order_ids: &empty_recent_ids,
                };

                // THE SEAL: a close is a GatedPerpOrder only if the gate builds it.
                match gates.evaluate_perp(&candidate, &inputs).gated {
                    Err(rej) => {
                        journal_line(
                            journal,
                            &serde_json::json!({
                                "event": "flatten_gate_rejected",
                                "market": pos.market.to_string(),
                                "check": format!("{:?}", rej.check),
                                "reason": rej.reason,
                                "at": clock.now().to_iso8601(),
                            }),
                        )?;
                        rejected_by_gate += 1;
                    }
                    Ok(gated) => {
                        // Venue requires IOC/FOK on reduce-only: IOC, post_only None.
                        match adapter
                            .place(&gated, TimeInForce::ImmediateOrCancel, None)
                            .await
                        {
                            Ok(placement) => {
                                journal_line(
                                    journal,
                                    &serde_json::json!({
                                        "event": "flatten_order_placed",
                                        "market": pos.market.to_string(),
                                        "venue_order_id": placement.venue_order_id.to_string(),
                                        "filled": placement.filled.raw(),
                                        "remaining": placement.remaining.raw(),
                                        "at": clock.now().to_iso8601(),
                                    }),
                                )?;
                                placed += 1;
                            }
                            Err(e) => {
                                journal_line(
                                    journal,
                                    &serde_json::json!({
                                        "event": "flatten_order_failed",
                                        "market": pos.market.to_string(),
                                        "reason": e.to_string(),
                                        "at": clock.now().to_iso8601(),
                                    }),
                                )?;
                                order_failed += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    let report = KillReport {
        action: "flatten_perps",
        orders_seen,
        orders_cancelled: cancelled,
        orders_cancel_failed: cancel_failed,
        positions_seen,
        flatten_orders_placed: placed,
        flatten_orders_failed: order_failed,
        flatten_orders_skipped_no_price: skipped_no_price,
        flatten_orders_rejected_by_gate: rejected_by_gate,
        at: clock.now().to_iso8601(),
    };
    journal_line(journal, &serde_json::to_value(&report)?)?;
    Ok(report)
}
