//! T4.3 ROTA slice 2: the daemon shapes the per-view JSON that the slice-1
//! fortuna-ops handlers serve. R2 is binding — fortuna-ops NEVER depends on
//! fortuna-runner; the daemon (which may depend on both) pre-shapes a
//! `serde_json::Value` per view and stuffs it into `DashboardSnapshot.views`.
//!
//! This slice populates the two SAFETY views fully — HEALTH (halt state,
//! latency quantiles, venue errors) and SETTLEMENT (limbo, voids, reversals)
//! — plus the primary scalars for GATES and STREAMS, all from the runner's
//! EXISTING `counters()` / `boards_json()` / halt accessors (zero runner
//! changes). The audit-derived arrays (recent_rejections, recent_watchdog),
//! the recorder filesystem scan, the rejections-by-check breakdown, and the
//! whole MONEY + COGNITION views are LATER slices (they need the R5 pool, a
//! runner read-path accessor, the filesystem, or two new ledger queries).
//! Those are asserted ABSENT here so the contract never fakes a field it
//! cannot yet source honestly. Written red-first against a `views_from` that
//! did not exist.

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
    assert!(v["gates"]["total_rejections"].is_number());
    // rejections_by_check needs a runner read-path accessor; recent_rejections
    // needs the audit pool — both later slices, asserted absent.
    assert!(v["gates"].get("rejections_by_check").is_none());
    assert!(v["gates"].get("recent_rejections").is_none());
    assert!(v["streams"]["venue_api_errors_total"].is_number());
    // The recorder filesystem scan is a later slice (reads data/perishable).
    assert!(v["streams"].get("recorder").is_none());
    // MONEY needs the new boards "account" field; COGNITION needs two new
    // ledger queries — whole-view later slices, never stubbed with fake
    // zeros that would read as "all clear".
    assert!(v.get("money").is_none(), "money is a later slice");
    assert!(v.get("cognition").is_none(), "cognition is a later slice");
}
