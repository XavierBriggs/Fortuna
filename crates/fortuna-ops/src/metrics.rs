//! Deterministic in-process metrics (spec Section 8). The registry is
//! plain derived state: BTreeMap-ordered, clock-free, no globals and no
//! background threads, so the deterministic core can update it like any
//! other fold. The IO edge renders it as Prometheus text exposition 0.0.4
//! (stable since 2014; research docs/research/ops/otel-rust-2026-06-10 —
//! the OTel Rust prometheus/OTLP exporters are Beta/RC, so the wire
//! format is the stable scrape standard and OTLP push is the documented
//! upgrade path, a transport swap only).
//!
//! Integer values only: FORTUNA's metrics are cents, counts, and flags;
//! ratios are the dashboard's job (per the no-f64-money convention).

use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("counter {name} cannot be incremented by negative {by} (monotone by definition)")]
    NegativeCounterIncrement { name: String, by: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Counter,
    Gauge,
}

/// Series key: metric name + canonical `{k="v",...}` label rendering.
fn series_key(name: &str, labels: &[(&str, &str)]) -> String {
    if labels.is_empty() {
        return name.to_string();
    }
    let mut sorted: Vec<(&str, &str)> = labels.to_vec();
    sorted.sort_unstable();
    let body = sorted
        .iter()
        .map(|(k, v)| format!("{k}=\"{}\"", escape_label_value(v)))
        .collect::<Vec<_>>()
        .join(",");
    format!("{name}{{{body}}}")
}

