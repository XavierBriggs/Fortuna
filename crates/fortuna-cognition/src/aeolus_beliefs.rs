//! F8: the PROPOSE-ONLY belief producer — turn a parsed+validated
//! `aeolus.forecast/v2` envelope (F6) into beliefs (source contract
//! `docs/design/aeolus-fortuna-source-contract.md` §2, §4).
//!
//! Two outputs, both pure and replay-deterministic, neither carrying an order,
//! a size, a price, or a side (I6 — the harness owns sizing/timing/execution;
//! this step emits beliefs ONLY):
//!
//! 1. **Binary bracket drafts** — one [`beliefs::BeliefDraft`] per `ge`/`lt`
//!    bracket. Each draft carries FORTUNA's OWN probability recomputed from the
//!    envelope's μ/σ via the F6 helpers (`bracket_prob_ge`/`bracket_prob_lt` —
//!    the SAME pinned normal CDF the envelope's own `p`'s used, so the values
//!    agree). This is the producer step: it emits `p_raw` only and sets
//!    `p == p_raw` — NO calibration here (calibration is a downstream layer,
//!    exactly like `funding_forecast`/`reconciliation`). Aeolus's own bracket
//!    `p` rides along in `evidence` as a cross-check DATUM (never an
//!    instruction, §5.11).
//!
//! 2. **One scalar draft** — a [`scalar_beliefs::ScalarBeliefDraft`] carrying
//!    the forecast distribution as a μ/σ quantile fan
//!    (`PredictiveDistribution::Scalar`), the vehicle F9 will CRPS-score against
//!    the realized temperature. The fan uses a PINNED standard-normal quantile
//!    grid (`v = μ + σ·z` for a fixed `(q, z)` table) so it is replay-byte-stable
//!    with no probit/erf-inverse — and because σ>0 is a parse invariant, the
//!    `v`'s are strictly increasing, so the distribution always `validate()`s.
//!
//! `in_bracket` brackets are SKIPPED (a single envelope threshold cannot define
//! a `floor ≤ high < cap` range; pairing two thresholds is a later mapper's
//! job — F6's `bracket_range_prob` takes a caller-supplied pair). The count is
//! surfaced on [`AeolusBeliefs::skipped_in_bracket`] so nothing is dropped
//! silently.
//!
//! f64 here is forecast-domain (probabilities/temperatures) only — never money
//! (§7). No `Clock::now`/`SystemTime`: the horizon is the envelope's
//! `resolution.settles_after`. No `panic!`/`unwrap`/`expect` (crate denies them
//! off-test): a bracket whose probability somehow fails to validate is skipped
//! (with its defect counted), never silently passed as an out-of-range belief.

use crate::aeolus_forecast::{
    bracket_prob_ge, bracket_prob_lt, AeolusForecast, Comparison, Variable,
};
use crate::beliefs::BeliefDraft;
use crate::scalar_beliefs::ScalarBeliefDraft;
use crate::scoring::{PredictiveDistribution, Quantile};
use serde_json::json;

/// The pinned standard-normal quantile grid `(q, z)` with `z = Φ⁻¹(q)`. Fixed
/// constants — NOT computed via an erf-inverse — so the fan is byte-identical
/// across toolchains (replay determinism, §7/I5). Strictly increasing in both
/// `q` and `z`, so `v = μ + σ·z` is strictly increasing for any σ>0 (the parse
/// invariant), which is exactly `validate_scalar`'s requirement.
const QUANTILE_GRID: [(f64, f64); 7] = [
    (0.05, -1.6448536269514722),
    (0.10, -1.2815515655446004),
    (0.25, -0.6744897501960817),
    (0.50, 0.0),
    (0.75, 0.6744897501960817),
    (0.90, 1.2815515655446004),
    (0.95, 1.6448536269514722),
];

/// The scalar-fan unit: the realized value F9 scores against is the NWS daily
/// high/low in degrees Fahrenheit.
const DEGF: &str = "degF";

/// The output of the F8 producer step: the binary bracket drafts, the single
/// scalar draft, and the count of `in_bracket` brackets skipped.
#[derive(Debug, Clone, PartialEq)]
pub struct AeolusBeliefs {
    /// One [`BeliefDraft`] per `ge`/`lt` bracket. Each validated before
    /// inclusion; a (post-parse impossible) validation failure is skipped, not
    /// emitted.
    pub binary: Vec<BeliefDraft>,
    /// The μ/σ distribution as a pinned quantile fan — F9's CRPS vehicle.
    pub scalar: ScalarBeliefDraft,
    /// How many `in_bracket` brackets were skipped (each needs a floor+cap pair
    /// a single threshold cannot supply). Surfaced so the skip is auditable.
    pub skipped_in_bracket: usize,
}

