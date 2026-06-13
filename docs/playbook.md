 The pattern: a Ralph fleet + one independent verifier

  The shape (this is the replicable core):
  - N autonomous build loops — each in its own git worktree, each owning one vertical track (venue/exec, observability, cognition/perps, sources, personas). Each runs continuously: take
  the next queue item, write tests first, build, commit, repeat; RALPH-STOP when its queue is exhausted.
  - One verifier/orchestrator (this session) — a separate context that never builds. It owns main: it adversarially gates every tranche, merges, maintains the coordination bus + the
  shared docs, manages disk, and thinks like the team's principal engineer.
  - One operator (you) — you make only the decisions that are irreversible or outward-facing (promotions, sign-offs, credentials, model/config choices, starting the soak) and you trigger
  verification passes. The agents do everything else.

  Why it actually works: the builder and the verifier are different agents with different incentives. The builder optimizes to ship; the verifier optimizes to catch. That independence —
  not any one model being smart — is the whole game.

  The coordination surface

  - One bus (docs/reviews/GATE-FINDINGS-LATEST.md), verifier-owned. Tracks read it at priority and ledger responses in GAPS.md; they never edit it.
  - GAPS = deferred/operator-blocked (forward). CHANGELOG = landed (backward). operator.md = your action list. BUILD_PLAN = the task list.
  - A protected invariants crate — the 7 safety properties as executable tests, additions-only, never weakened. The constitutional floor; touching it = auto-block.

  The gate discipline (the quality bar that held)

  1. Evidence before verdict — never trust a claim; re-query reality (git, the DB, the running process). "Demo is live" → I checked, it was the sim soak.
  2. Mutation-proof everything — break the tested property, confirm a test goes red. Green alone is not verification. Every merge this session was mutation-proven.
  3. Gate the merged tree, not the branch tip — a track's "complete" is relative to its base (caught a 91-commit staleness that would've reverted work).
  4. Fixtures are real, provenanced, secrets-scanned — never fabricated.
  5. Surface real scope; never fake progress — the demo-flip looked like a config line; it's a 4-phase feature. I said so rather than shipping a gate-open-no-venue half-state.

  The merge mechanic: merge --no-commit → gate the integrated tree → mutation-proof → seal or block → post-merge check --workspace + invariants → record on the bus + CHANGELOG.
  Union-merge shared docs.

  What we accomplished (this session)

  Gated + merged to main, every piece mutation-proven, all 7 invariants intact:
  - 3 belief producers — perp funding-forecast, perp/bracket basis, weather/macro personas
  - the first perp trader (perp_event_basis strategy, Sim)
  - the full ROTA observability console
  - the kill-switch I4 Kalshi live-plug + the Slack-halt I2 listener (the most safety-critical surface)
  - the Kalshi adapter clearance

  Plus real catches that protected the system: a two-ticker data inconsistency, the staleness revert, an orphan-vs-superseded false alarm, the demo-flip's true scope, and why the soak is
  idle (the input pipeline is empty, not just a toggle).

  The lessons worth carrying (replicate the mitigations, not just the wins)

  - Disk is the throttle — reclaim idle targets only, gated on 0-active; the check-then-act race is real.
  - Don't over-read operator intent — a "hold the quality bar" phrase is not a build authorization.
  - Filter the noise — routine per-commit pings get a one-liner; surface only what needs you.
  - Content ≠ ancestry — an "orphaned" commit may already be in main via another path; diff before recovering.

  ---
  The one-sentence version to replicate: fan out autonomous builders in isolated worktrees, put one adversarial verifier between them and main, give them a single written bus to
  coordinate, encode your non-negotiables as protected executable tests, and keep the human on only the irreversible decisions.