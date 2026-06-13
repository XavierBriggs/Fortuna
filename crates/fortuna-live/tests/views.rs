//! T4.3 ROTA slice 2: the daemon shapes the per-view JSON that the slice-1
//! fortuna-ops handlers serve. R2 is binding — fortuna-ops NEVER depends on
//! fortuna-runner; the daemon (which may depend on both) pre-shapes a
//! `serde_json::Value` per view and stuffs it into `DashboardSnapshot.views`.
//!
//! This slice populates the two SAFETY views fully — HEALTH (halt state,
//! latency quantiles, venue errors) and SETTLEMENT (limbo, voids, reversals)
//! — plus the primary scalars for GATES and STREAMS, all from the runner's
//! EXISTING `counters()` / `boards_json()` / halt accessors (zero runner
//! changes). LATER slices extended this file: the gates rejections-by-check
//! breakdown (slice 6) and the SIM-ONLY money subset (slice 7) are now
//! populated + tested below. COGNITION and the audit-derived arrays
//! (recent_rejections, recent_watchdog) remain LATER slices (new ledger
//! queries / the audit pool) and are asserted ABSENT so the contract never
//! fakes a field it cannot yet source honestly. Each view was written
//! red-first against a `views_from` that did not yet emit it.

use fortuna_live::views::{merge_ingest_views, views_from};
use fortuna_runner::{AuditSink, RunnerError, SimRunner};
use fortuna_sources::{
    FunnelCounts, IngestionTelemetry, SignalRecord, SourceTelemetry, TickTelemetry,
};
use fortuna_venues::sim::FaultConfig;

mod common;
use common::{runner_config, set_arb_books, strategy, t0};

#[derive(Default)]
struct NullSink;
impl AuditSink for NullSink {
    fn append(
        &mut self,
        _kind: &str,
        _ref_id: Option<&str>,
        _payload: serde_json::Value,
    ) -> Result<(), RunnerError> {
        Ok(())
    }
}

async fn ticked_runner(seed: u64, ticks: u32) -> SimRunner {
    let mut r = SimRunner::new(
        runner_config(seed),
        vec![strategy()],
        Box::new(NullSink),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);
    for _ in 0..ticks {
        r.tick().await.unwrap();
    }
    r
}

const GEN: &str = "2026-06-11T12:00:30.000Z";

#[tokio::test]
async fn health_view_is_fully_shaped_from_counters_and_halt_state() {
    let r = ticked_runner(7, 3).await;
    let v = views_from(&r, GEN);
    let h = &v["health"];
    // §5: every view carries the snapshot freshness stamp, caller-supplied.
    assert_eq!(h["generated_at"], GEN);
    assert_eq!(h["stage"], "sim");
    assert_eq!(h["halt_active"], false);
    assert!(h["halt_reason"].is_null(), "no halt => null reason");
    assert_eq!(h["ticks_total"], 3, "{h}");
    // R6 (binding amendment): the runner exports p90/p95/p99 ONLY — there is
    // no p50 to add. The key MUST NOT appear.
    assert!(
        h.get("fill_latency_p50_ms").is_none(),
        "R6: no p50 field exists"
    );
    for k in [
        "fill_latency_p90_ms",
        "fill_latency_p95_ms",
        "fill_latency_p99_ms",
    ] {
        assert!(h.get(k).is_some(), "{k} key present (number or null)");
    }
    // Gate note-6 reconciliation: the dead-man landed as a closure-owned
    // task with no Arc<AtomicI64> seam, so ROTA v1 reports null (external).
    assert!(
        h["dead_man_last_ping_age_secs"].is_null(),
        "note-6: dead-man age is null (capability absent)"
    );
    // No last-tick stamp is tracked by the runner today — honestly null,
    // never a fabricated age.
    assert!(h["last_tick_age_ms"].is_null());
    let venues = h["venues"].as_array().expect("venues array");
    assert_eq!(venues.len(), 1);
    assert_eq!(venues[0]["id"], "sim");
    assert!(venues[0]["healthy"].is_boolean());
    assert!(venues[0]["api_error_count"].is_number());
}

#[tokio::test]
async fn an_external_halt_surfaces_in_the_health_view_with_its_reason() {
    let mut r =
        SimRunner::new(runner_config(8), vec![strategy()], Box::new(NullSink), t0()).unwrap();
    r.apply_external_halt("drawdown breach (test)");
    let h = &views_from(&r, GEN)["health"];
    assert_eq!(h["halt_active"], true);
    // global_halted() returns exactly what the gate stores — the view must
    // surface that verbatim, not a paraphrase.
    assert_eq!(h["halt_reason"], "halt poll: drawdown breach (test)");
}