/// The wire string for the forecast variable (`tmax`/`tmin`) — used in the
/// scalar `event_key` and in both drafts' provenance.
fn variable_str(v: Variable) -> &'static str {
    match v {
        Variable::Tmax => "tmax",
        Variable::Tmin => "tmin",
    }
}

/// The provenance block both draft kinds carry. Aeolus is a self-describing
/// source: unlike the LLM mind (which cannot know its own prompt hash, so the
/// harness stamps its provenance), the producer KNOWS its model id, station,
/// variable, target date, run time, and model version — so it stamps them here
/// as DATA (§4). The harness may still augment this downstream.
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

/// Emit propose-only beliefs from a parsed+validated Aeolus forecast.
///
/// Infallible by construction: σ>0 (a parse invariant) means every `ge`/`lt`
/// probability is `Some` and lands strictly inside `(0,1)`, and the scalar fan's
/// `v`'s are strictly increasing — so the scalar draft is always well-formed.
/// The defensive `None`/`validate()` guards below cannot fire post-parse; they
/// keep the path panic-free (no `unwrap`/index) regardless, skipping rather than
/// emitting any draft that would carry an out-of-range probability.
pub fn emit_aeolus_beliefs(fc: &AeolusForecast) -> AeolusBeliefs {
    let mu = fc.mu();
    let sigma = fc.sigma();
    let prov = provenance(fc);
    let horizon = fc.resolution().settles_after;
    let skill = fc.skill();
    let var_str = variable_str(fc.variable());

    let mut binary = Vec::with_capacity(fc.brackets().len());
    let mut skipped_in_bracket = 0usize;

    for bracket in fc.brackets() {
        // FORTUNA's OWN probability from μ/σ via the F6 helpers (the same pinned
        // normal CDF the envelope's `p`'s used). `in_bracket` is skipped: one
        // threshold cannot define a range (the F6 range helper takes a pair).
        let p = match bracket.comparison {
            Comparison::Ge => bracket_prob_ge(bracket.threshold_f, mu, sigma),
            Comparison::Lt => bracket_prob_lt(bracket.threshold_f, mu, sigma),
            Comparison::InBracket => {
                skipped_in_bracket += 1;
                continue;
            }
        };
        // σ>0 post-parse ⇒ always `Some`; `None` (σ≤0 / non-finite) is impossible
        // here but skipped without panic rather than unwrapped.
        let Some(p) = p else {
            continue;
        };

        let evidence = json!([{
            "source": "aeolus",
            "ref": format!("{}@{}", fc.station(), fc.run_at().to_iso8601()),
            "p_aeolus": bracket.p,
            "p_fortuna": p,
            "divergence": (p - bracket.p).abs(),
            "crps": skill.crps,
            "crpss_vs_raw": skill.crpss_vs_raw,
            "n_scored": skill.n_scored,
        }]);

        let draft = BeliefDraft {
            event_id: format!("aeolus:{}", bracket.event_hint),
            // Propose-only (I6): the producer emits p_raw only; no calibration
            // here, so p == p_raw. Calibration is a downstream layer.
            p,
            p_raw: p,
            horizon,
            evidence,
            provenance: prov.clone(),
        };

        // A draft whose probability is somehow out of range MUST NOT silently
        // pass. Post-parse this never fires (p ∈ (ε,1−ε)); the guard skips
        // rather than emitting an invalid belief.
        if draft.validate().is_ok() {
            binary.push(draft);
        }
    }

    // The scalar μ/σ fan — F9's CRPS vehicle. Pinned grid ⇒ no erf-inverse;
    // σ>0 ⇒ strictly-increasing v ⇒ `validate_scalar` always Ok.
    let quantiles: Vec<Quantile> = QUANTILE_GRID
        .iter()
        .map(|&(q, z)| Quantile {
            q,
            v: mu + sigma * z,
        })
        .collect();
    let predictive = PredictiveDistribution::Scalar {
        quantiles,
        unit: DEGF.to_string(),
    };

    let scalar = ScalarBeliefDraft {
        event_key: format!("aeolus:{}:{}:{}", fc.station(), var_str, fc.target_date()),
        predictive,
        horizon,
        evidence: json!({
            "source": "aeolus",
            "mu": mu,
            "sigma": sigma,
            "crps": skill.crps,
            "crpss_vs_raw": skill.crpss_vs_raw,
            "n_scored": skill.n_scored,
            "window_days": skill.window_days,
        }),
        // Aeolus stamps its own provenance (it knows its model id/station), the
        // same block the binary drafts carry.
        provenance: prov,
    };

    AeolusBeliefs {
        binary,
        scalar,
        skipped_in_bracket,
    }
}
