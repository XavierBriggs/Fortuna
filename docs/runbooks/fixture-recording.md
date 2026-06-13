# Runbook: venue fixture recording (Kalshi demo)

**Who this is for:** the operator (or operator-authorized agent session)
recording venue API fixtures — the evidence every adapter is built against.
**When to read it:** before any recording session, and before committing any
captures.
**Status:** accurate as of commit `334612d` (2026-06-12). Two sessions have
been lived through; this runbook is those sessions turned into procedure.

Why fixtures: the house rule is "never invent venue API behavior. Build
adapters against `fixtures/kalshi/` recordings" (CLAUDE.md session rules).
Both recorded sessions caught the docs being WRONG on load-bearing points
(error-body shapes, orderbook ordering, cancel-read races) — recordings are
not a formality.

Related: [key-rotation-and-secrets.md](key-rotation-and-secrets.md) ·
[demo-flip.md](demo-flip.md) (what clearance unblocks)

---

## 1. The two recorders and their ground truth

| Surface | Recorder | Session record |
|---|---|---|
| Kalshi event API (27-item checklist) | [crates/fortuna-venues/examples/record_kalshi_fixtures.rs](../../crates/fortuna-venues/examples/record_kalshi_fixtures.rs) | [fixtures/kalshi/README.md](../../fixtures/kalshi/README.md) (recorded 2026-06-11) |
| Kinetics perps (research §12, 18 items) | [crates/fortuna-venues/examples/record_kinetics_fixtures.rs](../../crates/fortuna-venues/examples/record_kinetics_fixtures.rs) | [fixtures/kinetics-perps/SESSION-NOTES.md](../../fixtures/kinetics-perps/SESSION-NOTES.md) (recorded 2026-06-12) |

The checklists themselves live in the research docs the READMEs cite
(`docs/research/venue/kalshi-api-2026-06-10/research.md` §Uncertainties;
`docs/research/venue/kinetics-perps-2026-06-10/research.md` §12). Read the
relevant session record BEFORE re-running — both carry "known gaps left
open" lists that define what the next session must capture.

## 2. The safety rails you are relying on (both recorders, by construction)

From the recorder headers:

- **Demo hosts are HARDCODED** (`*.demo.kalshi.co`); production hosts do
  not appear in the code. Demo and prod keys do not cross-work, so a
  mixed-up key fails closed.
- **Demo-only env vars**: the recorders read `KALSHI_API_DEMO_KEY_ID` and
  `KALSHI_DEMO_PRIVATE_KEY_PATH` and never reference the production or
  kill-switch variable names.
- **Redaction by construction**: request headers are NOT recorded at all;
  no secret material is printed or written into fixtures or `.meta.json`.
- Orders are small (mock funds; kinetics: 1–2 contracts of the smallest
  perp) and tracked-and-canceled in a cleanup stage — with ONE deliberate
  exception, the kinetics funding position (see §4).

File conventions (both fixture READMEs): `<area>__<case>.json` = verbatim
response body; sibling `.meta.json` = method/path/status/sanitized request
body/note; `ws__*.jsonl` = verbatim WS text frames;
`session__manifest.meta.json` = the full session record. Kinetics gotcha:
the server's WS text frames carry a trailing newline, so those `.jsonl`
files contain blank separator lines — skip empty lines when replaying
(SESSION-NOTES header).

## 3. Running a session

**OPERATOR-JUDGMENT** — the recorders sign live requests against the Kalshi
demo environment and place real demo orders (mock funds). Preconditions:
operator authorization for the session (both lived sessions were
operator-directed); the demo credential pair is in `.env`; you have read
the session record's open-gaps list.

```
set -a && source .env && set +a
cargo run -p fortuna-venues --example record_kalshi_fixtures
```

or

```
set -a && source .env && set +a
cargo run -p fortuna-venues --example record_kinetics_fixtures
```

