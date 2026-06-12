//! T4.1 hard requirement 1 (kickoff): FAIL-CLOSED BOOT. Missing or
//! placeholder env, an unroutable Slack channel, or an unbootable venue
//! refuses to start with a PRECISE error naming the offender.
//!
//! Tests are written from the kickoff/spec text BEFORE the implementation
//! and run against PURE functions over injected maps — no test here may
//! ever read the real process environment (the kickoff pitfall: with
//! ANTHROPIC_API_KEY present, the real mind spends real money).

use fortuna_live::boot::{validate_env, BootError, DaemonToml, RequiredEnv};
use std::collections::BTreeMap;

/// A fully-populated, plausible env map (values are synthetic).
fn good_env() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(
        "DATABASE_URL".into(),
        "postgres://app@localhost/somedb".into(),
    );
    m.insert(
        "ANTHROPIC_API_KEY".into(),
        "sk-ant-synthetic-not-real".into(),
    );
    m.insert("FORTUNA_SLACK_BOT_TOKEN".into(), "xoxb-synthetic".into());
    m.insert("FORTUNA_SLACK_CHANNEL_TRADING".into(), "C0TRADING".into());
    m.insert("FORTUNA_SLACK_CHANNEL_ALERTS".into(), "C0ALERTS".into());
    m.insert("FORTUNA_SLACK_CHANNEL_REVIEW".into(), "C0REVIEW".into());
    m.insert("FORTUNA_SLACK_CHANNEL_DIGEST".into(), "C0DIGEST".into());
    m.insert("FORTUNA_SLACK_CHANNEL_OPS".into(), "C0OPS".into());
    m.insert(
        "FORTUNA_DEADMAN_URL".into(),
        "https://hc.example/ping/abc".into(),
    );
    m
}

#[test]
fn complete_env_validates() {
    let env = validate_env(&good_env()).expect("complete env must validate");
    assert_eq!(env.slack_channels.len(), 5);
    assert_eq!(env.slack_channels["trading"], "C0TRADING");
    assert!(env.anthropic_api_key.is_some());
}

#[test]
fn each_missing_var_is_named_precisely() {
    for var in [
        "DATABASE_URL",
        "FORTUNA_SLACK_BOT_TOKEN",
        "FORTUNA_SLACK_CHANNEL_TRADING",
        "FORTUNA_SLACK_CHANNEL_ALERTS",
        "FORTUNA_SLACK_CHANNEL_REVIEW",
        "FORTUNA_SLACK_CHANNEL_DIGEST",
        "FORTUNA_SLACK_CHANNEL_OPS",
        "FORTUNA_DEADMAN_URL",
    ] {
        let mut env = good_env();
        env.remove(var);
        match validate_env(&env) {
            Err(BootError::MissingEnv { var: v }) => assert_eq!(v, var),
            other => panic!("removing {var} must refuse with MissingEnv, got {other:?}"),
        }
    }
}

#[test]
fn placeholder_values_refuse_loudly() {
    // The exact ways a half-edited .env shows up, including the literal
    // .env.example placeholders.
    for (var, bad) in [
        ("DATABASE_URL", "postgres://USER:PASSWORD@localhost/fortuna"),
        ("FORTUNA_SLACK_BOT_TOKEN", "xoxb-REPLACE_ME"),
        ("FORTUNA_SLACK_CHANNEL_OPS", ""),
        ("FORTUNA_SLACK_CHANNEL_OPS", "   "),
        ("FORTUNA_DEADMAN_URL", "https://example.com/<your-uuid>"),
        ("ANTHROPIC_API_KEY", "sk-ant-your-key-here"),
        ("DATABASE_URL", "changeme"),
    ] {
        let mut env = good_env();
        env.insert(var.to_string(), bad.to_string());
        match validate_env(&env) {
            Err(BootError::PlaceholderEnv { var: v, .. }) => assert_eq!(v, var),
            other => panic!("{var}={bad:?} must refuse as placeholder, got {other:?}"),
        }
    }
}

#[test]
fn anthropic_key_is_optional_only_because_config_gates_it() {
    // mind_from_env treats an absent key as StubMind; whether the DAEMON
    // accepts that is a CONFIG decision (allow_stub_mind), enforced at
    // compose time. validate_env records the absence; it does not decide.
    let mut env = good_env();
    env.remove("ANTHROPIC_API_KEY");
    let parsed = validate_env(&env).expect("absent anthropic key is recorded, not refused here");
    assert!(parsed.anthropic_api_key.is_none());
}

#[test]
fn daemon_toml_parses_the_committed_example() {
    // The example config MUST parse — operators copy it. The [daemon]
    // section ships in the example with venue = "sim".
    let example = include_str!("../../../config/fortuna.example.toml");
    let cfg = DaemonToml::parse(example).expect("committed example must parse");
    assert_eq!(cfg.daemon.venue, "sim");
    assert_eq!(cfg.daemon.halt_poll_ms, 500);
    assert!(cfg.daemon.metrics_bind.starts_with("127.0.0.1:"));
    assert!(cfg.cognition.daily_budget_cents > 0);
    assert!(cfg.cognition.per_cycle_budget_cents > 0);
    assert!(!cfg.cognition.allow_stub_mind);
}

