//! Layer 1 structural validation (design §4.4): the pre-normalizer gate that
//! every fetched item passes before it is offered to the signals normalizer.
//! It enforces three structural properties the normalizer does not:
//!
//! 1. **Timestamp sanity** — an item claiming an event/publish time in the
//!    future (beyond a small clock-skew tolerance) is rejected. Point-in-time
//!    discipline (spec 5.11): `received_at` is authority, and a future claim
//!    is either a source bug or a clock attack — never fresh signal.
//! 2. **Stale-republication flag** — a content hash seen recently is a classic
//!    feed pathology (an old item re-emitted so it masquerades as fresh). The
//!    design says detect-and-flag, not silently re-ingest. NOTE: the
//!    authoritative dedup is the ledger's `UNIQUE(source, content_hash)`; this
//!    bounded in-memory buffer is a fast-path observability flag, not the
//!    source of truth (it cannot see beyond its window). Recorded in GAPS.
//! 3. **Per-tick volume envelope** — a source suddenly emitting many times its
//!    normal volume is throttled at a per-tick cap and the overflow counted
//!    (design §7 containment: bounds storage whether the burst is a poisoned
//!    feed or a real news spike — the trigger engine's debounce handles the
//!    latter downstream).
//!
//! Deterministic and Clock-driven: no wall time, so it replays under DST.

use std::collections::{HashSet, VecDeque};

use fortuna_core::clock::UtcTimestamp;

/// A fetched item as seen by Layer 1, before normalization. The adapter has
/// already computed the content hash and extracted any source-claimed time.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Hash of the canonical payload (the adapter computes it; the normalizer
    /// will recompute and dedup authoritatively).
    pub content_hash: String,
    /// The event/publish time the SOURCE claims, if any. Untrusted (it is
    /// source-supplied) — which is exactly why it is sanity-checked here.
    pub claimed_time: Option<UtcTimestamp>,
}

/// The structural verdict for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Passes Layer 1; offer it to the normalizer.
    Accept,
    /// Claimed time is in the future beyond tolerance; dropped.
    RejectFuture {
        claimed_ms: i64,
        now_ms: i64,
        tolerance_ms: i64,
    },
    /// Content hash was seen within the recent window; flagged, not re-ingested.
    RejectRepublished,
    /// This tick's accepted-item envelope is full; dropped and counted.
    RejectOverVolume { envelope: usize },
}

impl Verdict {
    pub fn is_accept(&self) -> bool {
        matches!(self, Verdict::Accept)
    }
}

/// Tunables for one source's validator (from config; defaults are
/// conservative). All durations are milliseconds to match `UtcTimestamp`.
#[derive(Debug, Clone)]
pub struct StructuralConfig {
    /// How far ahead of `now` a claimed time may be before it is rejected as
    /// future-dated. Absorbs benign clock skew between us and the source.
    pub future_skew_tolerance_ms: i64,
    /// Maximum items ACCEPTED per tick (per-source volume envelope, §7).
    pub volume_envelope: usize,
    /// How many recent content hashes to remember for republication flagging.
    pub recent_hash_capacity: usize,
}

impl Default for StructuralConfig {
    fn default() -> StructuralConfig {
        StructuralConfig {
            // 5 minutes: generous enough for source/our skew, tight enough that
            // a genuinely future-stamped item is still caught.
            future_skew_tolerance_ms: 5 * 60 * 1000,
            volume_envelope: 512,
            recent_hash_capacity: 4096,
        }
    }
}

/// One source's Layer 1 validator. Holds the bounded recent-hash memory and
/// the current tick's accept count. Single-source: the scheduler owns one per
/// source (no cross-source hash bleed — republication is per-source).
#[derive(Debug)]
pub struct StructuralValidator {
    cfg: StructuralConfig,
    recent_order: VecDeque<String>,
    recent_set: HashSet<String>,
    accepted_this_tick: usize,
    /// Overflow/anomaly counters since construction (telemetry, never reset by
    /// the tick boundary — operators watch these for poisoned feeds).
    pub over_volume_dropped: u64,
    pub future_rejected: u64,
    pub republished_flagged: u64,
}

impl StructuralValidator {
    pub fn new(cfg: StructuralConfig) -> StructuralValidator {
        let cap = cfg.recent_hash_capacity.max(1);
        StructuralValidator {
            cfg,
            recent_order: VecDeque::with_capacity(cap),
            recent_set: HashSet::with_capacity(cap),
            accepted_this_tick: 0,
            over_volume_dropped: 0,
            future_rejected: 0,
            republished_flagged: 0,
        }
    }

    /// Reset the per-tick accept count. The scheduler calls this once per poll
    /// of this source, before assessing that poll's items. Recent-hash memory
    /// deliberately persists across ticks (republication spans polls).
    pub fn begin_tick(&mut self) {
        self.accepted_this_tick = 0;
    }

    /// Assess one candidate as of `now`. Order matters: future-dated and
    /// republished items are dropped WITHOUT consuming volume budget (they are
    /// not real new signal), so a flood of duplicates cannot crowd out genuine
    /// items under the envelope.
    pub fn assess(&mut self, now: UtcTimestamp, candidate: &Candidate) -> Verdict {
        if let Some(claimed) = candidate.claimed_time {
            let claimed_ms = claimed.epoch_millis();
            let now_ms = now.epoch_millis();
            if claimed_ms > now_ms.saturating_add(self.cfg.future_skew_tolerance_ms) {
                self.future_rejected += 1;
                return Verdict::RejectFuture {
                    claimed_ms,
                    now_ms,
                    tolerance_ms: self.cfg.future_skew_tolerance_ms,
                };
            }
        }

        if self.recent_set.contains(&candidate.content_hash) {
            self.republished_flagged += 1;
            return Verdict::RejectRepublished;
        }

        if self.accepted_this_tick >= self.cfg.volume_envelope {
            self.over_volume_dropped += 1;
            return Verdict::RejectOverVolume {
                envelope: self.cfg.volume_envelope,
            };
        }

        self.remember(candidate.content_hash.clone());
        self.accepted_this_tick += 1;
        Verdict::Accept
    }

