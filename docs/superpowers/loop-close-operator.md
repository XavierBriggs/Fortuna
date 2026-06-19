# loop-close-operator.md — captain's escalation queue + decision log (Loop-Close & Provable Demo milestone)

The captain's action list for the operator + a timestamped log of ambiguities resolved by captain judgment.
Per the charter's **Ambiguity protocol**: the captain resolves ambiguity itself (grounded in invariants → architecture doc → spec → charter), logs it here, and escalates ONLY when irreversible/outward-facing or when the docs genuinely conflict.

## ⚠ Needs operator (irreversible / outward-facing — read these)
_(none open)_

## Decision log (captain judgment — timestamped, newest first)

- **2026-06-19T09:05Z** — **Operating model locked:** synchronous captain loop; roles = principal(captain)/QA(verifier agent)/SDE(builder agent)/architect(planning); flow spec→verify→plan→verify→implement; captain may apply small verifier-flagged nits with verification. Grounds: operator directive + charter. _No escalation needed._
- **2026-06-19T09:05Z** — **§9.1 backtest-vs-promotion:** resolved to conservative default — backtest = evidence + calibration seed; §11 promotion runs on the live-forward clock; backtest rows excluded from the go_nogo count via the `source="historical-import"` stamp. Grounds: CLAUDE.md "spec silent → conservative" + honesty-over-green. _Operator may later opt-in out-of-sample replay to the count (an explicit override) — not blocking._
- **2026-06-19T09:05Z** — **Config-vs-spec divergence:** the demo config + go_nogo use the SPEC §11 values (paper ≥30 trading days, fee/PnL <0.35, ≥60 synthesis beliefs), NOT the shipped example-config values (14 / 0.5 / 100). Divergence recorded in GAPS. Grounds: spec §11 authoritative. _No escalation._
- **2026-06-19T09:05Z** — **D4 + synthesis-binary scope:** D4 (per-producer scoring) is consumed by WS1 (G2+G4). Synthesis BINARY-belief resolution is DEFERRED (synthesis provenance shape is undefined; the demo head-to-head is Aeolus-vs-meteorologist). Grounds: investigation + YAGNI; recorded in GAPS. _No escalation._