#[tokio::test]
async fn a_halted_health_view_flags_that_rearm_requires_a_restart() {
    // M3: ROTA's halt indicator is the RUNNING daemon's state (active_halt),
    // which NEVER auto-clears on a re-arm — I2 is restart-gated. The health view
    // must carry that fact so the console can warn that a re-arm takes effect
    // only on restart; otherwise a re-armed-but-still-HALTED ROTA reads as a bug
    // (the four-state divergence in runbooks/halt-and-rearm.md). A clear daemon
    // must NOT flag it (no false "restart needed").
    let clear = views_from(&ticked_runner(3, 2).await, GEN);
    assert_eq!(
        clear["health"]["halt_active"], false,
        "precondition: not halted"
    );
    assert_ne!(
        clear["health"]["rearm_requires_restart"],
        serde_json::json!(true),
        "a clear daemon must not claim a restart is required: {}",
        clear["health"]
    );

    let mut r =
        SimRunner::new(runner_config(8), vec![strategy()], Box::new(NullSink), t0()).unwrap();
    r.apply_external_halt("drawdown breach (test)");
    let h = &views_from(&r, GEN)["health"];
    assert_eq!(h["halt_active"], true, "precondition: halted");
    assert_eq!(
        h["rearm_requires_restart"], true,
        "a halted running daemon must flag that a re-arm takes effect only on restart: {h}"
    );
}

#[tokio::test]
async fn settlement_view_carries_limbo_voids_and_reversals() {
    let r = ticked_runner(9, 3).await;
    let s = &views_from(&r, GEN)["settlement"];
    assert!(s["capital_in_limbo_cents"].is_number());
    assert!(s["settlements_overdue"].is_number());
    assert_eq!(s["settlement_voids_total"], 0);
    assert_eq!(s["settlement_reversals_total"], 0);
    // Audit-derived recents are a later (R5 pool) slice — ABSENT, not faked.
    assert!(s.get("recent_watchdog_events").is_none());
    // discrepancies_open is a ledger open_count query (deferred). The
    // lifetime counter is NOT mislabeled as "open" here.
    assert!(s.get("discrepancies_open").is_none());
}

#[tokio::test]
async fn gates_and_streams_carry_scalars_arrays_and_other_views_deferred() {
    let v = views_from(&ticked_runner(10, 2).await, GEN);
    let total = v["gates"]["total_rejections"]
        .as_u64()
        .expect("total is a number");
    // rejections_by_check is the per-check breakdown (runner read-path
    // accessor). Each entry is {check, count}; the counts MUST sum to the
    // total (a consistency invariant that holds for any run, including zero).
    let by_check = v["gates"]["rejections_by_check"]
        .as_array()
        .expect("rejections_by_check is an array");
    let mut sum = 0u64;
    for e in by_check {
        assert!(e["check"].is_string(), "each entry names its check: {e}");
        sum += e["count"].as_u64().expect("count is a number");
    }
    assert_eq!(
        sum, total,
        "the by-check breakdown sums to total_rejections"
    );
    // The sum==total invariant holds for ANY run; gates_rejections_by_check_is_
    // non_vacuous_on_a_rejecting_run pins the NON-ZERO, real-rejection case.
    assert!(v["gates"].get("recent_rejections").is_none());
    assert!(v["streams"]["venue_api_errors_total"].is_number());
    // The recorder filesystem scan is a later slice (reads data/perishable).
    assert!(v["streams"].get("recorder").is_none());
    // COGNITION needs two new ledger queries — a later slice, never stubbed
    // with fake zeros that would read as "all clear". (MONEY is now the
    // SIM-ONLY subset — see money_view_is_the_sim_only_account_subset.)
    assert!(v.get("cognition").is_none(), "cognition is a later slice");
}

