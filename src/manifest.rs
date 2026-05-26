//! Parse and serialize `manifest-<algorithm>.txt` and
//! `tagmanifest-<algorithm>.txt`.
//!
//! Each line is `<checksum><whitespace><path>`. The path may be
//! percent-encoded (see [`crate::path`]). Order is preserved on serialization
//! to match bagit-python's stable output.

use crate::error::{BagError, BagResult};
use crate::hash::Algorithm;
use crate::path;
use crate::spec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub checksum: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub algorithm: Algorithm,
    pub kind: ManifestKind,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestKind {
    Payload,
    Tag,
}

impl Manifest {
    pub fn filename(&self) -> String {
        let prefix = match self.kind {
            ManifestKind::Payload => spec::MANIFEST_PREFIX,
            ManifestKind::Tag => spec::TAGMANIFEST_PREFIX,
        };
        format!("{prefix}{}{}", self.algorithm.manifest_name(), spec::MANIFEST_SUFFIX)
    }

    pub fn parse(filename: &str, text: &str) -> BagResult<Self> {
        let (kind, algorithm) = classify_filename(filename)?;
        let mut entries = Vec::new();
        for raw in text.lines() {
            let line = raw.trim_end_matches('\r');
            if line.trim().is_empty() {
                continue;
            }
            let (checksum, rest) = line
                .split_once(char::is_whitespace)
                .ok_or_else(|| BagError::MalformedManifest {
                    file: filename.to_string(),
                    line: raw.to_string(),
                })?;
            let stored_path = rest.trim_start();
            if stored_path.is_empty() {
                return Err(BagError::MalformedManifest {
                    file: filename.to_string(),
                    line: raw.to_string(),
                });
            }
            let decoded = path::decode_from_manifest(stored_path);
            let normalized = path::normalize(&decoded)?;
            entries.push(ManifestEntry {
                checksum: checksum.to_ascii_lowercase(),
                path: normalized,
            });
        }
        Ok(Self { algorithm, kind, entries })
    }

    pub fn serialize(&self) -> String {
        // bagit-python uses two spaces between digest and path; we match for
        // byte-for-byte parity with tools that grep for that separator.
        let mut out = String::new();
        for entry in &self.entries {
            out.push_str(&entry.checksum);
            out.push_str("  ");
            out.push_str(&path::encode_for_manifest(&entry.path));
            out.push('\n');
        }
        out
    }
}

fn classify_filename(filename: &str) -> BagResult<(ManifestKind, Algorithm)> {
    let stem = filename
        .strip_suffix(spec::MANIFEST_SUFFIX)
        .ok_or_else(|| BagError::InvalidManifestName(filename.to_string()))?;
    if let Some(alg) = stem.strip_prefix(spec::MANIFEST_PREFIX) {
        return Ok((ManifestKind::Payload, Algorithm::from_name(alg)?));
    }
    if let Some(alg) = stem.strip_prefix(spec::TAGMANIFEST_PREFIX) {
        return Ok((ManifestKind::Tag, Algorithm::from_name(alg)?));
    }
    Err(BagError::InvalidManifestName(filename.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_manifest() {
        let text = "d41d8cd98f00b204e9800998ecf8427e data/empty\n\
                    900150983cd24fb0d6963f7d28e17f72 data/sub/file.txt\n";
        let m = Manifest::parse("manifest-md5.txt", text).unwrap();
        assert_eq!(m.algorithm, Algorithm::Md5);
        assert_eq!(m.kind, ManifestKind::Payload);
        assert_eq!(m.entries.len(), 2);
        assert_eq!(m.entries[1].path, "data/sub/file.txt");
    }

    #[test]
    fn classifies_tag_manifest() {
        let m = Manifest::parse("tagmanifest-sha256.txt", "").unwrap();
        assert_eq!(m.kind, ManifestKind::Tag);
        assert_eq!(m.algorithm, Algorithm::Sha256);
    }

    #[test]
    fn rejects_unknown_filename() {
        assert!(Manifest::parse("manifest-rot13.txt", "").is_err());
        assert!(Manifest::parse("not-a-manifest.txt", "").is_err());
    }

    #[test]
    fn serialize_round_trips() {
        let m = Manifest {
            algorithm: Algorithm::Sha512,
            kind: ManifestKind::Payload,
            entries: vec![ManifestEntry {
                checksum: "abcd".into(),
                path: "data/x.txt".into(),
            }],
        };
        let text = m.serialize();
        let parsed = Manifest::parse(&m.filename(), &text).unwrap();
        assert_eq!(parsed.entries, m.entries);
    }
}
