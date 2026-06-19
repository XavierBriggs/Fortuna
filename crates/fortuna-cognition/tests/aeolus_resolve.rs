//! WS1 Task 3 — producer-agnostic + fractional weather resolution.
//!
//! Tests 1-4 from the brief:
//!   1. Head-to-head: Aeolus + meteorologist beliefs for the SAME bracket score
//!      IDENTICALLY — both get `(outcome, brier)`, neither is skipped.
//!   2. Fractional: `#ge87.5` resolves correctly; reverts to i64 → silent None.
//!   3. Grammar parity: both event-id grammars parse to the same `(comparison,
//!      threshold)`.
//!   4. Aeolus integer regression: byte-identical `(outcome, brier)` vs pre-change.
//!
//! Tests 4 + 6 (scorecard regression) are the existing tests in aeolus_reliability.rs;
//! they must stay green after the i64→f64 widening.

use fortuna_cognition::aeolus_forecast::Comparison;
use fortuna_cognition::aeolus_resolve::{parse_bracket_hint, score_bracket};

// ── Test 3: Grammar parity ────────────────────────────────────────────────────
//
// Both event-id grammars must parse to the same (Comparison, threshold f64).
// Aeolus: `aeolus:knyc-2026-06-13-tmax-ge87`   (separator = `-`, last token = `ge87`)
// Persona: `weather:KNYC:tmax:2026-06-12#ge87`  (separator = `#`, last token = `ge87`)
// A bare `ge87` (the raw hint after any stripping) must also work.

#[test]
fn grammar_parity_aeolus_and_persona_event_ids_parse_identically() {
    // Full Aeolus event_id (the daemon passes the WHOLE event_id now — no strip_prefix).
    let aeolus = parse_bracket_hint("aeolus:knyc-2026-06-13-tmax-ge87");
    // Full persona event_id with `#` separator before the bracket token.
    let persona = parse_bracket_hint("weather:KNYC:tmax:2026-06-12#ge87");
    // Bare hint (legacy / direct usage).
    let bare = parse_bracket_hint("knyc-2026-06-13-tmax-ge87");

    assert!(aeolus.is_some(), "Aeolus full event_id must parse");
    assert!(persona.is_some(), "Persona full event_id must parse");
    assert!(bare.is_some(), "Bare hint must parse");

    // All three must agree on (comparison, threshold).
    assert_eq!(
        aeolus, persona,
        "Aeolus and persona grammars parse identically"
    );
    assert_eq!(
        bare, persona,
        "Bare hint and persona grammar parse identically"
    );

    // Check the actual values.
    let (cmp, thr) = aeolus.unwrap();
    assert_eq!(cmp, Comparison::Ge);
    assert!((thr - 87.0).abs() < 1e-12, "threshold = 87.0, got {thr}");
}

#[test]
fn grammar_parity_lt_bracket_both_grammars() {
    let aeolus = parse_bracket_hint("aeolus:knyc-2026-06-13-tmin-lt60");
    let persona = parse_bracket_hint("weather:KNYC:tmin:2026-06-12#lt60");
    assert_eq!(aeolus, persona, "lt grammar parity");
    let (cmp, thr) = aeolus.unwrap();
    assert_eq!(cmp, Comparison::Lt);
    assert!((thr - 60.0).abs() < 1e-12, "threshold = 60.0, got {thr}");
}

// ── Test 1: Head-to-head (the thesis) ────────────────────────────────────────
//
// Seed an Aeolus belief (event_id = "aeolus:knyc-2026-06-13-tmax-ge87") AND a
// meteorologist belief (event_id = "weather:KNYC:tmax:2026-06-13#ge87") for the
// SAME bracket + same `p`. Both must produce the SAME (outcome, brier). Before
// this slice the meteorologist belief was silently skipped (the daemon's
// `strip_prefix("aeolus:")` returned None and continued).

#[test]
fn head_to_head_aeolus_and_meteorologist_score_identically() {
    let p = 0.6719055375922601; // the recorded knyc ge87 probability
    let realized_high = 91.0_f64; // 91°F > 87 => TRUE

    // Aeolus event_id: the full event_id the daemon now passes directly.
    let aeolus_score = score_bracket("aeolus:knyc-2026-06-13-tmax-ge87", p, realized_high);
    // Meteorologist event_id: the persona grammar with `#` separator.
    let meteo_score = score_bracket("weather:KNYC:tmax:2026-06-13#ge87", p, realized_high);

    // NEITHER must be None — this was the bug: the meteorologist was skipped.
    assert!(
        aeolus_score.is_some(),
        "Aeolus belief must be scored (not None)"
    );
    assert!(
        meteo_score.is_some(),
        "Meteorologist belief MUST be scored — this is the thesis; None means the skip bug is back"
    );

    // Both must agree on the exact (outcome, brier) tuple.
    assert_eq!(
        aeolus_score, meteo_score,
        "Aeolus and meteorologist score identically for the same bracket + p"
    );

    // Spot-check the values: 91 ≥ 87 => TRUE, brier = (p−1)².
    let (outcome, brier) = aeolus_score.unwrap();
    assert!(outcome, "91 ≥ 87 => TRUE");
    assert!(
        (brier - (p - 1.0).powi(2)).abs() < 1e-12,
        "brier = (p−1)², got {brier}"
    );
}

