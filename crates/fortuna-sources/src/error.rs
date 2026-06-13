//! Crate error taxonomy (house rule: thiserror enums per crate, anyhow
//! only in binaries, no panics outside tests).

#[derive(Debug, thiserror::Error)]
pub enum SourcesError {
    /// The TOML document itself failed to parse or deserialize.
    #[error("sources config parse: {reason}")]
    ConfigParse { reason: String },
    /// A specific `[sources.<id>]` table is invalid. Fail-closed: nothing
    /// is defaulted into validity. (Field is `source_id`, not `source`:
    /// thiserror reserves `source` for the error-cause chain.)
    #[error("source `{source_id}` config invalid: {reason}")]
    ConfigInvalid { source_id: String, reason: String },
}
