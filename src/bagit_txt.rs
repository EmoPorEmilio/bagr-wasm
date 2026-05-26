//! Parse and serialize `bagit.txt`.
//!
//! The file is two fixed lines:
//! ```text
//! BagIt-Version: 0.97
//! Tag-File-Character-Encoding: UTF-8
//! ```

use crate::error::{BagError, BagResult};
use crate::spec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BagItDeclaration {
    pub version: String,
    pub encoding: String,
}

impl BagItDeclaration {
    pub fn default_v097() -> Self {
        Self {
            version: spec::BAGIT_VERSION.to_string(),
            encoding: spec::TAG_FILE_ENCODING.to_string(),
        }
    }

    pub fn parse(text: &str) -> BagResult<Self> {
        if text.as_bytes().starts_with("\u{feff}".as_bytes()) {
            return Err(BagError::MalformedBagItTxt(
                "bagit.txt must not contain a UTF-8 byte-order mark".into(),
            ));
        }
        let mut version = None;
        let mut encoding = None;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| BagError::MalformedBagItTxt(format!("missing colon: {raw:?}")))?;
            match key.trim() {
                "BagIt-Version" => version = Some(value.trim().to_string()),
                "Tag-File-Character-Encoding" => encoding = Some(value.trim().to_string()),
                other => {
                    return Err(BagError::MalformedBagItTxt(format!(
                        "unexpected key: {other}"
                    )))
                }
            }
        }
        let version = version
            .ok_or_else(|| BagError::MalformedBagItTxt("missing BagIt-Version".into()))?;
        let encoding = encoding
            .ok_or_else(|| BagError::MalformedBagItTxt("missing Tag-File-Character-Encoding".into()))?;
        if version != spec::BAGIT_VERSION {
            return Err(BagError::UnsupportedVersion(version));
        }
        if !encoding.eq_ignore_ascii_case(spec::TAG_FILE_ENCODING) {
            return Err(BagError::UnsupportedEncoding(encoding));
        }
        Ok(Self { version, encoding })
    }

    pub fn serialize(&self) -> String {
        format!(
            "BagIt-Version: {}\nTag-File-Character-Encoding: {}\n",
            self.version, self.encoding
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let original = BagItDeclaration::default_v097();
        let text = original.serialize();
        assert_eq!(BagItDeclaration::parse(&text).unwrap(), original);
    }

    #[test]
    fn rejects_other_versions() {
        let text = "BagIt-Version: 1.0\nTag-File-Character-Encoding: UTF-8\n";
        assert!(matches!(
            BagItDeclaration::parse(text),
            Err(BagError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn tolerates_crlf() {
        let text = "BagIt-Version: 0.97\r\nTag-File-Character-Encoding: UTF-8\r\n";
        assert!(BagItDeclaration::parse(text).is_ok());
    }

    #[test]
    fn rejects_bom() {
        let text = "\u{feff}BagIt-Version: 0.97\nTag-File-Character-Encoding: UTF-8\n";
        assert!(BagItDeclaration::parse(text).is_err());
    }
}
