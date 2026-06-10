//! The context assembler (spec 5.7): deterministic, budgeted context
//! packing per cycle type.
//!
//! Every build emits a MANIFEST (item ids + content hashes); the manifest
//! hash lives in belief provenance, making any decision reconstructable.
//! Replayability: every item is either an immutable stored item
//! (referenced by id + hash — VERIFIED here, fail-closed) or a computed
//! view snapshotted into an item by the caller. Point-in-time: only data
//! timestamped strictly BEFORE the cycle trigger enters context; later
//! items are excluded and counted, never absorbed.
//!
//! Injection hygiene at the formatting layer (5.11): item bodies render
//! inside delimited `<context-item>` data blocks — content is quoted
//! data, never prose the model should read as instructions. (I6 and the
//! gates bound the blast radius regardless.)

use fortuna_core::clock::UtcTimestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContextError {
    #[error(
        "context item {item_id} content does not match its claimed hash \
         (claimed {claimed}, computed {computed}) — refusing to assemble \
         from corrupted references"
    )]
    HashMismatch {
        item_id: String,
        claimed: String,
        computed: String,
    },
    #[error("manifest serialization failed: {reason} (a context without a true manifest hash is not replayable)")]
    ManifestSerialize { reason: String },
}

/// Section vocabulary in PRIORITY ORDER (spec 5.7). The discriminant
/// order IS the packing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Charter,
    AccountState,
    OpenBeliefs,
    MarketSnapshot,
    FreshSignals,
    Lessons,
    Episodic,
}

impl SectionKind {
    fn as_str(self) -> &'static str {
        match self {
            SectionKind::Charter => "charter",
            SectionKind::AccountState => "account_state",
            SectionKind::OpenBeliefs => "open_beliefs",
            SectionKind::MarketSnapshot => "market_snapshot",
            SectionKind::FreshSignals => "fresh_signals",
            SectionKind::Lessons => "lessons",
            SectionKind::Episodic => "episodic",
        }
    }
}

/// One candidate context item: an immutable stored item by reference, or
/// a computed view the CALLER snapshotted (body + hash assigned at
/// snapshot time — expensive recomputation traded for a stored copy).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextItem {
    pub item_id: String,
    pub section: SectionKind,
    pub body: String,
    pub content_hash: String,
    pub at: UtcTimestamp,
}

/// SHA-256 hex of a body (the item hashing convention).
pub fn content_hash_of(body: &str) -> String {
    let digest = Sha256::digest(body.as_bytes());
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[derive(Debug, Clone)]
pub struct AssemblerConfig {
    /// Budget in CHARACTERS of item-body content (deterministic;
    /// tokenizers are model-specific — see ASSUMPTIONS).
    pub budget_chars: usize,
    /// Strip entity identifiers from the RENDERED text (retrospective
    /// evaluation mode). The manifest always keeps real ids.
    pub anonymize: bool,
}

/// One manifest line: what went in, verifiable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestItem {
    pub item_id: String,
    pub section: SectionKind,
    pub content_hash: String,
}

/// The audit artifact: list of included items + the exclusion counts
/// (excluded-as-future and skipped-over-budget are REPORTED, not silent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextManifest {
    pub cycle_kind: String,
    pub trigger_at: UtcTimestamp,
    pub budget_chars: usize,
    pub used_chars: usize,
    pub items: Vec<ManifestItem>,
    pub excluded_future: usize,
    pub skipped_over_budget: usize,
}

/// An assembled context: the rendered text, the manifest, and its hash
/// (what belief provenance records).
#[derive(Debug, Clone)]
pub struct AssembledContext {
    pub rendered: String,
    pub manifest: ContextManifest,
    pub manifest_hash: String,
}

/// Assemble a context. Deterministic: same items + trigger + config =>
/// byte-identical render and manifest hash.
pub fn assemble_context(
    items: &[ContextItem],
    trigger_at: UtcTimestamp,
    cycle_kind: &str,
    config: &AssemblerConfig,
) -> Result<AssembledContext, ContextError> {
    // Verify EVERY offered item's hash first (fail-closed: a corrupted
    // reference poisons replayability whether or not it would fit).
    for item in items {
        let computed = content_hash_of(&item.body);
        if computed != item.content_hash {
            return Err(ContextError::HashMismatch {
                item_id: item.item_id.clone(),
                claimed: item.content_hash.clone(),
                computed,
            });
        }
    }

    // Point-in-time: strictly before the trigger.
    let mut excluded_future = 0usize;
    let mut eligible: Vec<&ContextItem> = Vec::new();
    for item in items {
        if item.at.epoch_millis() < trigger_at.epoch_millis() {
            eligible.push(item);
        } else {
            excluded_future += 1;
        }
    }

    // Stable packing order: section priority, then input order within a
    // section (sort is stable).
    eligible.sort_by_key(|i| i.section);

    let mut used = 0usize;
    let mut skipped = 0usize;
    let mut included: Vec<&ContextItem> = Vec::new();
    for item in eligible {
        let len = item.body.chars().count();
        if used + len > config.budget_chars {
            skipped += 1;
            continue; // greedy: later, smaller items may still fit
        }
        used += len;
        included.push(item);
    }

    // Render with pseudonyms when anonymizing (stable within the build).
    let mut pseudonyms: BTreeMap<String, String> = BTreeMap::new();
    let mut next_pseudonym = 0usize;
    let mut rendered = String::new();
    let mut current_section: Option<SectionKind> = None;
    for item in &included {
        if current_section != Some(item.section) {
            rendered.push_str(&format!("== {} ==\n", item.section.as_str()));
            current_section = Some(item.section);
        }
        let display_id = if config.anonymize {
            pseudonyms
                .entry(item.item_id.clone())
                .or_insert_with(|| {
                    next_pseudonym += 1;
                    format!("ITEM-{next_pseudonym}")
                })
                .clone()
        } else {
            item.item_id.clone()
        };
        rendered.push_str(&format!(
            "<context-item id=\"{display_id}\" section=\"{}\">\n{}\n</context-item>\n",
            item.section.as_str(),
            item.body
        ));
    }

    let manifest = ContextManifest {
        cycle_kind: cycle_kind.to_string(),
        trigger_at,
        budget_chars: config.budget_chars,
        used_chars: used,
        items: included
            .iter()
            .map(|i| ManifestItem {
                item_id: i.item_id.clone(),
                section: i.section,
                content_hash: i.content_hash.clone(),
            })
            .collect(),
        excluded_future,
        skipped_over_budget: skipped,
    };
    let manifest_json =
        serde_json::to_string(&manifest).map_err(|e| ContextError::ManifestSerialize {
            reason: e.to_string(),
        })?;
    let manifest_hash = content_hash_of(&manifest_json);

    Ok(AssembledContext {
        rendered,
        manifest,
        manifest_hash,
    })
}
