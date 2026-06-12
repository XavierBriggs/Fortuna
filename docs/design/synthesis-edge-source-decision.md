# DECISION: daemon synthesis edges come from EdgesRepo (adjudicated 2026-06-12)

Question (GAPS, design-blocked): where does the daemon-booted
SynthesisStrategy get its edges — EdgesRepo load, config, or the discovery
loop directly?

DECISION: **EdgesRepo, confirmed-tier only, loaded at composition and
refreshed on the daemon's segment cadence.** Adjudicated by the verification
session against the spec's grain; conservative option per CLAUDE.md.

Why the alternatives lose:
- CONFIG-DEFINED EDGES would let an edge trade without ever passing the
  discovery loop's confirmation machinery (T3.2 edge-confirmation cards) —
  a bypass of the validation ladder, I7-adjacent. Config may FILTER
  (categories, venues, max-edge count); it never DEFINES edges.
- DISCOVERY-LOOP-DIRECT (in-memory handoff) couples the daemon's trading
  composition to the discovery loop's liveness and loses replayability —
  the edge set at any tick must be reconstructible from the ledger (I5
  replay standard). The discovery loop's job is to WRITE edges (it already
  does); the ledger is the boundary between the loops.

Binding requirements for the implementation (T4.1 synthesis-in-main):
1. Load via EdgesRepo at composition; CONFIRMED tier only (the structural
   tier rule from T2.1 stays the enforcement — this load is in addition,
   not instead).
2. Refresh once per drive() segment (same cadence class as the halt poll);
   a refresh failure keeps the LAST-KNOWN set and counts/alerts — never
   trades on a guessed set, never crashes the loop.
3. Fail closed: zero confirmed edges => SynthesisStrategy composes with an
   empty set => zero candidates => the daemon runs mechanically-only. An
   empty edge set is a VALID state, not an error.
4. Config surface: [synthesis] filters only (categories allowlist, venue,
   max_edges cap with deterministic truncation order by edge id).
5. Tests: composition with seeded confirmed edges trades them; unconfirmed/
   superseded edges excluded; refresh failure keeps last-known + alerts;
   empty-set boots clean and trades nothing. Populated-path rule applies.

This unblocks: synthesis-in-main, mech_extremes+veto binding, the
mind/CostBudget binding (its consumer now exists), belief persistence in
the booted daemon, rich digest/reviews — i.e., the T4.1 tail and the soak
clock.
