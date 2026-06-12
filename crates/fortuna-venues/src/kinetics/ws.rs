//! Kinetics margin WS SESSION layer (T5.B4 slice 4; spec 5.15).
//!
//! Sans-IO, like the event-API layer: this module builds the EXACT
//! subscribe commands the operator's recorder sent (the recorded acks
//! prove the venue accepts them) and turns raw text frames into typed
//! session events with the snapshot/delta sequence discipline. The live
//! socket dial (TLS + signed handshake + redial policy) is the
//! composition's IO edge.
//!
//! Wire truths carried here:
//!
//! - Handshake signing (fixture finding 2, SETTLED): sign
//!   `GET /trade-api/ws/v2/margin` — the URL path itself — against the
//!   dedicated margin-WS host.
//! - REST and WS books order worst->best with best at the array END
//!   (finding 1): snapshot levels are re-sorted defensively, best first,
//!   never assumed.
//! - Pings are NOT guaranteed every 10s under flow (finding 14: a busy
//!   75s session saw zero pings across 1712 frames): liveness must come
//!   from frame flow, not ping cadence — nothing here keys on pings.
//! - The `user_orders` channel is not guaranteed to emit during a
//!   lifecycle (session notes): consumers must treat user_order frames
//!   as best-effort and reconcile via REST fills.
//! - A sequence gap on an orderbook sid means the book is TORN: the
//!   parser reports `SeqGap` and does NOT advance the baseline — every
//!   later delta keeps reporting the gap until a fresh snapshot resets
//!   it (resubscribe is the composition's move).

use crate::kinetics::client::BookSide;
use crate::kinetics::dto::{self, WsFrame};
use crate::VenueError;
use fortuna_core::market::{Contracts, MarketId};
use fortuna_core::perp::PerpPrice;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Production margin-WS endpoint (research §8b, archived AsyncAPI spec).
pub const MARGIN_WS_PROD_URL: &str =
    "wss://external-api-margin-ws.kalshi.com/trade-api/ws/v2/margin";
/// Demo margin-WS endpoint (RECORDED: the fixture session connected and
/// captured streams here).
pub const MARGIN_WS_DEMO_URL: &str =
    "wss://external-api-margin-ws.demo.kalshi.co/trade-api/ws/v2/margin";
/// The path string to SIGN for the handshake (finding 2: the URL path
/// itself, accepted 101 on the dedicated host).
pub const MARGIN_WS_SIGNING_PATH: &str = "/trade-api/ws/v2/margin";

/// Public book + trades subscription — byte-shape of the recorder's
/// accepted command.
pub fn subscribe_book_cmd(id: u64, market_tickers: &[&str]) -> Value {
    json!({
        "id": id,
        "cmd": "subscribe",
        "params": {"channels": ["orderbook_delta", "trade"], "market_tickers": market_tickers},
    })
}

/// Ticker subscription (marks + funding stream) with the initial
/// snapshot, as recorded.
pub fn subscribe_ticker_cmd(id: u64, market_tickers: &[&str]) -> Value {
    json!({
        "id": id,
        "cmd": "subscribe",
        "params": {
            "channels": ["ticker"],
            "market_tickers": market_tickers,
            "send_initial_snapshot": true,
        },
    })
}

/// Private channels, as recorded. `user_orders` may stay silent
/// (session notes); `fill` and `order_group_updates` are the reliable
/// pair observed.
pub fn subscribe_private_cmd(id: u64) -> Value {
    json!({
        "id": id,
        "cmd": "subscribe",
        "params": {"channels": ["user_orders", "fill", "order_group_updates"]},
    })
}

