# Evidence — Phase-1 Area (a): Repo & Workspace Map

Scope: FORTUNA at `/Users/xavierbriggs/fortuna-wt-ws3` (branch `feature/ws3-generic-backtest`).
Every claim cites `path:line`. AS-BUILT = code/Cargo.toml; AS-INTENDED = a doc/comment asserts it.
Read-only audit; nothing under the codebase was modified.

---

## 1. Workspace member crates (count = 17)

Workspace members are declared at `Cargo.toml:3-21` (`[workspace] members = [...]`), resolver "2"
(`Cargo.toml:2`). All 17 directories exist under `crates/` (verified `ls crates/`). All crates are
`version = "0.1.0"`, `edition = "2021"` (per each `[package]` block).

### Crate map

| crate | one-line responsibility | path-deps (normal `[dependencies]`) | produces bin? |
|---|---|---|---|
| **fortuna-core** | Event bus, clock, IDs, replay, money (`Cents`), book, market, perp price types | *(none — leaf)* | **yes** — auto bin `replay-verify` (`crates/fortuna-core/src/bin/replay-verify.rs`) |
| **fortuna-gates** | Gate pipeline (I1..I3); builds `GatedOrder` | fortuna-core (`crates/fortuna-gates/Cargo.toml:7`) | no |
| **fortuna-exec** | Order manager, fee models, idempotency | fortuna-core, fortuna-gates, fortuna-venues (`fortuna-exec/Cargo.toml:7-9`) | no |
| **fortuna-state** | Positions, balances, reconciliation vs venue; perp margin | fortuna-core, fortuna-venues (`fortuna-state/Cargo.toml:8-9`) | no |
| **fortuna-venues** | Venue adapter trait + Kalshi adapter (REST/WS) | fortuna-core, fortuna-gates (`fortuna-venues/Cargo.toml:7-8`) | no |
| **fortuna-ledger** | Belief ledger, journal, memory, audit, scorecards (Postgres/sqlx) | fortuna-core, fortuna-venues, fortuna-exec, fortuna-gates, fortuna-scoring (`fortuna-ledger/Cargo.toml:7-13`) | no |
| **fortuna-cognition** | Context assembler, decision loops, LLM provider trait, schemas, personas | fortuna-core, fortuna-scoring (`fortuna-cognition/Cargo.toml:7-8`) | no |
| **fortuna-scoring** | Pure scoring types (Scorecard, Brier/CLV); std+serde+thiserror only | *(none — leaf; `fortuna-scoring/Cargo.toml:6-8`)* | no |
| **fortuna-ops** | Metrics, Slack, dashboard, deadman, digest, export, ROTA API (axum+sqlx) | fortuna-core, fortuna-gates, fortuna-ledger, fortuna-scoring (`fortuna-ops/Cargo.toml:7-15`) | no |
| **fortuna-invariants** | PROTECTED: invariants I1..I7 as executable tests (additions-only) | fortuna-core, fortuna-gates (`fortuna-invariants/Cargo.toml:10-11`) | no |
| **fortuna-killswitch** | Standalone out-of-band kill switch (I4); must run with everything else dead | fortuna-core, fortuna-venues, fortuna-gates (`fortuna-killswitch/Cargo.toml:12-20`) | **yes** — default bin `fortuna-killswitch` (`src/main.rs`, no `[[bin]]`/`[lib]` table) |
| **fortuna-cli** | Operator CLI (`fortuna`): status, re-arm, kill reversal (Section 8) | fortuna-core, fortuna-gates, fortuna-ledger, fortuna-ops (`fortuna-cli/Cargo.toml:11-14`) | **yes** — `[[bin]] name="fortuna"` (`fortuna-cli/Cargo.toml:6-8`) |
| **fortuna-runner** | Strategy runners (mech_structural/extremes, synthesis, funding_forecast, perp_event_basis, promotion) | fortuna-core, fortuna-cognition, fortuna-gates, fortuna-exec, fortuna-state, fortuna-venues (`fortuna-runner/Cargo.toml:7-12`) | no |
| **fortuna-paper** | Paper-execution venue (paper-on-live-data, through-not-touch fills) | fortuna-core, fortuna-gates, fortuna-venues (`fortuna-paper/Cargo.toml:7-9`) | no |
| **fortuna-recorder** | Live venue-stream recorder → JSONL for replay | *(no fortuna path-deps; only reqwest/tokio/serde_json/anyhow, `fortuna-recorder/Cargo.toml:7-10`)* | **yes** — `[[bin]] name="fortuna-recorder"` (`fortuna-recorder/Cargo.toml:12-14`) |
| **fortuna-live** | Daemon composition root: boots/wires every crate, run-loop, ingestion, perp feed, telemetry | fortuna-core, fortuna-cognition, fortuna-exec, fortuna-gates, fortuna-killswitch, fortuna-ledger, fortuna-ops, fortuna-paper, fortuna-runner, fortuna-sources, fortuna-state, fortuna-venues (`fortuna-live/Cargo.toml:8-24`) | **yes** — `[[bin]] name="fortuna-live"` (`fortuna-live/Cargo.toml:39-41`) |
| **fortuna-sources** | Signal ingestion (Aeolus, NWS, RSS), scheduler, Layer-1 validator | fortuna-core, fortuna-cognition (`fortuna-sources/Cargo.toml:7-8`) | no |

