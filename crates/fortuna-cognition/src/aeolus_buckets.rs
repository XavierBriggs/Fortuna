//! Aeolus ↔ Kalshi bucket matching (contract:
//! `docs/design/aeolus-kalshi-bucket-matching.md`). Maps a forecast's μ/σ onto the
//! IN-RANGE buckets + tails Kalshi actually trades, so weather beliefs are
//! tradeable 1:1 (vs the cumulative ge-ladder, which only hits the tail).
//!
//! A Kalshi bucket is a DIFFERENCE of the cumulative ladder — `P(high ∈ [lo,hi]) =
//! ge(lo) − ge(hi+1)` — which F6 already computes (`bracket_range_prob`). So
//! [`aeolus_bucket_beliefs`] emits one propose-only `BeliefDraft` per DISCOVERED
//! bucket (Track-A passes the live day-set), each mapping `Direct` to its market.
//! For a complete day-set the per-bucket p's TELESCOPE to 1.0.
//!
//! PROPOSE-ONLY (I6): beliefs only, `p == p_raw` (no calibration here). Pure +
//! replay-deterministic (the pinned erf, §7); no `Clock::now`; never panics.

use crate::aeolus_forecast::{
    bracket_prob_ge, bracket_prob_lt, bracket_range_prob, AeolusForecast, Variable,
};
use crate::beliefs::{brier_score, BeliefDraft};
use serde_json::json;

/// One Kalshi temperature-bucket market the forecast can speak to. Track-A
/// constructs these from the live book; `market_key` is the raw Kalshi ticker.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherBucket {
    pub market_key: String,
    pub kind: BucketKind,
}

/// How a Kalshi bucket reads (contract §2). `InRange` is INCLUSIVE of both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketKind {
    /// Integer daily high ∈ [lo_f, hi_f] inclusive (Kalshi "87° to 88°" ⇒ {87,88}).
    InRange { lo_f: i64, hi_f: i64 },
    /// High ≥ threshold_f (upper tail).
    GreaterEq { threshold_f: i64 },
    /// High ≤ threshold_f (lower tail).
    LessEq { threshold_f: i64 },
}

impl BucketKind {
    /// FORTUNA's μ/σ probability for this bucket via the F6 helpers (the −0.5
    /// integer-degree correction lives inside them). `None` only when σ≤0 /
    /// non-finite (impossible post-parse). `InRange{lo,hi}` = `ge(lo) − ge(hi+1)`;
    /// `GreaterEq{M}` = `ge(M)`; `LessEq{M}` = `lt(M+1)`.
    pub fn probability(self, mu: f64, sigma: f64) -> Option<f64> {
        match self {
            BucketKind::InRange { lo_f, hi_f } => bracket_range_prob(lo_f, hi_f + 1, mu, sigma),
            BucketKind::GreaterEq { threshold_f } => bracket_prob_ge(threshold_f, mu, sigma),
            BucketKind::LessEq { threshold_f } => bracket_prob_lt(threshold_f + 1, mu, sigma),
        }
    }

    /// Whether the realized integer high satisfies this bucket (for F9 Brier).
    pub fn outcome(self, realized_f: f64) -> bool {
        match self {
            BucketKind::InRange { lo_f, hi_f } => {
                realized_f >= lo_f as f64 && realized_f <= hi_f as f64
            }
            BucketKind::GreaterEq { threshold_f } => realized_f >= threshold_f as f64,
            BucketKind::LessEq { threshold_f } => realized_f <= threshold_f as f64,
        }
    }

    /// A compact JSON description for the belief `evidence` (DATA, for F9/ROTA).
    fn describe(self) -> serde_json::Value {
        match self {
            BucketKind::InRange { lo_f, hi_f } => {
                json!({"kind": "in_range", "lo_f": lo_f, "hi_f": hi_f})
            }
            BucketKind::GreaterEq { threshold_f } => {
                json!({"kind": "ge", "threshold_f": threshold_f})
            }
            BucketKind::LessEq { threshold_f } => json!({"kind": "le", "threshold_f": threshold_f}),
        }
    }
}

fn variable_str(v: Variable) -> &'static str {
    match v {
        Variable::Tmax => "tmax",
        Variable::Tmin => "tmin",
    }
}

fn provenance(fc: &AeolusForecast) -> serde_json::Value {
    json!({
        "model_id": "aeolus",
        "station": fc.station(),
        "variable": variable_str(fc.variable()),
        "target_date": fc.target_date(),
        "run_at": fc.run_at().to_iso8601(),
        "model_version": fc.distribution().model_version,
    })
}

/// Emit one propose-only `BeliefDraft` per discovered Kalshi bucket (contract §3):
/// `event_id = aeolus:{market_key}`, `p == p_raw =` the μ/σ bucket probability,
/// `horizon = settles_after`, provenance/evidence stamped. Order-preserving; a
/// draft that somehow fails validation is skipped, never emitted.
pub fn aeolus_bucket_beliefs(fc: &AeolusForecast, buckets: &[WeatherBucket]) -> Vec<BeliefDraft> {
    let mu = fc.mu();
    let sigma = fc.sigma();
    let prov = provenance(fc);
    let horizon = fc.resolution().settles_after;
    let skill = fc.skill();

    let mut drafts = Vec::with_capacity(buckets.len());
    for bucket in buckets {
        let Some(p) = bucket.kind.probability(mu, sigma) else {
            continue; // σ>0 post-parse ⇒ unreachable; skip, never panic.
        };
        let evidence = json!([{
            "source": "aeolus",
            "ref": format!("{}@{}", fc.station(), fc.run_at().to_iso8601()),
            "bucket": bucket.kind.describe(),
            "p_fortuna": p,
            "crps": skill.crps,
            "crpss_vs_raw": skill.crpss_vs_raw,
            "n_scored": skill.n_scored,
        }]);
        let draft = BeliefDraft {
            event_id: format!("aeolus:{}", bucket.market_key),
            // Propose-only (I6): p == p_raw; calibration is downstream.
            p,
            p_raw: p,
            horizon,
            evidence,
            provenance: prov.clone(),
        };
        if draft.validate().is_ok() {
            drafts.push(draft);
        }
    }
    drafts
}

/// One discovered bucket's realized score (F9 per-kind, contract §5).
#[derive(Debug, Clone, PartialEq)]
pub struct BucketScore {
    pub market_key: String,
    pub p_fortuna: f64,
    pub outcome: bool,
    pub brier: f64,
}

/// Score each bucket belief by Brier against the realized daily high (the per-kind
/// outcome). The CRPS of the μ/σ fan is unchanged (F9's `score_reliability`). Pure;
/// never panics. (A bucket whose σ≤0 probability is `None` — impossible post-parse —
/// is skipped.)
pub fn score_bucket_briers(
    fc: &AeolusForecast,
    buckets: &[WeatherBucket],
    realized_f: f64,
) -> Vec<BucketScore> {
    let mu = fc.mu();
    let sigma = fc.sigma();
    buckets
        .iter()
        .filter_map(|bucket| {
            let p = bucket.kind.probability(mu, sigma)?;
            let outcome = bucket.kind.outcome(realized_f);
            Some(BucketScore {
                market_key: bucket.market_key.clone(),
                p_fortuna: p,
                outcome,
                brier: brier_score(p, outcome),
            })
        })
        .collect()
}