/// One typed session event.
#[derive(Debug, Clone)]
pub enum KineticsWsEvent {
    Subscribed {
        channel: String,
        sid: i64,
    },
    /// Normalized: bids best-first (descending), asks best-first
    /// (ascending) — re-sorted defensively per finding 1.
    BookSnapshot {
        market: MarketId,
        bids: Vec<(PerpPrice, Contracts)>,
        asks: Vec<(PerpPrice, Contracts)>,
    },
    BookDelta {
        market: MarketId,
        side: BookSide,
        price: PerpPrice,
        delta: Contracts,
        ts_ms: i64,
    },
    Ticker {
        market: MarketId,
        price: PerpPrice,
        bid: PerpPrice,
        ask: PerpPrice,
        funding_rate: f64,
        next_funding_time_ms: i64,
        settlement_mark: PerpPrice,
        liquidation_mark: PerpPrice,
        reference_price: PerpPrice,
        ts_ms: i64,
    },
    Trade {
        market: MarketId,
        price: PerpPrice,
        count: Contracts,
        taker_side: BookSide,
        ts_ms: i64,
    },
    /// Best-effort order echo (the channel can stay silent; REST is the
    /// reconciliation source of truth).
    UserOrder {
        order_id: String,
        client_order_id: String,
        market: MarketId,
        side: BookSide,
        price: PerpPrice,
        fill_count: Contracts,
        remaining_count: Contracts,
        last_updated_ts_ms: i64,
    },
    GroupUpdate {
        event_type: String,
        order_group_id: String,
        contracts_limit: Option<Contracts>,
        ts_ms: i64,
    },
    /// The book on this sid is TORN (lost delta, or delta before any
    /// snapshot). The baseline does not advance: resubscribe for a fresh
    /// snapshot before trusting this market again.
    SeqGap {
        sid: i64,
        expected: i64,
        got: i64,
    },
    /// A frame type this build does not consume; carried for ops
    /// visibility, never an error (the demo API build is newer than
    /// prod — a venue deploy must not become an outage).
    Ignored {
        frame_type: String,
    },
}

/// Stateful session parser: per-sid sequence tracking for the orderbook
/// consistency guarantee.
#[derive(Debug, Default)]
pub struct KineticsWsSession {
    seq_by_sid: BTreeMap<i64, i64>,
}

impl KineticsWsSession {
    pub fn new() -> KineticsWsSession {
        KineticsWsSession::default()
    }