Run from the repo root (the fixture dirs are repo-relative). Note: this
operator tooling reads the wall clock directly — that is an allowed
exception to the injected-Clock rule (recorder headers; the rule governs
the deterministic core).

## 4. The two gotchas the sessions taught (kinetics)

- **Margin enablement.** The demo account is not margin-enabled by default:
  the surface LOOKS open (public endpoints + WS handshake work) but order
  writes fail until per-account enablement — the first session ran
  degraded because of it (SESSION-NOTES, "THE SESSION RAN DEGRADED — read
  this first"). The recorder auto-detects enablement: when blocked it
  captures exactly one blocked-evidence probe per private family instead
  of thrashing; once the operator enables margin in the demo web app, the
  SAME command runs the full flow.
- **Funding-rate 0.** Demo's funding engine has been pegged at zero across
  every observed 04:00/12:00/20:00 UTC tick, and a zero payment posts NO
  funding_history entry — so the entry SHAPE remains uncaptured on demo
  (SESSION-NOTES "Item 10 disposition" + "Funding observation 2"). It will
  come from the prod read-only parity sweep (operator item 17). Meanwhile:
  the deliberately opened 1-contract KXBTCPERP1 long stays open to catch a
  nonzero tick — **DO NOT CLOSE IT** (SESSION-NOTES: zero carry cost at
  rate 0).

## 5. Secrets sweep before committing captures

The recorders redact by construction, but verify anyway — captures are the
one artifact class that round-trips through a live credentialed session.
Before `git add fixtures/`:

```
grep -rn "PRIVATE KEY" fixtures/ ; echo "private-key grep exit: $?"
grep -rln '"headers"' fixtures/ ; echo "headers grep exit: $?"
git status --short fixtures/
```

Both greps must come back empty (exit 1; verified clean at `334612d`). The
first catches PEM material; the second catches any recorded request-header
block (the recorders write none, by construction). Do NOT sweep for header
NAMES like `KALSHI-ACCESS-SIGNATURE` — those legitimately appear in capture
notes (e.g. `fixtures/kalshi/auth__missing_signature_header.meta.json`) and
are not secrets. Then review the staged list
file-by-file: a fixture commit must contain exactly the session's captures
and nothing else (a prior commit, 8b8b222, swept in 111 unintended fixture
files via a broad `git add fixtures/` — flagged in
GATE-FINDINGS-LATEST.md as a claim-vs-reality slip; stage deliberately).
If a grep ever hits, stop and follow
[key-rotation-and-secrets.md](key-rotation-and-secrets.md): committed =
compromised = rotate.

## 6. What is still open (do not re-discover this from scratch)

As of `334612d`, from
[fixtures/kalshi/README.md](../../fixtures/kalshi/README.md) "Known gaps"
and GAPS.md "Operator-blocked: Kalshi fixtures":

- Kalshi settlement record re-poll after the seeded market closes; VOIDED
  market settlement (capture when one occurs); series fee fields via event
  lookup; STP `maker` mode; a two-sided REST orderbook (#20 was vacuously
  empty); cursor-stability sub-items (#17).
- Prod-parity read-only re-record (#26) and a real maintenance-window
  `GET /exchange/status` (#27) — before first live use (also spec v0.9's
  demo/prod divergence discipline, spec.md "Demo/prod divergence").
- Kinetics: funding_history entry shape (prod parity sweep); items 11/15/17
  are prod/post-fee follow-ups.

## When to stop and escalate

- Any 401 wall that two signing implementations agree on → credential
  pairing problem, not code; the fix is operator-side key management (this
  exact wall cost the first session — GAPS.md Kalshi entry, "blocked first
  attempt"). Stop probing, fix the pair.
- The recorder starts thrashing a blocked surface (more than one failed
  probe per family) → bug in the degraded-mode rails; stop the session and
  ledger it.
- Anything in a capture looks like key material or a request header →
  stop; secrets runbook.
