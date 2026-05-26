//! Validate an existing bag against the BagIt 0.97 spec.
//!
//! Algorithm, mirroring bagit-python's `Bag.validate(fast=False)`:
//!
//! 1. `bagit.txt` exists, parses, version is 0.97, encoding is UTF-8.
//! 2. At least one `manifest-<alg>.txt` is present.
//! 3. Every payload file under `data/` is listed in every payload manifest,
//!    OR appears in `fetch.txt` (held).
//! 4. Every file listed in a manifest exists in the bag, OR is in `fetch.txt`.
//! 5. All checksums match (payload + tag manifests).
//! 6. If `bag-info.txt` declares `Payload-Oxum`, it matches the payload.

use crate::bag_info::{BagInfo, PayloadOxum};
use crate::bagit_txt::BagItDeclaration;
use crate::error::{BagError, BagResult};
use crate::fetch::Fetch;
use crate::io::BagSource;
use crate::manifest::Manifest;
use crate::spec;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub struct ValidationReport {
    pub declaration: Option<BagItDeclaration>,
    pub payload_manifests: Vec<String>,
    pub tag_manifests: Vec<String>,
    pub payload_files: usize,
    pub payload_octets: u64,
    pub held_files: usize,
}

/// Validate the bag exposed by `source`. Returns Ok with a report on success,
/// or the first violation encountered.
pub async fn validate<S: BagSource + ?Sized>(source: &S) -> BagResult<ValidationReport> {
    let listing: BTreeSet<String> = source.list().await?.into_iter().collect();

    // -- bagit.txt --
    if !listing.contains(spec::BAGIT_TXT) {
        return Err(BagError::Incomplete(format!("missing {}", spec::BAGIT_TXT)));
    }
    let declaration = BagItDeclaration::parse(&read_to_string(source, spec::BAGIT_TXT).await?)?;

    // -- find manifests --
    let mut payload_manifests = Vec::new();
    let mut tag_manifests = Vec::new();
    for name in &listing {
        if is_manifest_file(name, spec::MANIFEST_PREFIX) {
            payload_manifests.push(name.clone());
        } else if is_manifest_file(name, spec::TAGMANIFEST_PREFIX) {
            tag_manifests.push(name.clone());
        }
    }
    if payload_manifests.is_empty() {
        return Err(BagError::Incomplete(
            "no payload manifest (manifest-*.txt) present".into(),
        ));
    }

    // -- fetch.txt (optional) --
    let fetch = if listing.contains(spec::FETCH_TXT) {
        Fetch::parse(&read_to_string(source, spec::FETCH_TXT).await?)?
    } else {
        Fetch::default()
    };
    let held: BTreeSet<String> = fetch.entries.iter().map(|e| e.path.clone()).collect();

    // -- parse all manifests --
    let mut parsed_payload: Vec<Manifest> = Vec::new();
    for name in &payload_manifests {
        let text = read_to_string(source, name).await?;
        parsed_payload.push(Manifest::parse(name, &text)?);
    }
    let mut parsed_tag: Vec<Manifest> = Vec::new();
    for name in &tag_manifests {
        let text = read_to_string(source, name).await?;
        parsed_tag.push(Manifest::parse(name, &text)?);
    }

    // -- completeness: every declared path is either present or held --
    let payload_declared: BTreeSet<String> = parsed_payload
        .iter()
        .flat_map(|m| m.entries.iter().map(|e| e.path.clone()))
        .collect();
    for path in &payload_declared {
        if !listing.contains(path) && !held.contains(path) {
            return Err(BagError::Incomplete(format!(
                "{path} declared in payload manifest but not present and not held in fetch.txt"
            )));
        }
    }

    // -- every payload file is declared (in every payload manifest, per spec) --
    let payload_on_disk: BTreeSet<String> = listing
        .iter()
        .filter(|p| p.starts_with("data/"))
        .cloned()
        .collect();
    for m in &parsed_payload {
        let declared: BTreeSet<String> = m.entries.iter().map(|e| e.path.clone()).collect();
        for p in &payload_on_disk {
            if !declared.contains(p) {
                return Err(BagError::Incomplete(format!(
                    "{p} present in payload but missing from {}",
                    m.filename()
                )));
            }
        }
    }

    // -- tag manifest completeness (its entries must exist) --
    for m in &parsed_tag {
        for entry in &m.entries {
            if !listing.contains(&entry.path) {
                return Err(BagError::Incomplete(format!(
                    "{path} declared in {file} but not present",
                    path = entry.path,
                    file = m.filename()
                )));
            }
        }
    }

    // -- checksums: hash each unique file once for all algorithms it appears in --
    let mut needed: BTreeMap<String, Vec<&Manifest>> = BTreeMap::new();
    for m in parsed_payload.iter().chain(parsed_tag.iter()) {
        for entry in &m.entries {
            if held.contains(&entry.path) && !listing.contains(&entry.path) {
                continue; // a held payload file we don't have locally yet
            }
            needed.entry(entry.path.clone()).or_default().push(m);
        }
    }

    let mut payload_octets: u64 = 0;
    let mut payload_files_counted: u64 = 0;
    for (path, manifests) in &needed {
        let mut hashers: Vec<(crate::hash::Algorithm, Box<dyn crate::hash::StreamHasher>)> =
            manifests
                .iter()
                .map(|m| (m.algorithm, m.algorithm.hasher()))
                .collect();
        // dedup hashers by algorithm; same alg appears once
        hashers.sort_by_key(|(a, _)| *a as usize);
        hashers.dedup_by_key(|(a, _)| *a);

        let mut reader = source.open(path).await?;
        let mut bytes_seen: u64 = 0;
        while let Some(chunk) = reader.next_chunk().await? {
            bytes_seen += chunk.len() as u64;
            for (_, h) in hashers.iter_mut() {
                h.update(&chunk);
            }
        }

        let mut actual_by_alg: BTreeMap<crate::hash::Algorithm, String> = BTreeMap::new();
        for (alg, h) in hashers {
            actual_by_alg.insert(alg, h.finalize_hex());
        }

        for m in manifests {
            let expected = m
                .entries
                .iter()
                .find(|e| &e.path == path)
                .map(|e| e.checksum.as_str())
                .unwrap_or("");
            let actual = actual_by_alg
                .get(&m.algorithm)
                .map(String::as_str)
                .unwrap_or("");
            if !expected.eq_ignore_ascii_case(actual) {
                return Err(BagError::ChecksumMismatch {
                    path: path.clone(),
                    algorithm: m.algorithm.manifest_name().to_string(),
                    expected: expected.to_string(),
                    actual: actual.to_string(),
                });
            }
        }

        if path.starts_with("data/") {
            payload_octets += bytes_seen;
            payload_files_counted += 1;
        }
    }

    // -- Payload-Oxum check, when present --
    if listing.contains(spec::BAG_INFO_TXT) {
        let info = BagInfo::parse(&read_to_string(source, spec::BAG_INFO_TXT).await?)?;
        if let Some(declared) = info.payload_oxum() {
            let actual = PayloadOxum {
                octets: payload_octets,
                streams: payload_files_counted,
            };
            if declared != actual {
                return Err(BagError::OxumMismatch {
                    expected: declared.render(),
                    actual: actual.render(),
                });
            }
        }
    }

    Ok(ValidationReport {
        declaration: Some(declaration),
        payload_manifests,
        tag_manifests,
        payload_files: payload_on_disk.len(),
        payload_octets,
        held_files: held.len(),
    })
}

fn is_manifest_file(name: &str, prefix: &str) -> bool {
    name.starts_with(prefix) && name.ends_with(spec::MANIFEST_SUFFIX) && !name.contains('/')
}

async fn read_to_string<S: BagSource + ?Sized>(source: &S, path: &str) -> BagResult<String> {
    let mut reader = source.open(path).await?;
    let mut buf = Vec::new();
    while let Some(chunk) = reader.next_chunk().await? {
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf).map_err(|e| BagError::Io(format!("{path}: not UTF-8 ({e})")))
}
