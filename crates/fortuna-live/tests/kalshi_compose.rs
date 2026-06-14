//! Demo-flip Phase 2 — `compose_kalshi_runner` against a MOCK transport.
//!
//! NEVER hits the live Kalshi API: the composition is exercised through
//! `compose_kalshi_runner_with_transport`, the transport-injection seam, with a
//! scripted `MockKalshiTransport` (empty script — construction issues no venue
//! calls; the catalog is polled lazily in `tick()`, which this test never
//! drives). The credential gate is exercised through the public
//! `compose_kalshi_runner`, whose env check fails BEFORE any transport is built,
//! so that path also touches no network.
//!
//! The RSA key below is a THROWAWAY test key generated for this test alone — it
//! is not, and has never been, a credential for any real or demo account.

use fortuna_cognition::cycle::TriageDecision;
use fortuna_cognition::mind::{Mind, StubMind};
use fortuna_core::clock::SimClock;
use fortuna_live::boot::DaemonToml;
use fortuna_live::daemon::{compose_kalshi_runner, compose_kalshi_runner_with_transport};
use fortuna_ops::FortunaConfig;
use fortuna_runner::Stage;
use fortuna_venues::kalshi::MockKalshiTransport;
use fortuna_venues::Venue;
use sqlx::PgPool;
use std::collections::BTreeMap;
use std::sync::Arc;

fn t0() -> fortuna_core::clock::UtcTimestamp {
    fortuna_core::clock::UtcTimestamp::parse_iso8601("2026-06-11T12:00:00.000Z").unwrap()
}

fn stub_mind() -> Arc<dyn Mind> {
    Arc::new(StubMind::scripted(Vec::new()))
}

/// A throwaway PKCS#8 RSA-2048 private key (generated for THIS test only; never
/// a real/demo credential). `KalshiSigner::new` parses it; nothing here ever
/// signs a request to a live endpoint.
const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvwIBADANBgkqhkiG9w0BAQEFAASCBKkwggSlAgEAAoIBAQC0Mx0Y9vZd2Oka
8VeWeV4wLG5NMUBSQUOERbWvc6SCrGNghRn7yastgdQ897dhfRTSq3zZUzl/8g9M
ibhztHuXzORkYoAXXLdaj4AWQRmJ+L+TeQJ6TJveIJotBHbzSs2tCKFRJesondUU
UjkP9L56PVoJfVcv7LaNGLpylKI9gmVJ7bk+ujYAML2EvE+pqujrt0Ahp1fgMYD9
uLRIPlhzcxvedW9F2bXW5QXpuWE3PONj5X0VIpo28zYqqe6DORt2CdlUDqMzfa1z
BVIDO3nFuCgOpNOKDUMgPvLiyTWHrUmIzn1ZazE7vS0S4b9SC5vV9RqEujJ1BQ6G
trMDaV2NAgMBAAECggEAAOIgQiNRxVd/GwX0VTU+mDNbjg7P/yc5PsB9ucCyHX7d
VNeKL1EHgQdaJDtdn4F2tOqox8Lv7PfhidFCAXUwxud29iQCdzrZ3jyGVvWWO7Yn
sEAfWjyeZfoYb2COebZT6EV6zvRF4RLW/MzDYVfkiCJdWt8Nqps5MNtebJncartD
B1mqy0FN5caRb7HEogkbcAXW4hCXIVDLmOvsqe2zQRqJwa7htLJGpr32uosbk10y
yHFijR83CU5+kCXPB7C/Ee45SCqC9XTckTkqv0KaIuJy4Gr82FP2QvpuUOqrbrpo
6PEY8e2Uc/YeLFcScla6cqWSwy2MNu4AzUl8SVGduQKBgQDyYAOXjwCiQeJVnw9u
AAoo82sKXluFLXTMa36PQ7/KFPlOKqMYAsfMWlCEiSreeMbspCBqkWcOe6Ft7Vdh
D0DwCor5H84ujs/ny8ifO2aXMK//5OJJUKPTgv9PMJKd+njDUoLK2pj4Lzx/OvVS
K7eQtO5O53vbxb4NDefgiKSjOQKBgQC+VFeWsQqJ6vWVT0yYgWSbNaLyTZPJNeJt
ebSZFPBdhEtMCC54RUagwZ5KYuDkyJoa5Rb1BjW96xuynEEUIKx0/bCDDl1xSC8M
TzGQxY+jcIGsWnppTOPiz+WOTTgbUZB0ZoBhyk5uzkn8K8y4hs+7JDJ/T7LFElD7
Q5X8Rdpo9QKBgQChcQzTdeBBM8tTpsg7R/F8h28EEAe79KQ4yV0ahlEIhOHui/3o
r2lwF7RMI6WXXDF8THJ/KWzQu86yDwZF00g423zoJaRLZLrdNeLjFUjnafnBZC7K
ENmeuEHg+ISgj8bCq4INJn//yE7unFHtssrpq2qUyiG5KMTHozyRVdL8GQKBgQCf
UmLvlcvIn5JsJjFsCAR7mG6Kfj4T1LNyCMsQyeJbpf6R6tdbfkIdF3a1tgej+/hk
Qxjwiv45uLE61mnzu1YhqKs1SbUWuuIHX9OR6I7QtcEW0bZepyqsFnOGp0UsOR6/
EX6uXXdCchSkrtV0MgV6FlbfE4wGQ8reSjknMCIgcQKBgQDH8gR6DM4nlWjHa5Ol
BANFhjvNwTHbi8wR2pni9aJt1mRdvI2B4+zh+4xGOYx7znZ9i70yyr/BNpdNAzCv
A3MjaXA1SSXMx6/pHWP8AEt8gtiieynkTYnt+mtkm3fGBWZ8Akx6xInqN17FXImX
o1S6fp4jungqHKMsRDoGKe3nNg==
-----END PRIVATE KEY-----
";

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../config/fortuna.example.toml"
);

