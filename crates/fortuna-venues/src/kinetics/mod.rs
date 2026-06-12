//! Kinetics (Kalshi margin/perps) venue adapter — spec 5.15, T5.B4.
//!
//! FIXTURES-GATED against the operator-recorded captures in
//! fixtures/kinetics-perps/ (demo environment; see SESSION-NOTES.md
//! there for the load-bearing wire findings). Never invent venue
//! behavior beyond the captures + docs/research/venue/
//! kinetics-perps-2026-06-10/.
//!
//! Slice 1 (this module today): typed DTOs for the full recorded REST
//! surface + the three error-body shapes + the WS frame envelope, with a
//! 100%-fixture-coverage parse suite (tests/kinetics_dto.rs). The REST
//! client/transport, signing (same RSA-PSS recipe as the event API under
//! /trade-api/v2/margin/*, asymmetric skew window — findings 3/4), and
//! the adapter proper land in the next slices; the order path will
//! accept only `GatedPerpOrder` (type-level I1).

pub mod dto;
