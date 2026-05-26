//! Path normalization for manifest entries.
//!
//! BagIt manifests record forward-slash relative paths. bagit-python writes
//! POSIX-style paths even on Windows; we mirror that and additionally guard
//! against traversal (`..`, leading `/`) since untrusted bags can ship from
//! anywhere.

use crate::error::{BagError, BagResult};

/// Normalize a manifest path to its canonical forward-slash form and reject
/// anything that would escape the bag root.
pub fn normalize(raw: &str) -> BagResult<String> {
    let unified = raw.replace('\\', "/");
    let trimmed = unified.trim_start_matches("./");
    if trimmed.starts_with('/') {
        return Err(BagError::UnsafePath(raw.to_string()));
    }
    let mut out = Vec::new();
    for segment in trimmed.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return Err(BagError::UnsafePath(raw.to_string())),
            s => out.push(s),
        }
    }
    if out.is_empty() {
        return Err(BagError::UnsafePath(raw.to_string()));
    }
    Ok(out.join("/"))
}

/// Manifests percent-encode CR and LF in stored paths because those bytes
/// would otherwise break line parsing. bagit-python uses `%0D` and `%0A`.
pub fn encode_for_manifest(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'\r' => out.push_str("%0D"),
            b'\n' => out.push_str("%0A"),
            b'%' => out.push_str("%25"),
            _ => out.push(b as char),
        }
    }
    out
}

/// Inverse of [`encode_for_manifest`].
pub fn decode_from_manifest(stored: &str) -> String {
    let bytes = stored.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        assert!(normalize("../etc/passwd").is_err());
        assert!(normalize("data/../../oops").is_err());
        assert!(normalize("/abs/path").is_err());
    }

    #[test]
    fn collapses_redundant_separators() {
        assert_eq!(normalize("data//foo/./bar.txt").unwrap(), "data/foo/bar.txt");
        assert_eq!(normalize("./data/x").unwrap(), "data/x");
    }

    #[test]
    fn windows_separators_become_forward_slashes() {
        assert_eq!(normalize("data\\sub\\file.bin").unwrap(), "data/sub/file.bin");
    }

    #[test]
    fn round_trip_encoding() {
        let weird = "data/odd\nname\r.txt";
        let enc = encode_for_manifest(weird);
        assert!(!enc.contains('\n'));
        assert!(!enc.contains('\r'));
        assert_eq!(decode_from_manifest(&enc), weird);
    }
}
