//! Daily drawdown monitor (I2 support).
//!
//! This computes BREACHES; the halt FLAG lives in fortuna-gates and only a
//! human can clear it, out-of-band (invariant I2). The monitor's own
//! stickiness - once breached it keeps answering `Breach` for the rest of
//! the UTC day even if equity recovers - is defense in depth on top of that
//! flag, NOT the lock itself; it clears only on the UTC day roll.
//!
//! Day boundary: 00:00 UTC (spec: "day = 00:00 UTC"). `roll_day_if_needed`
//! baselines `day_start_equity` at the first observation and whenever `now`
//! crosses into a NEW (later) UTC day; repeated same-day calls never move
//! the baseline (intraday losses are never silently forgiven), and a
//! backwards day (impossible under the monotone `Clock`) never re-baselines
//! or clears stickiness. `check` rolls internally first, so a caller that
//! forgets `roll_day_if_needed` can never carry yesterday's baseline into
//! today.
//!
//! Breach rule: `loss = day_start_equity - current_equity` (checked);
//! verdict is `Breach` ONLY when `max_daily_loss > 0` AND `loss >=
//! max_daily_loss`. A non-positive limit disables the monitor (never
//! breaches) - configure a positive limit to arm it. The reported `loss` is
//! always the CURRENT loss, which may be negative (a gain) while a sticky
//! breach is still being reported.

use crate::StateError;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::money::Cents;

const MILLIS_PER_UTC_DAY: i64 = 86_400_000;

/// UTC day index (days since epoch; floor division handles pre-epoch too).
fn utc_day_index(at: UtcTimestamp) -> i64 {
    at.epoch_millis().div_euclid(MILLIS_PER_UTC_DAY)
}

/// The verdict for one equity observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawdownVerdict {
    Ok,
    /// Daily loss reached the limit (or did earlier today: sticky). `loss`
    /// is the current observation's loss; it may be below the limit (even
    /// negative) on sticky reports after a recovery.
    Breach {
        loss: Cents,
        limit: Cents,
    },
}

#[derive(Debug, Clone, Copy)]
struct Day {
    index: i64,
    start_equity: Cents,
}

/// Daily-loss breach computation with same-day stickiness.
#[derive(Debug, Clone)]
pub struct DrawdownMonitor {
    max_daily_loss: Cents,
    day: Option<Day>,
    breached_today: bool,
}

impl DrawdownMonitor {
    /// `max_daily_loss > 0` arms the monitor; non-positive disables it.
    pub fn new(max_daily_loss: Cents) -> DrawdownMonitor {
        DrawdownMonitor {
            max_daily_loss,
            day: None,
            breached_today: false,
        }
    }

    /// Baseline `day_start_equity` on the first observation or when `now`
    /// has crossed into a later UTC day; clears the sticky breach flag on
    /// roll. Same-day calls (and backwards time) are no-ops.
    pub fn roll_day_if_needed(&mut self, now: UtcTimestamp, current_equity: Cents) {
        let index = utc_day_index(now);
        let rolls = match self.day {
            None => true,
            Some(day) => index > day.index,
        };
        if rolls {
            self.day = Some(Day {
                index,
                start_equity: current_equity,
            });
            self.breached_today = false;
        }
    }

    /// Evaluate the daily-loss rule at `now`. Rolls the day internally
    /// first. Sticky: after a breach, keeps returning `Breach` until the
    /// next UTC day (the gates' human-cleared halt flag is the real lock).
    pub fn check(
        &mut self,
        now: UtcTimestamp,
        current_equity: Cents,
    ) -> Result<DrawdownVerdict, StateError> {
        self.roll_day_if_needed(now, current_equity);
        // roll_day_if_needed guarantees Some; the fallback (loss == 0) is
        // unreachable but beats panicking.
        let start_equity = match self.day {
            Some(day) => day.start_equity,
            None => current_equity,
        };
        let loss = start_equity
            .checked_sub(current_equity)
            .map_err(StateError::Money)?;
        let limit = self.max_daily_loss;
        if limit > Cents::ZERO && loss >= limit {
            self.breached_today = true;
        }
        if self.breached_today {
            Ok(DrawdownVerdict::Breach { loss, limit })
        } else {
            Ok(DrawdownVerdict::Ok)
        }
    }
}
