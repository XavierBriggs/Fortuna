//! Track E E.5 (scoring & promotion, design §10/§11): per-(persona, version)
//! calibration scoring + the beat-both-baselines promote/retire PROPOSAL.
//!
//! RECOMMENDATION-ONLY (the I7 analog): the daemon never self-promotes; this
//! emits a proposal the operator acts on out-of-band (a superseding registry
//! insert / `status='retired'`). A persona that can't beat the baselines is
//! retired ON THE RECORD.
//!
//! This EXTENDS the review LAYER additively (design §10): the persona scope is
//! the model scope + `{persona_id, persona_version}`. It reuses the existing
//! calibration primitives (`calibration_curve` / `calibration_quality` / Brier /
//! CLV) keyed by [`PersonaScope`], WITHOUT mutating the shared `review::ScopeKey`
//! — whose struct literal is built in Track A's `daemon.rs` (mutating it would
//! break Track A's composition). Folding the persona dims into `ScopeKey` + the
//! daemon wiring is a GATED Track-A coordination (see the design Fit-validation
//! note + GAPS).

use crate::beliefs::calibration_curve;
use crate::calibration::calibration_quality;
use serde::Serialize;

/// A persona scoring scope: the producing persona + version (design §10).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PersonaScope {
    pub persona_id: String,
    pub persona_version: i32,
}

/// The forward record for one persona scope: resolved (claimed p, outcome)
/// samples + the CLV measurements (mirrors `review::ScopeRecord`).
#[derive(Debug, Clone)]
pub struct PersonaScopeRecord {
    pub scope: PersonaScope,
    pub samples: Vec<(f64, bool)>,
    pub clv_bps: Vec<f64>,
}

/// The per-(persona, version) calibration scorecard.
#[derive(Debug, Clone, Serialize)]
pub struct PersonaScorecard {
    pub scope: PersonaScope,
    pub n: usize,
    pub brier_mean: f64,
    pub quality: f64,
    pub clv_mean_bps: Option<f64>,
}

/// Compute the scorecard for one persona scope (reuses the calibration
/// primitives; an empty record yields n=0 with a 0.0 Brier, never a panic).
pub fn score_persona(record: &PersonaScopeRecord) -> PersonaScorecard {
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
    let clv_mean_bps = if record.clv_bps.is_empty() {
        None
    } else {
        Some(record.clv_bps.iter().sum::<f64>() / record.clv_bps.len() as f64)
    };
    PersonaScorecard {
        scope: record.scope.clone(),
        n,
        brier_mean,
        quality,
        clv_mean_bps,
    }
}

