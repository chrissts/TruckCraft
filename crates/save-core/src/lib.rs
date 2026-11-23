#![forbid(unsafe_code)]

pub mod cargo;
pub mod cars;
pub mod coaches;
pub mod company;
pub mod convoy;
pub mod definitions;
pub mod discovery;
pub mod document;
pub mod editor;
pub mod events;
pub mod freight;
pub mod game_data;
pub mod plates;
pub mod profile;
pub mod profile_files;
pub mod session;
pub mod settings;
pub mod starter;
pub mod value;
pub mod vehicle;

pub fn decode_document(bytes: &[u8]) -> Result<document::Document> {
    if bytes.starts_with(b"SiiNunit") || bytes.starts_with(b"\xef\xbb\xbfSiiNunit") {
        return document::Document::from_bytes(bytes);
    }
    let decoded =
        scs_codec::sii::decode(bytes, false).map_err(|error| Error::Decode(error.to_string()))?;
    let text = String::from_utf8(decoded)
        .map_err(|_| Error::InvalidSii("decoded document is not UTF-8"))?;
    document::Document::parse(text)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid game definition: {0}")]
    InvalidDefinition(String),
    #[error("Invalid SII data: {0}")]
    InvalidSii(&'static str),
    #[error("Required save data is missing: {0}")]
    Missing(String),
    #[error("Invalid edit: {0}")]
    InvalidEdit(String),
    #[error("Save decoding failed: {0}")]
    Decode(String),
    #[error("File operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The save or profile changed after loading. Reload it before saving.")]
    FileChanged,
    #[error("The previous write failed. Reload the save before writing again.")]
    ReloadRequired,
    #[error(
        "Profile operation did not finish. Original profile recovery folder: {backup}. {reason}"
    )]
    ProfileRecovery {
        backup: std::path::PathBuf,
        reason: String,
    },
    #[error(
        "Save failed. Recovery snapshot: tsset-backups/{backup}. Earlier writes restored: {restored}. {reason}"
    )]
    Commit {
        backup: String,
        restored: bool,
        reason: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
