//! Kalshi API-key request signing. Research doc §1 (sources S3, S4, S1
//! securitySchemes), all doc-verbatim:
//!
//! - Message string = `{timestamp_ms}{METHOD}{path}` — exactly that
//!   concatenation, no separators. `path` is the full URL path from the API
//!   root INCLUDING the `/trade-api/v2` prefix and WITHOUT query parameters
//!   ("sign only `/trade-api/v2/portfolio/orders` — strip the `?` and
//!   everything after it"). The host never enters the signature.
//! - Algorithm: RSA-PSS over the UTF-8 message bytes — SHA-256, MGF1 with
//!   SHA-256, salt length equal to the digest length (32). The `rsa` crate's
//!   `Pss::new::<Sha256>()` is exactly this parameter set.
//! - Output: standard base64 into `KALSHI-ACCESS-SIGNATURE`; the same
//!   millisecond timestamp string goes into `KALSHI-ACCESS-TIMESTAMP`;
//!   `KALSHI-ACCESS-KEY` carries the API key id.
//!
//! Timestamp skew tolerance is UNDOCUMENTED (fixture checklist #2): the
//! signer takes the timestamp from the caller (ultimately the injected
//! `Clock`) and applies no slack of its own.
//!
//! The private key is process-lifetime secret material: it is never logged,
//! never serialized, and the manual `Debug` impl redacts it.

use crate::VenueError;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::{Pss, RsaPrivateKey};
use sha2::{Digest, Sha256};
use std::fmt;

/// Header names, verbatim from the docs' header table (research §1).
pub const HEADER_KEY: &str = "KALSHI-ACCESS-KEY";
pub const HEADER_SIGNATURE: &str = "KALSHI-ACCESS-SIGNATURE";
pub const HEADER_TIMESTAMP: &str = "KALSHI-ACCESS-TIMESTAMP";

/// Build the exact message string Kalshi signs:
/// `{timestamp_ms}{UPPERCASE_METHOD}{path_without_query}`.
///
/// `path` must be the full path from the API root (with the `/trade-api/v2`
/// prefix); any `?query` or `#fragment` suffix is stripped per the docs.
pub fn signing_message(timestamp_ms: i64, method: &str, path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    format!("{timestamp_ms}{}{path}", method.to_ascii_uppercase())
}

/// The three authentication headers for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedHeaders {
    pub api_key_id: String,
    pub timestamp_ms: String,
    pub signature_b64: String,
}

impl SignedHeaders {
    /// (name, value) pairs in documented order: key, signature, timestamp.
    pub fn as_header_pairs(&self) -> [(&'static str, &str); 3] {
        [
            (HEADER_KEY, self.api_key_id.as_str()),
            (HEADER_SIGNATURE, self.signature_b64.as_str()),
            (HEADER_TIMESTAMP, self.timestamp_ms.as_str()),
        ]
    }
}

/// Holds the RSA private key + API key id and signs request messages.
pub struct KalshiSigner {
    private_key: RsaPrivateKey,
    api_key_id: String,
}

impl KalshiSigner {
    /// Parse the operator-supplied private key PEM. Kalshi distributes the
    /// key as a one-time `.key` PEM download; both PKCS#8 ("BEGIN PRIVATE
    /// KEY") and PKCS#1 ("BEGIN RSA PRIVATE KEY") framings are accepted.
    /// The PEM string itself comes from an env var (never config, never the
    /// repo) per the house secrets rule.
    pub fn new(private_key_pem: &str, api_key_id: String) -> Result<Self, VenueError> {
        let private_key = RsaPrivateKey::from_pkcs8_pem(private_key_pem)
            .or_else(|_| RsaPrivateKey::from_pkcs1_pem(private_key_pem))
            .map_err(|e| VenueError::Invalid {
                reason: format!("kalshi private key PEM did not parse (pkcs8/pkcs1): {e}"),
            })?;
        Ok(KalshiSigner {
            private_key,
            api_key_id,
        })
    }

    /// Sign one request. `path` is the full path from the API root (with
    /// the `/trade-api/v2` prefix); the query string, if present, is
    /// stripped before signing per the docs.
    pub fn sign(
        &self,
        method: &str,
        path: &str,
        timestamp_ms: i64,
    ) -> Result<SignedHeaders, VenueError> {
        let message = signing_message(timestamp_ms, method, path);
        let digest = Sha256::digest(message.as_bytes());
        // PSS salt is random by construction; OsRng is the CSPRNG. This is
        // the one permitted randomness source here (signing is an IO-edge
        // concern; nothing deterministic consumes it).
        let signature = self
            .private_key
            .sign_with_rng(&mut rand::rngs::OsRng, Pss::new::<Sha256>(), &digest)
            .map_err(|e| VenueError::Invalid {
                reason: format!("RSA-PSS signing failed: {e}"),
            })?;
        Ok(SignedHeaders {
            api_key_id: self.api_key_id.clone(),
            timestamp_ms: timestamp_ms.to_string(),
            signature_b64: BASE64_STANDARD.encode(signature),
        })
    }
}

impl fmt::Debug for KalshiSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KalshiSigner")
            .field("api_key_id", &self.api_key_id)
            .field("private_key", &"<REDACTED>")
            .finish()
    }
}
