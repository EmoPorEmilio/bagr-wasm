//! Parse and serialize `fetch.txt`.
//!
//! Each line: `<url> <length-or-dash> <relative-path>`. Length `-` is allowed
//! to mean "unknown". Paths use forward slashes and may be percent-encoded.

use crate::error::{BagError, BagResult};
use crate::path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchEntry {
    pub url: String,
    pub length: Option<u64>,
    pub path: String,
}

#[derive(Debug, Default, Clone)]
pub struct Fetch {
    pub entries: Vec<FetchEntry>,
}

impl Fetch {
    pub fn parse(text: &str) -> BagResult<Self> {
        let mut entries = Vec::new();
        for raw in text.lines() {
            let line = raw.trim_end_matches('\r');
            if line.trim().is_empty() {
                continue;
            }
            let mut it = line.splitn(3, char::is_whitespace).map(str::trim);
            let url = it
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| BagError::MalformedFetch(format!("missing url: {raw:?}")))?;
            let length_str = it
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| BagError::MalformedFetch(format!("missing length: {raw:?}")))?;
            let path_str = it
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| BagError::MalformedFetch(format!("missing path: {raw:?}")))?;
            let length = if length_str == "-" {
                None
            } else {
                Some(length_str.parse::<u64>().map_err(|_| {
                    BagError::MalformedFetch(format!("invalid length: {length_str:?}"))
                })?)
            };
            entries.push(FetchEntry {
                url: url.to_string(),
                length,
                path: path::normalize(&path::decode_from_manifest(path_str))?,
            });
        }
        Ok(Self { entries })
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            let len = match entry.length {
                Some(n) => n.to_string(),
                None => "-".to_string(),
            };
            out.push_str(&entry.url);
            out.push(' ');
            out.push_str(&len);
            out.push(' ');
            out.push_str(&path::encode_for_manifest(&entry.path));
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_length() {
        let f = Fetch::parse("https://example.org/x 42 data/x\n").unwrap();
        assert_eq!(
            f.entries[0],
            FetchEntry {
                url: "https://example.org/x".into(),
                length: Some(42),
                path: "data/x".into(),
            }
        );
    }

    #[test]
    fn parses_unknown_length() {
        let f = Fetch::parse("https://example.org/x - data/x\n").unwrap();
        assert_eq!(f.entries[0].length, None);
    }

    #[test]
    fn round_trips() {
        let original = "https://example.org/x 42 data/x\n";
        let parsed = Fetch::parse(original).unwrap();
        assert_eq!(parsed.serialize(), original);
    }
}
