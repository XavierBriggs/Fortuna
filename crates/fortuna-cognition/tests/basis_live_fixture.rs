//! `perp_event_basis` BASIS KERNEL — the REAL-DATA end-to-end test (the
//! verifier-required e2e, now on REAL co-recorded data).
//!
//! Flow (end-to-end against the committed LIVE paired-cycle recording):
//!   1. Load `fixtures/perp-basis/paired_cycle_btc_perp_vs_kxbtc.json`
//!      ONE cycle_id-aligned pair: the KXBTCPERP perp (settlement_mark) + the
//!      KXBTC price-LEVEL ladder (50 active markets: 48 `between` $500 bins
//!      $51k→$75k + 1 `greater` top tail + 1 `less` bottom tail).
//!   2. Parse the `kxbtc_ladder` into `BracketBin`s: map the three
//!      `strike_type`s (`between`/`greater`/`less`) to `BracketStrike`, and the
//!      YES dollar-strings `(yes_bid + yes_ask)/2` to each bin's `f64`
//!      probability (the caller's dollar-string → probability boundary; the
//!      kernel stays string-format-agnostic).
//!   3. Read the perp `settlement_mark_dollars` (the BTC-spot value — the perp
//!      contract is BTC/10000, so `*_per_contract_dollars × 10000`; the fixture
//!      carries the already-scaled BTC value, which is the basis comparator's
//!      input).
//!   4. Call `compute_basis` and ASSERT the implied median ≈ $63,961 and the
//!      signed basis ≈ −$55 (the GAPS-validated numbers), within a few-dollar
//!      tolerance. Print the numbers (the headline: the kernel works on the
//!      live fixture).
//!
//! This is the e2e the verifier required, on REAL co-recorded market data
//! (perp book vs bracket ladder — two fully independent price sources that
//! agree to <0.1%). Synthetic LOGIC tests live in `basis.rs`.

use fortuna_cognition::basis::{bracket_implied_median, compute_basis, BracketBin, BracketStrike};
use serde::Deserialize;
use std::path::PathBuf;

/// The GAPS-validated numbers (paired_cycle_btc_perp_vs_kxbtc.meta.md):
/// perp settlement_mark → BTC $63,906.00, ladder implied median $63,961.53,
/// signed basis −$55.53.
const EXPECTED_MEDIAN: f64 = 63_961.53;
const EXPECTED_PERP_BTC: f64 = 63_906.00;
const EXPECTED_BASIS: f64 = -55.53;
/// A few-dollar tolerance: the kernel reproduces the validated numbers to well
/// inside a dollar (the f64 interpolation is exact), but the assertions allow a
/// few dollars of slack so a benign re-quantization of the fixture would not
/// red the e2e.
const TOL_DOLLARS: f64 = 3.0;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/perp-basis/paired_cycle_btc_perp_vs_kxbtc.json")
}

// ── the fixture shape (only the fields the basis needs) ──────────────────────

#[derive(Debug, Deserialize)]
struct PairedCycle {
    perp: Perp,
    kxbtc_ladder: Vec<LadderMarket>,
}

#[derive(Debug, Deserialize)]
struct Perp {
    /// The perp settlement mark as BTC SPOT dollars (per-contract × 10000 —
    /// the fixture carries the already-scaled BTC value). This is the basis
    /// comparator's perp-mark input.
    settlement_mark_dollars: String,
    /// The raw per-contract value (BTC/10000), kept only to PRINT the scale
    /// relationship in the e2e output.
    settlement_mark_per_contract_dollars: String,
}

#[derive(Debug, Deserialize)]
struct LadderMarket {
    strike_type: String,
    floor_strike: Option<f64>,
    cap_strike: Option<f64>,
    yes_bid_dollars: String,
    yes_ask_dollars: String,
    status: String,
}

/// Parse a YES dollar-string (`"0.0600"` = $0.06) into its `f64` value. On a
/// $1 payout the YES mid `(bid+ask)/2` is the bin's implied probability — this
/// is the caller's dollar-string → probability boundary (the kernel takes the
/// probability, not the string).
fn parse_dollars(s: &str) -> f64 {
    s.parse::<f64>()
        .unwrap_or_else(|e| panic!("YES dollar-string {s:?} → f64: {e}"))
}

/// Map one active KXBTC market to a `BracketBin`: the `strike_type` →
/// `BracketStrike`, and the YES mid → the bin probability. Returns `None` for a
/// market that is not `active` or that is missing a strike its kind requires
/// (defensive — the recorder only emits well-formed rows, but the parse is
/// total).
fn to_bin(m: &LadderMarket) -> Option<BracketBin> {
    if m.status != "active" {
        return None;
    }
    let prob = (parse_dollars(&m.yes_bid_dollars) + parse_dollars(&m.yes_ask_dollars)) / 2.0;
    let kind = match m.strike_type.as_str() {
        "between" => BracketStrike::Between {
            floor: m.floor_strike?,
            cap: m.cap_strike?,
        },
        "greater" => BracketStrike::Greater {
            floor: m.floor_strike?,
        },
        "less" => BracketStrike::Less { cap: m.cap_strike? },
        other => panic!("unexpected KXBTC strike_type {other:?} in the live fixture"),
    };
    Some(BracketBin { kind, prob })
}

