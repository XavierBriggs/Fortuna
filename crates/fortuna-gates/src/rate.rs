//! Dual token buckets (spec 5.3 check 7, I3).
//!
//! Integer math throughout: one token = 60_000 scaled units, so a sustained
//! rate of N tokens/minute refills exactly N units per millisecond. Refill
//! is driven by the injected clock (deterministic under DST). The bucket
//! only answers "would this consume?"; halting on breach is the pipeline's
//! job (I3: breach is a halt — refill never un-halts because halted orders
//! die at check 1 and never reach the bucket again).

use fortuna_core::clock::UtcTimestamp;

const SCALE: u64 = 60_000;

#[derive(Debug, Clone)]
pub(crate) struct Bucket {
    capacity_scaled: u64,
    tokens_scaled: u64,
    refill_per_minute: u64,
    last_refill: UtcTimestamp,
}

impl Bucket {
    pub(crate) fn new(burst: u32, sustained_per_min: u32, now: UtcTimestamp) -> Bucket {
        let capacity_scaled = u64::from(burst) * SCALE;
        Bucket {
            capacity_scaled,
            tokens_scaled: capacity_scaled, // starts full
            refill_per_minute: u64::from(sustained_per_min),
            last_refill: now,
        }
    }

    fn refill(&mut self, now: UtcTimestamp) {
        let elapsed_ms = now
            .epoch_millis()
            .saturating_sub(self.last_refill.epoch_millis())
            .max(0) as u64;
        // gain units = elapsed_ms * tokens_per_minute (since 1 token =
        // 60_000 units and a minute = 60_000 ms, the rates cancel exactly).
        let gain = elapsed_ms.saturating_mul(self.refill_per_minute);
        self.tokens_scaled = (self.tokens_scaled.saturating_add(gain)).min(self.capacity_scaled);
        self.last_refill = now;
    }

    /// Try to consume one token. False = breach.
    pub(crate) fn try_consume(&mut self, now: UtcTimestamp) -> bool {
        self.refill(now);
        if self.tokens_scaled >= SCALE {
            self.tokens_scaled -= SCALE;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    #[test]
    fn burst_then_refill_at_exact_rate() {
        // 2 burst, 60/min = 1 token/sec.
        let mut b = Bucket::new(2, 60, ts(0));
        assert!(b.try_consume(ts(0)));
        assert!(b.try_consume(ts(0)));
        assert!(!b.try_consume(ts(0))); // drained
        assert!(!b.try_consume(ts(999))); // 999ms: 0.999 tokens — not yet
        assert!(b.try_consume(ts(1_000))); // exactly one second: one token
        assert!(!b.try_consume(ts(1_000)));
    }

    #[test]
    fn refill_caps_at_burst() {
        let mut b = Bucket::new(2, 60, ts(0));
        assert!(b.try_consume(ts(0)));
        assert!(b.try_consume(ts(0)));
        // A year later: still only 2 tokens.
        let later = ts(31_536_000_000);
        assert!(b.try_consume(later));
        assert!(b.try_consume(later));
        assert!(!b.try_consume(later));
    }

    #[test]
    fn zero_burst_never_consumes() {
        let mut b = Bucket::new(0, 60, ts(0));
        assert!(!b.try_consume(ts(0)));
        // Refill cannot exceed zero capacity.
        assert!(!b.try_consume(ts(1_000_000)));
    }
}