### Dependency layering (normal deps only; built bottom-up)

- **L0 leaves (no fortuna deps):** `fortuna-core`, `fortuna-scoring`.
- **L1 (depend only on core/scoring):** `fortuna-gates` (→core), `fortuna-cognition` (→core,scoring).
- **L2:** `fortuna-venues` (→core,gates).
- **L3:** `fortuna-exec`, `fortuna-state`, `fortuna-paper`, `fortuna-killswitch` (→core,venues,gates),
  `fortuna-sources` (→core,cognition).
- **L4:** `fortuna-ledger` (→core,venues,exec,gates,scoring); `fortuna-runner`
  (→core,cognition,gates,exec,state,venues).
- **L5:** `fortuna-ops` (→core,gates,ledger,scoring); `fortuna-cli` (→core,gates,ledger,ops).
- **L6 (top / composition root):** `fortuna-live` depends on 12 fortuna crates
  (`fortuna-live/Cargo.toml:8-24`) — the daemon wiring layer.
- `fortuna-recorder` is a standalone HTTP→JSONL tool with NO fortuna path-deps
  (`fortuna-recorder/Cargo.toml:7-10`).
- `fortuna-invariants` is an aggregator: 2 normal deps (core,gates) but pulls 8 more crates as
  **dev-dependencies** to exercise the invariants (`fortuna-invariants/Cargo.toml:16-28`:
  venues, state, ledger, killswitch, cognition, runner, exec, paper, live).

### Load-bearing structural-boundary comments (AS-INTENDED, enforced by tests)

- **I4 killswitch independence:** `fortuna-killswitch/Cargo.toml:6-9` asserts the crate must NEVER
  depend on fortuna-ledger, sqlx, Postgres, cognition runtime, or the event loop; the
  `i4_killswitch_independence` invariant test "asserts this dependency boundary mechanically."
  tokio IS allowed (`Cargo.toml:26-32`, justified as already-transitive via venues + an IO-edge runtime).
- **I4 one-way revocation edge:** `fortuna-live` CONSUMES the killswitch sentinel
  (`fortuna-live/Cargo.toml:12-17`); comment notes this does not enter killswitch's own dep graph.
- **No cycle ledger↔ops:** `fortuna-ops/Cargo.toml:9-12` documents the cycle check (ledger's runtime
  deps are core/venues/exec/gates — ops not among them).
- **MarketView moved to core (I4 fix):** `fortuna-venues/Cargo.toml:9` (no cognition dep here).

---

## 2. Languages present + build/run/test entrypoints

### Rust (primary; 17-crate Cargo workspace)
- Build: `cargo build --workspace`. Test: `cargo test --workspace`.
- Toolchain pinned: `rust-toolchain.toml:2-3` → channel `stable`, components `rustfmt`, `clippy`.
- DST corpus + invariant guard: see §3.
- Workspace cargo env default `DATABASE_URL=postgres://localhost/fortuna_dev`, `force=false`
  (`.cargo/config.toml:16-17`) — routes cargo-launched processes at a local dev DB so `#[sqlx::test]`
  never touches the operator DB (`.cargo/config.toml:1-15`).

### Python (research harnesses, NOT in the Rust build; 3 independent packages under docs/)
Each is a setuptools project (`requires-python >=3.11`), with a console-script entrypoint and pytest:

