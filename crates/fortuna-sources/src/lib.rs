//! FORTUNA news-aggregation source adapters.
//!
//! Design authority: docs/superpowers/specs/2026-06-12-news-aggregation-design.md
//! (implements spec 5.11's adapter layer; the signals store is the seam to
//! everything downstream). This is an IO-edge crate like fortuna-venues:
//! everything here fetches and emits envelopes; nothing here decides.
//! Phase A invariant: NO model anywhere in the ingestion path — enforced
//! at config validation (see `config`), not by convention.

pub mod calendar;
pub mod config;
pub mod corroborate;
pub mod error;
pub mod factory;
pub mod fetch;
pub mod nws;
pub mod nws_climate;
pub mod rss;
pub mod scheduler;
pub mod validate;

pub use calendar::{
    calendar_claimed_time, CalendarFeed, CalendarSource, RELEASE_PRINTED_KIND,
    RELEASE_SCHEDULED_KIND,
};
pub use config::{EventWindow, ExtractionMode, SourceConfig, SourceKind, SourcesConfig};
pub use corroborate::{corroborate, Corroboration, CorroborationInput};
pub use error::SourcesError;
pub use factory::{build_scheduler, FactoryConfig};
pub use fetch::{
    Conditional, FetchCaps, FetchClient, FetchError, FetchOutcome, FetchTransport, HostPin,
    PoliteLimiter, RawHttpResponse, ReqwestFetchTransport,
};
pub use nws::{nws_claimed_time, NwsFeed, NwsSource};
pub use nws_climate::{nws_climate_claimed_time, NwsClimateSource, NWS_CLI_KIND};
pub use rss::{rss_claimed_time, RssSource, RSS_ITEM_KIND};
pub use scheduler::{
    AcceptedSignal, Alert, ClaimedTimeFn, DropReason, Dropped, Health, IngestionScheduler,
    SourceMetrics, SourceSchedule, TickOutcome,
};
pub use validate::{Candidate, StructuralConfig, StructuralValidator, Verdict};
