# FORTUNA

Model-driven autonomous trading system. The model is the mind; FORTUNA is everything
that makes the mind safe, stateful, and accountable.

- Design authority: docs/spec.md (v0.8)
- Constitution for agents: CLAUDE.md
- Mission prompt: PROMPT.md  |  Task list: BUILD_PLAN.md
- Start: `cp config/fortuna.example.toml config/fortuna.toml`, provision env secrets,
  then hand PROMPT.md to Claude Code from the repo root.

Layout: crates/ (nine-crate workspace; fortuna-invariants is PROTECTED), fixtures/
(operator-recorded venue API captures), scripts/ (DST + replay), config/.
