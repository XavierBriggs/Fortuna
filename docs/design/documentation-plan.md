# Documentation overhaul plan (operator-directed 2026-06-12)

Method: explore (done — inventory/staleness/source-map) -> this plan ->
4 parallel writers -> the docs gate (EXECUTES quickstart + runbooks).

STYLE CONTRACT (binding on every writer):
- Professional infra-docs voice. Why before how. Zero marketing fluff.
- EVERY command copy-pasteable; the docs gate executes them — write
  nothing you wouldn't run.
- EVERY factual claim traceable: cite the file, config key, or verdict it
  comes from (inline link or footnote). Claims without sources get cut by
  the gate.
- Honest status: what works / what's gated / what's operator-pending, as
  of a stated commit. No aspirational present tense.
- Ledgers (CLAUDE.md, spec, BUILD_PLAN, GAPS, ASSUMPTIONS) stay
  authoritative — docs POINT, never duplicate or paraphrase normatively.
- Consistent vocabulary: the seven invariants by number; the three planes
  (cognition/harness/safety); track names; UTC everywhere.
- Cross-link the set; each doc opens with who it is for and when to read it.

THE SET & OWNERS (parallel writers; none touch docs/design/* or ledgers
except the 3 surgical staleness fixes assigned to W4):
- W1: README.md (root, replaces shallow one) + docs/quickstart.md
- W2: docs/architecture.md (embed docs/diagrams/fortuna-architecture.svg)
      + docs/verification.md (the gate doctrine + the red-seed story)
- W3: docs/runbooks/{soak-start, halt-and-rearm, kill-switch-drill,
      troubleshooting, key-rotation-and-secrets, fixture-recording,
      backup-restore, demo-flip}.md — write what is TRUE today; where a
      procedure does not exist yet (backup), say so and ledger it.
- W4: docs/operations.md (CLI as BUILT + ROTA tour w/ the real screenshots
      in docs/reviews/rota-visual/ + the operator's daily rhythm) + the 3
      staleness fixes (PROMPT.md start/stop names; FINAL_REPORT fixtures
      note; fortuna-cli.md A1 cross-ref).

GATE: a docs-verifier executes docs/quickstart.md end-to-end from a clean
scratch state, executes every runbook command that is safe to execute
(marks the rest REVIEW-VERIFIED with reasoning), spot-traces 10 claims per
doc to their cited sources, and red-flags voice/contract violations.
