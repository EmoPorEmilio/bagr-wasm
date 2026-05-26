//! Parse and serialize `bag-info.txt`.
//!
//! `bag-info.txt` is RFC 5322-ish: `Field-Name: value`, continuation lines
//! start with whitespace, keys may repeat. We preserve insertion order and
//! allow duplicates, mirroring bagit-python's ordered multi-dict.

use crate::error::{BagError, BagResult};
use crate::spec;

#[derive(Debug, Default, Clone)]
pub struct BagInfo {
    /// Ordered list of (key, value) pairs. Keys may repeat.
    pub entries: Vec<(String, String)>,
}

impl BagInfo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(text: &str) -> BagResult<Self> {
        let mut entries: Vec<(String, String)> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            if line.starts_with(' ') || line.starts_with('\t') {
                let last = entries.last_mut().ok_or_else(|| {
                    BagError::MalformedBagInfo("continuation line before any key".into())
                })?;
                last.1.push(' ');
                last.1.push_str(line.trim());
            } else {
                let (key, value) = line.split_once(':').ok_or_else(|| {
                    BagError::MalformedBagInfo(format!("missing colon: {raw:?}"))
                })?;
                let key = key.trim();
                if key.is_empty() {
                    return Err(BagError::MalformedBagInfo("empty key".into()));
                }
                entries.push((key.to_string(), value.trim().to_string()));
            }
        }
        Ok(Self { entries })
    }

    /// First value matching `key` (case-insensitive, per RFC 5322).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    /// All values matching `key` in insertion order.
    pub fn get_all(&self, key: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            slot.1 = value;
        } else {
            self.entries.push((key.to_string(), value));
        }
    }

    pub fn payload_oxum(&self) -> Option<PayloadOxum> {
        let raw = self.get(spec::PAYLOAD_OXUM_KEY)?;
        PayloadOxum::parse(raw).ok()
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for (key, value) in &self.entries {
            // bag-info.txt has no formal wrapping requirement in 0.97; emit
            // as a single line and let consumers handle long values.
            out.push_str(key);
            out.push_str(": ");
            out.push_str(value);
            out.push('\n');
        }
        out
    }
}

/// `octetcount.streamcount`, where streamcount is the number of payload files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadOxum {
    pub octets: u64,
    pub streams: u64,
}

impl PayloadOxum {
    pub fn parse(s: &str) -> BagResult<Self> {
        let (o, c) = s
            .split_once('.')
            .ok_or_else(|| BagError::MalformedBagInfo(format!("invalid Payload-Oxum: {s:?}")))?;
        let octets = o
            .trim()
            .parse::<u64>()
            .map_err(|_| BagError::MalformedBagInfo(format!("invalid octetcount: {o:?}")))?;
        let streams = c
            .trim()
            .parse::<u64>()
            .map_err(|_| BagError::MalformedBagInfo(format!("invalid streamcount: {c:?}")))?;
        Ok(Self { octets, streams })
    }

    pub fn render(&self) -> String {
        format!("{}.{}", self.octets, self.streams)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_fields() {
        let text = "Source-Organization: Test\nBagging-Date: 2026-05-26\nPayload-Oxum: 123.4\n";
        let info = BagInfo::parse(text).unwrap();
        assert_eq!(info.get("Source-Organization"), Some("Test"));
        assert_eq!(
            info.payload_oxum().unwrap(),
            PayloadOxum { octets: 123, streams: 4 }
        );
    }

    #[test]
    fn folds_continuation_lines() {
        let text = "Contact-Name: Jane\n  Doe\n\tEsq.\n";
        let info = BagInfo::parse(text).unwrap();
        assert_eq!(info.get("Contact-Name"), Some("Jane Doe Esq."));
    }

    #[test]
    fn preserves_duplicates() {
        let text = "Contact-Email: a@x\nContact-Email: b@x\n";
        let info = BagInfo::parse(text).unwrap();
        assert_eq!(info.get_all("Contact-Email"), vec!["a@x", "b@x"]);
    }

    #[test]
    fn case_insensitive_lookup() {
        let text = "Source-Organization: Test\n";
        let info = BagInfo::parse(text).unwrap();
        assert_eq!(info.get("source-organization"), Some("Test"));
    }
}
