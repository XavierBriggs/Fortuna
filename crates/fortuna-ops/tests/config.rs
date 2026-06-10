//! Config-loader and secrets tests, written from `config/fortuna.example.toml`
//! and the repo conventions (spec Section 8: "Config in TOML, secrets in
//! environment files outside the repo"; secrets never in logs).

use std::collections::BTreeMap;

use fortuna_ops::{
    slack_channel_env_var, FortunaConfig, OpsError, Secrets, SlackConfig, ENV_DATABASE_URL,
    ENV_DEADMAN_URL, ENV_SLACK_BOT_TOKEN,
};

/// Minimal valid whole-shape config used as the mutation base.
fn base_toml() -> String {
    r#"
[gates.global]
max_total_exposure_cents = 1000
max_daily_loss_cents = 100
min_order_contracts = 1
max_order_contracts = 10
price_band_cents = 20
max_cross_cents = 5
per_market_exposure_cents = 500
per_event_exposure_cents = 600
require_event_mapping = false

[envelopes]
mech_structural_cents = 300

[sizing]
kelly_fraction = 0.25

[fees]

[slack]
channels = ["trading", "alerts", "review", "digest", "ops"]

[cognition]
synthesis_model = "claude-fable-5"
triage_model = "claude-haiku-4-5"
daily_budget_cents = 1500
shadow_budget_cents = 500

[deadman]
ping_interval_secs = 60
"#
    .to_string()
}

fn mutated(from: &str, to: &str) -> String {
    let base = base_toml();
    assert!(base.contains(from), "mutation target {from:?} not in base");
    base.replace(from, to)
}

fn config_err(toml_str: &str) -> OpsError {
    match FortunaConfig::load_str(toml_str) {
        Err(e) => e,
        Ok(_) => panic!("config unexpectedly valid"),
    }
}

#[test]
fn example_config_file_parses_and_validates() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let cfg = FortunaConfig::load_file(path).unwrap();

    // [gates] deserializes into fortuna_gates::GateConfig.
    assert_eq!(cfg.gates.global.max_total_exposure_cents, 800_000);
    assert!(cfg.gates.per_strategy.contains_key("mech_structural"));
    assert!(cfg.gates.rate.contains_key("kalshi"));

    // [envelopes] keys are stored with the `_cents` suffix stripped so they
    // align with [gates.per_strategy] strategy names; values stay cents.
    let expected: BTreeMap<String, i64> = [
        ("mech_extremes".to_string(), 200_000),
        ("mech_structural".to_string(), 300_000),
    ]
    .into_iter()
    .collect();
    assert_eq!(cfg.envelopes, expected);

    assert_eq!(cfg.sizing.kelly_fraction, 0.25);

    // [fees] is an opaque passthrough; the venues layer parses it.
    assert!(cfg.fees.get("kalshi").is_some());
    assert!(cfg.fees.get("polymarket_us").is_some());

    assert_eq!(
        cfg.slack.channels,
        vec!["trading", "alerts", "review", "digest", "ops"]
    );

    assert_eq!(cfg.cognition.synthesis_model, "claude-fable-5");
    assert_eq!(cfg.cognition.triage_model, "claude-haiku-4-5");
    assert_eq!(cfg.cognition.daily_budget_cents, 1_500);
    assert_eq!(cfg.cognition.shadow_budget_cents, 500);

    assert_eq!(cfg.deadman.ping_interval_secs, 60);
}

#[test]
fn envelope_key_without_cents_suffix_is_kept_raw() {
    let cfg = FortunaConfig::load_str(&mutated(
        "mech_structural_cents = 300",
        "mech_structural = 300",
    ))
    .unwrap();
    assert_eq!(cfg.envelopes.get("mech_structural"), Some(&300));
}

