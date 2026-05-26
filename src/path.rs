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
    // bagit-python only escapes CR and LF; literal `%` is left alone.
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '\r' => out.push_str("%0D"),
            '\n' => out.push_str("%0A"),
            _ => out.push(ch),
        }
    }
    out
}

/// Inverse of [`encode_for_manifest`]: undoes only `%0A`/`%0D` (case-insensitive),
/// leaving any other `%xx` sequences untouched so we don't accidentally collide
/// with literal `%` in paths produced by bagit-python.
pub fn decode_from_manifest(stored: &str) -> String {
    let bytes = stored.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let (h, l) = (bytes[i + 1], bytes[i + 2]);
            match (h, l) {
                (b'0', b'A' | b'a') => {
                    out.push(b'\n');
                    i += 3;
                    continue;
                }
                (b'0', b'D' | b'd') => {
                    out.push(b'\r');
                    i += 3;
                    continue;
                }
                _ => {}
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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

    #[test]
    fn literal_percent_is_preserved() {
        // bagit-python doesn't encode `%`, so we must round-trip a path
        // containing a literal percent without mangling it.
        let raw = "data/100%done.txt";
        let enc = encode_for_manifest(raw);
        assert_eq!(enc, raw);
        assert_eq!(decode_from_manifest(&enc), raw);
    }

    #[test]
    fn preserves_utf8_path_characters() {
        let raw = "data/Gmail - Gracias por rellenar este formulario_ TALLER INTEGRADOR – Módulo_ Diseño de Interacción.pdf";
        let enc = encode_for_manifest(raw);
        assert_eq!(enc, raw);
        assert_eq!(decode_from_manifest(&enc), raw);
    }
}
