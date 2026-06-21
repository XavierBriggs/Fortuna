//! Pool-Adjacent-Violators (PAV) weighted isotonic regression.
//!
//! Given `values` and matching `weights`, [`pav`] returns the nondecreasing
//! sequence that minimises the weighted squared error to `values` — the
//! standard isotonic fit. This is the binning-free machinery behind the CORP
//! reliability decomposition (research §3.1, Dimitriadis–Gneiting–Jordan 2021):
//! the recalibrated event-rate per the p-sorted order is exactly the PAV fit of
//! the outcomes.
//!
//! Pure: `std` only. No panic — empty input yields empty output, and a
//! length mismatch returns a clone of `values` rather than indexing out of
//! bounds (a defensive contract; callers always pass matched slices).

/// One contiguous pooled block: the weighted mean of its members and the
/// total weight + member count it covers.
struct Block {
    mean: f64,
    weight: f64,
    len: usize,
}

/// Weighted Pool-Adjacent-Violators isotonic (nondecreasing) regression.
///
/// Returns a `Vec<f64>` the same length as `values`, nondecreasing, equal to
/// the weighted-least-squares isotonic fit. The classic single-pass merge keeps
/// a stack of pooled blocks; whenever the incoming block's mean is below the top
/// of the stack it is merged (weighted), repeated until monotonicity is
/// restored — overall O(n) amortised.
///
/// Contract: empty input → empty output; mismatched lengths → clone of
/// `values` (no panic). Zero/negative total block weight degenerates to a plain
/// arithmetic mean over the block's members, so all-zero weights still yield a
/// valid monotone fit rather than a division-by-zero.
pub fn pav(values: &[f64], weights: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }
    if values.len() != weights.len() {
        // Defensive: never index past either slice.
        return values.to_vec();
    }

    let mut stack: Vec<Block> = Vec::with_capacity(values.len());
    for (i, &v) in values.iter().enumerate() {
        let w = weights[i];
        let mut block = Block {
            mean: v,
            weight: w,
            len: 1,
        };
        // Merge while the previous block's mean violates monotonicity.
        // `pop()` (not `last()`+`expect`) keeps the borrow short and panic-free.
        while let Some(prev) = stack.pop() {
            if prev.mean <= block.mean + f64::EPSILON {
                stack.push(prev); // monotone — restore and stop.
                break;
            }
            block = merge(&prev, &block);
        }
        stack.push(block);
    }

    let mut out = Vec::with_capacity(values.len());
    for block in &stack {
        for _ in 0..block.len {
            out.push(block.mean);
        }
    }
    out
}

/// Merge two adjacent blocks into their weighted mean (falling back to a
/// member-count mean when the combined weight is non-positive).
fn merge(a: &Block, b: &Block) -> Block {
    let weight = a.weight + b.weight;
    let len = a.len + b.len;
    let mean = if weight > 0.0 {
        (a.mean * a.weight + b.mean * b.weight) / weight
    } else {
        // All-zero (or non-positive) weights: average by member count so the
        // pooled value stays finite and monotone.
        (a.mean * a.len as f64 + b.mean * b.len as f64) / len as f64
    };
    Block { mean, weight, len }
}
