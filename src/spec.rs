//! BagIt 0.97 spec constants.
//!
//! Reference: <https://datatracker.ietf.org/doc/html/draft-kunze-bagit-08>
//! (0.97 predates RFC 8493; this is the version Library of Congress shipped
//! by default until 1.0.)

/// The only version this library accepts.
pub const BAGIT_VERSION: &str = "0.97";

/// Required encoding for tag files in 0.97.
pub const TAG_FILE_ENCODING: &str = "UTF-8";

/// The reserved tag filename declaring version and encoding.
pub const BAGIT_TXT: &str = "bagit.txt";

/// Optional metadata file.
pub const BAG_INFO_TXT: &str = "bag-info.txt";

/// Optional fetch declarations.
pub const FETCH_TXT: &str = "fetch.txt";

/// Payload directory (note trailing slash semantics handled by `path` module).
pub const DATA_DIR: &str = "data";

/// Manifest filename prefix.
pub const MANIFEST_PREFIX: &str = "manifest-";

/// Tag manifest filename prefix.
pub const TAGMANIFEST_PREFIX: &str = "tagmanifest-";

/// Manifest filename suffix.
pub const MANIFEST_SUFFIX: &str = ".txt";

/// The `bag-info.txt` field that records `octetcount.streamcount` for the
/// payload. bagit-python always writes this; we validate it when present.
pub const PAYLOAD_OXUM_KEY: &str = "Payload-Oxum";

/// `bag-info.txt` field for total payload size as a human string.
pub const BAG_SIZE_KEY: &str = "Bag-Size";

/// `bag-info.txt` field for bagging date (`YYYY-MM-DD`).
pub const BAGGING_DATE_KEY: &str = "Bagging-Date";
