use thiserror::Error;

pub type BagResult<T> = Result<T, BagError>;

#[derive(Debug, Error)]
pub enum BagError {
    #[error("io error: {0}")]
    Io(String),

    #[error("unsupported BagIt version: {0} (this library only handles 0.97)")]
    UnsupportedVersion(String),

    #[error("unsupported tag-file character encoding: {0}")]
    UnsupportedEncoding(String),

    #[error("malformed bagit.txt: {0}")]
    MalformedBagItTxt(String),

    #[error("malformed manifest line in {file}: {line:?}")]
    MalformedManifest { file: String, line: String },

    #[error("malformed bag-info.txt: {0}")]
    MalformedBagInfo(String),

    #[error("malformed fetch.txt: {0}")]
    MalformedFetch(String),

    #[error("unsupported checksum algorithm: {0}")]
    UnsupportedAlgorithm(String),

    #[error("bag is incomplete: {0}")]
    Incomplete(String),

    #[error("checksum mismatch for {path} ({algorithm}): expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: String,
        algorithm: String,
        expected: String,
        actual: String,
    },

    #[error("payload oxum mismatch: bag-info says {expected}, payload is {actual}")]
    OxumMismatch { expected: String, actual: String },

    #[error("path escapes the bag: {0}")]
    UnsafePath(String),

    #[error("invalid manifest filename: {0}")]
    InvalidManifestName(String),
}