#[test]
fn synthesis_section_is_optional_and_parses_when_present() {
    // S3b opt-in: the [synthesis] section's PRESENCE composes synthesis into the
    // daemon (wired at compose_runner); ABSENT => the daemon runs mechanically-
    // only (fail closed). Its fields only FILTER the confirmed edge set.
    let example = include_str!("../../../config/fortuna.example.toml");
    // The committed example ships WITHOUT [synthesis] -> opt-out.
    let without = DaemonToml::parse(example).expect("parse ok");
    assert!(
        without.synthesis.is_none(),
        "no [synthesis] => the daemon stays mechanically-only"
    );
    // Present: the filters parse into the optional section (NON-VACUOUS values).
    let with = format!("{example}\n[synthesis]\nvenue = \"kalshi\"\nmax_edges = 8\n");
    let syn = DaemonToml::parse(&with)
        .expect("parse with [synthesis] ok")
        .synthesis
        .expect("the [synthesis] section is present");
    assert_eq!(syn.venue.as_deref(), Some("kalshi"));
    assert_eq!(syn.max_edges, Some(8));
}

#[test]
fn review_section_parses_from_the_committed_example_and_is_optional() {
    // T4.1/M2 slice A: the [review] section's PRESENCE composes the weekly/
    // monthly review cadence (the wiring slice consumes it); its GO/NO-GO
    // thresholds are REQUIRED (no silent default for a risk gate). The committed
    // example ships [review].
    let example = include_str!("../../../config/fortuna.example.toml");
    let review = DaemonToml::parse(example)
        .expect("committed example with [review] parses")
        .review
        .expect("the example ships a [review] section");
    assert_eq!(review.min_paper_days_mechanical, 14);
    assert_eq!(review.min_resolved_beliefs_synthesis, 100);
    assert_eq!(review.max_fee_pnl_ratio, 0.5);
    // to_thresholds maps 1:1 into the cognition layer's GoNoGoThresholds.
    let th = review.to_thresholds();
    assert_eq!(th.min_paper_days_mechanical, 14);
    assert_eq!(th.min_resolved_beliefs_synthesis, 100);

    // Opt-in: a config without [review] leaves it None (fail closed). Rename
    // only the section header (not the comment mention) so it parses as an
    // ignored unknown section.
    let without = example.replace("\n[review]\n", "\n[review_disabled]\n");
    assert!(
        DaemonToml::parse(&without)
            .expect("parse ok")
            .review
            .is_none(),
        "no [review] => None (the review cadence is opt-in)"
    );
}

#[test]
fn venue_kalshi_refuses_until_fixture_clearance() {
    // Kickoff hard requirement 7 / GAPS: sim is the only bootable venue
    // in T4.1; kalshi refuses WITH the reason.
    let example = include_str!("../../../config/fortuna.example.toml");
    let cfg = DaemonToml::parse(&example.replace("venue = \"sim\"", "venue = \"kalshi\""))
        .expect("parse is fine; the refusal is a boot check");
    match cfg.validate_bootable() {
        Err(BootError::VenueNotBootable { venue, reason }) => {
            assert_eq!(venue, "kalshi");
            assert!(
                reason.contains("fixture"),
                "reason must cite fixture clearance: {reason}"
            );
        }
        other => panic!("kalshi must refuse to boot, got {other:?}"),
    }
}

#[test]
fn unknown_venue_refuses() {
    let example = include_str!("../../../config/fortuna.example.toml");
    let cfg = DaemonToml::parse(&example.replace("venue = \"sim\"", "venue = \"polymarket_us\""))
        .expect("parse ok");
    assert!(matches!(
        cfg.validate_bootable(),
        Err(BootError::VenueNotBootable { .. })
    ));
}

#[test]
fn sim_venue_is_bootable() {
    let example = include_str!("../../../config/fortuna.example.toml");
    let cfg = DaemonToml::parse(example).expect("parse ok");
    cfg.validate_bootable().expect("sim boots");
}

#[test]
fn halt_poll_over_500ms_refuses() {
    // ASSUMPTIONS pin (kickoff requirement 5): halt-state poll <= 500ms.
    let example = include_str!("../../../config/fortuna.example.toml");
    let cfg = DaemonToml::parse(&example.replace("halt_poll_ms = 500", "halt_poll_ms = 2000"))
        .expect("parse ok");
    match cfg.validate_bootable() {
        Err(BootError::BadConfig { reason }) => {
            assert!(reason.contains("halt_poll_ms"), "{reason}");
        }
        other => panic!("halt poll 2000ms must refuse, got {other:?}"),
    }
}

#[test]
fn missing_daemon_section_refuses_with_precise_error() {
    // Fail-closed: a config without [daemon] cannot boot a daemon.
    let example = include_str!("../../../config/fortuna.example.toml");
    let stripped: String = {
        let start = example.find("[daemon]").expect("example carries [daemon]");
        // Cut the [daemon] section (it is last-or-bounded by the next header).
        let rest = &example[start..];
        let end = rest[1..]
            .find("\n[")
            .map(|i| start + 1 + i)
            .unwrap_or(example.len());
        format!("{}{}", &example[..start], &example[end..])
    };
    match DaemonToml::parse(&stripped) {
        Err(BootError::BadConfig { reason }) => assert!(reason.contains("daemon"), "{reason}"),
        other => panic!("missing [daemon] must refuse, got {other:?}"),
    }
}

#[test]
fn required_env_never_displays_secret_values() {
    // House secrets rule: Debug/Display of the parsed env must redact.
    let env = validate_env(&good_env()).expect("validates");
    let dbg = format!("{env:?}");
    assert!(
        !dbg.contains("sk-ant-synthetic-not-real"),
        "api key leaked into Debug"
    );
    assert!(
        !dbg.contains("xoxb-synthetic"),
        "slack token leaked into Debug"
    );
    assert!(
        !dbg.contains("postgres://app@localhost"),
        "db url leaked into Debug"
    );
    // Channel ids are not secrets; they may appear.
    let _: &RequiredEnv = &env;
}
