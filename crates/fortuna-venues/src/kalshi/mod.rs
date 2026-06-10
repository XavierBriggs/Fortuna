//! Kalshi venue adapter (T1.1). **DOC-DERIVED IMPLEMENTATION — read this.**
//!
//! Everything in this module was built from Kalshi's official documentation
//! as captured on 2026-06-10 in
//! `docs/research/venue/kalshi-api-2026-06-10/research.md` (plus the raw
//! OpenAPI/AsyncAPI/page archives under that directory's `raw/`). No venue
//! API behavior was invented; where the docs are ambiguous the code takes
//! the conservative option and the gap is listed in the research doc's
//! **Uncertainties / fixture-confirmation checklist (27 items)**.
//!
//! ## Clearance
//!
//! - **Cleared for Sim development only.** The adapter may be compiled,
//!   unit-tested against the doc-derived samples in
//!   `tests/kalshi_doc_samples/`, and used to develop downstream plumbing.
//! - **NOT cleared for paper or live use.** Operator-recorded fixtures under
//!   `fixtures/kalshi/` must confirm every item of the research doc's
//!   27-item checklist (auth round-trip, 409 duplicate body, cancel
//!   reconcile, cursor semantics, fee/unit typing, status vocabulary, ...)
//!   before any promotion. See also `tests/kalshi_doc_samples/README.md`.
//!
//! ## Layout
//!
//! - [`auth`] — RSA-PSS request signing (research §1).
//! - [`dto`] — serde DTOs for the V2 REST surface + the Side/Action <->
//!   `outcome_side`/`book_side` mapping (research §3-§10).
//! - [`client`] — `KalshiTransport` trait, `ReqwestKalshiTransport` (signs
//!   every request, classifies 429/network/timeout), and the scripted
//!   `MockKalshiTransport` used by tests and later fixture replay.
//! - [`adapter`] — `KalshiVenue: Venue` (markets/book/place/cancel/positions/
//!   balance/open_orders/fills) plus per-fill fee reconciliation against the
//!   per-series fee cache.
//!
//! ## Non-negotiables encoded here
//!
//! - Money is integer cents: Kalshi's fixed-point dollar strings are parsed
//!   with `rust_decimal::Decimal` then `Cents::from_dollars_exact`; a
//!   sub-cent remainder on a `linear_cent` market is a hard error, and
//!   markets with `deci_cent`/`tapered_deci_cent` structures are filtered
//!   out of `markets()` entirely (GAPS rule: integer-cent core).
//! - `place` accepts only `GatedOrder` (I1) and the model never touches this
//!   module (I6).
//! - The V2 cancel response body is treated as advisory only (documented
//!   wrong-order response bug); cancellation is confirmed by a follow-up
//!   GET of the order (research §6).

pub mod adapter;
pub mod auth;
pub mod client;
pub mod dto;

pub use adapter::{FeeReconciliation, KalshiVenue};
pub use auth::{KalshiSigner, SignedHeaders};
pub use client::{
    KalshiTransport, MockKalshiTransport, ReqwestKalshiTransport, KALSHI_DEMO_BASE_URL,
    KALSHI_PROD_BASE_URL,
};
