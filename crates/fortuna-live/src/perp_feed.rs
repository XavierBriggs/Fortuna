//! Slice 4e — the Sim-soak PerpTick FEED.
//!
//! The perp strategies (`funding_forecast`, `perp_event_basis`) fire only on
//! `EventPayload::PerpTick`s, and the deterministic Sim loop only sources
//! `BookSnapshot`s from the `SimVenue` — so in a Sim soak the producers are
//! INERT (the slice-4 architectural finding). This feed closes that: it loads
//! RECORDED kinetics WS `ticker` frames (NEVER fabricated — recorded demo
//! captures or the committed fixture) and yields them as `PerpTick` components
//! one-per-segment for `SimRunner::inject_perp_tick` (the slice-4b seam), so
//! `funding_forecast` produces a scalar belief each segment, which slice-4d
//! persists. The build path is exactly the live producer's: a `ticker` frame →
//! [`KineticsPerpObservation::from_ws_ticker`] (slice 4a) → `(market, marks,
//! funding)` + the kinetics venue id.

use fortuna_core::market::{MarketId, VenueId};
use fortuna_core::perp::{FundingObservation, PerpMarks};
use fortuna_venues::kinetics::dto::{self, WsFrame};
use fortuna_venues::kinetics::perp_observation::KineticsPerpObservation;

use crate::daemon::DaemonError;

/// A replayable feed of recorded PerpTicks for a Sim soak. Holds the parsed
/// per-frame components and a cursor that LOOPS when the recording is
/// exhausted (a continuous soak feed).
#[derive(Debug)]
pub struct PerpTickFeed {
    ticks: Vec<(VenueId, MarketId, PerpMarks, FundingObservation)>,
    cursor: usize,
}

impl PerpTickFeed {
    /// Load a `.jsonl` of recorded kinetics WS frames; every `ticker` frame
    /// becomes one PerpTick (other frame kinds are skipped). Fails if the file
    /// is unreadable, a `ticker` frame is malformed, or there are zero ticker
    /// frames (an empty feed is a config error — nothing to feed).
    pub fn from_ws_ticker_jsonl(path: &str) -> Result<Self, DaemonError> {
        let raw = std::fs::read_to_string(path).map_err(|e| DaemonError::Compose {
            reason: format!("perp_feed: cannot read {path}: {e}"),
        })?;
        let venue = VenueId::new("kinetics").map_err(|e| DaemonError::Compose {
            reason: format!("perp_feed venue id: {e}"),
        })?;
        let mut ticks = Vec::new();
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            // Only `ticker` frames carry a PerpTick; everything else (orderbook
            // snapshots/deltas, trades) is skipped. A malformed ticker frame is
            // a hard error (never feed an invented PerpTick).
            if let Ok(WsFrame::Ticker { msg, .. }) = dto::parse_ws_frame(line) {
                let obs = KineticsPerpObservation::from_ws_ticker(&msg).map_err(|e| {
                    DaemonError::Compose {
                        reason: format!("perp_feed: malformed recorded ticker frame: {e}"),
                    }
                })?;
                ticks.push((venue.clone(), obs.market, obs.marks, obs.funding));
            }
        }
        if ticks.is_empty() {
            return Err(DaemonError::Compose {
                reason: format!(
                    "perp_feed: {path} contains zero `ticker` frames — nothing to feed"
                ),
            });
        }
        Ok(PerpTickFeed { ticks, cursor: 0 })
    }

    /// The number of distinct recorded ticks (one soak loop).
    pub fn len(&self) -> usize {
        self.ticks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    /// The next PerpTick components, LOOPING over the recording so the soak
    /// never runs dry. (Constructor guarantees `ticks` is non-empty.)
    pub fn next_tick(&mut self) -> (VenueId, MarketId, PerpMarks, FundingObservation) {
        let i = self.cursor % self.ticks.len();
        self.cursor = self.cursor.wrapping_add(1);
        self.ticks[i].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed kinetics WS capture (recorded, NEVER fabricated): 489
    /// frames, 74 of them `ticker`s (the rest orderbook deltas/snapshots, trades,
    /// `subscribed` acks). The feed keeps ONLY the tickers.
    const TICKER_FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/kinetics-perps/ws__public_orderbook_ticker.jsonl"
    );

    /// The private-lifecycle capture carries order events only — ZERO tickers.
    const TICKERLESS_FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/kinetics-perps/ws__private_lifecycle.jsonl"
    );

    #[test]
    fn loads_one_perptick_per_recorded_ticker_frame_skipping_the_rest() {
        // Proves the WHOLE recorded build path (a `ticker` frame ->
        // KineticsPerpObservation::from_ws_ticker -> components) against real
        // captured data: the 74 ticker frames become 74 PerpTicks; the 415
        // non-ticker frames are skipped, not errored.
        let feed = PerpTickFeed::from_ws_ticker_jsonl(TICKER_FIXTURE).expect("fixture parses");
        assert_eq!(feed.len(), 74, "one PerpTick per recorded ticker frame");
        assert!(!feed.is_empty());
    }

    #[test]
    fn next_tick_stamps_the_kinetics_venue_and_loops_when_exhausted() {
        let mut feed = PerpTickFeed::from_ws_ticker_jsonl(TICKER_FIXTURE).expect("fixture parses");
        let n = feed.len();
        let first = feed.next_tick();
        // The venue crate is bus-free; the FEED stamps the venue id (design §3.2).
        assert_eq!(
            first.0.as_str(),
            "kinetics",
            "the feed assembles the venue id"
        );
        // Draw the remaining n-1, then one more: the cursor WRAPS (a continuous
        // soak feed never runs dry) and re-yields the first tick's market.
        for _ in 1..n {
            feed.next_tick();
        }
        let wrapped = feed.next_tick();
        assert_eq!(wrapped.1, first.1, "cursor wrapped back to the first tick");
    }

    #[test]
    fn zero_ticker_frames_is_a_config_error_not_a_silent_empty_feed() {
        // A capture with no tickers (the lifecycle frames) is REFUSED — an empty
        // feed would silently leave the producers inert, exactly the bug this
        // slice closes. (Fail closed with a precise reason.)
        let err = PerpTickFeed::from_ws_ticker_jsonl(TICKERLESS_FIXTURE).unwrap_err();
        let DaemonError::Compose { reason } = err else {
            panic!("expected a Compose error, got {err:?}");
        };
        assert!(
            reason.contains("zero `ticker` frames"),
            "the reason names the empty-feed cause: {reason}"
        );
    }

    #[test]
    fn missing_file_fails_closed() {
        let err = PerpTickFeed::from_ws_ticker_jsonl("/no/such/recording.jsonl").unwrap_err();
        let DaemonError::Compose { reason } = err else {
            panic!("expected a Compose error, got {err:?}");
        };
        assert!(
            reason.contains("cannot read"),
            "names the read failure: {reason}"
        );
    }
}
