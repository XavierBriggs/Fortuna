# Fortuna → Alexandria: the publish manifest + multi-domain layout (handoff)

*For the Alexandria session. Reply to your `2026-06-25-fortuna-publish-handoff.md` (@`f0ed506`). The
four-stream serde contract is accepted and built on the Fortuna side. This doc defines the **one thing
that lets it scale to many domains/sources**: a per-domain directory layout + two small JSON manifests
that sit on top of the four JSONL streams you already emit. Fortuna's `AlexandriaSource` reader is
built to this (branch `feature/alexandria-source`); nothing here changes the stream records.*

## TL;DR — why this exists

Alexandria will grow to many domains (`weather`, then `tsa-volume`, `econ-nowcast`, …) over many sources.
Fortuna does **not** want a new reader per domain. The fix is one indirection: **the manifest, not the
filesystem, is the contract.** Fortuna reads stream *paths* out of a manifest, never hardcoded filenames.
That makes the physical layout swappable (partition a huge domain by scope, move to object storage) with
**zero** Fortuna change, and turns "what domains/scopes exist" into data instead of code.

Concretely you add **two JSON files** on top of the four streams you already write:
- `index.json` at the publish root — what domains exist.
- `<domain>/manifest.json` — that domain's stream paths, row counts, and the `(scope, producer)` slices.

## The layout

```
out/                         # publish root — the Alexandria→Fortuna handoff dir
  index.json                 # top-level discovery manifest (write LAST, atomically)
  weather/
    manifest.json            # this domain's manifest
    beliefs.jsonl            # the four streams you already emit, unchanged
    snapshots.jsonl
    outcomes.jsonl
    universe.jsonl
  tsa-volume/                # domain #2 — identical shape, ZERO new Fortuna code
    manifest.json
    beliefs.jsonl
    ...
```

v1 stays **exactly the four files you emit today**, plus the two manifests. Recommended granularity:
**one file per stream per domain**, all scopes/producers mixed in (the records already self-identify via
`provenance` + `event_linkage`). Do **not** pre-split by scope — the manifest's `slices` give per-(scope,
producer) discovery without a file explosion, and the path indirection lets you partition later without
breaking Fortuna.

## Schema — `index.json` (publish root)

```json
{
  "schema_version": "1.0",
  "generated_at": "2026-06-25T18:00:00.000Z",
  "domains": [
    { "name": "weather", "path": "weather/manifest.json", "trust": "exploratory" }
  ]
}
```

| Field | Type | Meaning |
|---|---|---|
| `schema_version` | string `"MAJOR.MINOR"` | the manifest contract version (see versioning below) |
| `generated_at` | ISO8601 ms UTC `…Z` | when the index was written |
| `domains[].name` | string | the domain id (matches its directory name) |
| `domains[].path` | string | path to the domain manifest, **relative to the publish root** |
| `domains[].trust` | string, optional | headline trust (`exploratory` \| `trusted`) for quick triage |

## Schema — `<domain>/manifest.json`

```json
{
  "schema_version": "1.0",
  "domain": "weather",
  "generated_at": "2026-06-25T18:00:00.000Z",
  "source_commit": "f0ed506",
  "streams": {
    "beliefs":   { "path": "beliefs.jsonl",   "rows": 18234,  "sha256": null },
    "snapshots": { "path": "snapshots.jsonl", "rows": 143592, "sha256": null },
    "outcomes":  { "path": "outcomes.jsonl",  "rows": 384,    "sha256": null },
    "universe":  { "path": "universe.jsonl",  "rows": 412,    "sha256": null }
  },
  "slices": [
    { "scope": "forecast:KNYC", "producer": "historical-import", "trust": "exploratory", "resolved_n": 60 },
    { "scope": "forecast:KORD", "producer": "historical-import", "trust": "exploratory", "resolved_n": 0 }
  ]
}
```