/// A baseline the persona must beat (§11): the no-persona raw-source-direct
/// beliefs, or the market-implied beliefs — each measured over the same events.
#[derive(Debug, Clone, Copy)]
pub struct Baseline {
    pub brier_mean: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionVerdict {
    /// Below the evaluation floor — keep scoring, ZERO capital (§11).
    Evaluating { resolved: usize, needed: usize },
    /// Beat BOTH baselines after the floor — propose promotion (operator acts).
    Promotable,
    /// Reached the floor but cannot beat the baselines — propose retirement.
    RetireCandidate,
}

/// A promote/retire proposal for one (persona, version) — recommendation-only.
#[derive(Debug, Clone, Serialize)]
pub struct PersonaPromotionProposal {
    pub scope: PersonaScope,
    pub verdict: PromotionVerdict,
    pub n_resolved: usize,
    pub brier_mean: f64,
    pub clv_mean_bps: Option<f64>,
    /// Whether this version's Brier beats the prior version's (Some iff a prior
    /// scorecard was supplied).
    pub beats_prior_version: Option<bool>,
    pub rationale: String,
}

/// Propose promote/retire for a (persona, version) — RECOMMENDATION-ONLY (I7).
/// The §11 gate: only after `min_resolved` resolved beliefs may a scope be
/// PROMOTABLE, and only if it beats the no-persona AND the market baseline
/// (Brier ≤ both) with positive CLV. Below the floor it is EVALUATING (scored,
/// zero-capital); at/above the floor but not beating both, RETIRE_CANDIDATE.
pub fn propose_promotion(
    card: &PersonaScorecard,
    prior: Option<&PersonaScorecard>,
    no_persona: Baseline,
    market: Baseline,
    min_resolved: usize,
) -> PersonaPromotionProposal {
    // The §11 gate is THREE INDEPENDENT conditions — kept as separate named
    // booleans so the gate structure stays faithful to the spec and a future
    // refactor cannot silently drop one (e.g. fold CLV into the market check).
    let clv = card.clv_mean_bps.unwrap_or(0.0); // no measurable CLV => 0 => not promotable
    let beats_no_persona = card.brier_mean <= no_persona.brier_mean;
    let beats_market_brier = card.brier_mean <= market.brier_mean;
    let positive_clv = clv > 0.0;
    // A tie (`<=`) counts as beating, per §11 "Brier ≤ market".
    let beats_prior_version = prior.map(|p| card.brier_mean <= p.brier_mean);

    let verdict = if card.n < min_resolved {
        PromotionVerdict::Evaluating {
            resolved: card.n,
            needed: min_resolved,
        }
    } else if beats_no_persona && beats_market_brier && positive_clv {
        PromotionVerdict::Promotable
    } else {
        PromotionVerdict::RetireCandidate
    };

    let rationale = match &verdict {
        PromotionVerdict::Evaluating { resolved, needed } => {
            format!("evaluating: {resolved}/{needed} resolved (zero-capital until the gate passes)")
        }
        PromotionVerdict::Promotable => format!(
            "promotable: Brier {:.3} <= market {:.3} & raw {:.3}, CLV {clv:+.0}bp",
            card.brier_mean, market.brier_mean, no_persona.brier_mean
        ),
        PromotionVerdict::RetireCandidate => format!(
            "retire on the record: Brier {:.3} vs market {:.3} / raw {:.3}, CLV {clv:+.0}bp \
             (cannot beat both baselines)",
            card.brier_mean, market.brier_mean, no_persona.brier_mean
        ),
    };

    PersonaPromotionProposal {
        scope: card.scope.clone(),
        verdict,
        n_resolved: card.n,
        brier_mean: card.brier_mean,
        clv_mean_bps: card.clv_mean_bps,
        beats_prior_version,
        rationale,
    }
}

/// One persona scope's weekly-review input: its resolved record + (optionally)
/// the prior version's scorecard (for `beats_prior_version`) + the two §11
/// baselines (the no-persona raw-source-direct beliefs and the market-implied
/// beliefs, both over the SAME resolved events).
#[derive(Debug, Clone)]
pub struct PersonaReviewInput {
    pub record: PersonaScopeRecord,
    pub prior: Option<PersonaScorecard>,
    pub no_persona: Baseline,
    pub market: Baseline,
}

/// The weekly-review persona folding (§10/§11) — the entry point Track A's daemon
/// calls in `drive()`'s weekly review. Scores each registered `(persona, version)`
/// and proposes promote/retire. RECOMMENDATION-ONLY (I7): the daemon routes the
/// returned proposals to `#fortuna-review` and the operator acts out-of-band; the
/// daemon never self-promotes. Order-preserving over `inputs`.
///
/// This is the ADDITIVE PARALLEL realization of the §10 ScopeKey extension (design
/// §21): it scores personas by [`PersonaScope`] ALONGSIDE the synthesis
/// `review::ScopeKey` review, WITHOUT editing the shared `ScopeKey` struct (whose
/// literal lives in Track A's daemon composition — extending its fields would break
/// that, which the loop forbids touching unilaterally). The operator gets the same
/// outcome — persona verdicts in the weekly digest — with no daemon-composition break.
pub fn weekly_persona_proposals(
    inputs: &[PersonaReviewInput],
    min_resolved: usize,
) -> Vec<PersonaPromotionProposal> {
    inputs
        .iter()
        .map(|input| {
            let card = score_persona(&input.record);
            propose_promotion(
                &card,
                input.prior.as_ref(),
                input.no_persona,
                input.market,
                min_resolved,
            )
        })
        .collect()
}
