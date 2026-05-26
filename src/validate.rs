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

/// Mode-flag for [`validate_with`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidateOptions {
    /// When true, perform only the Payload-Oxum check (and require it to be
    /// present) — matches bagit-python's `fast=True`. Skips completeness and
    /// checksum verification.
    pub fast: bool,
    /// When true, perform completeness checks but skip checksum verification.
    pub completeness_only: bool,
}

/// Validate the bag exposed by `source`. Returns Ok with a report on success,
/// or the first violation encountered.
pub async fn validate<S: BagSource + ?Sized>(source: &S) -> BagResult<ValidationReport> {
    validate_with(source, ValidateOptions::default()).await
}

/// Like [`validate`] but with [`ValidateOptions`].
pub async fn validate_with<S: BagSource + ?Sized>(
    source: &S,
    options: ValidateOptions,
) -> BagResult<ValidationReport> {
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

    // -- bagit-python's _validate_structure_payload_directory: data/ must exist --
    // We have no notion of empty directories, so we treat "data/ exists" as
    // "at least one file under data/ is on disk or declared in a manifest or fetched".
    let has_data = listing.iter().any(|p| p.starts_with("data/"));

    // -- fetch.txt (optional) --
    let fetch = if listing.contains(spec::FETCH_TXT) {
        let f = Fetch::parse(&read_to_string(source, spec::FETCH_TXT).await?)?;
        for entry in &f.entries {
            crate::fetch::validate_url(&entry.url)?;
            if !entry.path.starts_with("data/") {
                return Err(BagError::MalformedFetch(format!(
                    "fetch.txt path must live under data/: {:?}",
                    entry.path
                )));
            }
        }
        f
    } else {
        Fetch::default()
    };
    let held: BTreeSet<String> = fetch.entries.iter().map(|e| e.path.clone()).collect();
    let url_for: std::collections::BTreeMap<&str, &str> = fetch
        .entries
        .iter()
        .map(|e| (e.path.as_str(), e.url.as_str()))
        .collect();

    // Cache parsed bag-info.txt for Payload-Oxum lookup; both fast and full
    // modes need it.
    let bag_info = if listing.contains(spec::BAG_INFO_TXT) {
        Some(BagInfo::parse(&read_to_string(source, spec::BAG_INFO_TXT).await?)?)
    } else {
        None
    };

    // -- fast mode: just check Payload-Oxum and return --
    if options.fast {
        let info = bag_info.as_ref().ok_or_else(|| {
            BagError::Incomplete("fast validation requires bag-info.txt with Payload-Oxum".into())
        })?;
        let declared = info.payload_oxum().ok_or_else(|| {
            BagError::Incomplete("fast validation requires bag-info.txt to include Payload-Oxum".into())
        })?;
        let mut total_octets: u64 = 0;
        let mut total_files: u64 = 0;
        for p in listing.iter().filter(|p| p.starts_with("data/")) {
            total_octets += source.size(p).await?;
            total_files += 1;
        }
        let actual = PayloadOxum { octets: total_octets, streams: total_files };
        if declared != actual {
            return Err(BagError::OxumMismatch {
                expected: declared.render(),
                actual: actual.render(),
            });
        }
        return Ok(ValidationReport {
            declaration: Some(declaration),
            payload_manifests,
            tag_manifests,
            payload_files: total_files as usize,
            payload_octets: total_octets,
            held_files: held.len(),
        });
    }

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

    // -- completeness: every declared path must be present locally --
    // bagit-python does NOT exempt held files: they are still required to be
    // on disk at validation time. We only "skip" them if the host supplies a
    // working fetch_url() in the bridge — that branch is handled below in
    // the checksum step, where a held file's bytes are pulled from the URL.
    let payload_declared: BTreeSet<String> = parsed_payload
        .iter()
        .flat_map(|m| m.entries.iter().map(|e| e.path.clone()))
        .collect();
    for path in &payload_declared {
        if !listing.contains(path) && !held.contains(path) {
            return Err(BagError::Incomplete(format!(
                "{path} declared in payload manifest but not present on disk"
            )));
        }
    }

    // After completeness, we know any held file is at least listed in fetch.txt;
    // whether it's actually retrievable is decided when we go to hash it.
    if !has_data && payload_declared.is_empty() && held.is_empty() {
        return Err(BagError::Incomplete(
            "no payload: data/ directory is empty and no fetch entries declared".into(),
        ));
    }

    // -- every payload file must appear in at least one payload manifest --
    // bagit-python aggregates entries across all payload manifests, so a file
    // listed in `manifest-sha256.txt` but missing from `manifest-md5.txt`
    // does not fail completeness on its own.
    let payload_on_disk: BTreeSet<String> = listing
        .iter()
        .filter(|p| p.starts_with("data/"))
        .cloned()
        .collect();
    for p in &payload_on_disk {
        if !payload_declared.contains(p) {
            return Err(BagError::Incomplete(format!(
                "{p} present in payload but missing from all payload manifests"
            )));
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

    // -- completeness-only mode: skip the actual checksum verification --
    if options.completeness_only {
        return Ok(ValidationReport {
            declaration: Some(declaration),
            payload_manifests,
            tag_manifests,
            payload_files: payload_on_disk.len(),
            payload_octets: 0,
            held_files: held.len(),
        });
    }

    // -- checksums: hash each unique file once for all algorithms it appears in --
    let mut needed: BTreeMap<String, Vec<&Manifest>> = BTreeMap::new();
    for m in parsed_payload.iter().chain(parsed_tag.iter()) {
        for entry in &m.entries {
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

        // Held files that aren't on disk: ask the host to fetch them. If the
        // host doesn't implement fetch_url(), this is a hard error — matching
        // bagit-python except we surface a clearer message.
        let mut reader = if listing.contains(path) {
            source.open(path).await?
        } else if let Some(url) = url_for.get(path.as_str()) {
            match source.fetch_url(url).await? {
                Some(r) => r,
                None => {
                    return Err(BagError::Incomplete(format!(
                        "{path} is held in fetch.txt ({url}) but no fetch handler is configured"
                    )))
                }
            }
        } else {
            return Err(BagError::Incomplete(format!(
                "{path} missing and not held in fetch.txt"
            )));
        };
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
    if let Some(info) = bag_info.as_ref() {
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
