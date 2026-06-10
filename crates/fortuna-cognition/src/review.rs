//! The weekly and monthly review jobs (spec 5.8).
//!
//! Weekly: calibration audit per (model, strategy, category) scope —
//! reliability curve, Brier mean, CLV, quality, and a VERSIONED refit at
//! n >= 50 (spec 5.10) — plus GO/NO-GO recommendations against the
//! Section 11 thresholds and mind commentary with lesson candidates.
//! The deterministic core never depends on the mind: commentary is
//! layered on top and any mind failure degrades to a report without it.
//!
//! Monthly: envelope allocation recommendations (advisory — allocation
//! is set by the operator at monthly review and recorded in config,
//! spec 5.14), the fee/PnL and cost-of-cognition audit, lesson decay
//! (unconfirmed lessons demote, spec 5.6), and the operator checklist
//! (kill-switch test, backup restore drill — reminders; the job never
//! performs operator actions).
//!
//! I7 discipline: everything here RECOMMENDS. No output type carries a
//! stage, an envelope write, or a promotion; those are operator actions.

use crate::beliefs::{calibration_curve, CalibrationBucket};
use crate::calibration::{calibration_quality, fit_platt, CalibrationMethod, CalibrationParams};
use crate::context::{assemble_context, AssemblerConfig, ContextItem};
use crate::mind::Mind;
use fortuna_core::clock::UtcTimestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReviewError {
    #[error("context assembly failed: {0}")]
    Context(#[from] crate::context::ContextError),
}

/// One calibration scope (spec 5.10: per model, strategy, category).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ScopeKey {
    pub model_id: String,
    pub strategy: String,
    pub category: String,
}

/// The forward record for one scope: resolved (claimed p, outcome)
/// samples and the CLV measurements that were measurable.
#[derive(Debug, Clone)]
pub struct ScopeRecord {
    pub key: ScopeKey,
    pub samples: Vec<(f64, bool)>,
    pub clv_bps: Vec<f64>,
}

/// The weekly calibration audit row for one scope.
#[derive(Debug, Clone)]
pub struct ScopeCalibration {
    pub key: ScopeKey,
    pub n: usize,
    pub brier_mean: f64,
    pub quality: f64,
    pub curve: Vec<CalibrationBucket>,
    /// A versioned refit, present only at n >= FULL_AUTONOMY_N with an
    /// identifiable record.
    pub fitted: Option<CalibrationParams>,
    /// Why the fit was refused, when it was (degenerate record).
    pub fit_defect: Option<String>,
    /// The version a refit would carry (prior + 1) — recorded so the
    /// below-threshold and refused cases still document the ladder.
    pub fitted_version_would_be: u32,
    pub clv_mean_bps: Option<f64>,
}

/// The deterministic calibration audit. Refits are attempted ONLY at
/// n >= 50 (spec 5.10); a degenerate record refuses with the reason
/// rather than fitting a lie. Versions advance from the caller-supplied
/// prior version per scope.
pub fn calibration_report(
    records: &[ScopeRecord],
    prior_versions: &BTreeMap<ScopeKey, u32>,
) -> Vec<ScopeCalibration> {
    records
        .iter()
        .map(|record| {
            let n = record.samples.len();
            let brier_mean = if n == 0 {
                0.0
            } else {
                record
                    .samples
                    .iter()
                    .map(|(p, outcome)| {
                        let target = if *outcome { 1.0 } else { 0.0 };
                        (p - target) * (p - target)
                    })
                    .sum::<f64>()
                    / n as f64
            };
            let curve = calibration_curve(&record.samples, 10);
            let quality = calibration_quality(&curve, n);
            let next_version = prior_versions.get(&record.key).copied().unwrap_or(0) + 1;

            let (fitted, fit_defect) = if n >= crate::calibration::FULL_AUTONOMY_N {
                match fit_platt(&record.samples) {
                    Ok(platt) => (
                        Some(CalibrationParams {
                            version: next_version,
                            method: CalibrationMethod::Platt(platt),
                            // Conservative default until the weekly audit
                            // supports more (spec 5.10).
                            extremization_k: 1.0,
                            fitted_on_n: n,
                        }),
                        None,
                    ),
                    Err(e) => (
                        None,
                        Some(format!("refit refused: degenerate record ({e})")),
                    ),
                }
            } else {
                (None, None)
            };

            let clv_mean_bps = if record.clv_bps.is_empty() {
                None
            } else {
                Some(record.clv_bps.iter().sum::<f64>() / record.clv_bps.len() as f64)
            };

            ScopeCalibration {
                key: record.key.clone(),
                n,
                brier_mean,
                quality,
                curve,
                fitted,
                fit_defect,
                fitted_version_would_be: next_version,
                clv_mean_bps,
            }
        })
        .collect()
}

