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

use fortuna_core::clock::UtcTimestamp;
use fortuna_exec::IntentJournal;
use fortuna_gates::GateCheck;
use fortuna_runner::SimRunner;
use fortuna_sources::{FunnelCounts, IngestionTelemetry, SignalRecord, SourceTelemetry};
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
    // to total_rejections). §5's "number" is the 1-based spec position: the
    // runner keys rejections by the GateCheck Debug name, so we recover the
    // number by matching that name back to its GateCheck and reading `index()`
    // — NOT a guess (both sides derive from the SAME Debug impl, so they stay
    // in lockstep). An unrecognised key — never produced by the runner — would
    // degrade to a null number rather than a fabricated one.
    let rejections_by_check: Vec<Value> = runner
        .rejections_by_check()
        .into_iter()
        .map(|(check, count)| {
            let number = GateCheck::ALL
                .iter()
                .find(|g| format!("{g:?}") == check)
                .map(|g| g.index());
            json!({ "check": check, "count": count, "number": number })
        })
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

    // Per-strategy P&L (mission item 3): the digest's own attribution — realized
    // PnL, fees, fill count, open exposure per strategy. A read-only fold over the
    // runner's state (the SAME digest the daily Slack report uses), surfaced here
    // for the Strategy P&L board. Unrealized PnL is the mark-loop gap (Money board)
    // — not faked here; this is realized + fees + open exposure only.
    let digest = runner.digest_snapshot();
    let strategy_rows: Vec<Value> = digest
        .strategies
        .iter()
        .map(|s| {
            json!({
                "strategy": s.strategy,
                "realized_pnl_cents": s.realized_pnl_cents,
                "fees_cents": s.fees_cents,
                "fills": s.fills,
                "open_exposure_cents": s.open_exposure_cents,
            })
        })
        .collect();
    let strategy_fills: u64 = digest.strategies.iter().map(|s| s.fills).sum();
    let strategy_count = digest.strategies.len();

    json!({
        "health": {
            "generated_at": generated_at,
            "stage": "sim",
            "halt_active": halt_active,
            "halt_reason": halt_reason,
            // M3 / I2: ROTA's halt is the RUNNING daemon's state (active_halt),
            // which never auto-clears on a re-arm — re-arm is RESTART-GATED. Flag
            // that so the console can warn a re-arm takes effect only on restart;
            // a re-armed-but-still-HALTED ROTA is by-design, not a bug.
            "rearm_requires_restart": halt_active,
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
        "strategies": {
            "generated_at": generated_at,
            "title": "Strategy P&L",
            "columns": [
                {"key":"strategy","label":"Strategy"},
                {"key":"realized_pnl_cents","label":"Realized","cents":true},
                {"key":"fees_cents","label":"Fees","cents":true},
                {"key":"fills","label":"Fills"},
                {"key":"open_exposure_cents","label":"Open exp","cents":true},
            ],
            "rows": strategy_rows,
            "summary": {"strategies": strategy_count, "fills": strategy_fills},
        },
    })
}

/// OBS-2c: merge the LIVE ingestion boards — D-contract V1 Live Feed, V2 Sources
/// Health, V3 Ingest Funnel — into the snapshot's `views`, shaped from the
/// daemon-published `IngestionTelemetry` (track-D OBS-2b handle) into the exact
/// `{title, columns, rows, summary}` board envelopes the ROTA handlers serve
/// verbatim. Pure + clock-free: the caller passes `generated_at` (the daemon's
/// clock read), used for the last-success age. HONEST GATE: a never-published
/// telemetry (empty `generated_at` — ingestion off or pre-first-tick) merges
/// NOTHING, so the boards stay honest-degraded ("unavailable") rather than showing
/// fabricated zeros — which also keeps the daemon's snapshot byte-unchanged when
/// ingestion is off (daemon_smoke). Untrusted signal `summary`/source text is
/// carried as DATA; the ROTA renderer esc()'s it (spec 5.11), never interpreted.
pub fn merge_ingest_views(views: &mut Value, tel: &IngestionTelemetry, generated_at: &str) {
    if tel.generated_at.is_empty() {
        return; // never ticked (ingestion off / pre-first-tick) => leave degraded
    }
    let Some(obj) = views.as_object_mut() else {
        return;
    };
    obj.insert(
        "ingest_sources".to_string(),
        sources_board(&tel.sources, generated_at),
    );
    obj.insert(
        "ingest_feed".to_string(),
        feed_board(&tel.recent, generated_at),
    );
    obj.insert(
        "ingest_funnel".to_string(),
        funnel_board(&tel.funnel, generated_at),
    );
}

