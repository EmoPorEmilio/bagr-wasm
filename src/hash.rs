use crate::error::{BagError, BagResult};
use md5::{Digest as _, Md5};
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha512};

/// Checksum algorithms supported for BagIt 0.97 manifests.
///
/// `Sha1` and `Sha224` are accepted for **reading** legacy bags (bagit-python
/// uses `hashlib.algorithms_guaranteed`, so they're common in the wild) but
/// not the default when building new bags — bagit-python defaults to
/// `sha256+sha512`, and so do we.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Algorithm {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha512,
}

impl Algorithm {
    /// Parse a manifest filename suffix (`md5`, `sha1`, `sha224`, `sha256`,
    /// `sha512`).
    ///
    /// Comparison is case-insensitive and dashes/underscores are ignored so
    /// `SHA-256` and `sha256` both work, matching bagit-python's normalization.
    pub fn from_name(s: &str) -> BagResult<Self> {
        let norm: String = s
            .chars()
            .filter(|c| !matches!(c, '-' | '_'))
            .flat_map(|c| c.to_lowercase())
            .collect();
        match norm.as_str() {
            "md5" => Ok(Self::Md5),
            "sha1" => Ok(Self::Sha1),
            "sha224" => Ok(Self::Sha224),
            "sha256" => Ok(Self::Sha256),
            "sha512" => Ok(Self::Sha512),
            _ => Err(BagError::UnsupportedAlgorithm(s.to_string())),
        }
    }

    /// Canonical lowercase manifest suffix.
    pub fn manifest_name(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha224 => "sha224",
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }

    /// Allocate a fresh streaming hasher.
    pub fn hasher(self) -> Box<dyn StreamHasher> {
        match self {
            Self::Md5 => Box::new(Md5Hasher(Md5::new())),
            Self::Sha1 => Box::new(Sha1Hasher(Sha1::new())),
            Self::Sha224 => Box::new(Sha224Hasher(Sha224::new())),
            Self::Sha256 => Box::new(Sha256Hasher(Sha256::new())),
            Self::Sha512 => Box::new(Sha512Hasher(Sha512::new())),
        }
    }
}

/// Object-safe streaming hasher trait so we can hold heterogenous algorithms
/// in one collection while feeding bytes once.
pub trait StreamHasher: Send {
    fn update(&mut self, bytes: &[u8]);
    fn finalize_hex(self: Box<Self>) -> String;
}

struct Md5Hasher(Md5);
impl StreamHasher for Md5Hasher {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finalize_hex(self: Box<Self>) -> String {
        hex::encode(self.0.finalize())
    }
}

struct Sha1Hasher(Sha1);
impl StreamHasher for Sha1Hasher {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finalize_hex(self: Box<Self>) -> String {
        hex::encode(self.0.finalize())
    }
}

struct Sha224Hasher(Sha224);
impl StreamHasher for Sha224Hasher {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finalize_hex(self: Box<Self>) -> String {
        hex::encode(self.0.finalize())
    }
}

struct Sha256Hasher(Sha256);
impl StreamHasher for Sha256Hasher {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finalize_hex(self: Box<Self>) -> String {
        hex::encode(self.0.finalize())
    }
}

struct Sha512Hasher(Sha512);
impl StreamHasher for Sha512Hasher {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finalize_hex(self: Box<Self>) -> String {
        hex::encode(self.0.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_names() {
        assert_eq!(Algorithm::from_name("md5").unwrap(), Algorithm::Md5);
        assert_eq!(Algorithm::from_name("sha256").unwrap(), Algorithm::Sha256);
        assert_eq!(Algorithm::from_name("sha512").unwrap(), Algorithm::Sha512);
    }

    #[test]
    fn parses_dashed_and_uppercased() {
        assert_eq!(Algorithm::from_name("SHA-256").unwrap(), Algorithm::Sha256);
        assert_eq!(Algorithm::from_name("SHA_512").unwrap(), Algorithm::Sha512);
    }

    #[test]
    fn accepts_legacy_sha_variants() {
        assert_eq!(Algorithm::from_name("sha1").unwrap(), Algorithm::Sha1);
        assert_eq!(Algorithm::from_name("sha224").unwrap(), Algorithm::Sha224);
    }

    #[test]
    fn hashes_match_known_vectors() {
        let mut h = Algorithm::Md5.hasher();
        h.update(b"abc");
        assert_eq!(h.finalize_hex(), "900150983cd24fb0d6963f7d28e17f72");

        let mut h = Algorithm::Sha256.hasher();
        h.update(b"abc");
        assert_eq!(
            h.finalize_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