#[tokio::test]
async fn gates_rejections_by_check_is_non_vacuous_on_a_rejecting_run() {
    // r5test-slice6 gate finding #1: the "sum == total" invariant is VACUOUS
    // when total is zero (the arb run produces no rejections, so a stubbed/empty
    // accessor passes). Force REAL rejections with an unreachable net-edge floor
    // and assert a NON-EMPTY breakdown summing to a NON-ZERO total — now an empty
    // or fabricated accessor FAILS.
    let mut cfg = runner_config(20);
    cfg.gate_config = toml::from_str(
        "[global]\n\
         max_total_exposure_cents = 800000\n\
         max_daily_loss_cents = 50000\n\
         min_order_contracts = 1\n\
         max_order_contracts = 1000\n\
         price_band_cents = 45\n\
         max_cross_cents = 10\n\
         per_market_exposure_cents = 100000\n\
         per_event_exposure_cents = 150000\n\
         require_event_mapping = false\n\
         [per_strategy.mech_structural]\n\
         max_exposure_cents = 200000\n\
         max_order_notional_cents = 10000\n\
         min_net_edge_bps = 100000\n\
         [rate.sim]\n\
         burst = 100\n\
         sustained_per_min = 600\n\
         market_burst = 50\n\
         market_sustained_per_min = 300\n",
    )
    .unwrap();
    let mut r = SimRunner::new(cfg, vec![strategy()], Box::new(NullSink), t0()).unwrap();
    set_arb_books(&r);
    for _ in 0..3 {
        r.tick().await.unwrap();
    }

    let v = views_from(&r, GEN);
    let total = v["gates"]["total_rejections"].as_u64().unwrap();
    assert!(
        total > 0,
        "an unreachable edge floor must REJECT real orders: {v}"
    );
    let by_check = v["gates"]["rejections_by_check"].as_array().unwrap();
    assert!(
        !by_check.is_empty(),
        "non-empty breakdown on a rejecting run: {v}"
    );
    let sum: u64 = by_check.iter().map(|e| e["count"].as_u64().unwrap()).sum();
    assert_eq!(
        sum, total,
        "the by-check breakdown sums to the NON-ZERO total"
    );
    // each entry names a real check.
    for e in by_check {
        assert!(e["check"].is_string(), "{e}");
        assert!(e["count"].as_u64().unwrap() >= 1, "{e}");
    }
}

#[tokio::test]
async fn gates_rejection_view_carries_the_spec_gate_number() {
    // r5test-slice6 gate finding #3: the old rationale claimed §5's "number"
    // field "would be a guess" because the runner keys rejections by check NAME.
    // That is FALSE — the names ARE the GateCheck Debug variants and
    // GateCheck::index() gives the exact 1-based spec position, so the number is
    // RECOVERABLE, never guessed. An unreachable net-edge floor rejects at the
    // EdgeFloor gate (spec position 6); assert the view carries that number.
    let mut cfg = runner_config(20);
    cfg.gate_config = toml::from_str(
        "[global]\n\
         max_total_exposure_cents = 800000\n\
         max_daily_loss_cents = 50000\n\
         min_order_contracts = 1\n\
         max_order_contracts = 1000\n\
         price_band_cents = 45\n\
         max_cross_cents = 10\n\
         per_market_exposure_cents = 100000\n\
         per_event_exposure_cents = 150000\n\
         require_event_mapping = false\n\
         [per_strategy.mech_structural]\n\
         max_exposure_cents = 200000\n\
         max_order_notional_cents = 10000\n\
         min_net_edge_bps = 100000\n\
         [rate.sim]\n\
         burst = 100\n\
         sustained_per_min = 600\n\
         market_burst = 50\n\
         market_sustained_per_min = 300\n",
    )
    .unwrap();
    let mut r = SimRunner::new(cfg, vec![strategy()], Box::new(NullSink), t0()).unwrap();
    set_arb_books(&r);
    for _ in 0..3 {
        r.tick().await.unwrap();
    }

    let v = views_from(&r, GEN);
    let by_check = v["gates"]["rejections_by_check"].as_array().unwrap();
    assert!(
        !by_check.is_empty(),
        "a rejecting run must populate the breakdown: {v}"
    );
    // EVERY entry carries its real 1-based spec position (1..=10) — never null,
    // never a fabricated guess.
    for e in by_check {
        let n = e["number"]
            .as_u64()
            .unwrap_or_else(|| panic!("rejection entry is missing its spec gate number: {e}"));
        assert!(
            (1..=10).contains(&n),
            "spec gate number must be 1..=10: {e}"
        );
    }
    // The unreachable net-edge floor rejects at EdgeFloor — spec gate 6.
    let edge = by_check
        .iter()
        .find(|e| e["check"] == "EdgeFloor")
        .unwrap_or_else(|| panic!("expected an EdgeFloor rejection on this run: {v}"));
    assert_eq!(
        edge["number"].as_u64().unwrap(),
        6,
        "EdgeFloor is spec gate 6: {edge}"
    );
}