#[test]
fn envelope_suffix_collision_is_rejected() {
    let err = config_err(&mutated(
        "mech_structural_cents = 300",
        "mech_structural_cents = 300\nmech_structural = 400",
    ));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn envelope_nonpositive_value_is_rejected() {
    let err = config_err(&mutated(
        "mech_structural_cents = 300",
        "mech_structural_cents = 0",
    ));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn kelly_fraction_zero_is_rejected() {
    let err = config_err(&mutated("kelly_fraction = 0.25", "kelly_fraction = 0.0"));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn kelly_fraction_above_one_is_rejected() {
    let err = config_err(&mutated("kelly_fraction = 0.25", "kelly_fraction = 1.01"));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn kelly_fraction_nan_is_rejected() {
    let err = config_err(&mutated("kelly_fraction = 0.25", "kelly_fraction = nan"));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn kelly_fraction_of_exactly_one_is_accepted() {
    let cfg =
        FortunaConfig::load_str(&mutated("kelly_fraction = 0.25", "kelly_fraction = 1.0")).unwrap();
    assert_eq!(cfg.sizing.kelly_fraction, 1.0);
}

#[test]
fn invalid_gates_section_is_rejected_via_gate_validation() {
    let err = config_err(&mutated(
        "max_total_exposure_cents = 1000",
        "max_total_exposure_cents = -1",
    ));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn missing_required_section_is_rejected() {
    let err = config_err(&mutated("[deadman]\nping_interval_secs = 60", ""));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn deadman_zero_interval_is_rejected() {
    let err = config_err(&mutated(
        "ping_interval_secs = 60",
        "ping_interval_secs = 0",
    ));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn duplicate_slack_channels_are_rejected() {
    let err = config_err(&mutated(
        r#"channels = ["trading", "alerts", "review", "digest", "ops"]"#,
        r#"channels = ["trading", "trading", "review", "digest", "ops"]"#,
    ));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn empty_slack_channel_list_is_rejected() {
    let err = config_err(&mutated(
        r#"channels = ["trading", "alerts", "review", "digest", "ops"]"#,
        "channels = []",
    ));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn negative_cognition_budget_is_rejected() {
    let err = config_err(&mutated(
        "daily_budget_cents = 1500",
        "daily_budget_cents = -1",
    ));
    assert!(matches!(err, OpsError::Config { .. }), "got {err:?}");
}

#[test]
fn load_file_on_missing_path_is_an_io_error() {
    let err = match FortunaConfig::load_file("/nonexistent/fortuna-ops-test.toml") {
        Err(e) => e,
        Ok(_) => panic!("unexpectedly loaded a nonexistent file"),
    };
    assert!(matches!(err, OpsError::Io(_)), "got {err:?}");
}

// ---------------------------------------------------------------- secrets --

fn slack_config() -> SlackConfig {
    SlackConfig {
        channels: vec![
            "trading".to_string(),
            "alerts".to_string(),
            "review".to_string(),
            "digest".to_string(),
            "ops".to_string(),
        ],
    }
}

fn full_env() -> BTreeMap<String, String> {
    [
        (ENV_SLACK_BOT_TOKEN, "xoxb-secret-token-123"),
        ("FORTUNA_SLACK_CHANNEL_TRADING", "C0TRADING"),
        ("FORTUNA_SLACK_CHANNEL_ALERTS", "C0ALERTS"),
        ("FORTUNA_SLACK_CHANNEL_REVIEW", "C0REVIEW"),
        ("FORTUNA_SLACK_CHANNEL_DIGEST", "C0DIGEST"),
        ("FORTUNA_SLACK_CHANNEL_OPS", "C0OPS"),
        (ENV_DEADMAN_URL, "https://hc.example/ping/uuid-secret-77"),
        (
            ENV_DATABASE_URL,
            "postgres://fortuna:pw-secret@localhost/db",
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

#[test]
fn secrets_from_lookup_reads_every_configured_name() {
    let env = full_env();
    let secrets = Secrets::from_lookup(&slack_config(), |name| env.get(name).cloned());

    assert_eq!(
        secrets.get(ENV_SLACK_BOT_TOKEN),
        Some("xoxb-secret-token-123")
    );
    assert_eq!(
        secrets.get(ENV_DEADMAN_URL),
        Some("https://hc.example/ping/uuid-secret-77")
    );
    assert_eq!(
        secrets.get(ENV_DATABASE_URL),
        Some("postgres://fortuna:pw-secret@localhost/db")
    );
    let ids = secrets.slack_channel_ids();
    assert_eq!(ids.len(), 5);
    assert_eq!(ids.get("trading").map(String::as_str), Some("C0TRADING"));
    assert_eq!(ids.get("ops").map(String::as_str), Some("C0OPS"));
}

#[test]
fn secrets_require_returns_value_or_typed_missing_error() {
    let env = full_env();
    let secrets = Secrets::from_lookup(&slack_config(), |name| env.get(name).cloned());
    assert_eq!(
        secrets.require(ENV_SLACK_BOT_TOKEN).unwrap(),
        "xoxb-secret-token-123"
    );

    let none = Secrets::from_lookup(&slack_config(), |_| None);
    let err = none.require(ENV_SLACK_BOT_TOKEN).unwrap_err();
    match err {
        OpsError::MissingSecret { name } => assert_eq!(name, ENV_SLACK_BOT_TOKEN),
        other => panic!("expected MissingSecret, got {other:?}"),
    }
}

#[test]
fn secrets_channel_lookup_uses_uppercased_env_name() {
    assert_eq!(
        slack_channel_env_var("trading"),
        "FORTUNA_SLACK_CHANNEL_TRADING"
    );
    let env = full_env();
    let secrets = Secrets::from_lookup(&slack_config(), |name| env.get(name).cloned());
    assert_eq!(
        secrets.get("FORTUNA_SLACK_CHANNEL_REVIEW"),
        Some("C0REVIEW")
    );
    assert_eq!(secrets.get("FORTUNA_SLACK_CHANNEL_NOPE"), None);
}

#[test]
fn secrets_debug_output_redacts_every_value() {
    let env = full_env();
    let secrets = Secrets::from_lookup(&slack_config(), |name| env.get(name).cloned());
    let debug = format!("{secrets:?}");

    for value in env.values() {
        assert!(
            !debug.contains(value.as_str()),
            "secret value {value:?} leaked into Debug output: {debug}"
        );
    }
    assert!(debug.contains("<redacted>"), "got {debug}");
    // Channel NAMES are not secrets (they are in the committed example
    // config); presence must remain visible for operability.
    assert!(debug.contains("trading"), "got {debug}");
}

#[test]
fn secrets_empty_env_value_is_treated_as_missing() {
    let secrets = Secrets::from_lookup(&slack_config(), |name| {
        (name == ENV_SLACK_BOT_TOKEN).then(String::new)
    });
    assert_eq!(secrets.get(ENV_SLACK_BOT_TOKEN), None);
    assert!(matches!(
        secrets.require(ENV_SLACK_BOT_TOKEN),
        Err(OpsError::MissingSecret { .. })
    ));
}

#[test]
fn secrets_from_env_reads_the_process_environment() {
    // Only this test touches these variable names; channel name chosen to be
    // unique to this test so parallel tests cannot interfere.
    let cfg = SlackConfig {
        channels: vec!["it_env_chan".to_string()],
    };
    let var = slack_channel_env_var("it_env_chan");
    std::env::set_var(&var, "C42TEST");
    let secrets = Secrets::from_env(&cfg);
    assert_eq!(secrets.get(&var), Some("C42TEST"));
    assert_eq!(
        secrets.slack_channel_ids().get("it_env_chan"),
        Some(&"C42TEST".to_string())
    );
    std::env::remove_var(&var);
}
