# Operator decisions — recorded 2026-06-12 20:41 UTC (verbatim: "Ok I approve everything except the leverage cap")

1. PROTECTED-CRATE WAIVE BATCH 5: APPROVED. The track-C invariant
   additions (perp I1 seal, I2 margin-equity extension, I3 cross-domain
   halt) — verified pure additions (628 insertions / 0 deletions) across
   four gate audits. This record converts the rule-based automatic BLOCKs
   for that touch. (Batches 1-4 were signed 2026-06-11, recorded at
   825d144.)
2. F1 DISPOSITION: ACCEPTED. The T5.B5 tick-wording correction + the
   recorded-curves converter (RiskCurve::from_leverage_estimates, shape-
   tested against fixtures) stand as the resolution; re-gate verified.
3. REARM SEMANTICS: AGREED. Option (a) restart-gated rearm stands
   (I2-conservative); the CLI/ROTA notices remain queued from the pool.
4. LEVERAGE CAP: 2x CONFIRMED (operator, 2026-06-12 21:30 UTC, after
   explanation). BUILD ITEM (pool/track-C): a [perp] config entry
   `max_leverage = 2.0` (or integer bps form per house style), enforced in
   the leverage-cap gate check as min(config, venue curve), with a pinned
   test asserting an order at 2.01x is refused while 1.99x passes, and an
   ASSUMPTIONS note that loosening it later is an I7-style operator
   review. Until built, the venue curves remain the ceiling — the build
   item closes that gap.
