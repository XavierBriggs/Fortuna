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

use fortuna_live::views::views_from;
use fortuna_runner::{AuditSink, RunnerError, SimRunner};

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
    // recent_rejections still needs the audit pool — a later slice, absent.
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
async fn money_view_is_the_sim_only_account_subset() {
    // R6 + the r5-pool gate's verifier-endorsed unblock: ship the SIM-ONLY
    // money subset rather than fake the §5 floating/total. settled = venue
    // cash, committed = reserved exposure (both real); floating + total are
    // NULL because the mark loop (their only source) is not exposed — honestly
    // null, never a faked zero. Positions carry the per-market detail.
    let r = ticked_runner(11, 3).await;
    let m = &views_from(&r, GEN)["money"];
    assert_eq!(
        m["basis"], "sim-only",
        "the account block is labeled SIM-ONLY: {m}"
    );
    assert!(m["settled_cents"].is_number(), "settled = venue cash: {m}");
    assert!(
        m["committed_cents"].is_number(),
        "committed = reserved exposure: {m}"
    );
    assert!(
        m["floating_cents"].is_null(),
        "floating deferred — no mark loop, never faked: {m}"
    );
    assert!(
        m["total_cents"].is_null(),
        "total = settled + floating, undefined without floating: {m}"
    );
    let positions = m["positions"].as_array().expect("positions array");
    for p in positions {
        assert!(p["market"].is_string(), "{p}");
        assert!(p.get("yes_qty").is_some(), "§5 names it yes_qty: {p}");
        assert!(p["realized_pnl_cents"].is_number(), "{p}");
    }
}
