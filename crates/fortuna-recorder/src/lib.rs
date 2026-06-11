//! B0 perishable-data recorder — pure logic (operator amendment A,
//! confirmed 2026-06-11): row shaping, top-of-book derivation, day-file
//! naming. The capture LOOP (network, wall clock) lives in main.rs; this
//! crate half is deterministic and tested.
//!
//! Prices ride as integer TEN-THOUSANDTHS of a dollar (the perps tick,
//! research §3) — never f64. The verbatim wire body is always stored
//! unmodified; derived fields are companions, not replacements.

use serde_json::{json, Value};

/// Parse a fixed-point dollars string ("6.2587", "0.0100", "12") into
/// integer ten-thousandths of a dollar. Refuses negatives, malformed
/// strings, and >4 fractional digits (the venue tick is 1e-4; extra
/// precision would silently truncate, which is data loss, not parsing).
pub fn to_tenthousandths(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || s.starts_with('-') || s.starts_with('+') {
        return None;
    }
    let (whole, frac) = match s.split_once('.') {
        Some((w, f)) => (w, f),
        None => (s, ""),
    };
    if frac.len() > 4 || !whole.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let whole: i64 = whole.parse().ok()?;
    let frac_padded: String = frac.chars().chain(std::iter::repeat('0')).take(4).collect();
    let frac: i64 = if frac_padded.is_empty() {
        0
    } else {
        frac_padded.parse().ok()?
    };
    whole.checked_mul(10_000)?.checked_add(frac)
}

/// Best bid / best ask derived from a perps orderbook response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopOfBook {
    pub best_bid_tenthousandths: i64,
    pub best_ask_tenthousandths: i64,
    pub spread_tenthousandths: i64,
}

/// Derive top-of-book from the VERBATIM `/margin/.../orderbook` body:
/// `{"orderbook":{"bids":[[price,qty],...],"asks":[...]}}`.
///
/// The live capture (research raw/live_prod_orderbook_btc.json) shows
/// ordering that CONTRADICTS the spec text (research §11 conflict), so
/// ordering is never trusted: best bid = numeric max of bids, best ask =
/// numeric min of asks. Levels with qty "0" are ignored. Returns None for
/// an empty or one-sided book (a spread from half a book is a lie) or any
/// unparseable level (refuse, don't guess).
pub fn top_of_book(body: &str) -> Option<TopOfBook> {
    let v: Value = serde_json::from_str(body).ok()?;
    let book = v.get("orderbook")?;
    let side = |name: &str| -> Option<Vec<(i64, i64)>> {
        book.get(name)?
            .as_array()?
            .iter()
            .map(|lvl| {
                let pair = lvl.as_array()?;
                let price = to_tenthousandths(pair.first()?.as_str()?)?;
                let qty = to_tenthousandths(pair.get(1)?.as_str()?)?;
                Some((price, qty))
            })
            .collect()
    };
    let bids = side("bids")?;
    let asks = side("asks")?;
    let best_bid = bids.iter().filter(|(_, q)| *q > 0).map(|(p, _)| *p).max()?;
    let best_ask = asks.iter().filter(|(_, q)| *q > 0).map(|(p, _)| *p).min()?;
    Some(TopOfBook {
        best_bid_tenthousandths: best_bid,
        best_ask_tenthousandths: best_ask,
        spread_tenthousandths: best_ask - best_bid,
    })
}

/// One JSONL row. The wire body rides VERBATIM as a string; `derived` is
/// optional companion data (e.g. top-of-book). `cycle_id` pairs every row
/// captured in the same sweep (perp books <-> bracket quotes pairing).
pub fn capture_row(
    cycle_id: u64,
    captured_at_ms: i64,
    stream: &str,
    key: &str,
    status: u16,
    body: &str,
    derived: Option<Value>,
) -> Value {
    json!({
        "v": 1,
        "cycle_id": cycle_id,
        "captured_at_ms": captured_at_ms,
        "stream": stream,
        "key": key,
        "status": status,
        "body": body,
        "derived": derived,
    })
}