// --------------------------------------------------------------- go/no-go

#[derive(Debug, Clone)]
pub struct GoNoGoThresholds {
    pub min_paper_days_mechanical: u32,
    pub min_resolved_beliefs_synthesis: usize,
    pub max_fee_pnl_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyKindView {
    Mechanical,
    Synthesis,
}

/// One strategy's forward record as the GO/NO-GO check consumes it.
#[derive(Debug, Clone)]
pub struct StrategyRecord {
    pub strategy: String,
    pub kind: StrategyKindView,
    pub paper_days: u32,
    pub resolved_beliefs: usize,
    pub realized_pnl_cents: i64,
    pub fees_cents: i64,
    pub clv_mean_bps: Option<f64>,
    pub invariant_violations: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Go,
    NoGo,
    InsufficientData,
}

/// A RECOMMENDATION (I7: promotion is a human action). The type carries
/// no stage and no mutation surface — a name, a verdict, and reasons.
#[derive(Debug, Clone)]
pub struct GoNoGoRecommendation {
    pub strategy: String,
    pub verdict: Verdict,
    pub reasons: Vec<String>,
}

/// Section 11 thresholds as deterministic checks. Each verdict carries
/// its reasons; the operator decides.
pub fn go_nogo(
    records: &[StrategyRecord],
    thresholds: &GoNoGoThresholds,
) -> Vec<GoNoGoRecommendation> {
    records
        .iter()
        .map(|r| {
            let mut reasons = Vec::new();

            // Any invariant violation is an unconditional NO-GO.
            if r.invariant_violations > 0 {
                reasons.push(format!(
                    "invariant violations recorded: {} (unconditional NO-GO)",
                    r.invariant_violations
                ));
                return GoNoGoRecommendation {
                    strategy: r.strategy.clone(),
                    verdict: Verdict::NoGo,
                    reasons,
                };
            }

            // Window sufficiency.
            match r.kind {
                StrategyKindView::Mechanical => {
                    if r.paper_days < thresholds.min_paper_days_mechanical {
                        reasons.push(format!(
                            "paper window {} days < {} required",
                            r.paper_days, thresholds.min_paper_days_mechanical
                        ));
                        return GoNoGoRecommendation {
                            strategy: r.strategy.clone(),
                            verdict: Verdict::InsufficientData,
                            reasons,
                        };
                    }
                }
                StrategyKindView::Synthesis => {
                    if r.resolved_beliefs < thresholds.min_resolved_beliefs_synthesis {
                        reasons.push(format!(
                            "resolved beliefs {} < {} required",
                            r.resolved_beliefs, thresholds.min_resolved_beliefs_synthesis
                        ));
                        return GoNoGoRecommendation {
                            strategy: r.strategy.clone(),
                            verdict: Verdict::InsufficientData,
                            reasons,
                        };
                    }
                    // Positive CLV is the market-relative GO criterion for
                    // prediction markets (Section 11).
                    match r.clv_mean_bps {
                        None => {
                            reasons.push(
                                "CLV not yet measurable (no benchmark-snapshot overlap)"
                                    .to_string(),
                            );
                            return GoNoGoRecommendation {
                                strategy: r.strategy.clone(),
                                verdict: Verdict::InsufficientData,
                                reasons,
                            };
                        }
                        Some(clv) if clv <= 0.0 => {
                            reasons.push(format!("CLV {clv:.1} bps <= 0"));
                            return GoNoGoRecommendation {
                                strategy: r.strategy.clone(),
                                verdict: Verdict::NoGo,
                                reasons,
                            };
                        }
                        Some(clv) => reasons.push(format!("CLV +{clv:.1} bps")),
                    }
                }
            }

            // Expectancy net of fees.
            let net = r.realized_pnl_cents - r.fees_cents;
            if net <= 0 {
                reasons.push(format!("expectancy net of fees not positive ({net} cents)"));
                return GoNoGoRecommendation {
                    strategy: r.strategy.clone(),
                    verdict: Verdict::NoGo,
                    reasons,
                };
            }

            // Fee drag.
            if r.realized_pnl_cents > 0 {
                let ratio = r.fees_cents as f64 / r.realized_pnl_cents as f64;
                if ratio >= thresholds.max_fee_pnl_ratio {
                    reasons.push(format!(
                        "fee/PnL ratio {ratio:.2} >= {:.2}",
                        thresholds.max_fee_pnl_ratio
                    ));
                    return GoNoGoRecommendation {
                        strategy: r.strategy.clone(),
                        verdict: Verdict::NoGo,
                        reasons,
                    };
                }
                reasons.push(format!("fee/PnL ratio {ratio:.2}"));
            }

            reasons.push(format!("net expectancy +{net} cents over the window"));
            GoNoGoRecommendation {
                strategy: r.strategy.clone(),
                verdict: Verdict::Go,
                reasons,
            }
        })
        .collect()
}

// ------------------------------------------------------------ weekly review

/// A lesson PROPOSAL for the operator review queue (spec 5.6/8: lesson
/// promotion is an approve/reject item; promotion to semantic memory is
/// the operator's insert).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LessonCandidate {
    pub body: String,
    pub provenance: serde_json::Value,
}