/// The committed example rewritten for the Kalshi demo (venue=kalshi,
/// stage=paper) with a [kalshi] section and a [synthesis] arm appended, so the
/// composition exercises BOTH the mech_structural and the (Paper-staged)
/// synthesis arms.
fn kalshi_dcfg_text() -> String {
    let base = std::fs::read_to_string(EXAMPLE_PATH).unwrap();
    let base = base
        .replace("venue = \"sim\"", "venue = \"kalshi\"")
        .replace("stage = \"sim\"", "stage = \"paper\"");
    format!(
        "{base}\n\
         [kalshi]\n\
         series = [\"KXHIGHNY\"]\n\
         bracket_sets = [[\"KXHIGHNY-A\", \"KXHIGHNY-B\", \"KXHIGHNY-C\"]]\n\
         [synthesis]\n\
         venue = \"kalshi\"\n"
    )
}

/// A non-placeholder demo-credential env, matching the established convention
/// the fixture recorders use: a valid key id plus `KALSHI_DEMO_PRIVATE_KEY_PATH`
/// pointing at a freshly-written file holding the throwaway test PEM (so the
/// path genuinely resolves to a readable key). The refusal tests below then
/// knock out exactly one piece to prove the credential gate fires.
fn cred_env() -> BTreeMap<String, String> {
    let key_path = std::env::temp_dir().join("fortuna-kalshi-compose-test-key.pem");
    std::fs::write(&key_path, TEST_KEY_PEM).expect("write throwaway test key to temp file");
    let mut env = BTreeMap::new();
    env.insert("KALSHI_API_DEMO_KEY_ID".into(), "demo-key-id-abc123".into());
    env.insert(
        "KALSHI_DEMO_PRIVATE_KEY_PATH".into(),
        key_path.to_string_lossy().into_owned(),
    );
    env
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn compose_kalshi_runner_builds_a_paper_kalshi_runner(pool: PgPool) {
    let text = kalshi_dcfg_text();
    let dcfg = DaemonToml::parse(&text).expect("kalshi demo config parses");
    dcfg.validate_bootable().expect("kalshi @ paper boots");
    let full = FortunaConfig::load_file(EXAMPLE_PATH).expect("full config parses");

    // The transport-injection seam with a scripted mock — construction issues no
    // venue calls (the catalog is polled lazily in tick(), never driven here), so
    // an empty script suffices and NO network is touched.
    let transport = Arc::new(MockKalshiTransport::new());
    let clock = Arc::new(SimClock::new(t0()));

    let runner = compose_kalshi_runner_with_transport(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        7,
        clock,
        stub_mind(),
        TriageDecision::AlwaysAccept,
        transport.clone(),
    )
    .await
    .expect("compose_kalshi_runner_with_transport succeeds against the mock transport");

    // The venue is the Kalshi adapter.
    assert_eq!(
        runner.venue().id().as_str(),
        "kalshi",
        "the composed venue is the Kalshi adapter"
    );

    // The synthesis arm composed AT STAGE::PAPER (the one documented difference
    // from compose_runner — compose_runner stages it Sim). MUTATION-PROOF: if
    // compose_kalshi_runner staged it Sim, this assert reds.
    let stages = runner.strategy_stages();
    let synth = stages
        .iter()
        .find(|(id, _)| id.as_str() == "synthesis")
        .expect("the [synthesis] arm composed");
    assert_eq!(
        synth.1,
        Stage::Paper,
        "the Kalshi demo runs the synthesis arm at Stage::Paper (not Sim)"
    );

    // mech_structural always composes (the demo's arb world from [kalshi]).
    assert!(
        stages
            .iter()
            .any(|(id, _)| id.as_str() == "mech_structural"),
        "mech_structural is composed from [kalshi].bracket_sets: {stages:?}"
    );

    // Construction made no venue calls (the catalog poll is deferred to tick()).
    assert!(
        transport.calls().is_empty(),
        "compose issues no venue HTTP at construction: {:?}",
        transport.calls()
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn compose_kalshi_runner_refuses_a_missing_key_path_credential(pool: PgPool) {
    // The credential gate fires in the PUBLIC compose_kalshi_runner BEFORE any
    // transport is built (the env check is first), so this path touches no
    // network. An absent KALSHI_DEMO_PRIVATE_KEY_PATH is a Compose error naming
    // the var, never its value.
    let text = kalshi_dcfg_text();
    let dcfg = DaemonToml::parse(&text).expect("parses");
    let full = FortunaConfig::load_file(EXAMPLE_PATH).expect("full config parses");

    let mut env = cred_env();
    env.remove("KALSHI_DEMO_PRIVATE_KEY_PATH");
    let clock = Arc::new(SimClock::new(t0()));
    let result = compose_kalshi_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        7,
        clock,
        stub_mind(),
        TriageDecision::AlwaysAccept,
        &env,
    )
    .await;
    let err = result.err().expect("missing key-path credential refuses");
    let msg = err.to_string();
    assert!(
        msg.contains("KALSHI_DEMO_PRIVATE_KEY_PATH"),
        "the refusal names the missing credential var: {msg}"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn compose_kalshi_runner_refuses_a_placeholder_key_path(pool: PgPool) {
    // A half-edited credential (placeholder path) refuses loudly — never trusted.
    let text = kalshi_dcfg_text();
    let dcfg = DaemonToml::parse(&text).expect("parses");
    let full = FortunaConfig::load_file(EXAMPLE_PATH).expect("full config parses");

    let mut env = cred_env();
    env.insert(
        "KALSHI_DEMO_PRIVATE_KEY_PATH".into(),
        "/keys/your-demo-private-key.pem".into(),
    );
    let clock = Arc::new(SimClock::new(t0()));
    let result = compose_kalshi_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        7,
        clock,
        stub_mind(),
        TriageDecision::AlwaysAccept,
        &env,
    )
    .await;
    let err = result.err().expect("placeholder key path refuses");
    assert!(
        err.to_string().contains("KALSHI_DEMO_PRIVATE_KEY_PATH"),
        "the refusal names the offending credential var: {err}"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn compose_kalshi_runner_refuses_an_unreadable_key_path(pool: PgPool) {
    // The NEW failure mode the path indirection introduces: a present,
    // non-placeholder path that does not resolve to a readable file. The boot
    // gate (required/check_value) ACCEPTS the path string, then the file read
    // fails — a Compose error naming the PATH (a filesystem location, never the
    // key body), still touching no network (the read fails before any transport
    // is built).
    let text = kalshi_dcfg_text();
    let dcfg = DaemonToml::parse(&text).expect("parses");
    let full = FortunaConfig::load_file(EXAMPLE_PATH).expect("full config parses");

    let mut env = cred_env();
    env.insert(
        "KALSHI_DEMO_PRIVATE_KEY_PATH".into(),
        "/no/such/fortuna-demo-key.pem".into(),
    );
    let clock = Arc::new(SimClock::new(t0()));
    let result = compose_kalshi_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        7,
        clock,
        stub_mind(),
        TriageDecision::AlwaysAccept,
        &env,
    )
    .await;
    let err = result.err().expect("unreadable key path refuses");
    let msg = err.to_string();
    assert!(
        msg.contains("cannot read") && msg.contains("KALSHI_DEMO_PRIVATE_KEY_PATH"),
        "the refusal names the unreadable path var and reports it cannot read it: {msg}"
    );
}
