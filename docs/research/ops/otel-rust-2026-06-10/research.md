# OpenTelemetry Rust ecosystem status — research 2026-06-10

Question: what should FORTUNA's "Metrics (OpenTelemetry, scraped to a local
dashboard)" (spec Section 8) be built on, as of today?

## Sources

| id | url | what |
|---|---|---|
| S1 | https://github.com/open-telemetry/opentelemetry-rust | project README + component status table |
| S2 | https://docs.rs/opentelemetry-prometheus/latest/opentelemetry_prometheus/ | prometheus exporter docs |
| S3 | https://prometheus.io/docs/instrumenting/exposition_formats/ | text exposition format |
| S4 | (search snapshot) | stale "discontinued" claims about opentelemetry-prometheus |

## Findings

1. **opentelemetry-rust 0.32.0 is the latest release (2026-05-09; 42
   releases)** (S1). Component status table (S1, verbatim labels):
   - Metrics-API: **Stable**
   - Metrics-SDK: **Stable**
   - Metrics-OTLP Exporter: **RC**
   - Metrics-Prometheus Exporter: **Beta**
   - Traces API/SDK: Beta; Logs API/SDK: Stable.
2. **Stale-information trap:** search summaries still circulate a
   "development of the Prometheus exporter has been discontinued /
   0.29 final / unmaintained protobuf dependency" claim (S4). The live
   docs.rs page for 0.32.0 carries NO deprecation notice and a full usage
   example (S2); the README lists the exporter as Beta (S1). The
   discontinuation was a real episode in the crate's history that has
   since been reversed/superseded. Conclusion: alive but BETA.
3. **The Prometheus text exposition format 0.0.4 is stable since 2014**
   (S3) and is the universal scrape format (Prometheus, Grafana Alloy,
   VictoriaMetrics all ingest it). It is line-oriented, trivially
   generatable (`# HELP` / `# TYPE` / `name{labels} value`), and requires
   no dependencies to emit.

## Decision for T1.5

- The deterministic core NEVER talks to a telemetry SDK: fortuna-ops gets
  an in-process `MetricsRegistry` (BTreeMap-ordered counters/gauges) that
  the runner updates like any other derived state. Replay-safe, no
  globals, no background threads.
- The IO edge serves **Prometheus text exposition 0.0.4** from the
  registry at `GET /metrics` on the read-only dashboard server. Zero new
  telemetry deps; scrapeable by everything; metric NAMES follow the spec
  Section 8 list so a later wire-format swap is a transport change only.
- **OTLP push (opentelemetry + opentelemetry_sdk + opentelemetry-otlp,
  Metrics SDK Stable / exporter RC) is the documented upgrade path** when
  the operator stands up a collector; adopting a Beta/RC exporter stack
  for a Phase-1 local scrape adds risk for no capability. Recorded in
  ASSUMPTIONS; the swap point is the exporter side of the registry only.
