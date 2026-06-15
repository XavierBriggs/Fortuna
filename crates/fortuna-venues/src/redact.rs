//! Secret redaction for persisted venue artifacts (fixtures, recordings).
//!
//! The fixture recorders (`examples/record_*_fixtures.rs`) sign LIVE demo
//! requests, so the process holds real credential material — above all the API
//! key id. Venue RESPONSE bodies are written VERBATIM (they are the
//! adapter-parse fixtures), and an authenticated endpoint or an auth-error body
//! can echo the submitted key id. The house rule is absolute: no secret ever
//! lands in the repo, logs, or audit payloads (CLAUDE.md secrets rule; spec
//! 5.x). This scrubs known secret VALUES out of any text before it is written —
//! defense-in-depth so the operator's post-rotation fixture re-record is
//! provably safe to commit (verifier TRACK-A ASSIGNMENT (1), 2026-06-14).
//!
//! Pure and deterministic: matches on the literal secret string, never on a
//! pattern, so it can never alter legitimate market data that merely looks
//! credential-shaped (and a credential id is a 36-char UUID — far longer than
//! the [`MIN_SECRET_LEN`] floor that guards against a short "secret" nuking the
//! whole body).

/// Placeholder substituted for every occurrence of a known secret value.
/// Matches the `<REDACTED>` convention already used by `KalshiSigner`'s manual
/// `Debug` impl (kalshi/auth.rs), so a redacted fixture reads consistently.
pub const REDACTED: &str = "<REDACTED>";

/// Minimum secret length we will redact. An empty or very short "secret" would
/// match huge swaths of legitimate data (e.g. `"1"` inside every price/count),
/// corrupting the fixture; real credential ids are long (a Kalshi key id is a
/// 36-char UUID), so we refuse to redact anything shorter and leave the text
/// untouched rather than risk mangling it.
const MIN_SECRET_LEN: usize = 8;

/// Replace every occurrence of each value in `secrets` with [`REDACTED`].
///
/// Secrets shorter than [`MIN_SECRET_LEN`] (or empty) are skipped — see the
/// constant's rationale. Order-independent and idempotent on already-redacted
/// text (the placeholder contains none of the secrets). Non-secret content is
/// returned byte-for-byte unchanged.
pub fn redact_secrets(input: &str, secrets: &[&str]) -> String {
    let mut out = input.to_string();
    for secret in secrets {
        if secret.len() >= MIN_SECRET_LEN {
            out = out.replace(secret, REDACTED);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic 36-char UUID-shaped id; NEVER a real key (this is a test).
    const KEY_ID: &str = "40981234-0000-0000-0000-00000000abcd";

    #[test]
    fn redacts_a_key_id_embedded_in_a_response_body() {
        let body = format!(r#"{{"member":{{"key_id":"{KEY_ID}"}},"balance":1234}}"#);
        let out = redact_secrets(&body, &[KEY_ID]);
        assert!(
            !out.contains(KEY_ID),
            "the key id must NOT survive redaction"
        );
        assert!(out.contains(REDACTED), "placeholder must be substituted in");
        assert!(
            out.contains("\"balance\":1234"),
            "non-secret data must be preserved verbatim"
        );
    }

    #[test]
    fn leaves_text_unchanged_when_no_secret_is_present() {
        // Real market data: a temperature-bracket ticker and a price. None of it
        // is the key id, so the recorder must write it byte-for-byte.
        let body = r#"{"ticker":"KXHIGHNY-26JUN14-T90","yes_bid":42}"#;
        assert_eq!(redact_secrets(body, &[KEY_ID]), body);
    }

    #[test]
    fn redacts_every_occurrence_and_multiple_secrets() {
        let a = "40981234-0000-0000-0000-00000000abcd";
        let b = "deadbeef-1111-2222-3333-444455556666";
        let s = format!("{a} appears {a} twice and {b} once");
        let out = redact_secrets(&s, &[a, b]);
        assert!(!out.contains(a) && !out.contains(b));
        assert_eq!(
            out.matches(REDACTED).count(),
            3,
            "two occurrences of `a` and one of `b` must all be scrubbed"
        );
    }

    #[test]
    fn refuses_to_redact_short_or_empty_secrets() {
        // A 1-char "secret" would mangle legitimate counts/prices; the length
        // floor must hold and leave the body intact.
        let body = r#"{"count":1,"price":1,"side":"yes"}"#;
        assert_eq!(redact_secrets(body, &["1", ""]), body);
    }

    #[test]
    fn idempotent_on_already_redacted_text() {
        let once = redact_secrets(&format!("k={KEY_ID}"), &[KEY_ID]);
        let twice = redact_secrets(&once, &[KEY_ID]);
        assert_eq!(once, twice);
    }
}
