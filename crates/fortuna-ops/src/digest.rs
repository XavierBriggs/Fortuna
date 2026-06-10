//! The daily digest (spec Section 8: #fortuna-digest, daily morning
//! digest). Composition is a PURE function of explicit inputs — the
//! caller assembles `DigestInputs` from runner/ledger state and sends the
//! text through the audited Slack router; nothing here reads clocks,
//! databases, or networks.

/// One strategy's day in numbers (cents; the composer renders dollars).
#[derive(Debug, Clone)]
pub struct StrategyDigestRow {
    pub strategy: String,
    pub realized_pnl_cents: i64,
    pub fees_cents: i64,
    pub fills: u64,
    pub open_exposure_cents: i64,
}

#[derive(Debug, Clone)]
pub struct DigestInputs {
    /// UTC date this digest covers (day boundary 00:00 UTC, spec).
    pub date_utc: String,
    /// sim | paper | live-min | scaled — the digest never hides stage.
    pub stage: String,
    pub strategies: Vec<StrategyDigestRow>,
    pub halts_active: u64,
    pub discrepancies_open: u64,
    pub settlements_overdue: u64,
    pub capital_in_limbo_cents: i64,
    pub veto_decisions: u64,
    pub veto_suppressed: u64,
}

fn dollars(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("${sign}{}.{:02}", abs / 100, abs % 100)
}

/// Deterministic digest text. Honesty numbers (halts, discrepancies,
/// overdue settlements, capital in limbo) always surface — a digest that
/// only celebrates PnL is a marketing email.
pub fn compose_daily_digest(inputs: &DigestInputs) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "FORTUNA daily digest — {} (stage: {})\n",
        inputs.date_utc, inputs.stage
    ));
    let gross: i64 = inputs.strategies.iter().map(|s| s.realized_pnl_cents).sum();
    let fees: i64 = inputs.strategies.iter().map(|s| s.fees_cents).sum();
    let fills: u64 = inputs.strategies.iter().map(|s| s.fills).sum();
    out.push_str(&format!(
        "realized PnL {} | fees {} | net {} | fills {}\n",
        dollars(gross),
        dollars(fees),
        dollars(gross - fees),
        fills
    ));
    for s in &inputs.strategies {
        out.push_str(&format!(
            "  {}: pnl {} fees {} fills {} exposure {}\n",
            s.strategy,
            dollars(s.realized_pnl_cents),
            dollars(s.fees_cents),
            s.fills,
            dollars(s.open_exposure_cents)
        ));
    }
    out.push_str(&format!(
        "halts active: {} | discrepancies open: {} | settlements overdue: {}\n",
        inputs.halts_active, inputs.discrepancies_open, inputs.settlements_overdue
    ));
    out.push_str(&format!(
        "capital in limbo: {} | vetoes: {} ({} suppressed)\n",
        dollars(inputs.capital_in_limbo_cents),
        inputs.veto_decisions,
        inputs.veto_suppressed
    ));
    out
}
