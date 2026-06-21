//! fortuna-live — the T4.1 live composition daemon (BUILD_PLAN Phase 4).
//!
//! The LIBRARY half is deterministic and test-injected: boot validation is
//! pure functions over caller-supplied maps and strings — nothing in this
//! crate's lib reads the process environment, the filesystem, or a clock.
//! The BINARY (main.rs) is the only place that touches the real world, and
//! it fails closed on anything the boot layer refuses.

pub mod aeolus_venue;
pub mod audit_bridge;
pub mod boot;
pub mod compose;
pub mod daemon;
pub mod funding_poller;
pub mod ingestion;
pub mod perp_feed;
pub mod perp_tick_producer;
pub mod run_loop;
pub mod telemetry;
pub mod views;
