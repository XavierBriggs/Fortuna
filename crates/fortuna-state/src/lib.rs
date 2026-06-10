//! fortuna-state: positions, account views, marks, reservations. Spec 5.14, 5.13.
//!
//! Account views: settled, committed, floating, total; deployable = settled -
//! committed. Conservative-side marking (bid for long, ask for short;
//! wide/stale book => conservative bound + wide-mark flag). Reservation ledger
//! is DERIVED state: rebuilt at boot from open intents and positions.
//! Exposure accounting: resolution_pending and disputed positions remain in
//! exposure at worst case while excluded from bankroll. Drawdown halt flags
//! (I2): human re-arm only, via CLI.
