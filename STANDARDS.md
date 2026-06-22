# FORTUNA STANDARDS

> Purpose: the code-writing rules a linter cannot enforce for you.
> Holds: money/numeric rules, determinism rules, error handling at money boundaries, observability conventions, the strategy-module recipe, the backtest discipline, the cross-language boundary, and naming.
> Excludes: anything a formatter or linter enforces (those live in tool configs, referenced below), the invariants themselves (CONSTITUTION.md), and rationale (decisions/).

## Tool configs (do not restate their rules here)

- Formatting and lints run on toolchain defaults pinned by `rust-toolchain.toml` (stable + rustfmt + clippy). There is intentionally no `rustfmt.toml` or `clippy.toml`; defaults are the standard.
- The hard bar is the CI invocation, not a config file: `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`. Both must be clean.
- There is no `deny.toml` / `cargo-deny` gate yet. Adding one is an open item (see GAPS.md); until then, supply-chain review is manual.
- Run before claiming done (these are the gate, not a suggestion): `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; `scripts/run-dst.sh 2000`; `scripts/check-protected-invariants.sh`.

## Money and numeric rules

- Money is integer cents in the `Cents(i64)` newtype. Perp prices are `PerpPrice(i64)` in venue ten-thousandths. Never `f64` for money, prices, sizes, or fees in any core/gates/exec/state/venues path.
- Use checked arithmetic only on money types; overflow returns an error, never wraps or panics.
- `rust_decimal::Decimal` appears only at venue payload conversion boundaries. Convert string-to-Decimal-to-newtype; never parse money through `f64`.
- Rounding is always against us: fees round up, gains floor, costs/exposure ceil. Fee config coefficients are decimal strings, never float literals.
- `PerpPrice` and `Cents` are type-separated and must never be cross-assigned. A perp value reaches `Cents` only at notional/PnL/fee boundaries, rounded against us.
- Probabilities and process statistics (p, Brier, CLV, Sharpe, DSR, calibration) are `f64` and live only in cognition, scoring, and backtest. They never become an order price except by minting an integer `Cents`/`PerpPrice` at the boundary, which the gates then re-check.

## Determinism rules

- All time comes from the injected `Clock`. `SystemTime::now()`, `Instant::now()`, and `chrono::Utc::now()` are forbidden outside `Clock` implementations and outside read-only capture tools (recorder, examples). A wall-clock read in a decision path is a defect.
- No RNG in any gate/exec/state/cognition decision path. Seeded RNG is allowed only in the DST harness and in required CSPRNG uses (venue request signing).
- Loops take `&dyn Clock` (or `Arc<dyn Clock>`) and a cancellation token; nothing sleeps on wall time directly.
- Deterministic identifiers use a content hash (FNV-1a in the backtest), never `DefaultHasher` (its seed is process-random and breaks replay).

## Error handling at money boundaries

- No `panic!`, `unwrap`, or `expect` in any money path (gates, exec, state, venues, ledger money columns). Use `thiserror` error enums per crate; `anyhow` only in binaries.
- State machines are enums with explicit transition functions returning `Result`. An illegal transition is an error plus an audit row, never a silent coerce.
- Fee math reconciles charged-vs-modeled on every fill; a mismatch writes a discrepancy, never a silent absorb.

## Observability conventions

- Every Slack message is also an audit row. Route through the single ops Slack client; the channel is chosen from the config table by message type.
- Every context build emits a manifest (item ids and hashes) into the audit log; belief provenance carries the manifest hash so a decision is reconstructable. (The provenance must carry the full mandated set; see GAPS.md for the current `prompt_hash` defect.)
- Append-only tables are INSERT-only at the application layer. A correction is a new superseding row, never an update. New persistent tables holding decision/audit/score data must carry an append-only trigger and be added to the parametric I5 test.
- Metrics are Prometheus-text exposition from fortuna-ops; there is no OpenTelemetry (a deliberate, documented choice).

## Strategy-module recipe

A strategy implements `Strategy` in fortuna-runner: `id`, `kind` (Mechanical | Synthesis), `stage` (`Sim | Paper | LiveMin | Scaled`), `on_event` (returns `OrderIntent`s; never sizes itself), `metrics`. It declares the minimum edge-confidence tier it accepts. Sizing is the harness's job (fractional Kelly via the reservation ledger), never the strategy's or the model's. A new strategy enters at `Sim` and earns each stage through the CONSTITUTION ramp gates against the validation subsystem's evidence.

## Backtest discipline

When adding to or validating with fortuna-backtest: enforce point-in-time strict `<` (no equal-timestamp leakage); recompute scores through the same path as live (parity by construction, not a parallel implementation); supply real purge/embargo windows before trusting a GO surface (an empty window list silently understates overfitting); deflate DSR against the joint family trial count, not the per-config count; keep the source read-only; let Brier be the gated headline and CLV corroborate only. The backtest validates deterministic components, never LLM-decision PnL.

## Cross-language boundary

Rust is the system. The Python research harnesses under `docs/` (deuce, heater, kairos) are out-of-band research MVPs, not part of the workspace and not on the live path. They produce research memos and candidate signals; anything that becomes a live signal enters FORTUNA only through the `Source` trait as untrusted, normalized data. Do not import Python results into the core except as a registered, scored signal source.

## Naming

IDs are ULIDs. Timestamps are UTC ISO8601. Crates are `fortuna-<concern>`. The spec's "Provider trait" is the code's `Mind` trait (use `Mind` in code). System mythology is spent at the system level only; loops and components stay descriptive.