/// The structured commentary contract the weekly review asks the mind
/// to put in its journal body. Strict: free prose fails the parse and
/// the review degrades to its deterministic core.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WeeklyCommentary {
    commentary: String,
    #[serde(default)]
    lesson_candidates: Vec<LessonCandidate>,
}

#[derive(Debug)]
pub struct WeeklyReview {
    pub calibration: Vec<ScopeCalibration>,
    pub recommendations: Vec<GoNoGoRecommendation>,
    pub commentary: Option<String>,
    pub lesson_candidates: Vec<LessonCandidate>,
    /// Why commentary is missing, when it is (mind failure, no journal,
    /// contract-violating body). The deterministic core is unaffected.
    pub commentary_defect: Option<String>,
    pub manifest_hash: String,
    pub cost_cents: i64,
}

/// The weekly review job. The deterministic core (calibration audit +
/// GO/NO-GO recommendations) is computed FIRST and survives any mind
/// outcome; commentary and lesson candidates are layered on top.
pub async fn weekly_review(
    mind: &dyn Mind,
    context_items: &[ContextItem],
    records: &[ScopeRecord],
    prior_versions: &BTreeMap<ScopeKey, u32>,
    strategies: &[StrategyRecord],
    thresholds: &GoNoGoThresholds,
    now: UtcTimestamp,
) -> Result<WeeklyReview, ReviewError> {
    let calibration = calibration_report(records, prior_versions);
    let recommendations = go_nogo(strategies, thresholds);

    let assembler = AssemblerConfig {
        budget_chars: 200_000,
        anonymize: false,
    };
    let ctx = assemble_context(context_items, now, "weekly_review", &assembler)?;
    let manifest_hash = ctx.manifest_hash.clone();

    let (commentary, lesson_candidates, commentary_defect, cost_cents) =
        match mind.decide(&ctx).await {
            Ok(output) => match output.journal {
                Some(journal) => match serde_json::from_str::<WeeklyCommentary>(&journal.body) {
                    Ok(parsed) => {
                        let candidates: Vec<LessonCandidate> = parsed
                            .lesson_candidates
                            .into_iter()
                            .filter(|c| !c.body.trim().is_empty())
                            .collect();
                        (Some(parsed.commentary), candidates, None, output.cost_cents)
                    }
                    Err(e) => (
                        None,
                        Vec::new(),
                        Some(format!(
                            "commentary body violated the contract (never repaired): {e}"
                        )),
                        output.cost_cents,
                    ),
                },
                None => (
                    None,
                    Vec::new(),
                    Some("mind produced no journal for the weekly review".to_string()),
                    output.cost_cents,
                ),
            },
            Err(e) => (
                None,
                Vec::new(),
                Some(format!("mind failed: {e} (deterministic core unaffected)")),
                0,
            ),
        };

    Ok(WeeklyReview {
        calibration,
        recommendations,
        commentary,
        lesson_candidates,
        commentary_defect,
        manifest_hash,
        cost_cents,
    })
}

