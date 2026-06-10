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
}
