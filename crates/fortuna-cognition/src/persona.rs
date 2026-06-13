//! Track E E.2: persona definition + skill-file loader (design §6).
//!
//! A persona IS a domain-analyst skill: a trusted, operator-authored file
//! `config/personas/<id>/persona.md` (TOML frontmatter + the trusted method
//! body) plus `config/personas/<id>/schema.json` (the findings output schema).
//! This module is the LOADER CORE — pure parse + content-hash + registry
//! validation, no filesystem IO (cognition stays deterministic/core; the
//! composition does the trivial `std::fs::read_to_string` at the edge and calls
//! [`PersonaDef::parse`]).
//!
//! `method_hash` is the SHA-256 of the ENTIRE `persona.md` file (frontmatter +
//! body), reusing the crate's [`crate::context::content_hash_of`] convention.
//! [`PersonaDef::validate_against`] refuses a method whose hash does not match
//! the active personas-registry head (design §5/§6) — so an operator's promotion
//! (a file edit + a superseding registry insert) is deliberate and audited,
//! exactly as `lessons`/`calibration_params` supersede.
//!
//! Trust boundary (design §4): the method body is TRUSTED operator scaffolding
//! (injected as the Mind transport system message at run time, slice E.3) — it
//! is loaded ONLY from this trusted file, never from the DB, a signal, or any
//! model-writable surface. The untrusted signals a persona later reads are a
//! separate stream handled by the runner.

use crate::context::content_hash_of;
use serde::Deserialize;
use thiserror::Error;

/// Frontmatter metadata (TOML, between `+++` fences) — maps 1:1 to the personas
/// registry row (design §5/§6). `deny_unknown_fields`: an unexpected key is a
/// counted defect, never silently ignored (house style on the definition surface).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaMeta {
    /// The persona id, e.g. `meteorologist` (the registry `persona_id`).
    pub id: String,
    /// Bumps per method change; must match the registry head version.
    pub version: i32,
    pub domain: String,
    pub domain_tags: Vec<String>,
    /// Signal kinds this persona may read (the runner enforces; design §7).
    pub reads_signal_kinds: Vec<String>,
    /// `cheap` | `synthesis` — resolved to a model by Track M's factory.
    pub tier: String,
    /// The dedup/serialization key template, e.g. `weather:{station}:tmax:{date}`.
    pub region_key: String,
    pub output_schema_version: String,
}

/// A loaded, content-hashed persona definition (the loader's output).
#[derive(Debug, Clone)]
pub struct PersonaDef {
    pub meta: PersonaMeta,
    /// SHA-256 hex of the ENTIRE `persona.md` file — the provenance anchor that
    /// proves which method produced an analysis and lets the loader refuse a
    /// config/registry mismatch (design §5/§6).
    pub method_hash: String,
    /// The trusted method body (everything after the closing `+++` fence).
    /// Injected as the Mind transport system message at run time (design §4) —
    /// never packed as a context data item.
    pub method: String,
    /// The findings output schema (`schema.json`), validated as JSON here; the
    /// runner enforces findings against it strictly in slice E.3.
    pub schema: serde_json::Value,
}

/// The registry head a definition is validated against — a PURE input. Cognition
/// does not depend on `fortuna-ledger`, so the composition maps
/// `PersonasRepo::head(...)` onto this small struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryHead {
    pub version: i32,
    pub method_hash: String,
    /// `active` | `retired`.
    pub status: String,
}

/// Errors loading or validating a persona definition. No panics in the cognition
/// path (house style) — every failure is a typed, degradable error.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PersonaError {
    #[error("persona.md is missing its opening/closing `+++` TOML frontmatter fence")]
    NoFrontmatter,
    #[error("persona.md frontmatter is not valid TOML / is missing a required field: {0}")]
    Frontmatter(String),
    #[error("persona.md tier must be 'cheap' or 'synthesis', got '{0}'")]
    BadTier(String),
    #[error("schema.json is not valid JSON: {0}")]
    Schema(String),
    #[error("persona '{id}' is not in the registry (no active row) — promote it first")]
    NotRegistered { id: String },
    #[error(
        "persona '{id}' is not active in the registry (status '{status}') — \
         only an 'active' head may run"
    )]
    Inactive { id: String, status: String },
    #[error(
        "persona '{id}' version {file} does not match the registry head version {registry} \
         (bump deliberately: edit the file AND insert a superseding registry row)"
    )]
    VersionMismatch {
        id: String,
        file: i32,
        registry: i32,
    },
    #[error(
        "persona '{id}' method_hash {actual} does not match the registry head {expected} \
         (config/registry mismatch — promote deliberately, never edit in place)"
    )]
    HashMismatch {
        id: String,
        expected: String,
        actual: String,
    },
}

