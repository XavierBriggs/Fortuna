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
4. LEVERAGE CAP: DECISION DEFERRED — operator requested explanation
   before deciding (provided in-session). Until decided, the binding
   ceiling is the venue's risk curves via the gate arm; no [perp] config
   leverage entry exists.