#[tokio::test]
async fn money_view_is_the_sim_only_account_subset() {
    // R6 + the r5-pool gate's verifier-endorsed unblock: ship the SIM-ONLY
    // money subset rather than fake the §5 floating/total. settled = venue
    // cash, committed = reserved exposure (both real); floating + total are
    // NULL because the mark loop (their only source) is not exposed — honestly
    // null, never a faked zero. Positions carry the per-market detail.
    //
    // r5test-slice6 gate finding #4 (vacuous-test class, 2nd occurrence): the
    // shape-only `is_number()`/`is_some()` asserts here passed under a
    // fabricated/zeroed panel. Pin the REAL ground truth of the 11/3 arb seed
    // instead. The three legs fill at their asks (25/28/30 cents) for 50 each:
    // notional 50*(25+28+30) = 4150, taker fees 66+71+74 = 211, so the venue
    // spends 4361 and cash falls 1_000_000 -> 995_639. committed is 0 because
    // every leg FILLED (nothing rests); the non-zero committed case is pinned
    // by money_view_committed_is_non_zero_when_capital_is_reserved below.
    let r = ticked_runner(11, 3).await;
    let m = &views_from(&r, GEN)["money"];
    assert_eq!(m["basis"], "sim-only", "labeled SIM-ONLY: {m}");
    assert_eq!(
        m["settled_cents"], 995_639,
        "settled = venue cash after the 4361 spent on fills: {m}"
    );
    assert_eq!(
        m["committed_cents"], 0,
        "every leg filled, so nothing is reserved: {m}"
    );
    assert!(
        m["floating_cents"].is_null(),
        "floating deferred — no mark loop, never faked: {m}"
    );
    assert!(
        m["total_cents"].is_null(),
        "total = settled + floating, undefined without floating: {m}"
    );
    // The three bracket legs, keyed by market (order-independent): 50 contracts
    // each, the exact per-leg taker fee, no realized pnl yet (none settled).
    let positions = m["positions"].as_array().expect("positions array");
    let by_market: std::collections::BTreeMap<&str, &serde_json::Value> = positions
        .iter()
        .map(|p| (p["market"].as_str().expect("market is a string"), p))
        .collect();
    assert_eq!(by_market.len(), 3, "all three legs present: {m}");
    for (market, fees) in [("BKT-LO", 66), ("BKT-MID", 71), ("BKT-HI", 74)] {
        let p = by_market
            .get(market)
            .unwrap_or_else(|| panic!("{market} missing: {m}"));
        assert_eq!(p["yes_qty"], 50, "{market} filled 50 YES: {p}");
        assert_eq!(p["fees_cents"], fees, "{market} taker fee: {p}");
        assert_eq!(p["realized_pnl_cents"], 0, "{market} unsettled: {p}");
    }
}

#[tokio::test]
async fn money_view_committed_is_non_zero_when_capital_is_reserved() {
    // r5test-slice6 gate finding #4: committed_cents (= venue reserved) must be
    // asserted NON-ZERO at least once, or a fabricated zero passes silently.
    // Inject an ack delay (FaultConfig doc: "accepted but processes only at the
    // next tick") so the three arb legs are PLACED and reserve their worst-case
    // cash but never fill — settled stays at the full starting cash while
    // committed holds the 4361 reservation and no position is booked.
    let mut cfg = runner_config(11);
    cfg.faults = FaultConfig {
        ack_delay_pm: 1000,
        ..FaultConfig::none(11)
    };
    let mut r = SimRunner::new(cfg, vec![strategy()], Box::new(NullSink), t0()).unwrap();
    set_arb_books(&r);
    r.tick().await.unwrap();

    let m = &views_from(&r, GEN)["money"];
    assert_eq!(m["basis"], "sim-only", "{m}");
    let committed = m["committed_cents"]
        .as_i64()
        .expect("committed is a number");
    assert!(
        committed > 0,
        "capital is reserved, committed must be > 0: {m}"
    );
    assert_eq!(
        committed, 4361,
        "exact worst-case reservation of the three legs: {m}"
    );
    assert_eq!(
        m["settled_cents"], 1_000_000,
        "nothing filled while reserved, so cash is untouched: {m}"
    );
    assert!(
        m["positions"]
            .as_array()
            .expect("positions array")
            .is_empty(),
        "orders are reserved but unfilled — no position booked yet: {m}"
    );
}

