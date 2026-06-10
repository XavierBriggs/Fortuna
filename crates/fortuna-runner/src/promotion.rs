//! Stage promotion records (I7, spec Section 11): "promotion is a human
//! decision against these thresholds; demotion is automatic on breach."
//!
//! A strategy's DECLARED stage (code/config) is a CAP, not an
//! entitlement. The stage it actually runs at is derived by walking its
//! operator action records from Sim upward: each promotion step
//! requires a contiguous record (no skipped stages) by a HUMAN actor
//! ("system" or blank actors cannot promote). Demotion steps apply
//! regardless of actor — the system may always retreat, never advance,
//! on its own authority. The composition sources records from the audit
//! log (operator CLI writes them); this module is the pure derivation.

use crate::Stage;

/// One stage-change record (an operator action row in the composition).
#[derive(Debug, Clone)]
pub struct PromotionRecord {
    pub strategy: String,
    pub from: Stage,
    pub to: Stage,
    /// Operator identity. Promotions with a blank or "system" actor are
    /// IGNORED — promotion is a human action (I7).
    pub actor: String,
    /// UTC ISO8601 of the action (records are walked in slice order;
    /// the composition supplies them time-ordered).
    pub at: String,
}

impl PromotionRecord {
    fn human_actor(&self) -> bool {
        let actor = self.actor.trim();
        !actor.is_empty() && !actor.eq_ignore_ascii_case("system")
    }
}

fn next_up(stage: Stage) -> Option<Stage> {
    match stage {
        Stage::Sim => Some(Stage::Paper),
        Stage::Paper => Some(Stage::LiveMin),
        Stage::LiveMin => Some(Stage::Scaled),
        Stage::Scaled => None,
    }
}

/// Derive the stage a strategy actually runs at. Everything starts at
/// Sim; promotion records advance ONE contiguous step at a time and
/// require a human actor; demotion records (to < from) apply whenever
/// they match the current stage, any actor. The declared stage caps the
/// result: records can never raise a strategy above its declaration.
pub fn effective_stage(declared: Stage, records: &[PromotionRecord]) -> Stage {
    let mut current = Stage::Sim;
    for record in records {
        if record.to > record.from {
            // Promotion: exactly one contiguous step, human actor only.
            if record.from == current && next_up(current) == Some(record.to) && record.human_actor()
            {
                current = record.to;
            }
        } else if record.to < record.from && record.from == current {
            // Demotion: automatic on breach — any actor may step down.
            current = record.to;
        }
    }
    current.min(declared)
}
