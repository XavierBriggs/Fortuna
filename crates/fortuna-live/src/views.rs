//! T4.3 ROTA slice 2 — daemon-side view shaping.
//!
//! R2 (binding amendment) is the reason this lives in fortuna-live, not
//! fortuna-ops: the dashboard crate MUST NOT depend on fortuna-runner, so it
//! cannot shape runner state itself. The daemon — which composes both — does
//! the shaping and hands fortuna-ops a finished `serde_json::Value` per view
//! via `DashboardSnapshot.views`; the slice-1 rota handlers serve that
//! verbatim (or "unavailable" when a key is absent).
//!
//! `views_from` is a PURE read over the runner's existing accessors
//! (`counters()`, `boards_json()`, `active_halt()`) — no clock, no IO, no
//! money path — so the metrics between-segments closure can call it under a
//! non-blocking `try_write`. It is panic-free by construction (no unwrap;
//! missing board keys degrade to conservative defaults).
//!
//! POPULATED THIS SLICE: `health` (halt state, fill-latency quantiles, venue
//! error count) and `settlement` (capital in limbo, overdue, voids,
//! reversals) — the two SAFETY panels — plus the primary scalars of `gates`
//! (total rejections) and `streams` (venue API errors).
//!
//! DELIBERATELY ABSENT (each needs a capability this slice lacks; a faked
//! value reads to an operator as "all clear", so we emit NOTHING rather than
//! a zero we cannot stand behind):
//!   - `money`: now the SIM-ONLY subset (R6) — settled=cash, committed=
//!     reserved from the boards "account" block + positions; floating/total
//!     stay NULL until the mark loop is exposed (their only source).
//!   - `cognition`: needs `BeliefsRepo::recent` + calibration-scope
//!     enumeration — two new ledger queries (R7).
//!   - `gates.recent_rejections` / `settlement.recent_watchdog_events`: the
//!     recent-event tails need the R5 dedicated audit pool + a query.
//!     (`gates.rejections_by_check` is now POPULATED via the new
//!     `SimRunner::rejections_by_check()` accessor — sorted {check, count},
//!     summing to total_rejections; §5's per-check "number" is omitted as the
//!     runner keys by check name only.)
//!   - `streams.recorder` + per-venue `book_age_ms`: the recorder filesystem
//!     scan and the NEW boards book-age field (later slices).
//!   - `health.last_tick_age_ms`: no last-tick wall stamp is tracked — null,
//!     never a fabricated age.

use fortuna_exec::IntentJournal;
use fortuna_runner::SimRunner;
use serde_json::{json, Value};

/// Shape the counter/board-derived ROTA views from the runner's existing
/// read accessors. The result is the `DashboardSnapshot.views` payload; keys
/// not yet sourceable are omitted (the handler then reports them unavailable).
///
/// `generated_at` is supplied by the caller (the binary's between-segments
/// closure, which holds the runner's injected clock) and stamped into every
/// view per the §5 contract — keeping this function pure and clock-free so
/// the library never reads a clock and the unit tests stay deterministic.
pub fn views_from<J: IntentJournal + Send>(runner: &SimRunner<J>, generated_at: &str) -> Value {
    let c = runner.counters();
    let boards = runner.boards_json();
    let ops = &boards["ops"];

    // Conservative quantile read: null when nothing was observed (never 0,
    // which would falsely claim a measured sub-millisecond latency).
    let quant = |p: f64| match c.fill_latency.quantile_ms(p) {
        Some(ms) => json!(ms),
        None => Value::Null,
    };

    let (halt_active, halt_reason) = match runner.active_halt() {
        Some(reason) => (true, Value::String(reason)),
        None => (false, Value::Null),
    };

    // boards uses a -1 sentinel for "no settlements pending" — that is zero
    // capital in limbo, not negative cents.
    let limbo = ops["capital_in_limbo_cents"].as_i64().unwrap_or(0).max(0);
    let overdue = ops["settlements_overdue"].as_u64().unwrap_or(0);

    // The per-check gate-rejection breakdown (sorted by check name; counts sum
    // to total_rejections). §5's "number" field is omitted: the runner keys by
    // check NAME only, so a fabricated gate number would be a guess.
    let rejections_by_check: Vec<Value> = runner
        .rejections_by_check()
        .into_iter()
        .map(|(check, count)| json!({ "check": check, "count": count }))
        .collect();

    // SIM-ONLY money account (R6): settled = cash, committed = reserved (both
    // real, from the boards "account" block); floating + total are NULL (the
    // mark loop is their only source and is not exposed — never faked). The
    // positions are reshaped from the boards' yes/no to §5's yes_qty/no_qty.
    let account = &boards["account"];
    let money_positions: Vec<Value> = boards["positions"]
        .as_array()
        .map(|ps| {
            ps.iter()
                .map(|p| {
                    json!({
                        "market": p["market"],
                        "yes_qty": p["yes"],
                        "no_qty": p["no"],
                        "realized_pnl_cents": p["realized_pnl_cents"],
                        "fees_cents": p["fees_cents"],
                        "lifecycle": p["lifecycle"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    json!({
        "health": {
            "generated_at": generated_at,
            "stage": "sim",
            "halt_active": halt_active,
            "halt_reason": halt_reason,
            "ticks_total": c.ticks,
            "last_tick_age_ms": Value::Null,
            "fill_latency_p90_ms": quant(0.90),
            "fill_latency_p95_ms": quant(0.95),
            "fill_latency_p99_ms": quant(0.99),
            "dead_man_last_ping_age_secs": Value::Null,
            "venues": [ {
                "id": "sim",
                "healthy": c.venue_api_errors == 0,
                "api_error_count": c.venue_api_errors,
            } ],
        },
        "settlement": {
            "generated_at": generated_at,
            "capital_in_limbo_cents": limbo,
            "settlements_overdue": overdue,
            "settlement_voids_total": c.settlement_voids,
            "settlement_reversals_total": c.settlement_reversals,
        },
        "money": {
            "generated_at": generated_at,
            // SIM-ONLY: a live venue's settled/floating model is the full
            // §5 contract; until the mark loop is exposed, total + floating
            // are null and the basis is labelled so an operator never reads
            // this as the complete picture.
            "basis": "sim-only",
            "settled_cents": account["cash_cents"].clone(),
            "committed_cents": account["reserved_cents"].clone(),
            "floating_cents": Value::Null,
            "total_cents": Value::Null,
            "positions": money_positions,
        },
        "gates": {
            "generated_at": generated_at,
            "total_rejections": c.gate_rejections,
            "rejections_by_check": rejections_by_check,
        },
        "streams": {
            "generated_at": generated_at,
            "venue_api_errors_total": c.venue_api_errors,
            "venues": [ {
                "id": "sim",
                // book age needs the NEW boards field (later slice).
                "book_age_ms": Value::Null,
                // WS gap/resync render stub 0 until T4.2 ships the dial
                // (design §4) — a documented stub, not a faked measurement.
                "ws_gap_count": 0,
                "resync_count": 0,
            } ],
        },
    })
}