/// V2 Sources Health board envelope from the per-source telemetry. `last_ok_age_s`
/// and `empty_rate_pct` are the only derived columns (now − last_success_at;
/// empty_polls·100/polls), null when not computable — never a fabricated 0.
fn sources_board(sources: &[SourceTelemetry], generated_at: &str) -> Value {
    let now_ms = UtcTimestamp::parse_iso8601(generated_at)
        .map(|t| t.epoch_millis())
        .ok();
    let mut rows = Vec::with_capacity(sources.len());
    let (mut healthy, mut degraded, mut quarantined, mut acc_total, mut drop_total) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    for s in sources {
        match s.health {
            "healthy" => healthy += 1,
            "quarantined" => quarantined += 1,
            _ => degraded += 1,
        }
        acc_total += s.accepted;
        drop_total += s.dropped_future + s.dropped_republished + s.dropped_over_volume;
        let last_ok_age_s = match (&s.last_success_at, now_ms) {
            (Some(ts), Some(now)) => UtcTimestamp::parse_iso8601(ts)
                .ok()
                .map(|t| ((now - t.epoch_millis()).max(0)) / 1000),
            _ => None,
        };
        let empty_rate_pct = if s.polls > 0 {
            Some(s.empty_polls * 100 / s.polls)
        } else {
            None
        };
        rows.push(json!({
            "source_id": s.source_id,
            "health": s.health,
            "last_ok_age_s": last_ok_age_s,
            "polls": s.polls,
            "accepted": s.accepted,
            "dropped_future": s.dropped_future,
            "dropped_republished": s.dropped_republished,
            "dropped_over_volume": s.dropped_over_volume,
            "empty_rate_pct": empty_rate_pct,
            "quarantines": s.quarantines,
            "next_due_at": s.next_due_at,
        }));
    }
    json!({
        "title": "Sources Health",
        "generated_at": generated_at,
        "columns": [
            {"key":"source_id","label":"Source"},
            {"key":"health","label":"Health","pill":true},
            {"key":"last_ok_age_s","label":"Last OK"},
            {"key":"polls","label":"Polls"},
            {"key":"accepted","label":"Acc"},
            {"key":"dropped_future","label":"D:fut"},
            {"key":"dropped_republished","label":"D:rep"},
            {"key":"dropped_over_volume","label":"D:vol"},
            {"key":"empty_rate_pct","label":"304%"},
            {"key":"quarantines","label":"Quar"},
            {"key":"next_due_at","label":"Next due"},
        ],
        "rows": rows,
        "summary": {"healthy": healthy, "degraded": degraded, "quarantined": quarantined,
                    "accepted": acc_total, "dropped": drop_total},
    })
}

/// V1 Live Signal Feed board envelope from the recent-signals ring (newest-first,
/// the published ring is already newest-first; show up to FEED_SHOWN).
fn feed_board(recent: &[SignalRecord], generated_at: &str) -> Value {
    const FEED_SHOWN: usize = 50;
    let mut rows = Vec::new();
    let (mut accepted, mut dropped) = (0u64, 0u64);
    for r in recent.iter().take(FEED_SHOWN) {
        if r.status == "accepted" {
            accepted += 1;
        } else {
            dropped += 1;
        }
        rows.push(json!({
            "at": r.at,
            "source_id": r.source_id,
            "kind": r.kind,
            "claimed_time": r.claimed_time,
            "status": r.status,
            "summary": r.summary,
        }));
    }
    let shown = rows.len();
    json!({
        "title": "Live Signal Feed",
        "generated_at": generated_at,
        "columns": [
            {"key":"at","label":"Time (UTC)"},
            {"key":"source_id","label":"Source"},
            {"key":"kind","label":"Kind"},
            {"key":"claimed_time","label":"Claimed"},
            {"key":"status","label":"Status","pill":true},
            {"key":"summary","label":"Data"},
        ],
        "rows": rows,
        "summary": {"window": shown, "accepted": accepted, "dropped": dropped},
    })
}

/// V3 Ingest Funnel board envelope (stage table) from the process-wide counts. The
/// loop-side stages (normalized/persisted) are real once the loop ticks (OBS-2a);
/// the empty-`generated_at` gate above means an unticked funnel is never shown.
fn funnel_board(f: &FunnelCounts, generated_at: &str) -> Value {
    let pct = |n: u64| {
        if f.fetched > 0 {
            n * 100 / f.fetched
        } else {
            0
        }
    };
    json!({
        "title": "Ingest Funnel",
        "generated_at": generated_at,
        "columns": [
            {"key":"stage","label":"Stage"},
            {"key":"count","label":"Count"},
            {"key":"retain_pct","label":"Retain %"},
            {"key":"dropped","label":"Dropped"},
            {"key":"detail","label":"Detail"},
        ],
        "rows": [
            {"stage":"Fetched","count":f.fetched,"retain_pct":100,"dropped":0,
             "detail":"raw items returned by the adapters"},
            {"stage":"Validated","count":f.validated_accepted,"retain_pct":pct(f.validated_accepted),
             "dropped":f.validated_dropped,"detail":"refused by Layer-1 (future / republished / over_volume)"},
            {"stage":"Normalized","count":f.normalized,"retain_pct":pct(f.normalized),"dropped":0,
             "detail":"became SignalEnvelopes"},
            {"stage":"Persisted","count":f.persisted,"retain_pct":pct(f.persisted),"dropped":f.deduped,
             "detail":format!("deduped {} · persist_failures {}", f.deduped, f.persist_failures)},
        ],
        "summary": {"fetched": f.fetched, "persisted": f.persisted,
                    "retain_pct": pct(f.persisted), "persist_failures": f.persist_failures},
    })
}
