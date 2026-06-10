//! `fortuna` — the operator CLI (spec Section 8; the I2 re-arm path).
//!
//! Drawdown-halt re-arm and kill-switch reversal are CLI-ONLY by design:
//! Slack may request, the CLI confirms; a compromised Slack token must not
//! be able to un-halt a halted system.
//!
//! Commands:
//!   fortuna status
//!   fortuna halt   <global|strategy:<id>|venue:<id>> --reason "..." --operator <name>
//!   fortuna rearm  <global|strategy:<id>|venue:<id>> --reason "..." --operator <name>
//!   fortuna kill   [--flatten] --journal <path>
//!
//! halt/rearm write durable halt_events + an audit row; the running system
//! restores flags from the fold at boot and observes operator events via its
//! halt-poll (runner, T0.10). `kill` execs the STANDALONE fortuna-killswitch
//! binary — this CLI is a trigger, never a substitute for it.
//!
//! Binaries may use anyhow (conventions); the no-unwrap rule still holds.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

use anyhow::{bail, Context, Result};
use fortuna_core::clock::{Clock, RealClock};
use fortuna_ledger::{parse_halt_scope, AuditWriter, HaltsRepo};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fortuna: {e:#}");
            ExitCode::from(1)
        }
    }
}

struct Args {
    command: String,
    positional: Vec<String>,
    reason: Option<String>,
    operator: Option<String>,
    journal: Option<String>,
    flatten: bool,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        command: String::new(),
        positional: Vec::new(),
        reason: None,
        operator: None,
        journal: None,
        flatten: false,
    };
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--reason" => {
                i += 1;
                args.reason = raw.get(i).cloned();
            }
            "--operator" => {
                i += 1;
                args.operator = raw.get(i).cloned();
            }
            "--journal" => {
                i += 1;
                args.journal = raw.get(i).cloned();
            }
            "--flatten" => args.flatten = true,
            other if args.command.is_empty() => args.command = other.to_string(),
            other => args.positional.push(other.to_string()),
        }
        i += 1;
    }
    if args.command.is_empty() {
        bail!(
            "usage: fortuna <status|halt|rearm|kill> [scope] \
             [--reason ..] [--operator ..] [--journal ..] [--flatten]"
        );
    }
    Ok(args)
}

fn run() -> Result<()> {
    let args = parse_args()?;
    match args.command.as_str() {
        "kill" => kill(&args),
        "status" | "halt" | "rearm" => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio runtime")?;
            runtime.block_on(db_command(&args))
        }
        other => bail!("unknown command {other:?}"),
    }
}

/// Trigger the STANDALONE kill switch. This must keep working with Postgres
/// down, so it never touches the database — it execs the independent binary.
fn kill(args: &Args) -> Result<()> {
    let journal = args
        .journal
        .clone()
        .unwrap_or_else(|| "/tmp/fortuna-killswitch.jsonl".to_string());
    let action = if args.flatten { "report" } else { "freeze" };
    eprintln!("fortuna: invoking standalone kill switch ({action}, journal {journal})");
    let status = std::process::Command::new("fortuna-killswitch")
        .args([action, "--journal", &journal])
        .status()
        .or_else(|_| {
            // Dev fallback: through cargo when the installed binary is absent.
            std::process::Command::new(env!("CARGO"))
                .args([
                    "run",
                    "-q",
                    "-p",
                    "fortuna-killswitch",
                    "--",
                    action,
                    "--journal",
                    &journal,
                ])
                .status()
        })
        .context("spawning fortuna-killswitch")?;
    if !status.success() {
        bail!("kill switch exited with {status}");
    }
    Ok(())
}

async fn db_command(args: &Args) -> Result<()> {
    let url = std::env::var("DATABASE_URL").context(
        "DATABASE_URL is required for status/halt/rearm (the kill command works without it)",
    )?;
    let pool = fortuna_ledger::connect(&url).await?;
    let halts = HaltsRepo::new(pool.clone());
    let clock = RealClock;
    let now = clock.now();

    match args.command.as_str() {
        "status" => {
            let active = halts.active().await?;
            if active.is_empty() {
                println!("halts: none");
            } else {
                println!("halts ({}):", active.len());
                for (scope, reason) in active {
                    println!("  {} — {reason}", fortuna_ledger::halt_scope_string(&scope));
                }
            }
            let audit = AuditWriter::new(
                pool,
                std::sync::Arc::new(RealClock),
                now.epoch_millis() as u64,
            );
            for kind in ["halt", "gate_decision", "order"] {
                let rows = audit.recent(kind, 3).await?;
                if !rows.is_empty() {
                    println!("recent {kind}:");
                    for r in rows {
                        println!("  {} {}", r.at, r.payload);
                    }
                }
            }
            Ok(())
        }
        "halt" | "rearm" => {
            let scope_raw = args
                .positional
                .first()
                .context("scope required: global | strategy:<id> | venue:<id>")?;
            let scope = parse_halt_scope(scope_raw)
                .with_context(|| format!("unparseable scope {scope_raw:?}"))?;
            let reason = args.reason.clone().context("--reason is required")?;
            let operator = args
                .operator
                .clone()
                .context("--operator <name> is required (operator actions are attributed)")?;
            let audit = AuditWriter::new(
                pool.clone(),
                std::sync::Arc::new(RealClock),
                now.epoch_millis() as u64,
            );
            if args.command == "halt" {
                halts.record_set(&scope, &reason, &operator, now).await?;
                audit
                    .append(
                        "halt",
                        Some(&operator),
                        None,
                        serde_json::json!({"action": "set", "scope": scope_raw, "reason": reason}),
                    )
                    .await?;
                println!(
                    "halt set on {scope_raw}; the runner enforces it within its poll interval"
                );
            } else {
                // I2: THE human re-arm path. Out-of-band by construction.
                halts.record_rearm(&scope, &reason, &operator, now).await?;
                audit
                    .append(
                        "halt",
                        Some(&operator),
                        None,
                        serde_json::json!({"action": "rearm", "scope": scope_raw, "reason": reason}),
                    )
                    .await?;
                println!("re-armed {scope_raw} (operator: {operator})");
            }
            Ok(())
        }
        _ => bail!("unreachable"),
    }
}
