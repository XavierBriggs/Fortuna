//! FORTUNA news-aggregation source adapters.
//!
//! Design authority: docs/superpowers/specs/2026-06-12-news-aggregation-design.md
//! (implements spec 5.11's adapter layer; the signals store is the seam to
//! everything downstream). This is an IO-edge crate like fortuna-venues:
//! everything here fetches and emits envelopes; nothing here decides.
//! Phase A invariant: NO model anywhere in the ingestion path — enforced
//! at config validation (see `config`), not by convention.

pub mod config;
pub mod error;

pub use config::{EventWindow, ExtractionMode, SourceConfig, SourceKind, SourcesConfig};
pub use error::SourcesError;