// ----------------------------------------------------------- monthly review

/// One strategy's month as the allocation recommender consumes it.
#[derive(Debug, Clone)]
pub struct AllocationInput {
    pub strategy: String,
    pub envelope_cents: i64,
    pub realized_pnl_cents: i64,
    pub fees_cents: i64,
    pub cognition_cost_cents: i64,
}

/// ADVISORY (spec 5.14: allocation is set by the operator at monthly
/// review and recorded in config). Carries a rationale, never a write.
#[derive(Debug, Clone)]
pub struct AllocationRecommendation {
    pub strategy: String,
    pub current_envelope_cents: i64,
    pub recommended_envelope_cents: i64,
    pub rationale: String,
}

#[derive(Debug, Clone)]
pub struct CostAudit {
    pub total_realized_pnl_cents: i64,
    pub total_fees_cents: i64,
    pub total_cognition_cost_cents: i64,
}

/// An active lesson and its review date (decay input).
#[derive(Debug, Clone)]
pub struct LessonStatusView {
    pub lesson_id: String,
    pub review_at: UtcTimestamp,
}

#[derive(Debug)]
pub struct MonthlyReview {
    pub allocations: Vec<AllocationRecommendation>,
    pub cost_audit: CostAudit,
    /// Active lessons whose review date passed unconfirmed: demote
    /// (spec 5.6 decay). The composition applies via superseding insert.
    pub lessons_due_demotion: Vec<String>,
    /// Reminders for operator-only actions (spec 5.8). The job never
    /// performs them.
    pub operator_checklist: Vec<String>,
}

/// The monthly review: fully deterministic. The allocation rule is
/// conservative (ASSUMPTIONS): a strategy whose month is net negative
/// after fees AND cognition cost halves; everything else holds. The
/// recommendation total never exceeds the current total (no invented
/// capital); increases are the operator's call.
pub fn monthly_review(
    strategies: &[AllocationInput],
    active_lessons: &[LessonStatusView],
    now: UtcTimestamp,
) -> MonthlyReview {
    let allocations = strategies
        .iter()
        .map(|s| {
            let net = s.realized_pnl_cents - s.fees_cents - s.cognition_cost_cents;
            let (recommended, rationale) = if net < 0 {
                (
                    s.envelope_cents / 2,
                    format!(
                        "net {net} cents after fees and cognition cost: halve until the \
                         record supports more"
                    ),
                )
            } else {
                (
                    s.envelope_cents,
                    format!("net +{net} cents: hold (increases are an operator decision)"),
                )
            };
            AllocationRecommendation {
                strategy: s.strategy.clone(),
                current_envelope_cents: s.envelope_cents,
                recommended_envelope_cents: recommended,
                rationale,
            }
        })
        .collect();

    let cost_audit = CostAudit {
        total_realized_pnl_cents: strategies.iter().map(|s| s.realized_pnl_cents).sum(),
        total_fees_cents: strategies.iter().map(|s| s.fees_cents).sum(),
        total_cognition_cost_cents: strategies.iter().map(|s| s.cognition_cost_cents).sum(),
    };

    let lessons_due_demotion = active_lessons
        .iter()
        .filter(|l| l.review_at.epoch_millis() <= now.epoch_millis())
        .map(|l| l.lesson_id.clone())
        .collect();

    MonthlyReview {
        allocations,
        cost_audit,
        lessons_due_demotion,
        operator_checklist: vec![
            "kill-switch test (I4: monthly; operator runs the drill end to end)".to_string(),
            "Postgres backup restore drill (operator restores last backup to scratch)".to_string(),
            "capital allocation: review recommendations and record the decision in config"
                .to_string(),
            "model-version evaluation: review shadow comparison results (T3.3) if any".to_string(),
        ],
    }
}