| pkg | path | console script | test entrypoint | notable deps |
|---|---|---|---|---|
| **deuce** (tennis win-prob) | `docs/deuce/` | `deuce = "deuce.cli:main"` (`pyproject.toml:22-23`) | `pytest` (`testpaths=["tests"]`, `pyproject.toml:32-33`) | pandas,numpy,requests,openpyxl,xlrd (`:10-17`) |
| **heater** (pitcher-K props) | `docs/heater/` | `heater = "heater.cli:main"` (`pyproject.toml:22-23`) | `pytest` (`pyproject.toml:32-33`) | pandas,numpy,requests,cryptography; optional `data`=pybaseball (`:10-19`) |
| **kairos** (perp funding/basis) | `docs/kairos/` | `kairos = "kairos.cli:main"` (`pyproject.toml:22-23`) | `pytest` (`pyproject.toml:32-33`) | +websocket-client,cryptography (`:10-17`) |

- Install/run: `pip install -e docs/<pkg>[dev]` then run the console script (e.g. `heater ...`) or `pytest`.
- Python lint/format: each declares `[tool.ruff] line-length=100, target-version="py311"`
  (`docs/deuce/pyproject.toml:28-30`, `docs/heater/pyproject.toml:28-30`, `docs/kairos/pyproject.toml:28-30`)
  and `dev = ["pytest>=7.4","ruff>=0.4"]`.
- File counts (`find docs -name '*.py'`): kairos 16 pkg + 14 tests; deuce 14 pkg + 7 tests; heater 11 pkg
  + 7 tests; each has 1 `scripts/get_*.py` data-puller.
- `research-workspace/` contains only `research-workspace/PLAN.md` (no code).

### Shell (Bash; orchestration only)
- All 6 scripts in `scripts/` are `#!/usr/bin/env bash` with `set -euo pipefail` — see §3/§5.

---

## 3. Rust build/test/DST/invariants commands & CI (quoted)

### House requirements (AS-INTENDED)
`CLAUDE.md` "Definition of done": tests-from-spec-first; `cargo fmt --check`, clippy `-D warnings`, full
test suite, and the DST corpus (`scripts/run-dst.sh`) all pass. Conventions line:
`cargo clippy --workspace --all-targets -- -D warnings` clean.

### scripts/run-dst.sh (AS-BUILT)
Usage `scripts/run-dst.sh [N_RANDOM_SEEDS]`, default N=2000 (`run-dst.sh:30`). `set -euo pipefail`
(`:29`). It runs (each line is a separate `cargo test`):
- `fortuna-core --test dst` (regression seeds + N random, `:31-32`)
- `fortuna-runner --test synthesis_dst` (`:34`), `--test settlement_dst` (`:36`),
  `--test funding_forecast_dst` (`:40`), `--test perp_event_basis_dst` (`:43`), `--test paper_live_dst` (`:46`)
- `fortuna-state --test perp_dst` (`:38`)
- `fortuna-cognition --test persona_dst` (`:49`), `--test persona_orchestrator_dst` (`:56`)
- `fortuna-live --test daemon_smoke` (`:59`)
- `fortuna-sources --test ingest_dst` (`:64`)
- Contract: "Exits non-zero on ANY invariant violation OR build failure" (`:26-28`); a harness that fails
  to build fails the battery (`:27-28`).
- The custom DST harness is wired by `crates/fortuna-core/Cargo.toml:22-24` (`[[test]] name="dst", harness=false`).

### scripts/check-protected-invariants.sh (AS-BUILT)
Usage `check-protected-invariants.sh [base-ref]` default `main` (`:11,:17`). Diffs
`crates/fortuna-invariants/tests` vs base; **fails (exit 1)** if any `-` (removed/changed) line appears in
an existing test file (`:26-38`); new files / pure appends are all `+` lines so they pass (`:6-9`).
Enforces the CLAUDE.md "additions-only" protected-directory rule.

### CI workflow `.github/workflows/ci.yml` — job `check` (quoted steps)
Triggers: push to main + all PRs (`ci.yml:2-4`). Postgres 16 service, `DATABASE_URL=...fortuna_test`
(`ci.yml:8-22`). Toolchain `dtolnay/rust-toolchain@stable` w/ rustfmt+clippy (`:25-26`), `rust-cache@v2` (`:27`).
Steps:
- fmt: `cargo fmt --all --check` (`ci.yml:28-29`)
- clippy: `cargo clippy --workspace --all-targets -- -D warnings` (`:30-31`)
- tests: `cargo test --workspace` (`:32-33`)
- invariant tests: `cargo test -p fortuna-invariants -- --list` ("must compile; ignored allowed", `:34-35`)
- dst corpus: `./scripts/run-dst.sh ${DST_SEEDS:-2000}` (`:36-37`)
Second job `protected-dir-guard` (PR-only, `:38-40`): warns (`::warning::`, non-blocking) if a PR touches
`crates/fortuna-invariants/` (`ci.yml:44-49`).

