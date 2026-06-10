//! T1.1 tests: Kalshi RSA-PSS request signing, written from
//! docs/research/venue/kalshi-api-2026-06-10/research.md §1 BEFORE the
//! implementation.
//!
//! Contract under test (research §1, all doc-verbatim):
//! - message = `{timestamp_ms}{METHOD}{path}` with NO separators, where path
//!   is the full URL path from the API root INCLUDING `/trade-api/v2`,
//!   WITHOUT query parameters;
//! - RSA-PSS over the UTF-8 message bytes: SHA-256, MGF1-SHA256, salt length
//!   = digest length (32), standard base64 output;
//! - headers KALSHI-ACCESS-KEY / KALSHI-ACCESS-SIGNATURE /
//!   KALSHI-ACCESS-TIMESTAMP, timestamp in epoch milliseconds.
//!
//! Verification strategy: every signature produced by `KalshiSigner` is
//! verified against an in-test RSA public key using the exact PSS parameters
//! (`Pss::new::<Sha256>()`, salt = digest length). A signature that verifies
//! against the expected message bytes proves both the message construction
//! and the PSS parameter choice.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use fortuna_venues::kalshi::auth::{
    signing_message, KalshiSigner, HEADER_KEY, HEADER_SIGNATURE, HEADER_TIMESTAMP,
};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::EncodePrivateKey;
use rsa::{Pss, RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// One shared 2048-bit keypair per test binary (keygen is the slow part).
fn test_key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).expect("test RSA keygen"))
}

fn signer() -> KalshiSigner {
    let pem = test_key()
        .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
        .expect("pem");
    KalshiSigner::new(&pem, "a952bcbe-ec3b-4b5b-b8f9-11dae589608c".to_string()).expect("signer")
}

/// Verify `sig_b64` against `message` with the exact documented PSS params.
fn verifies(message: &str, sig_b64: &str) -> bool {
    let public = RsaPublicKey::from(test_key());
    let sig = BASE64_STANDARD.decode(sig_b64).expect("base64");
    let digest = Sha256::digest(message.as_bytes());
    public.verify(Pss::new::<Sha256>(), &digest, &sig).is_ok()
}

// ---- message construction (research §1, doc-verbatim example) ----

#[test]
fn signing_message_is_timestamp_method_path_with_no_separators() {
    // Verbatim docs example: `1703123456789GET/trade-api/v2/portfolio/balance`.
    assert_eq!(
        signing_message(1703123456789, "GET", "/trade-api/v2/portfolio/balance"),
        "1703123456789GET/trade-api/v2/portfolio/balance"
    );
}

#[test]
fn signing_message_strips_query_parameters() {
    // Verbatim docs warning: sign `/trade-api/v2/portfolio/orders`, not
    // `/trade-api/v2/portfolio/orders?limit=5`.
    assert_eq!(
        signing_message(
            1703123456789,
            "GET",
            "/trade-api/v2/portfolio/orders?limit=5"
        ),
        "1703123456789GET/trade-api/v2/portfolio/orders"
    );
}

#[test]
fn signing_message_uppercases_the_method() {
    // The docs always show uppercase methods; the signer normalizes.
    assert_eq!(
        signing_message(1, "delete", "/trade-api/v2/portfolio/events/orders/x"),
        "1DELETE/trade-api/v2/portfolio/events/orders/x"
    );
}

// ---- signature construction ----

#[test]
fn signature_verifies_with_exact_pss_params() {
    let s = signer();
    let headers = s
        .sign("GET", "/trade-api/v2/portfolio/balance", 1703123456789)
        .expect("sign");
    assert!(verifies(
        "1703123456789GET/trade-api/v2/portfolio/balance",
        &headers.signature_b64
    ));
}

