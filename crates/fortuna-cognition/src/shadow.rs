//! The shadow-mode model comparison harness (spec Section 11).
//!
//! A challenger model runs FULL decision cycles in shadow: it receives
//! the IDENTICAL AssembledContext the incumbent received (the pairing
//! key is the manifest hash, so "identical" is provable), its beliefs
//! are logged and scored like any others under its own model_id, and NO
//! orders exist anywhere in the path — `ShadowRun` carries beliefs only.
//!
//! Shadow operates under its OWN cost budget (never the live budget)
//! and samples cycles deterministically (first K per UTC day) rather
//! than shadowing every one. Only paired contexts count toward the
//! comparison.
//!
//! `evaluate_model_swap` is the I7 gate: a Promote RECOMMENDATION
//! requires >= `min_resolved_per_category` resolved paired beliefs in
//! EVERY active category, challenger mean Brier <= incumbent, and
//! challenger mean CLV >= incumbent where both are measurable. No
//! record, no promotion. Applying a swap is an operator config change.

use crate::beliefs::BeliefDraft;
use crate::context::AssembledContext;
use crate::cycle::ShadowSampler;
use crate::mind::{CostBudget, Mind};
use fortuna_core::clock::UtcTimestamp;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShadowError {
    #[error("shadow harness misconfigured: {reason}")]
    Config { reason: String },
}

#[derive(Debug, Clone)]
pub struct ShadowHarnessConfig {
    pub challenger_model_id: String,
    /// Shadowed cycles per UTC day (deterministic first-K sampling).
    pub daily_sample_quota: u32,
    pub per_cycle_cap_cents: i64,
    pub per_day_cap_cents: i64,
}

/// One shadow run: the challenger's beliefs on the incumbent's exact
/// context. STRUCTURALLY zero orders — no field can carry one.
#[derive(Debug)]
pub struct ShadowRun {
    pub manifest_hash: String,
    pub beliefs: Vec<BeliefDraft>,
    pub cost_cents: i64,
}

/// The challenger-side harness. Holds its own budget and sampler;
/// failures are counted and never propagate into the live loop.
pub struct ShadowHarness {
    challenger_model_id: String,
    sampler: ShadowSampler,
    budget: CostBudget,
    failures: u64,
}

impl ShadowHarness {
    pub fn new(config: ShadowHarnessConfig) -> ShadowHarness {
        ShadowHarness {
            challenger_model_id: config.challenger_model_id,
            sampler: ShadowSampler::new(config.daily_sample_quota),
            budget: CostBudget::new(config.per_cycle_cap_cents, config.per_day_cap_cents),
            failures: 0,
        }
    }

    /// Cycles the challenger failed (provider/schema/refusal). Observable
    /// for the ops layer; never fatal to the caller.
    pub fn failures(&self) -> u64 {
        self.failures
    }

    /// Shadow this cycle if the day's sample quota and the shadow budget
    /// allow. Returns Ok(None) when unsampled, throttled, or failed —
    /// the live loop never sees a challenger problem.
    pub async fn maybe_shadow(
        &mut self,
        challenger: &dyn Mind,
        incumbent_context: &AssembledContext,
        now: UtcTimestamp,
    ) -> Result<Option<ShadowRun>, ShadowError> {
        // Budget BEFORE sampling: an unaffordable cycle must not consume
        // the day's sample quota.
        if self.budget.check(now).is_err() {
            return Ok(None);
        }
        if !self.sampler.should_shadow(now) {
            return Ok(None);
        }

        let output = match challenger.decide(incumbent_context).await {
            Ok(output) => output,
            Err(_e) => {
                self.failures += 1;
                return Ok(None);
            }
        };
        self.budget.record_spend(output.cost_cents, now);

        // Stamp the challenger's identity and the PAIRING KEY into every
        // belief (the harness knows; the model cannot).
        let mut beliefs = output.beliefs;
        for belief in &mut beliefs {
            belief.provenance = json!({
                "model_id": self.challenger_model_id,
                "context_manifest_hash": incumbent_context.manifest_hash,
                "shadow": true,
                "cost_cents": output.cost_cents,
            });
        }

        Ok(Some(ShadowRun {
            manifest_hash: incumbent_context.manifest_hash.clone(),
            beliefs,
            cost_cents: output.cost_cents,
        }))
    }
}

// --------------------------------------------------------------- swap gate