    /// Record a hash in the bounded recent buffer, evicting the oldest when at
    /// capacity (FIFO — a stable, replayable eviction policy).
    fn remember(&mut self, hash: String) {
        if self.recent_set.insert(hash.clone()) {
            self.recent_order.push_back(hash);
            while self.recent_order.len() > self.cfg.recent_hash_capacity {
                if let Some(evicted) = self.recent_order.pop_front() {
                    self.recent_set.remove(&evicted);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    fn cand(hash: &str, claimed: Option<i64>) -> Candidate {
        Candidate {
            content_hash: hash.to_string(),
            claimed_time: claimed.map(ts),
        }
    }

    fn cfg(envelope: usize, capacity: usize, skew_ms: i64) -> StructuralConfig {
        StructuralConfig {
            future_skew_tolerance_ms: skew_ms,
            volume_envelope: envelope,
            recent_hash_capacity: capacity,
        }
    }

    #[test]
    fn accepts_a_fresh_in_window_item() {
        let mut v = StructuralValidator::new(StructuralConfig::default());
        v.begin_tick();
        assert_eq!(
            v.assess(ts(1_000_000), &cand("h1", Some(999_000))),
            Verdict::Accept
        );
    }

    #[test]
    fn rejects_future_dated_beyond_tolerance_but_allows_within() {
        let mut v = StructuralValidator::new(cfg(512, 4096, 60_000));
        v.begin_tick();
        // 60s tolerance: a claim 60s ahead is fine; 60.001s ahead is rejected.
        assert!(v
            .assess(ts(1_000_000), &cand("ok", Some(1_060_000)))
            .is_accept());
        let verdict = v.assess(ts(1_000_000), &cand("bad", Some(1_060_001)));
        assert!(matches!(verdict, Verdict::RejectFuture { .. }));
        assert_eq!(v.future_rejected, 1);
    }

    #[test]
    fn items_without_a_claimed_time_skip_the_future_check() {
        let mut v = StructuralValidator::new(StructuralConfig::default());
        v.begin_tick();
        assert!(v.assess(ts(0), &cand("h", None)).is_accept());
    }

    #[test]
    fn flags_republished_content_without_consuming_volume() {
        // Envelope of 1: prove the duplicate does NOT eat the single slot.
        let mut v = StructuralValidator::new(cfg(1, 4096, 0));
        v.begin_tick();
        assert!(v.assess(ts(10), &cand("dup", None)).is_accept());
        // Same hash again -> republished flag, not an over-volume drop.
        assert_eq!(
            v.assess(ts(11), &cand("dup", None)),
            Verdict::RejectRepublished
        );
        assert_eq!(v.republished_flagged, 1);
        assert_eq!(v.over_volume_dropped, 0);
    }

    #[test]
    fn republication_is_detected_across_ticks() {
        let mut v = StructuralValidator::new(cfg(512, 4096, 0));
        v.begin_tick();
        assert!(v.assess(ts(10), &cand("yesterday", None)).is_accept());
        v.begin_tick(); // next poll
        assert_eq!(
            v.assess(ts(86_400_010), &cand("yesterday", None)),
            Verdict::RejectRepublished
        );
    }

    #[test]
    fn enforces_per_tick_volume_envelope_then_resets_next_tick() {
        let mut v = StructuralValidator::new(cfg(2, 4096, 0));
        v.begin_tick();
        assert!(v.assess(ts(0), &cand("a", None)).is_accept());
        assert!(v.assess(ts(0), &cand("b", None)).is_accept());
        assert_eq!(
            v.assess(ts(0), &cand("c", None)),
            Verdict::RejectOverVolume { envelope: 2 }
        );
        assert_eq!(v.over_volume_dropped, 1);
        // New tick: budget refreshes (distinct hashes so no republication).
        v.begin_tick();
        assert!(v.assess(ts(0), &cand("d", None)).is_accept());
    }

    #[test]
    fn recent_hash_buffer_is_bounded_and_evicts_fifo() {
        // Capacity 2: after 3 distinct hashes, the first is evicted and may
        // re-appear as "fresh" (the bounded buffer cannot see that far back;
        // the ledger UNIQUE constraint is the real backstop).
        let mut v = StructuralValidator::new(cfg(512, 2, 0));
        v.begin_tick();
        assert!(v.assess(ts(0), &cand("h1", None)).is_accept());
        assert!(v.assess(ts(0), &cand("h2", None)).is_accept());
        assert!(v.assess(ts(0), &cand("h3", None)).is_accept()); // evicts h1
                                                                 // h1 is now beyond the window -> accepted again (buffer is bounded).
        assert!(v.assess(ts(0), &cand("h1", None)).is_accept());
        // h2/h3 still remembered.
        assert_eq!(
            v.assess(ts(0), &cand("h3", None)),
            Verdict::RejectRepublished
        );
    }

    #[test]
    fn future_dated_does_not_consume_volume() {
        let mut v = StructuralValidator::new(cfg(1, 4096, 0));
        v.begin_tick();
        // A future item is rejected and must not eat the envelope slot.
        assert!(matches!(
            v.assess(ts(1000), &cand("future", Some(10_000))),
            Verdict::RejectFuture { .. }
        ));
        assert!(v.assess(ts(1000), &cand("real", None)).is_accept());
    }
}
