//! Kalshi websocket MESSAGE layer (doc-derived; research
//! docs/research/venue/kalshi-api-2026-06-10 §11, verbatim official
//! examples + the archived AsyncAPI spec).
//!
//! Scope discipline: this module parses documented frames into canonical
//! stream events and builds documented commands. The live socket DIAL
//! (TLS upgrade with the signed-handshake auth, ping/pong keep-alive,
//! redial policy) is FIXTURE-GATED like the REST adapter — it lands only
//! after the operator's recording session confirms the contract
//! (GAPS.md). Nothing here invents wire behavior.
//!
//! Pricing convention: we subscribe with `use_yes_price: true` so BOTH
//! sides arrive on the YES scale (the no-leg default is the documented
//! trap; the flag's flip-to-default is Kalshi's announced migration).
//! The yes side maps to bids, the no side to asks, prices already
//! yes-scale. Sub-cent prices and fractional counts are REJECTED — the
//! cents-only core does not trade deci-cent structures.

use crate::kalshi::dto;
use crate::stream::StreamEvent;
use crate::VenueError;
use fortuna_core::market::{MarketId, Side};
use fortuna_core::money::Cents;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The documented subscribe command for the channels FORTUNA consumes:
/// the order book (snapshot + deltas) and public trade prints.
pub fn subscribe_orderbook_cmd(id: u64, market_tickers: &[&str]) -> Value {
    json!({
        "id": id,
        "cmd": "subscribe",
        "params": {
            "channels": ["orderbook_delta", "trade"],
            "market_tickers": market_tickers,
            "use_yes_price": true,
        }
    })
}

/// One parsed frame.
#[derive(Debug)]
pub enum KalshiWsEvent {
    /// Canonical market data.
    Stream(StreamEvent),
    Subscribed {
        channel: String,
        sid: u64,
    },
    Unsubscribed {
        sid: u64,
    },
    /// Listing/ack responses we acknowledge but do not act on.
    Ok,
    /// Server error frame (documented error table; e.g. code 6 =
    /// already subscribed).
    Error {
        code: i64,
        message: String,
    },
    /// A sequence gap on an orderbook subscription: at least one delta
    /// was lost and the book is TORN. The composition must resubscribe
    /// (fresh snapshot) before trusting this market again.
    SeqGap {
        sid: u64,
        expected: u64,
        got: u64,
    },
    /// Documented frame types FORTUNA does not consume (lifecycle,
    /// tickers, fills-via-ws — REST fills remain the fee source of
    /// truth). Carried with their type tag for ops visibility.
    Ignored {
        frame_type: String,
    },
}

/// Stateful frame parser: tracks per-sid sequence numbers for the
/// snapshot/delta consistency guarantee the docs describe.
#[derive(Debug, Default)]
pub struct KalshiWsParser {
    seq_by_sid: BTreeMap<u64, u64>,
}

impl KalshiWsParser {
    pub fn new() -> KalshiWsParser {
        KalshiWsParser::default()
    }