    /// Parse one text frame. Blank keep-alive lines (the recorded
    /// streams carry trailing-newline blanks) are the caller's filter;
    /// this expects JSON.
    pub fn parse_frame(&mut self, text: &str) -> Result<KineticsWsEvent, VenueError> {
        match dto::parse_ws_frame(text)? {
            WsFrame::Subscribed { msg, .. } => Ok(KineticsWsEvent::Subscribed {
                channel: msg.channel,
                sid: msg.sid,
            }),
            WsFrame::OrderbookSnapshot { sid, seq, msg } => {
                // A snapshot RESETS the sequence baseline for its sid.
                self.seq_by_sid.insert(sid, seq);
                let mut bids = typed_levels(&msg.bid)?;
                let mut asks = typed_levels(&msg.ask)?;
                // Finding 1: recorded order is worst->best; normalize to
                // best-first without assuming either.
                bids.sort_by_key(|(p, _)| std::cmp::Reverse(p.raw()));
                asks.sort_by_key(|(p, _)| p.raw());
                Ok(KineticsWsEvent::BookSnapshot {
                    market: market_id(&msg.market_ticker)?,
                    bids,
                    asks,
                })
            }
            WsFrame::OrderbookDelta { sid, seq, msg } => {
                match self.seq_by_sid.get(&sid).copied() {
                    Some(prev) if seq == prev + 1 => {
                        self.seq_by_sid.insert(sid, seq);
                    }
                    Some(prev) => {
                        // Torn: do NOT advance; keep reporting until a
                        // fresh snapshot rebaselines.
                        return Ok(KineticsWsEvent::SeqGap {
                            sid,
                            expected: prev + 1,
                            got: seq,
                        });
                    }
                    None => {
                        return Ok(KineticsWsEvent::SeqGap {
                            sid,
                            expected: 0,
                            got: seq,
                        });
                    }
                }
                Ok(KineticsWsEvent::BookDelta {
                    market: market_id(&msg.market_ticker)?,
                    side: book_side(&msg.side)?,
                    price: dto::parse_perp_price(&msg.price)?,
                    delta: signed_count(&msg.delta)?,
                    ts_ms: msg.ts_ms,
                })
            }
            WsFrame::Ticker { msg, .. } => Ok(KineticsWsEvent::Ticker {
                market: market_id(&msg.market_ticker)?,
                price: dto::parse_perp_price(&msg.price)?,
                bid: dto::parse_perp_price(&msg.bid)?,
                ask: dto::parse_perp_price(&msg.ask)?,
                funding_rate: msg.funding_rate.rate,
                next_funding_time_ms: msg.funding_rate.next_funding_time_ms,
                settlement_mark: dto::parse_perp_price(&msg.settlement_mark_price.price)?,
                liquidation_mark: dto::parse_perp_price(&msg.liquidation_mark_price.price)?,
                reference_price: dto::parse_perp_price(&msg.reference_price.price)?,
                ts_ms: msg.ts_ms,
            }),
            WsFrame::Trade { msg, .. } => {
                let count = dto::parse_whole_count(&msg.count)?;
                if count.raw() <= 0 {
                    return Err(VenueError::Invalid {
                        reason: format!("trade with non-positive count {count}"),
                    });
                }
                Ok(KineticsWsEvent::Trade {
                    market: market_id(&msg.market_ticker)?,
                    price: dto::parse_perp_price(&msg.price)?,
                    count,
                    taker_side: book_side(&msg.taker_side)?,
                    ts_ms: msg.ts_ms,
                })
            }
            WsFrame::UserOrder { msg, .. } => Ok(KineticsWsEvent::UserOrder {
                order_id: msg.order_id,
                client_order_id: msg.client_order_id,
                market: market_id(&msg.ticker)?,
                side: book_side(&msg.side)?,
                price: dto::parse_perp_price(&msg.price)?,
                fill_count: dto::parse_whole_count(&msg.fill_count)?,
                remaining_count: dto::parse_whole_count(&msg.remaining_count)?,
                last_updated_ts_ms: msg.last_updated_ts_ms,
            }),
            WsFrame::OrderGroupUpdate { msg, .. } => Ok(KineticsWsEvent::GroupUpdate {
                event_type: msg.event_type,
                order_group_id: msg.order_group_id,
                contracts_limit: match &msg.contracts_limit_fp {
                    Some(raw) => Some(dto::parse_whole_count(raw)?),
                    None => None,
                },
                ts_ms: msg.ts_ms,
            }),
            WsFrame::Unknown(value) => Ok(KineticsWsEvent::Ignored {
                frame_type: value
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("<untyped>")
                    .to_string(),
            }),
        }
    }
}

fn market_id(raw: &str) -> Result<MarketId, VenueError> {
    MarketId::new(raw).map_err(|e| VenueError::Invalid {
        reason: format!("ws market ticker: {e}"),
    })
}

fn book_side(raw: &str) -> Result<BookSide, VenueError> {
    match raw {
        "bid" => Ok(BookSide::Bid),
        "ask" => Ok(BookSide::Ask),
        other => Err(VenueError::Invalid {
            reason: format!("ws side {other:?} is neither bid nor ask"),
        }),
    }
}

/// Signed fixed-point count ("-1.00"): whole contracts, sign preserved.
fn signed_count(raw: &str) -> Result<Contracts, VenueError> {
    dto::parse_whole_count(raw)
}

fn typed_levels(levels: &[(String, String)]) -> Result<Vec<(PerpPrice, Contracts)>, VenueError> {
    levels
        .iter()
        .map(|(price, count)| {
            Ok((
                dto::parse_perp_price(price)?,
                dto::parse_whole_count(count)?,
            ))
        })
        .collect()
}
