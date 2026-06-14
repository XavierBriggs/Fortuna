//! F5: identity-tuple dedup for Aeolus forecasts.
//!
//! The forecast-identity tuple is `(station, target_date, variable, run_at)`
//! (contract `docs/design/aeolus-fortuna-source-contract.md` §2). Two envelopes
//! sharing it ARE the same forecast. The dedup SLOT is everything but `run_at` —
//! `(station, variable, target_date)` — and within a slot the **newest `run_at`
//! wins**: a re-issued forecast (a later GEFS run) supersedes the earlier one and
//! must never be double-counted. A corrected μ/σ at the SAME `run_at` is a
//! revision (contract §3 ETag rule); it is resolved to the LATER-received
//! envelope, which supersedes.
//!
//! Pure + deterministic: first-seen slot order is preserved, so the output is a
//! pure function of the input slice (no clock, no map iteration order, no panic).

use crate::aeolus_forecast::{AeolusForecast, Variable};

/// The dedup slot — one forecast target. The newest `run_at` in a slot is live.
type Slot = (String, Variable, String);

fn slot_of(f: &AeolusForecast) -> Slot {
    (
        f.station().to_string(),
        f.variable(),
        f.target_date().to_string(),
    )
}

/// Collapse `forecasts` to one per `(station, variable, target_date)`: the newest
/// `run_at` wins; a tie on `run_at` (a same-run revision) resolves to the
/// LATER-received envelope (it supersedes, §3). Output preserves the first-seen
/// order of each surviving slot — deterministic for a given input.
pub fn dedup_forecasts(forecasts: Vec<AeolusForecast>) -> Vec<AeolusForecast> {
    // (slot, winner) in first-seen order — a Vec (not a map) keeps the output
    // order a pure function of the input, with no hash/btree iteration surprises.
    let mut kept: Vec<(Slot, AeolusForecast)> = Vec::new();
    for f in forecasts {
        let slot = slot_of(&f);
        match kept.iter_mut().find(|(s, _)| *s == slot) {
            // `>=` so a strictly-newer run wins AND a same-`run_at` revision
            // (later-received) supersedes — never double-counting either.
            Some((_, existing)) => {
                if f.run_at().epoch_millis() >= existing.run_at().epoch_millis() {
                    *existing = f;
                }
            }
            None => kept.push((slot, f)),
        }
    }
    kept.into_iter().map(|(_, f)| f).collect()
}
