//! fortuna-live — the T4.1 daemon binary (composition lands in later
//! iterations; this binary already FAILS CLOSED on bad boot inputs).
//!
//! The ONLY place in the crate that touches the real world: process env,
//! the config file, and (eventually) the runtime. Per the T4.1 kickoff
//! daemon corollary, the operator's .env is loaded EXPLICITLY here and a
//! bare-cargo-run inherited DATABASE_URL is never trusted on its own —
//! boot refuses when the other secrets are absent, which is exactly what
//! an un-sourced environment looks like.

use anyhow::{bail, Context, Result};
use fortuna_live::boot::{validate_env, DaemonToml};
use std::collections::BTreeMap;

fn main() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/fortuna.toml".to_string());
    let config_text = std::fs::read_to_string(&config_path).with_context(|| {
        format!("reading config at {config_path} (copy config/fortuna.example.toml)")
    })?;
    let cfg = DaemonToml::parse(&config_text).context("config rejected")?;
    cfg.validate_bootable().context("boot check failed")?;

    let env: BTreeMap<String, String> = std::env::vars().collect();
    let validated = validate_env(&env).context(
        "environment rejected (set -a && source .env && set +a, or a systemd EnvironmentFile)",
    )?;

    if validated.anthropic_api_key.is_none() && !cfg.cognition.allow_stub_mind {
        bail!(
            "ANTHROPIC_API_KEY is absent and [cognition] allow_stub_mind = false: \
             booting would silently run the stub mind. Set the key or opt into \
             the stub explicitly."
        );
    }

    println!(
        "fortuna-live: boot inputs validated (venue={}, tick={}ms, halt_poll={}ms, metrics={})",
        cfg.daemon.venue,
        cfg.daemon.tick_interval_ms,
        cfg.daemon.halt_poll_ms,
        cfg.daemon.metrics_bind
    );
    bail!(
        "the runtime composition is not wired yet (T4.1 in progress; BUILD_PLAN Phase 4). \
         Refusing to pretend to run."
    );
}