/// Exposition-format label escaping: backslash, double-quote, line feed.
fn escape_label_value(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// The registry. Families carry an optional HELP string and a TYPE; series
/// are integer-valued.
#[derive(Debug, Default)]
pub struct MetricsRegistry {
    help: BTreeMap<String, String>,
    kinds: BTreeMap<String, Kind>,
    /// family name -> (series key -> value)
    series: BTreeMap<String, BTreeMap<String, i64>>,
}

impl MetricsRegistry {
    pub fn new() -> MetricsRegistry {
        MetricsRegistry::default()
    }

    pub fn describe_counter(&mut self, name: &str, help: &str) {
        self.help.insert(name.to_string(), help.to_string());
        self.kinds.insert(name.to_string(), Kind::Counter);
    }

    pub fn describe_gauge(&mut self, name: &str, help: &str) {
        self.help.insert(name.to_string(), help.to_string());
        self.kinds.insert(name.to_string(), Kind::Gauge);
    }

    /// Monotone accumulation; a negative increment is an ERROR (a counter
    /// that can decrease is a gauge wearing a costume).
    pub fn inc_counter(
        &mut self,
        name: &str,
        labels: &[(&str, &str)],
        by: i64,
    ) -> Result<(), MetricsError> {
        if by < 0 {
            return Err(MetricsError::NegativeCounterIncrement {
                name: name.to_string(),
                by,
            });
        }
        self.kinds.entry(name.to_string()).or_insert(Kind::Counter);
        let key = series_key(name, labels);
        *self
            .series
            .entry(name.to_string())
            .or_default()
            .entry(key)
            .or_insert(0) += by;
        Ok(())
    }

    pub fn set_gauge(&mut self, name: &str, labels: &[(&str, &str)], value: i64) {
        self.kinds.entry(name.to_string()).or_insert(Kind::Gauge);
        let key = series_key(name, labels);
        self.series
            .entry(name.to_string())
            .or_default()
            .insert(key, value);
    }

    /// Prometheus text exposition 0.0.4. Families sorted by name, series
    /// sorted by key; `# HELP`/`# TYPE` once per family; LF-terminated.
    pub fn render_prometheus(&self) -> String {
        let mut out = String::new();
        for (family, series) in &self.series {
            if let Some(help) = self.help.get(family) {
                out.push_str(&format!("# HELP {family} {help}\n"));
            }
            let kind = match self.kinds.get(family) {
                Some(Kind::Counter) => "counter",
                Some(Kind::Gauge) | None => "gauge",
            };
            if self.help.contains_key(family) {
                out.push_str(&format!("# TYPE {family} {kind}\n"));
            }
            for (key, value) in series {
                out.push_str(&format!("{key} {value}\n"));
            }
        }
        out
    }

    /// Flat series view for the dashboard's JSON boards.
    pub fn snapshot(&self) -> BTreeMap<String, i64> {
        let mut out = BTreeMap::new();
        for series in self.series.values() {
            for (k, v) in series {
                out.insert(k.clone(), *v);
            }
        }
        out
    }

    /// Shape the registered metrics into a ROTA board envelope (mission item 6: the
    /// telemetry pane — "the Prometheus stack on the console"). One row per metric
    /// SERIES: the subsystem (derived from the `fortuna_<sub>_` name prefix so the
    /// operator can scan by layer — ingest / gate / exec / state / venue / kill-switch
    /// / cognition / …), the full series key (name + labels), its type
    /// (counter/gauge), and its integer value; grouped by subsystem then metric (the
    /// `series` BTreeMap is already name-sorted, which groups the shared
    /// `fortuna_<sub>_` prefixes). A PURE read of the ALREADY-STRUCTURED registry — no
    /// Prometheus-TEXT parsing (R2): the daemon calls this into
    /// `snapshot.views["telemetry"]` and ROTA serves it verbatim via `read_view`.
    pub fn telemetry_board(&self, generated_at: &str) -> serde_json::Value {
        let mut rows: Vec<serde_json::Value> = Vec::new();
        for (family, series) in &self.series {
            let kind = match self.kinds.get(family) {
                Some(Kind::Counter) => "counter",
                Some(Kind::Gauge) | None => "gauge",
            };
            // Subsystem = the first token after the `fortuna_` namespace prefix; a
            // name without that prefix groups under "other" (honest, never dropped).
            let subsystem = family
                .strip_prefix("fortuna_")
                .and_then(|s| s.split('_').next())
                .filter(|s| !s.is_empty())
                .unwrap_or("other");
            for (key, value) in series {
                rows.push(serde_json::json!({
                    "subsystem": subsystem,
                    "metric": key,
                    "type": kind,
                    "value": value,
                }));
            }
        }
        let families = self.series.len();
        let series_count = rows.len();
        serde_json::json!({
            "title": "Telemetry",
            "generated_at": generated_at,
            "columns": [
                {"key":"subsystem","label":"Subsystem"},
                {"key":"metric","label":"Metric"},
                {"key":"type","label":"Type"},
                {"key":"value","label":"Value"},
            ],
            "rows": rows,
            "summary": {"families": families, "series": series_count},
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mission item 6: telemetry_board shapes the registered metrics into the ROTA
    // board envelope — one row per series, subsystem derived from the fortuna_<sub>_
    // prefix, type + integer value, grouped by subsystem (the name-sorted families).
    // POPULATED-path: real registered counters/gauges across two subsystems + one
    // labelled multi-series family + one non-fortuna name (→ "other").
    #[test]
    fn telemetry_board_shapes_registered_metrics_by_subsystem() {
        let mut m = MetricsRegistry::new();
        m.describe_gauge("fortuna_exec_working_orders", "live orders");
        m.set_gauge("fortuna_exec_working_orders", &[], 3);
        m.describe_counter("fortuna_gate_rejections_total", "gate rejections");
        m.inc_counter("fortuna_gate_rejections_total", &[("check", "edge")], 5)
            .unwrap();
        m.inc_counter("fortuna_gate_rejections_total", &[("check", "rate")], 2)
            .unwrap();
        m.set_gauge("uptime_seconds", &[], 99); // no fortuna_ prefix → "other"

        let board = m.telemetry_board("2026-06-13T12:00:00.000Z");
        assert_eq!(board["title"], "Telemetry");
        assert_eq!(board["generated_at"], "2026-06-13T12:00:00.000Z");
        // 3 families (exec gauge, gate counter, the bare "other"); 4 series rows
        // (the gate family has two labelled series).
        assert_eq!(board["summary"]["families"], 3);
        assert_eq!(board["summary"]["series"], 4);
        let rows = board["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        // The exec gauge row: subsystem "exec", type "gauge", value 3.
        let exec = rows
            .iter()
            .find(|r| r["metric"] == "fortuna_exec_working_orders")
            .expect("exec series present");
        assert_eq!(exec["subsystem"], "exec");
        assert_eq!(exec["type"], "gauge");
        assert_eq!(exec["value"], 3);
        // The gate counter's two labelled series carry subsystem "gate", type
        // "counter", and their real values.
        let gate_edge = rows
            .iter()
            .find(|r| r["metric"] == "fortuna_gate_rejections_total{check=\"edge\"}")
            .expect("gate edge series present");
        assert_eq!(gate_edge["subsystem"], "gate");
        assert_eq!(gate_edge["type"], "counter");
        assert_eq!(gate_edge["value"], 5);
        // A non-fortuna name groups honestly under "other", never dropped.
        let other = rows
            .iter()
            .find(|r| r["metric"] == "uptime_seconds")
            .expect("other series present");
        assert_eq!(other["subsystem"], "other");
    }
}
