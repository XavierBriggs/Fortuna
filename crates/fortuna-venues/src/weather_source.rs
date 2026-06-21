//! Venue-neutral `WeatherMarketSource` trait (C1 decoupling).
//!
//! Moved off `kalshi::` so `fortuna-live` can import it without referencing any
//! Kalshi type. The `KalshiWeatherSource` impl stays in `kalshi/weather.rs`;
//! only the TRAIT lives here, next to the other venue-neutral types.

use async_trait::async_trait;
use fortuna_core::market::MarketView;

use crate::VenueError;

/// The day-set source the F7 live plug-in consumes. Given a venue temperature
/// SERIES (e.g. `KXHIGHNY`) and a forecast TARGET DATE (`YYYY-MM-DD`), return
/// the markets that grade on that date as venue-neutral [`MarketView`]s.
///
/// A date with NO live markets yields an empty `Vec` — NOT an error ("a
/// synthesized event with no live market is simply not traded", contract). A
/// transport or response-parse failure is an `Err`: a malformed venue frame is a
/// hard error, never a fabricated market.
#[async_trait]
pub trait WeatherMarketSource: Send + Sync {
    async fn day_set(&self, series: &str, target_date: &str)
        -> Result<Vec<MarketView>, VenueError>;
}