/// OBS-2c: a representative live ingestion snapshot (one source, one signal, a
/// funnel) for the merge tests.
fn sample_telemetry() -> IngestionTelemetry {
    IngestionTelemetry {
        generated_at: "2026-06-13T13:00:00.000Z".to_string(),
        sources: vec![SourceTelemetry {
            source_id: "nws_alerts".to_string(),
            kind: "nws.alert".to_string(),
            domain_tags: vec!["weather".to_string()],
            trust_tier: 1,
            health: "healthy",
            last_poll_at: Some("2026-06-13T12:59:50.000Z".to_string()),
            last_success_at: Some("2026-06-13T12:59:48.000Z".to_string()),
            next_due_at: Some("2026-06-13T13:00:30.000Z".to_string()),
            polls: 420,
            empty_polls: 360,
            fetch_errors: 0,
            accepted: 58,
            dropped_future: 3,
            dropped_republished: 11,
            dropped_over_volume: 0,
            quarantines: 0,
            rearms: 0,
            last_error: None,
        }],
        funnel: FunnelCounts {
            fetched: 1240,
            validated_accepted: 1052,
            validated_dropped: 188,
            normalized: 1052,
            deduped: 4,
            persisted: 1048,
            persist_failures: 0,
        },
        recent: vec![SignalRecord {
            at: "2026-06-13T12:59:48.000Z".to_string(),
            source_id: "nws_alerts".to_string(),
            kind: "nws.alert".to_string(),
            claimed_time: Some("2026-06-13T12:59:40.000Z".to_string()),
            status: "accepted".to_string(),
            summary: "Severe Thunderstorm Warning — Kings County NY".to_string(),
        }],
        last_tick: TickTelemetry::default(),
    }
}

// OBS-2c: the live-ingestion shaping produces the EXACT board envelopes the ROTA
// handlers serve + the renderer renders (so live daemon data renders identically
// to the screenshot-verified harness seeds). POPULATED-path — real rows + the
// daemon-side derivations (last-OK age, 304-rate, retention %).
#[test]
fn merge_ingest_views_shapes_the_three_live_boards_to_the_handler_envelopes() {
    let tel = sample_telemetry();
    let mut views = serde_json::json!({ "health": { "x": 1 } });
    merge_ingest_views(&mut views, &tel, "2026-06-13T13:00:00.000Z");
    // Existing views are preserved.
    assert_eq!(views["health"]["x"], 1, "{views}");

    // V2 Sources Health.
    let src = &views["ingest_sources"];
    assert_eq!(src["title"], "Sources Health");
    assert_eq!(src["rows"][0]["source_id"], "nws_alerts");
    assert_eq!(src["rows"][0]["health"], "healthy");
    assert_eq!(src["rows"][0]["dropped_over_volume"], 0);
    // Derived daemon-side: last_ok_age_s = 12s (13:00:00 − 12:59:48);
    // empty_rate_pct = 360·100/420 = 85 (integer).
    assert_eq!(src["rows"][0]["last_ok_age_s"], 12, "{src}");
    assert_eq!(src["rows"][0]["empty_rate_pct"], 85, "{src}");
    assert_eq!(src["summary"]["healthy"], 1);
    let cols = src["columns"].as_array().unwrap();
    assert!(
        cols.iter()
            .any(|c| c["key"] == "health" && c["pill"] == true),
        "health column is a pill so the renderer colors it: {src}"
    );

    // V1 Live Signal Feed (untrusted summary carried verbatim as DATA).
    let feed = &views["ingest_feed"];
    assert_eq!(feed["rows"][0]["status"], "accepted");
    assert_eq!(
        feed["rows"][0]["summary"],
        "Severe Thunderstorm Warning — Kings County NY"
    );
    assert_eq!(feed["summary"]["accepted"], 1);

    // V3 Ingest Funnel — retention from the real counts (1048·100/1240 = 84).
    let funnel = &views["ingest_funnel"];
    assert_eq!(funnel["rows"][0]["stage"], "Fetched");
    assert_eq!(
        funnel["rows"][1]["dropped"], 188,
        "validate-stage drop: {funnel}"
    );
    assert_eq!(funnel["summary"]["persisted"], 1048);
    assert_eq!(funnel["summary"]["retain_pct"], 84, "{funnel}");
}

// OBS-2c honesty gate: an empty (Default) telemetry — ingestion off or
// pre-first-tick — merges NOTHING, so the boards stay honest-degraded
// ("unavailable"), never fabricated zeros. This is also why the daemon snapshot
// is byte-unchanged when ingestion is off (daemon_smoke).
#[test]
fn merge_ingest_views_is_inert_when_ingestion_never_ticked() {
    let tel = IngestionTelemetry::default();
    assert_eq!(tel.generated_at, "", "default telemetry carries no stamp");
    let mut views = serde_json::json!({ "health": { "x": 1 } });
    merge_ingest_views(&mut views, &tel, "2026-06-13T13:00:00.000Z");
    assert!(
        views.get("ingest_sources").is_none(),
        "no fabricated board when ingestion is off: {views}"
    );
    assert!(views.get("ingest_feed").is_none(), "{views}");
    assert!(views.get("ingest_funnel").is_none(), "{views}");
    assert_eq!(views["health"]["x"], 1, "existing views untouched: {views}");
}