#[test]
fn basis_kernel_on_live_paired_cycle() {
    // 1. Load the committed live paired-cycle fixture.
    let path = fixture_path();
    let bytes = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read live fixture {}: {e}", path.display()));
    let cycle: PairedCycle = serde_json::from_str(&bytes)
        .unwrap_or_else(|e| panic!("parse live fixture {}: {e}", path.display()));

    // 2. Parse the ladder (all three strike types) → BracketBins.
    let bins: Vec<BracketBin> = cycle.kxbtc_ladder.iter().filter_map(to_bin).collect();

    // Census the partition (grounded against the fixture / meta.md: 48 between
    // + 1 greater + 1 less, all active).
    let n_between = bins
        .iter()
        .filter(|b| matches!(b.kind, BracketStrike::Between { .. }))
        .count();
    let n_greater = bins
        .iter()
        .filter(|b| matches!(b.kind, BracketStrike::Greater { .. }))
        .count();
    let n_less = bins
        .iter()
        .filter(|b| matches!(b.kind, BracketStrike::Less { .. }))
        .count();
    assert_eq!(n_between, 48, "live ladder has 48 active `between` bins");
    assert_eq!(n_greater, 1, "live ladder has 1 active `greater` top tail");
    assert_eq!(n_less, 1, "live ladder has 1 active `less` bottom tail");

    // 3. The perp mark as BTC-spot dollars (the comparator input). The fixture
    //    carries it already-scaled; PRINT the per-contract value to show the
    //    ×10000 (BTC/10000 contract) scale relationship.
    let perp_btc = cycle
        .perp
        .settlement_mark_dollars
        .parse::<f64>()
        .unwrap_or_else(|e| panic!("perp settlement_mark_dollars → f64: {e}"));
    let perp_per_contract = cycle
        .perp
        .settlement_mark_per_contract_dollars
        .parse::<f64>()
        .unwrap_or_else(|e| panic!("perp settlement_mark_per_contract_dollars → f64: {e}"));

    // 4. The implied median over the FULL partition (tails included in the
    //    cumulative; the crossing lands in a `between` bin for this cycle).
    let median = bracket_implied_median(&bins).expect("the live ladder has a `between`-bin median");

    // The full basis signal. fee_floor/min_basis are nominal here (this test
    // pins the median + basis NUMBERS, not the fee-trap verdict — that is
    // covered by the synthetic fee-trap test).
    let sig =
        compute_basis(&bins, perp_btc, 10.0, 5.0).expect("a median exists, so a signal exists");

    // ── the headline: the kernel reproduces the GAPS-validated numbers ──
    println!("[basis kernel — LIVE paired cycle btc_perp vs kxbtc]");
    println!(
        "  perp settlement_mark (per-contract): ${perp_per_contract}  ×10000 → BTC ${perp_btc}"
    );
    println!("  active bins: {n_between} between + {n_greater} greater + {n_less} less");
    println!("  ladder implied MEDIAN  : ${median:.2}   (GAPS ≈ ${EXPECTED_MEDIAN})");
    println!("  perp BTC mark          : ${perp_btc:.2}   (GAPS ≈ ${EXPECTED_PERP_BTC})");
    println!(
        "  SIGNED BASIS (perp−median): ${:.2}   (GAPS ≈ ${EXPECTED_BASIS})",
        sig.signed_basis
    );

    // The perp mark read straight from the fixture is the validated BTC value.
    assert!(
        (perp_btc - EXPECTED_PERP_BTC).abs() < TOL_DOLLARS,
        "perp BTC mark {perp_btc} ≉ GAPS {EXPECTED_PERP_BTC}"
    );
    // The contract scale: per-contract × 10000 = BTC spot (BTC/10000 contract).
    assert!(
        (perp_per_contract * 10_000.0 - perp_btc).abs() < 1e-6,
        "per-contract × 10000 must equal the BTC settlement mark"
    );
    // The kernel's implied median matches the validated $63,961.53.
    assert!(
        (median - EXPECTED_MEDIAN).abs() < TOL_DOLLARS,
        "implied median {median} ≉ GAPS {EXPECTED_MEDIAN}"
    );
    assert!(
        (sig.bracket_implied_median - median).abs() < 1e-9,
        "the signal's median equals the standalone median"
    );
    // The signed basis matches the validated −$55.53.
    assert!(
        (sig.signed_basis - EXPECTED_BASIS).abs() < TOL_DOLLARS,
        "signed basis {} ≉ GAPS {EXPECTED_BASIS}",
        sig.signed_basis
    );
    // Sign: the perp mark sits fractionally BELOW the ladder median this cycle.
    assert!(
        sig.signed_basis < 0.0,
        "this cycle's basis is negative (perp below ladder median), got {}",
        sig.signed_basis
    );
}
