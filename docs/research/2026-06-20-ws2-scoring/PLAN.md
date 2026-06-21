# Deep-research PLAN — WS2 scoring-math grounding

**Restated query.** Ground FORTUNA's WS2 "proof layer" probabilistic-forecast scoring math in
the authoritative literature + industry practice before implementing. Three goals: (1) CORRECTNESS
of the formulas/decompositions; (2) INDUSTRY PRACTICE for evaluating + promoting probabilistic
forecasts; (3) creative-but-sound methods for an EDGE. Deliverable = ADOPT-in-WS2 vs DEFER-to-WS3
recommendations with citations, tied to `docs/superpowers/specs/2026-06-20-ws2-proof-layer-design.md`.

**Query type.** Depth-first (one core question — "is our scoring math correct + best-in-class?" —
attacked from several methodological angles).

**Subagent tasks (parallel wave 1):**
- SA-1 (correctness, discrete): Brier + Murphy/Bröcker decomposition (REL/RES/UNC, the identity,
  binning bias + debiased alternatives), RPS for ordinal ladders, Log score + ε-handling, reliability
  diagrams + binning bias. Authorities: Murphy 1973, Brier 1950, Bröcker 2009, Gneiting & Raftery 2007.
- SA-2 (correctness, distributional): CRPS (quantile/pinball form), PIT histogram + calibration
  interpretation + uniformity, the calibration taxonomy. Authorities: Gneiting–Balabdaoui–Raftery 2007,
  Dawid, Hersbach 2000 (CRPS decomposition), Laio–Tamea (PIT).
- SA-3 (industry practice + GO-gate honesty): how quant finance / prediction markets / sports-betting
  (CLV) / weather-epi (CRPS, WIS) / ML evaluate + promote probabilistic forecasts; multiple-testing /
  backtest-overfitting toolkit — Deflated Sharpe, PBO/CSCV, Diebold–Mariano. Authorities: Bailey &
  López de Prado, Diebold–Mariano 1995, COVID-hub WIS (Bracher et al 2021).
- SA-4 (edge methods): CORP reliability diagrams + consistency bands (Dimitriadis–Gneiting–Jordan 2021),
  Murphy diagrams / consistent scoring functions + forecast dominance (Ehm–Gneiting–Jordan–Krüger 2016),
  isotonic/Platt recalibration, decision-curve / economic value, CLV-as-primary-signal.

**Answer format.** A decision-grade report (`REPORT.md` → `docs/research/2026-06-20-ws2-scoring-grounding.md`):
Summary → Correctness findings (per metric, with any spec corrections) → Industry practice → Edge
methods → ADOPT-now/DEFER table → Confidence & caveats → Sources.

**Verify pass:** re-check the load-bearing claims — the Murphy identity exact form + binning bias, the
Log ε-flooring propriety, CRPS pinball equivalence, PBO/DSR definitions — against fetched primary sources.