    pub fn parse_frame(&mut self, text: &str) -> Result<KalshiWsEvent, VenueError> {
        let frame: Value = serde_json::from_str(text).map_err(|e| VenueError::Invalid {
            reason: format!("websocket frame is not JSON: {e}"),
        })?;
        let Some(frame_type) = frame["type"].as_str().filter(|t| !t.is_empty()) else {
            return Err(VenueError::Invalid {
                reason: "websocket frame missing its type tag".to_string(),
            });
        };
        match frame_type {
            "subscribed" => Ok(KalshiWsEvent::Subscribed {
                channel: frame["msg"]["channel"].as_str().unwrap_or("").to_string(),
                sid: frame["msg"]["sid"].as_u64().unwrap_or(0),
            }),
            "unsubscribed" => Ok(KalshiWsEvent::Unsubscribed {
                sid: frame["sid"].as_u64().unwrap_or(0),
            }),
            "ok" => Ok(KalshiWsEvent::Ok),
            "error" => Ok(KalshiWsEvent::Error {
                code: frame["msg"]["code"].as_i64().unwrap_or(-1),
                message: frame["msg"]["msg"].as_str().unwrap_or("").to_string(),
            }),
            "orderbook_snapshot" => {
                let (sid, seq) = sid_seq(&frame)?;
                // A snapshot RESETS the sequence baseline for its sid.
                self.seq_by_sid.insert(sid, seq);
                let market = market_of(&frame)?;
                let yes_bids = parse_levels(&frame["msg"]["yes_dollars_fp"])?;
                let mut yes_asks = parse_levels(&frame["msg"]["no_dollars_fp"])?;
                // use_yes_price=true: prices already yes-scale; the ask
                // side sorts ascending (parse order is best-to-worst on
                // each side per the docs; normalize defensively).
                yes_asks.sort_by_key(|l| l.price.raw());
                let mut bids_sorted = yes_bids;
                bids_sorted.sort_by_key(|l| std::cmp::Reverse(l.price.raw()));
                Ok(KalshiWsEvent::Stream(StreamEvent::BookSnapshot {
                    market,
                    yes_bids: bids_sorted,
                    yes_asks,
                }))
            }
            "orderbook_delta" => {
                let (sid, seq) = sid_seq(&frame)?;
                if let Some(prev) = self.seq_by_sid.get(&sid).copied() {
                    if seq != prev + 1 {
                        // Do NOT advance the baseline: every subsequent
                        // delta keeps reporting the gap until resync.
                        return Ok(KalshiWsEvent::SeqGap {
                            sid,
                            expected: prev + 1,
                            got: seq,
                        });
                    }
                    self.seq_by_sid.insert(sid, seq);
                } else {
                    // Delta before any snapshot on this sid: torn from
                    // the start.
                    return Ok(KalshiWsEvent::SeqGap {
                        sid,
                        expected: 0,
                        got: seq,
                    });
                }
                let market = market_of(&frame)?;
                let side = match frame["msg"]["side"].as_str() {
                    Some("yes") => Side::Yes,
                    Some("no") => Side::No,
                    other => {
                        return Err(VenueError::Invalid {
                            reason: format!("orderbook_delta with side {other:?}"),
                        })
                    }
                };
                let yes_price = price_cents(&frame["msg"]["price_dollars"])?;
                let delta_contracts = signed_count(&frame["msg"]["delta_fp"])?;
                Ok(KalshiWsEvent::Stream(StreamEvent::BookDelta {
                    market,
                    side,
                    yes_price,
                    delta_contracts,
                }))
            }
            "trade" => {
                let market = market_of(&frame)?;
                let yes_price = price_cents(&frame["msg"]["yes_price_dollars"])?;
                let qty = signed_count(&frame["msg"]["count_fp"])?;
                if qty <= 0 {
                    return Err(VenueError::Invalid {
                        reason: format!("trade with non-positive count {qty}"),
                    });
                }
                Ok(KalshiWsEvent::Stream(StreamEvent::Trade {
                    market,
                    yes_price,
                    qty,
                }))
            }
            other => Ok(KalshiWsEvent::Ignored {
                frame_type: other.to_string(),
            }),
        }
    }
}

/// Book frames MUST carry sid + seq (the consistency guarantee hangs on
/// them); a frame without either is malformed, not seq-0 (strict-reject
/// per the independent gate: lenient zeros could alias real sequence
/// state).
fn sid_seq(frame: &Value) -> Result<(u64, u64), VenueError> {
    match (frame["sid"].as_u64(), frame["seq"].as_u64()) {
        (Some(sid), Some(seq)) => Ok((sid, seq)),
        _ => Err(VenueError::Invalid {
            reason: "orderbook frame missing sid/seq".to_string(),
        }),
    }
}

fn market_of(frame: &Value) -> Result<MarketId, VenueError> {
    let ticker = frame["msg"]["market_ticker"]
        .as_str()
        .ok_or_else(|| VenueError::Invalid {
            reason: "frame missing market_ticker".to_string(),
        })?;
    MarketId::new(ticker).map_err(|e| VenueError::Invalid {
        reason: format!("bad market ticker in frame: {e}"),
    })
}

fn price_cents(value: &Value) -> Result<Cents, VenueError> {
    let raw = value.as_str().ok_or_else(|| VenueError::Invalid {
        reason: "price field is not a string".to_string(),
    })?;
    dto::parse_dollars_to_cents_exact(raw)
}

/// Signed fixed-point contract count ("-54.00"): integral or refused.
fn signed_count(value: &Value) -> Result<i64, VenueError> {
    let raw = value.as_str().ok_or_else(|| VenueError::Invalid {
        reason: "count field is not a string".to_string(),
    })?;
    let (negative, digits) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let count = dto::parse_count_integral(digits)?;
    Ok(if negative { -count.raw() } else { count.raw() })
}

/// Parse `[["0.0800", "300.00"], ...]` levels (yes-scale prices). An
/// ABSENT side is the documented representation of an empty side
/// (one-sided snapshots are normal for thin books — the archived
/// AsyncAPI schema; the batch gate caught the first cut erroring here).
fn parse_levels(value: &Value) -> Result<Vec<crate::PriceLevel>, VenueError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let Some(rows) = value.as_array() else {
        return Err(VenueError::Invalid {
            reason: "book side is neither absent nor an array".to_string(),
        });
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (Some(price), Some(count)) = (row[0].as_str(), row[1].as_str()) else {
            return Err(VenueError::Invalid {
                reason: "book level is not a [price, count] string pair".to_string(),
            });
        };
        out.push(crate::PriceLevel {
            price: dto::parse_dollars_to_cents_exact(price)?,
            qty: dto::parse_count_integral(count)?,
        });
    }
    Ok(out)
}