/// UTC day directory name (YYYY-MM-DD) for an epoch-milliseconds stamp.
pub fn day_dir(epoch_ms: i64) -> String {
    let days = epoch_ms.div_euclid(86_400_000);
    // Howard Hinnant's civil_from_days: deterministic, no deps.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // The verbatim live capture from the Phase A research
    // (raw/live_prod_orderbook_btc.json) — asks descend, bids ascend,
    // i.e. the ordering the spec text does NOT describe.
    const LIVE_BTC_BOOK: &str = r#"{"orderbook":{"asks":[["6.2587","16.00"],["6.2586","798.00"],["6.2585","4540.00"],["6.2583","1738.00"],["6.2578","9.00"]],"bids":[["6.2569","40.00"],["6.2573","1.00"],["6.2574","7322.00"],["6.2576","799.00"],["6.2577","7467.00"]]}}"#;

    #[test]
    fn tenthousandths_parses_fixed_point_strings() {
        assert_eq!(to_tenthousandths("6.2587"), Some(62_587));
        assert_eq!(to_tenthousandths("0.0001"), Some(1));
        assert_eq!(to_tenthousandths("0.52"), Some(5_200));
        assert_eq!(to_tenthousandths("12"), Some(120_000));
        assert_eq!(to_tenthousandths("798.00"), Some(7_980_000));
    }

    #[test]
    fn tenthousandths_refuses_garbage_negatives_and_overprecision() {
        assert_eq!(to_tenthousandths(""), None);
        assert_eq!(to_tenthousandths("-1.00"), None);
        assert_eq!(to_tenthousandths("+1.00"), None);
        assert_eq!(to_tenthousandths("1.23456"), None); // 5dp would truncate
        assert_eq!(to_tenthousandths("abc"), None);
        assert_eq!(to_tenthousandths("1.2x"), None);
        assert_eq!(to_tenthousandths("1.2.3"), None);
    }

    #[test]
    fn top_of_book_ignores_wire_ordering() {
        // Best ask is the LAST element of the live asks array; best bid is
        // the LAST element of bids. Numeric selection must find both.
        let tob = top_of_book(LIVE_BTC_BOOK).expect("live book parses");
        assert_eq!(tob.best_bid_tenthousandths, 62_577); // 6.2577
        assert_eq!(tob.best_ask_tenthousandths, 62_578); // 6.2578
        assert_eq!(tob.spread_tenthousandths, 1); // one tick wide
    }

    #[test]
    fn top_of_book_refuses_one_sided_or_empty_books() {
        assert_eq!(
            top_of_book(r#"{"orderbook":{"asks":[],"bids":[["6.25","1.00"]]}}"#),
            None
        );
        assert_eq!(
            top_of_book(r#"{"orderbook":{"asks":[["6.26","1.00"]],"bids":[]}}"#),
            None
        );
        assert_eq!(top_of_book(r#"{"orderbook":{}}"#), None);
        assert_eq!(top_of_book("not json"), None);
    }

    #[test]
    fn top_of_book_skips_zero_quantity_levels() {
        let body = r#"{"orderbook":{"asks":[["6.2570","0.00"],["6.2580","5.00"]],"bids":[["6.2560","2.00"]]}}"#;
        let tob = top_of_book(body).expect("parses");
        // The zero-qty 6.2570 ask must not become best ask.
        assert_eq!(tob.best_ask_tenthousandths, 62_580);
        assert_eq!(tob.best_bid_tenthousandths, 62_560);
    }

    #[test]
    fn top_of_book_refuses_unparseable_levels() {
        let body = r#"{"orderbook":{"asks":[["oops","1.00"]],"bids":[["6.25","1.00"]]}}"#;
        assert_eq!(top_of_book(body), None);
    }

    #[test]
    fn capture_row_shape_is_stable() {
        let row = capture_row(
            7,
            1_781_159_370_164,
            "perp_orderbook",
            "KXBTCPERP",
            200,
            "{}",
            None,
        );
        assert_eq!(row["v"], 1);
        assert_eq!(row["cycle_id"], 7);
        assert_eq!(row["captured_at_ms"], 1_781_159_370_164i64);
        assert_eq!(row["stream"], "perp_orderbook");
        assert_eq!(row["key"], "KXBTCPERP");
        assert_eq!(row["status"], 200);
        assert_eq!(row["body"], "{}");
        assert!(row["derived"].is_null());
    }

    #[test]
    fn day_dir_utc_dates_and_rollover() {
        assert_eq!(day_dir(0), "1970-01-01");
        // 2026-06-11 06:29:30.164 UTC (the taker-fill fixture timestamp).
        assert_eq!(day_dir(1_781_159_370_164), "2026-06-11");
        // One ms before / at the UTC midnight boundary of 2026-06-11.
        let midnight = 1_781_136_000_000; // 2026-06-11T00:00:00Z
        assert_eq!(day_dir(midnight - 1), "2026-06-10");
        assert_eq!(day_dir(midnight), "2026-06-11");
    }
}