#[test]
fn head_to_head_not_satisfied_case() {
    // realized 86 < 87 => FALSE for both grammars.
    let p = 0.6719055375922601;
    let realized = 86.0_f64;

    let aeolus = score_bracket("aeolus:knyc-2026-06-13-tmax-ge87", p, realized);
    let meteo = score_bracket("weather:KNYC:tmax:2026-06-13#ge87", p, realized);

    assert!(aeolus.is_some());
    assert!(meteo.is_some());
    assert_eq!(aeolus, meteo);

    let (outcome, brier) = aeolus.unwrap();
    assert!(!outcome, "86 < 87 => FALSE");
    assert!((brier - p * p).abs() < 1e-12, "brier = p², got {brier}");
}

// ── Test 2: Fractional brackets ───────────────────────────────────────────────
//
// A fractional threshold (ge87.5) must parse and resolve correctly:
//   realized 86 => FALSE (86 < 87.5)
//   realized 88 => TRUE  (88 ≥ 87.5)
//
// Mutation-check comment: reverting parse_bracket_hint to `i64::parse` would make
// "ge87.5".parse::<i64>() return Err, leaving the belief unscored (None).

#[test]
fn fractional_bracket_resolves_correctly() {
    let event_id = "weather:KNYC:tmax:2026-06-13#ge87.5";
    let p = 0.5;

    // Parse must succeed and yield threshold=87.5.
    let parsed = parse_bracket_hint(event_id);
    assert!(
        parsed.is_some(),
        "fractional bracket must parse — None means the i64::parse bug is back"
    );
    let (cmp, thr) = parsed.unwrap();
    assert_eq!(cmp, Comparison::Ge);
    assert!(
        (thr - 87.5).abs() < 1e-12,
        "threshold must be 87.5, got {thr}"
    );

    // realized 86 < 87.5 => FALSE.
    let below = score_bracket(event_id, p, 86.0);
    assert!(
        below.is_some(),
        "score_bracket must not return None for fractional bracket"
    );
    let (out_below, _) = below.unwrap();
    assert!(!out_below, "86 < 87.5 => FALSE");

    // realized 88 >= 87.5 => TRUE.
    let above = score_bracket(event_id, p, 88.0);
    assert!(above.is_some());
    let (out_above, _) = above.unwrap();
    assert!(out_above, "88 >= 87.5 => TRUE");

    // Exact boundary: realized 87.5 >= 87.5 => TRUE.
    let boundary = score_bracket(event_id, p, 87.5);
    assert!(boundary.is_some());
    let (out_boundary, _) = boundary.unwrap();
    assert!(out_boundary, "87.5 >= 87.5 => TRUE (ge is >=, inclusive)");
}

#[test]
fn fractional_lt_bracket_resolves_correctly() {
    let event_id = "weather:KNYC:tmin:2026-06-13#lt59.5";
    let p = 0.5;

    // realized 59 < 59.5 => TRUE.
    let (out, _) = score_bracket(event_id, p, 59.0).expect("must score");
    assert!(out, "59 < 59.5 => TRUE");

    // realized 60 >= 59.5 => FALSE.
    let (out2, _) = score_bracket(event_id, p, 60.0).expect("must score");
    assert!(!out2, "60 >= 59.5 => FALSE (lt is strict <)");
}

// ── Test 4: Aeolus integer regression (golden) ───────────────────────────────
//
// The existing integer-bracket resolution must be byte-identical to pre-change.
// The existing inline test `scores_a_bracket_against_the_persisted_probability`
// in aeolus_resolve.rs covers this; we add an external regression test here
// to lock down the specific golden values from the recorded knyc_tmax fixture.

#[test]
fn aeolus_integer_bracket_golden_regression() {
    // These values come directly from the recorded knyc_tmax fixture (ge87, p≈0.672).
    // The pre-change daemon stripped "aeolus:" before calling score_bracket; now
    // the FULL event_id is passed — both must give the same answer.
    let p = 0.6719055375922601;

    // Pre-change: score_bracket("knyc-2026-06-13-tmax-ge87", p, 91.0)
    let old_style = score_bracket("knyc-2026-06-13-tmax-ge87", p, 91.0);
    // Post-change: score_bracket("aeolus:knyc-2026-06-13-tmax-ge87", p, 91.0)
    let new_style = score_bracket("aeolus:knyc-2026-06-13-tmax-ge87", p, 91.0);

    assert!(old_style.is_some(), "pre-change style must still work");
    assert!(new_style.is_some(), "new full-event-id style must work");
    assert_eq!(
        old_style, new_style,
        "old (stripped) and new (full) event_id style produce identical scores"
    );

    let (outcome, brier) = old_style.unwrap();
    assert!(outcome, "91 >= 87 => TRUE");
    // brier = (0.6719055375922601 − 1.0)² — pinned to the exact value.
    let expected_brier = (p - 1.0_f64).powi(2);
    assert!(
        (brier - expected_brier).abs() < 1e-12,
        "brier = (p−1)² = {expected_brier}, got {brier}"
    );
}

// ── Non-finite guard ─────────────────────────────────────────────────────────

#[test]
fn non_finite_threshold_is_rejected() {
    // A crafted event_id that would produce NaN or Inf if not guarded.
    // These can't come from normal producers but must be fail-closed.
    assert_eq!(
        parse_bracket_hint("weather:X#geNaN"),
        None,
        "NaN threshold must be rejected"
    );
    assert_eq!(
        parse_bracket_hint("weather:X#geInf"),
        None,
        "Inf threshold must be rejected (not a valid temperature)"
    );
    assert_eq!(
        parse_bracket_hint("weather:X#ge-inf"),
        None,
        "negative-inf prefix splits by '-', last token = 'inf', fails is_finite"
    );
}
