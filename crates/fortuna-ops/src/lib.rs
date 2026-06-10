//! fortuna-ops: operations surface. Spec Section 8. I4.
//!
//! Config loader (TOML; secrets from env only). Slack client with channel
//! routing (every outbound message also writes an audit row; re-arm and
//! kill-reversal are CLI-ONLY). CLI: status, halt, re-arm, kill. Dead-man
//! pinger (external monitor). Metrics (OpenTelemetry), minimal read-only
//! dashboard, nightly accounting export.
//!
//! BINARY `fortuna-killswitch`: STANDALONE. Own credentials, flat-file/SQLite
//! state, NO Postgres, NO dependence on the main runtime. Freeze-and-cancel
//! default; flatten best-effort without the planner (spec 5.4 exemption).
//! Must run correctly with everything else dead. Tested monthly.