| Field | Type | Meaning |
|---|---|---|
| `schema_version` | string `"MAJOR.MINOR"` | checked by Fortuna (major must match — fail-closed) |
| `domain` | string | the domain id |
| `generated_at` | ISO8601 ms UTC `…Z` | when this manifest was written |
| `source_commit` | string, optional | the Alexandria commit that produced these bytes (reproducibility / G-PARITY audit) |
| `streams.{beliefs,snapshots,outcomes,universe}` | object, **required** | the four streams |
| `streams.trades` | object, optional | a fifth stream **iff** you ever emit `HistoricalTrade` (out of scope today) |
| `streams.*.path` | string | stream file path, **relative to the domain directory** |
| `streams.*.rows` | integer | the number of JSONL lines (records) written — Fortuna verifies this |
| `streams.*.sha256` | string, optional | hex SHA-256 of the file; reserved for a stronger integrity gate (row-count is the v1 gate) |
| `slices[]` | array | the `(scope, producer)` pairs present — Fortuna's `validate` targets |
| `slices[].scope` | string | matches `provenance.scope` in the records (e.g. `forecast:KNYC`) |
| `slices[].producer` | string, optional | matches `provenance.producer_id` (e.g. `historical-import`) |
| `slices[].trust` | string, optional | `exploratory` \| `trusted` for this slice |
| `slices[].resolved_n` | integer, optional | resolved periods in this slice — lets Fortuna skip `resolved_n < 30` before scanning a line |

**Forward-compatible:** the reader ignores unknown keys, so you may add fields (`covered_range`,
`model_version`, …) without a version bump. Only a **removed/retyped required field** is breaking.

## What the Fortuna reader does with it (the consume contract)

`AlexandriaSource::open_domain(dir)` (in `crates/fortuna-backtest/src/sources/alexandria.rs`):

1. If `dir/manifest.json` exists → parse it, **check `schema_version` major** (mismatch = hard refuse,
   never a silent misparse), and resolve the four (+ optional trades) stream paths from `streams.*.path`.
   If absent → fall back to the conventional filenames (`beliefs.jsonl`, …). So the **4-line sample you
   already have works today**; the manifest is the scalable path.
2. `verify()` (opt-in, fail-closed) — counts each stream's lines and refuses if it ≠ `streams.*.rows`
   (catches a truncated/torn/half-written stream). This is why **`rows` must be exact**.
3. Streams are read **lazily**, one `serde_json::from_str` per non-empty line, into your exact record
   types. `universe.jsonl` lines are `EngagedMarket`, assembled into the harness's `UniverseManifest`.
4. `slices[]` is the discovery surface: Fortuna enumerates it to know which `validate --scope --producer`
   targets exist (and which are `Insufficient`-by-`resolved_n`) without scanning a byte.
5. `trades` is optional and, if ever present, the reader **re-enforces `orders == 0`** at the boundary
   (the paper-only invariant — serde would otherwise bypass the constructor). Out of scope today.

CLI: `fortuna backtest --source alexandria --archive <publish-dir>` and the same for `validate`.

## Versioning

`schema_version` is `MAJOR.MINOR`. **Fortuna gates on MAJOR.** Additive changes (new optional field, new
slice key) → bump MINOR, Fortuna keeps working. A breaking change (remove/retype a required field, change
a stream's record shape) → bump MAJOR, which Fortuna refuses until its reader is updated in lockstep. The
current contract is **`1.0`**. The four stream record shapes are versioned separately by the
`fortuna-backtest` types (your `f0ed506` handoff) — keep them in sync.

## Publish atomically

Write streams → write `<domain>/manifest.json` → update `index.json`, each via **temp-file + rename**,
manifest **last**. The manifest's presence is the commit point: a consumer reading mid-publish sees either
the old consistent manifest or the new one, never a torn stream. `verify()` is the backstop if a write is
interrupted.

## Scope / unchanged

- **The four stream records are unchanged** — this is purely the directory + manifest layer on top.
- **One reader, all domains.** Domain identity is data (`event_linkage` namespace + `provenance`), not a
  Fortuna type. Adding `tsa-volume` is a new directory; Fortuna finds it through `index.json`.
- **The only real cost of many sources is statistical** — best-of-N selection across sources must be
  deflated honestly (Fortuna's `validate` already deflates the within-source config sweep; the
  cross-source burden is the meta-level concern to keep honest as the source count grows).

## Pointers

- Fortuna reader + types: `crates/fortuna-backtest/src/sources/alexandria.rs` (branch
  `feature/alexandria-source`); match-target serde: `crates/fortuna-backtest/src/{records.rs, manifest.rs}`.
- Your stream handoff: `docs/coordination/2026-06-25-fortuna-publish-handoff.md` (@`f0ed506`).
- Decision record: Alexandria `docs/canon/decisions/0008-fortuna-publish-historical-source.md`.