#[test]
fn signature_covers_path_without_query() {
    let s = signer();
    let with_query = s
        .sign(
            "GET",
            "/trade-api/v2/portfolio/orders?limit=5&cursor=abc",
            42,
        )
        .expect("sign");
    let without_query = s
        .sign("GET", "/trade-api/v2/portfolio/orders", 42)
        .expect("sign");
    // Both must verify against the SAME (query-stripped) message.
    let message = "42GET/trade-api/v2/portfolio/orders";
    assert!(verifies(message, &with_query.signature_b64));
    assert!(verifies(message, &without_query.signature_b64));
    // And NOT against the un-stripped message.
    assert!(!verifies(
        "42GET/trade-api/v2/portfolio/orders?limit=5&cursor=abc",
        &with_query.signature_b64
    ));
}

#[test]
fn pss_is_randomized_but_always_verifies() {
    // PSS uses a random salt: two signatures over the same message differ,
    // yet both verify. Guards against accidentally using deterministic
    // PKCS#1 v1.5 padding.
    let s = signer();
    let a = s
        .sign("POST", "/trade-api/v2/portfolio/events/orders", 7)
        .expect("a");
    let b = s
        .sign("POST", "/trade-api/v2/portfolio/events/orders", 7)
        .expect("b");
    assert_ne!(a.signature_b64, b.signature_b64);
    let message = "7POST/trade-api/v2/portfolio/events/orders";
    assert!(verifies(message, &a.signature_b64));
    assert!(verifies(message, &b.signature_b64));
}

// ---- headers shape (research §1 header table) ----

#[test]
fn signed_headers_have_documented_names_and_values() {
    let s = signer();
    let headers = s
        .sign("GET", "/trade-api/v2/portfolio/balance", 1703123456789)
        .expect("sign");
    assert_eq!(headers.api_key_id, "a952bcbe-ec3b-4b5b-b8f9-11dae589608c");
    assert_eq!(headers.timestamp_ms, "1703123456789");
    // Signature must be valid standard base64.
    assert!(BASE64_STANDARD.decode(&headers.signature_b64).is_ok());

    let pairs = headers.as_header_pairs();
    assert_eq!(pairs[0].0, HEADER_KEY);
    assert_eq!(pairs[1].0, HEADER_SIGNATURE);
    assert_eq!(pairs[2].0, HEADER_TIMESTAMP);
    assert_eq!(HEADER_KEY, "KALSHI-ACCESS-KEY");
    assert_eq!(HEADER_SIGNATURE, "KALSHI-ACCESS-SIGNATURE");
    assert_eq!(HEADER_TIMESTAMP, "KALSHI-ACCESS-TIMESTAMP");
    assert_eq!(pairs[0].1, "a952bcbe-ec3b-4b5b-b8f9-11dae589608c");
    assert_eq!(pairs[2].1, "1703123456789");
}

// ---- key handling ----

#[test]
fn accepts_pkcs1_pem_too() {
    // Kalshi's downloaded `.key` files have been observed in both PKCS#8
    // ("BEGIN PRIVATE KEY") and PKCS#1 ("BEGIN RSA PRIVATE KEY") framing in
    // official examples; the signer accepts either.
    let pem = test_key()
        .to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)
        .expect("pkcs1 pem");
    let s = KalshiSigner::new(&pem, "key-id".to_string()).expect("signer");
    let h = s.sign("GET", "/trade-api/v2/markets", 5).expect("sign");
    assert!(verifies("5GET/trade-api/v2/markets", &h.signature_b64));
}

#[test]
fn rejects_garbage_pem() {
    let err = KalshiSigner::new("not a pem", "key-id".to_string());
    assert!(err.is_err());
}

#[test]
fn debug_output_never_contains_key_material() {
    let key = test_key();
    let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).expect("pem");
    let s = KalshiSigner::new(&pem, "key-id-visible".to_string()).expect("signer");
    let dbg = format!("{s:?}");
    // No PEM framing, no base64 key body, no primes.
    assert!(!dbg.contains("PRIVATE KEY"));
    let pem_body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    let probe = &pem_body[..32];
    assert!(!dbg.contains(probe));
    // The key id is operational metadata and may appear.
    assert!(dbg.contains("key-id-visible"));
    assert!(dbg.contains("REDACTED"));
}
