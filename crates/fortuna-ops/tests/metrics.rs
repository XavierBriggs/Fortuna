//! T1.5: the deterministic metrics registry + Prometheus text exposition
//! renderer (spec Section 8; research docs/research/ops/otel-rust-2026-06-10:
//! exposition format 0.0.4 is the stable scrape surface; OTLP is the
//! documented upgrade path).
//!
//! Doctrine under test:
//! - The registry is deterministic state: BTreeMap ordering, no clocks, no
//!   globals; same updates => byte-identical render.
//! - Counters are monotone (negative increments are errors, never silent).
//! - The render is valid exposition format: # HELP / # TYPE once per
//!   family, label values escaped, line-feed terminated.
//!
//! Written BEFORE src/metrics.rs per the repository TDD doctrine.

use fortuna_ops::metrics::MetricsRegistry;

#[test]
fn render_is_valid_exposition_format_and_sorted() {
    let mut m = MetricsRegistry::new();
    m.describe_counter("fortuna_fills_total", "Fills applied to the books");
    m.describe_gauge("fortuna_exposure_cents", "Open exposure, worst case");
    m.inc_counter("fortuna_fills_total", &[("venue", "sim")], 3)
        .unwrap();
    m.inc_counter("fortuna_fills_total", &[("venue", "kalshi")], 2)
        .unwrap();
    m.set_gauge(
        "fortuna_exposure_cents",
        &[("strategy", "mech_extremes")],
        12_345,
    );

    let out = m.render_prometheus();
    let expected = "\
# HELP fortuna_exposure_cents Open exposure, worst case\n\
# TYPE fortuna_exposure_cents gauge\n\
fortuna_exposure_cents{strategy=\"mech_extremes\"} 12345\n\
# HELP fortuna_fills_total Fills applied to the books\n\
# TYPE fortuna_fills_total counter\n\
fortuna_fills_total{venue=\"kalshi\"} 2\n\
fortuna_fills_total{venue=\"sim\"} 3\n";
    assert_eq!(out, expected);
}

#[test]
fn counters_are_monotone_and_accumulate() {
    let mut m = MetricsRegistry::new();
    m.inc_counter("c_total", &[], 1).unwrap();
    m.inc_counter("c_total", &[], 4).unwrap();
    assert!(m.render_prometheus().contains("c_total 5\n"));
    assert!(
        m.inc_counter("c_total", &[], -1).is_err(),
        "a negative increment is an error, never a silent decrease"
    );
}

#[test]
fn gauges_overwrite_and_go_negative() {
    let mut m = MetricsRegistry::new();
    m.set_gauge("g", &[], 10);
    m.set_gauge("g", &[], -7);
    assert!(m.render_prometheus().contains("g -7\n"));
}

#[test]
fn label_values_escape_quotes_backslashes_and_newlines() {
    let mut m = MetricsRegistry::new();
    m.set_gauge("g", &[("reason", "a\"b\\c\nd")], 1);
    assert!(m
        .render_prometheus()
        .contains("g{reason=\"a\\\"b\\\\c\\nd\"} 1\n"));
}

#[test]
fn same_updates_render_byte_identically() {
    let build = || {
        let mut m = MetricsRegistry::new();
        m.describe_counter("a_total", "a");
        m.inc_counter("a_total", &[("k", "v")], 7).unwrap();
        m.set_gauge("b", &[("x", "1"), ("y", "2")], -3);
        m.render_prometheus()
    };
    assert_eq!(build(), build());
}

#[test]
fn snapshot_exposes_values_for_dashboards() {
    let mut m = MetricsRegistry::new();
    m.set_gauge("g", &[("k", "v")], 9);
    m.inc_counter("c_total", &[], 2).unwrap();
    let snap = m.snapshot();
    assert_eq!(snap.get("g{k=\"v\"}"), Some(&9));
    assert_eq!(snap.get("c_total"), Some(&2));
}