### CI workflow `.github/workflows/invariants-dst.yml` — job `verify` (quoted steps)
Triggers: PRs + push to main (`:8-11`); concurrency cancel-in-progress (`:13-15`). Postgres 16 service,
`SQLX_OFFLINE="true"`, `DATABASE_URL=...fortuna` (`:20-35`). Toolchain stable + clippy,rustfmt (`:40-42`),
rust-cache (`:43`). Steps:
- Protected-invariant guard: `bash scripts/check-protected-invariants.sh "origin/${BASE_REF}"`
  (`:46-49`; "the load-bearing governance gate: the invariants crate is additions-only")
- fmt: `cargo fmt --check` (`:51-52`)
- clippy: `cargo clippy --workspace --all-targets -- -D warnings` (`:54-55`)
- pre-build killswitch: `cargo build -p fortuna-killswitch` (`:59-60`; so the i4 test's nested `cargo run`
  doesn't build-under-test and wedge)
- invariants: `cargo test -p fortuna-invariants` (`:62-63`) — note: this RUNS them, vs ci.yml's `--list` only.
- DST corpus: `bash scripts/run-dst.sh` (`:65-66`) — no seed arg, so default 2000.

**Drift between the two workflows (AS-BUILT):** both run on the same triggers and both run fmt/clippy/DST,
but `invariants-dst.yml` additionally (a) runs the protected-invariants diff guard as a hard gate, (b) actually
EXECUTES `fortuna-invariants` tests (ci.yml only `--list`s them), (c) pre-builds the killswitch, and (d) uses
`SQLX_OFFLINE=true` against DB `fortuna` while ci.yml uses DB `fortuna_test` with no SQLX_OFFLINE. The
`invariants-dst.yml` header says it "runs once a GitHub remote exists — the repo is local-only by policy until
then" (`invariants-dst.yml:6`), i.e. neither workflow has executed yet on a remote. `.git` is a gitlink/worktree
pointer (`ls .git` shows a file, not a dir), consistent with a local worktree.

---

## 4. Lint / format config status

- **No Rust lint/format config files exist.** `find` for `rustfmt.toml`, `.rustfmt.toml`, `clippy.toml`,
  `.clippy.toml`, `deny.toml` (excluding `target/`) returned ZERO hits. So `cargo fmt` and
  `cargo clippy -D warnings` run on **defaults** — the CLAUDE.md requirement is enforced by CI flags
  (`-D warnings` in both workflows) and the pinned toolchain, NOT by any repo config file.
- `rust-toolchain.toml:1-3` pins `channel="stable"` + components `["rustfmt","clippy"]` (AS-BUILT).
- No `cargo-deny` / supply-chain config present (no `deny.toml`).
- Per-crate inner lint attributes DO exist in source (not config files): e.g.
  `crates/fortuna-killswitch/src/main.rs:13-19` `#![deny(clippy::unwrap_used, expect_used, panic, todo, unimplemented)]`
  — encodes the CLAUDE.md "no panic/unwrap/expect in money paths" rule at the crate level.
- Python: ruff config inline in each pyproject (`[tool.ruff]`, see §2); no standalone `ruff.toml`.

---

## 5. Binaries & entrypoint scripts

### Rust binaries (5 total)
| bin name | crate | declaration |
|---|---|---|
| `fortuna` | fortuna-cli | explicit `[[bin]]` `fortuna-cli/Cargo.toml:6-8` → `src/main.rs` |
| `fortuna-live` | fortuna-live | explicit `[[bin]]` `fortuna-live/Cargo.toml:39-41` → `src/main.rs` |
| `fortuna-recorder` | fortuna-recorder | explicit `[[bin]]` `fortuna-recorder/Cargo.toml:12-14` → `src/main.rs` |
| `fortuna-killswitch` | fortuna-killswitch | **default bin** — `src/main.rs` present, no `[[bin]]`/`[lib]` table in Cargo.toml (crate also has `src/lib.rs`, so it's lib+default-bin) |
| `replay-verify` | fortuna-core | **auto-discovered** at `crates/fortuna-core/src/bin/replay-verify.rs` (Cargo auto-detects `src/bin/*.rs`); no `[[bin]]` entry |

(`cargo metadata` was unavailable offline; the above is from `grep [[bin]]`, `find src/main.rs`,
and `ls src/bin`. `fortuna-cli/src/main.rs:1` confirms `fortuna` is the operator CLI;
`fortuna-killswitch/src/main.rs:1-12` confirms the standalone kill-switch binary.)

### Entrypoint scripts (`scripts/`, all bash; from `ls scripts/` + headers)
| script | purpose (from header) |
|---|---|
| `run-dst.sh` | DST corpus runner (§3) |
| `check-protected-invariants.sh` | protected-invariant additions-only guard (§3) |
| `killswitch-test.sh` | monthly kill-switch self-test, run with runtime down; `env -u DATABASE_URL cargo run -q -p fortuna-killswitch -- self-test` (`killswitch-test.sh:6-12`) |
| `replay.sh` | replay verify: `replay.sh <recording.jsonl>` (structural, T0.2) or `--seed <N>` (re-run DST seed, T0.4) (`replay.sh:1-12`) |
| `demo-launch.sh` | launch Kalshi paper-on-live-data demo as daemon+recorder; kill-switch clear is operator-gated (`demo-launch.sh:1-9`) |
| `refresh-demo-markets.sh` | refresh operator-local demo ticker seeds in `config/fortuna.toml` from live Kalshi listings (`refresh-demo-markets.sh:1-12`) |

---

## 6. Drift: spec's 8-crate layout (5.1) vs the 17 as-built crates

Spec `docs/spec.md:95-102` lists **8 crates** (the L0 layout): fortuna-core, -gates, -exec, -state,
-venues, -ledger, -cognition, -ops. All 8 exist as-built with responsibilities matching the spec
comments (e.g. core="event bus, clock, ids, replay" `spec.md:95` vs `fortuna-core/src/`).

**9 crates exist that the spec 5.1 layout does NOT name** (AS-BUILT additions beyond the as-intended layout):
1. **fortuna-scoring** — pure Scorecard/Brier/CLV types (a leaf extracted to avoid the ledger↔cognition
   cycle, per `fortuna-ledger/Cargo.toml:11-13`).
2. **fortuna-invariants** — the PROTECTED executable-invariants crate (mandated by CLAUDE.md "Protected
   directory" but not in spec 5.1's box).
3. **fortuna-killswitch** — spec 5.1 folds the kill switch INTO `fortuna-ops` ("metrics, slack, kill switch,
   CLI", `spec.md:102`); as-built it is its OWN crate, REQUIRED by I4 structural independence
   (`fortuna-killswitch/Cargo.toml:6-9`). This is a deliberate, invariant-driven split, not accidental drift.
4. **fortuna-cli** — spec 5.1 also folds CLI into `fortuna-ops`; as-built it is a separate crate
   (`fortuna-cli/Cargo.toml`).
5. **fortuna-runner** — strategy runners; not named in 5.1 (the spec discusses strategies in 5.4/5.8/phases).
6. **fortuna-paper** — paper venue; not in 5.1 (spec mentions paper-fill realism in Section 11).
7. **fortuna-recorder** — venue-stream recorder for replay; not in 5.1.
8. **fortuna-live** — daemon composition root; not in 5.1 (5.1 shows only the crate boxes + config/).
9. **fortuna-sources** — signal ingestion; spec describes this subsystem in **5.11** (`spec.md:246`) but it
   is NOT in the 5.1 crate box.

So the drift is: **the spec 5.1 box is an L0 sketch (8 crates) and was never updated to the realized
17-crate layout.** The 8 spec crates all exist; the extra 9 are (a) cycle-break extractions (scoring),
(b) invariant-mandated splits (killswitch, invariants, cli out of "ops"), and (c) subsystems the spec
describes in prose elsewhere (runner/strategies, paper, recorder, live-daemon, sources/5.11) but never
added to the 5.1 diagram. None contradict the spec; the spec diagram is simply stale relative to code.

---

## 7. Open questions / could-not-verify
- `cargo metadata` could not run (offline sandbox), so the 5-binary list rests on file inspection rather
  than a build-system query. The 3 explicit `[[bin]]` entries + 2 convention-based bins (killswitch
  default-bin, fortuna-core `src/bin/replay-verify.rs`) are unambiguous from the file layout, but a build
  would confirm there are no additional auto-bins elsewhere. (No other `src/bin/` dirs were found; only
  fortuna-core has one.)
- Neither CI workflow appears to have run on a remote yet (`invariants-dst.yml:6` says local-only;
  `.git` is a worktree gitlink). "What CI enforces" is therefore as-configured, not as-observed-green.