/// One PAIRED resolution: incumbent and challenger scored on the same
/// context (same manifest hash) and the same outcome.
#[derive(Debug, Clone)]
pub struct PairedScore {
    pub category: String,
    pub manifest_hash: String,
    pub incumbent_brier: f64,
    pub challenger_brier: f64,
    pub incumbent_clv_bps: Option<f64>,
    pub challenger_clv_bps: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SwapThresholds {
    /// Spec Section 11: ">= 30 resolved beliefs per active category".
    pub min_resolved_per_category: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapVerdict {
    /// The operator MAY promote (a recommendation, never an action).
    PromoteRecommended,
    Hold,
}

/// Per-category evidence for the operator.
#[derive(Debug, Clone)]
pub struct CategoryComparison {
    pub category: String,
    pub n: usize,
    pub incumbent_brier_mean: f64,
    pub challenger_brier_mean: f64,
    pub incumbent_clv_mean_bps: Option<f64>,
    pub challenger_clv_mean_bps: Option<f64>,
    pub qualified: bool,
}

#[derive(Debug, Clone)]
pub struct SwapEvaluation {
    pub verdict: SwapVerdict,
    pub categories: Vec<CategoryComparison>,
    pub reasons: Vec<String>,
}

/// The I7 model-swap gate (spec Section 11): Promote is RECOMMENDED only
/// when every active category has enough PAIRED resolutions and the
/// challenger's mean Brier is no worse and mean CLV no worse (where both
/// sides measured). Duplicate (category, manifest) pairs never inflate
/// the count. No record, no promotion.
pub fn evaluate_model_swap(
    paired: &[PairedScore],
    active_categories: &[String],
    thresholds: &SwapThresholds,
) -> SwapEvaluation {
    // Deduplicate on the pairing key: a context scored twice is one pair.
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut per_category: BTreeMap<&str, Vec<&PairedScore>> = BTreeMap::new();
    for score in paired {
        if seen.insert((score.category.clone(), score.manifest_hash.clone())) {
            per_category
                .entry(score.category.as_str())
                .or_default()
                .push(score);
        }
    }

    let mut categories = Vec::new();
    let mut reasons = Vec::new();
    let mut all_qualified = true;

    for category in active_categories {
        let scores = per_category
            .get(category.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let n = scores.len();
        let mean = |f: &dyn Fn(&PairedScore) -> f64| -> f64 {
            if n == 0 {
                0.0
            } else {
                scores.iter().map(|s| f(s)).sum::<f64>() / n as f64
            }
        };
        let incumbent_brier_mean = mean(&|s| s.incumbent_brier);
        let challenger_brier_mean = mean(&|s| s.challenger_brier);
        let clv_mean = |f: &dyn Fn(&PairedScore) -> Option<f64>| -> Option<f64> {
            let measured: Vec<f64> = scores.iter().filter_map(|s| f(s)).collect();
            if measured.is_empty() {
                None
            } else {
                Some(measured.iter().sum::<f64>() / measured.len() as f64)
            }
        };
        let incumbent_clv_mean_bps = clv_mean(&|s| s.incumbent_clv_bps);
        let challenger_clv_mean_bps = clv_mean(&|s| s.challenger_clv_bps);

        let mut qualified = true;
        if n < thresholds.min_resolved_per_category {
            qualified = false;
            reasons.push(format!(
                "category '{category}': {n} paired resolutions < {} required",
                thresholds.min_resolved_per_category
            ));
        } else {
            if challenger_brier_mean > incumbent_brier_mean {
                qualified = false;
                reasons.push(format!(
                    "category '{category}': challenger Brier {challenger_brier_mean:.3} worse \
                     than incumbent {incumbent_brier_mean:.3}"
                ));
            }
            if let (Some(inc), Some(ch)) = (incumbent_clv_mean_bps, challenger_clv_mean_bps) {
                if ch < inc {
                    qualified = false;
                    reasons.push(format!(
                        "category '{category}': challenger CLV {ch:.1} bps worse than \
                         incumbent {inc:.1} bps"
                    ));
                }
            }
        }
        all_qualified &= qualified;
        categories.push(CategoryComparison {
            category: category.clone(),
            n,
            incumbent_brier_mean,
            challenger_brier_mean,
            incumbent_clv_mean_bps,
            challenger_clv_mean_bps,
            qualified,
        });
    }

    let verdict = if all_qualified && !active_categories.is_empty() {
        SwapVerdict::PromoteRecommended
    } else {
        SwapVerdict::Hold
    };
    if verdict == SwapVerdict::PromoteRecommended {
        reasons.push(
            "challenger qualifies in every active category (operator applies any swap)".to_string(),
        );
    }
    SwapEvaluation {
        verdict,
        categories,
        reasons,
    }
}