impl PersonaDef {
    /// Parse + content-hash a persona from the raw `persona.md` and `schema.json`
    /// contents. PURE: no filesystem IO. `method_hash` covers the whole
    /// `persona.md` (frontmatter + body), so any edit forces a new hash and a
    /// deliberate registry bump.
    pub fn parse(persona_md: &str, schema_json: &str) -> Result<PersonaDef, PersonaError> {
        let method_hash = content_hash_of(persona_md);
        let (front, method) = split_frontmatter(persona_md)?;
        let meta: PersonaMeta =
            toml::from_str(front).map_err(|e| PersonaError::Frontmatter(e.to_string()))?;
        if meta.tier != "cheap" && meta.tier != "synthesis" {
            return Err(PersonaError::BadTier(meta.tier));
        }
        let schema: serde_json::Value =
            serde_json::from_str(schema_json).map_err(|e| PersonaError::Schema(e.to_string()))?;
        Ok(PersonaDef {
            meta,
            method_hash,
            method: method.to_string(),
            schema,
        })
    }

    /// Validate this definition against the personas-registry head (design §6):
    /// the head must exist, be active, and its version + method_hash must match
    /// this file. Any mismatch refuses — the operator promotes deliberately (a
    /// file edit + a superseding registry insert), never an in-place edit.
    pub fn validate_against(&self, head: Option<&RegistryHead>) -> Result<(), PersonaError> {
        let head = head.ok_or_else(|| PersonaError::NotRegistered {
            id: self.meta.id.clone(),
        })?;
        // Fail-closed: ONLY an explicitly 'active' head may run. A 'retired' head
        // — or any unrecognized future/corrupt status — refuses (defense in depth
        // beyond the ledger's active|retired CHECK constraint).
        if head.status != "active" {
            return Err(PersonaError::Inactive {
                id: self.meta.id.clone(),
                status: head.status.clone(),
            });
        }
        if head.version != self.meta.version {
            return Err(PersonaError::VersionMismatch {
                id: self.meta.id.clone(),
                file: self.meta.version,
                registry: head.version,
            });
        }
        if head.method_hash != self.method_hash {
            return Err(PersonaError::HashMismatch {
                id: self.meta.id.clone(),
                expected: head.method_hash.clone(),
                actual: self.method_hash.clone(),
            });
        }
        Ok(())
    }
}

/// Split a `+++`-fenced TOML-frontmatter document into `(frontmatter, body)`.
/// The file must begin with a `+++` line and have a later closing `+++` line;
/// everything after the closing fence is the trusted method body. Conservative:
/// a missing fence is a typed error, never a panic or a silent empty body.
fn split_frontmatter(raw: &str) -> Result<(&str, &str), PersonaError> {
    // Opening fence on the first line.
    let after_open = raw
        .strip_prefix("+++\n")
        .or_else(|| raw.strip_prefix("+++\r\n"))
        .ok_or(PersonaError::NoFrontmatter)?;
    // Closing fence: a line that is exactly `+++` (so `+++` inside the TOML is
    // not mistaken for the close — it must start a line and be the whole line).
    // All slicing goes through checked `.get(..)` so a malformed input is a typed
    // PersonaError, never a panic (the indices are ASCII-anchored, so this is
    // belt-and-suspenders for the no-panic house rule).
    let mut search_from = 0usize;
    loop {
        let rel = after_open
            .get(search_from..)
            .and_then(|s| s.find("\n+++"))
            .ok_or(PersonaError::NoFrontmatter)?;
        let fence_start = search_from + rel + 1; // index of the '+' after the '\n'
        let after_fence = after_open
            .get(fence_start + 3..) // skip the three '+'
            .ok_or(PersonaError::NoFrontmatter)?;
        // The fence line must end here (EOF, '\n', or '\r\n') — not `+++foo`.
        let body = if let Some(b) = after_fence.strip_prefix('\n') {
            Some(b)
        } else if let Some(b) = after_fence.strip_prefix("\r\n") {
            Some(b)
        } else if after_fence.is_empty() {
            Some("")
        } else {
            None
        };
        match body {
            Some(body) => {
                let front = after_open
                    .get(..search_from + rel)
                    .ok_or(PersonaError::NoFrontmatter)?;
                return Ok((front, body));
            }
            // `+++` was not a standalone fence line; keep searching.
            None => search_from = fence_start + 3,
        }
    }
}
