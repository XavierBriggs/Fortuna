//! Conservative-side marking (spec 5.14 Marking policy).
//!
//! Unrealized value and all halt math use conservative-side marks on the
//! $1-payout binary contract (prices in cents, payout 100c/contract -
//! consistent with `OrderBook`'s (0, 100) price domain):
//!
//! - The YES lot marks at the best YES BID x qty (what a forced exit would
//!   fetch right now).
//! - The NO lot marks at (100 - best YES ASK) x qty (the NO bid derived from
//!   the YES book: no_bid(p) == 100 - yes_ask(p)).
//! - Lots are marked INDEPENDENTLY and summed: a held YES+NO pair marks at
//!   roughly the 100c pair value (bid + (100 - ask)), which is exactly its
//!   worth at settlement minus the spread.
//!
//! Degraded books (mid-marking in thin books manufactures both false halts
//! and hidden losses; FORTUNA prices its own positions pessimistically):
//!
//! - If the book is STALE (`now - as_of` STRICTLY greater than
//!   `max_book_age_ms`; age exactly equal is NOT stale) or the spread
//!   (ask - bid, evaluated only when BOTH touches exist) STRICTLY exceeds
//!   `max_spread_cents` (exactly equal is NOT wide): the mark still uses the
//!   touch price but sets `wide_flag = true`.
//! - If NO touch exists on the needed side, or there is no book at all:
//!   `value = Cents::ZERO` with `wide_flag = true`. Zero is the conservative
//!   bound - there is no reliable exit value, and a binary position is never
//!   worth less than zero.
//! - A degenerate ask above the 100c payout (malformed book) would imply a
//!   negative NO value; it clamps to `Cents::ZERO` with `wide_flag = true`.
//!
//! A flat position (both lots zero) marks at zero with `wide_flag = false`:
//! there is nothing to mark, so no degraded-book noise.

use crate::StateError;
use fortuna_core::book::OrderBook;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::money::Cents;
use serde::{Deserialize, Serialize};

/// Payout of one binary contract, in cents.
const PAYOUT_PER_CONTRACT: Cents = Cents::new(100);

/// Staleness/width thresholds (config-owned; spec 5.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkPolicy {
    /// A book strictly older than this (vs the injected `now`) is stale.
    pub max_book_age_ms: i64,
    /// A spread strictly wider than this (cents) flags the mark wide.
    pub max_spread_cents: i64,
}

/// A conservative mark: position value plus the degraded-book flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub value: Cents,
    /// True when the mark came from a stale book, a wide spread, a missing
    /// touch/book, or a degenerate price - i.e. it is the conservative
    /// bound, not a reliable exit value.
    pub wide_flag: bool,
}

impl Mark {
    const fn degraded() -> Mark {
        Mark {
            value: Cents::ZERO,
            wide_flag: true,
        }
    }
}

/// Mark a position's YES and NO lots against the (optional) current book,
/// independently, and sum. `now` comes from the caller's `Clock`. See the
/// module doc for the exact policy.
pub fn mark_lots(
    yes_qty: i64,
    no_qty: i64,
    book: Option<&OrderBook>,
    now: UtcTimestamp,
    policy: &MarkPolicy,
) -> Result<Mark, StateError> {
    if yes_qty < 0 || no_qty < 0 {
        return Err(StateError::Arithmetic {
            op: "negative lot quantity",
        });
    }
    let yes = mark_one_side(yes_qty, true, book, now, policy)?;
    let no = mark_one_side(no_qty, false, book, now, policy)?;
    Ok(Mark {
        value: yes.value.checked_add(no.value).map_err(StateError::Money)?,
        wide_flag: yes.wide_flag || no.wide_flag,
    })
}

fn mark_one_side(
    qty: i64,
    is_yes: bool,
    book: Option<&OrderBook>,
    now: UtcTimestamp,
    policy: &MarkPolicy,
) -> Result<Mark, StateError> {
    if qty == 0 {
        return Ok(Mark {
            value: Cents::ZERO,
            wide_flag: false,
        });
    }
    let Some(book) = book else {
        return Ok(Mark::degraded());
    };

    let age_ms = now
        .epoch_millis()
        .checked_sub(book.as_of.epoch_millis())
        .ok_or(StateError::Arithmetic { op: "book age" })?;
    let stale = age_ms > policy.max_book_age_ms;

    let too_wide = match (book.best_bid(), book.best_ask()) {
        (Some(bid), Some(ask)) => {
            ask.price
                .checked_sub(bid.price)
                .map_err(StateError::Money)?
                .raw()
                > policy.max_spread_cents
        }
        // Spread is only evaluated when both touches exist.
        _ => false,
    };

    // The touch we could exit into, per contract.
    let touch = if is_yes {
        book.best_bid().map(|l| Ok(l.price))
    } else {
        book.best_ask().map(|l| {
            PAYOUT_PER_CONTRACT
                .checked_sub(l.price)
                .map_err(StateError::Money)
        })
    };
    let (per_contract, degenerate) = match touch.transpose()? {
        None => return Ok(Mark::degraded()),
        Some(p) if p.raw() < 0 => (Cents::ZERO, true),
        Some(p) => (p, false),
    };

    let value = per_contract.checked_mul(qty).map_err(StateError::Money)?;
    Ok(Mark {
        value,
        wide_flag: stale || too_wide || degenerate,
    })
}
