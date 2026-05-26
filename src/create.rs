//! Create a BagIt 0.97 bag.
//!
//! Caller provides a [`BagSource`] of payload files (any layout) and a
//! [`BagSink`] to receive the assembled bag. We stream each payload file
//! into the sink under `data/...`, hash it on the fly with each requested
//! algorithm, then write manifests, `bagit.txt`, `bag-info.txt`, and
//! tag manifests.

use crate::bag_info::{BagInfo, PayloadOxum};
use crate::bagit_txt::BagItDeclaration;
use crate::error::BagResult;
use crate::hash::{Algorithm, StreamHasher};
use crate::io::{BagSink, BagSource};
use crate::manifest::{Manifest, ManifestEntry, ManifestKind};
use crate::path;
use crate::spec;

/// Options for [`create`].
#[derive(Debug, Clone)]
pub struct CreateOptions {
    /// Algorithms to compute manifests for. At least one is required.
    pub algorithms: Vec<Algorithm>,
    /// Initial `bag-info.txt` contents. `Payload-Oxum`, `Bagging-Date`, and
    /// `Bag-Software-Agent` are filled in automatically if absent.
    pub bag_info: BagInfo,
    /// When true, also emit `tagmanifest-*.txt` for each algorithm.
    pub include_tag_manifests: bool,
    /// Override `Bagging-Date` (otherwise the caller-supplied value in
    /// `bag_info` wins, then nothing — wasm callers should pass today's date
    /// from JS since wasm32 has no clock).
    pub bagging_date: Option<String>,
    /// Override `Bag-Software-Agent` (otherwise the caller-supplied value in
    /// `bag_info` wins, then `DEFAULT_SOFTWARE_AGENT`).
    pub software_agent: Option<String>,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            // bagit-python's DEFAULT_CHECKSUMS.
            algorithms: vec![Algorithm::Sha256, Algorithm::Sha512],
            bag_info: BagInfo::new(),
            include_tag_manifests: true,
            bagging_date: None,
            software_agent: None,
        }
    }
}

impl CreateOptions {
    pub const DEFAULT_SOFTWARE_AGENT: &'static str =
        "bagr-wasm <https://github.com/emoporemilio/bagr-wasm>";
}

/// Assemble a bag in `sink` from payload files in `source`.
pub async fn create<S: BagSource + ?Sized, D: BagSink + ?Sized>(
    source: &S,
    sink: &mut D,
    options: CreateOptions,
) -> BagResult<()> {
    assert!(!options.algorithms.is_empty(), "at least one algorithm required");
    let algorithms = {
        let mut a = options.algorithms.clone();
        a.sort();
        a.dedup();
        a
    };

    // Each payload manifest accumulates (checksum, path) entries.
    let mut payload_entries: Vec<Vec<ManifestEntry>> = vec![Vec::new(); algorithms.len()];
    let mut total_octets: u64 = 0;
    let mut total_files: u64 = 0;

    let mut paths = source.list().await?;
    paths.sort();
    for raw in paths {
        let rel = path::normalize(&raw)?;
        let stored = format!("data/{rel}");

        let mut hashers: Vec<Box<dyn StreamHasher>> =
            algorithms.iter().map(|a| a.hasher()).collect();
        let mut writer = sink.create(&stored).await?;
        let mut reader = source.open(&raw).await?;
        let mut bytes_seen: u64 = 0;
        while let Some(chunk) = reader.next_chunk().await? {
            bytes_seen += chunk.len() as u64;
            writer.write_chunk(&chunk).await?;
            for h in hashers.iter_mut() {
                h.update(&chunk);
            }
        }
        writer.close().await?;

        for (i, h) in hashers.into_iter().enumerate() {
            payload_entries[i].push(ManifestEntry {
                checksum: h.finalize_hex(),
                path: stored.clone(),
            });
        }
        total_octets += bytes_seen;
        total_files += 1;
    }

    // bagit.txt
    let declaration = BagItDeclaration::default_v097();
    write_text(sink, spec::BAGIT_TXT, &declaration.serialize()).await?;

    // bag-info.txt with defaults filled in to match bagit-python's make_bag.
    let mut info = options.bag_info.clone();
    if info.get(spec::BAGGING_DATE_KEY).is_none() {
        if let Some(date) = options.bagging_date.as_deref() {
            info.set(spec::BAGGING_DATE_KEY, date);
        }
    }
    if info.get("Bag-Software-Agent").is_none() {
        let agent = options
            .software_agent
            .as_deref()
            .unwrap_or(CreateOptions::DEFAULT_SOFTWARE_AGENT);
        info.set("Bag-Software-Agent", agent);
    }
    info.set(
        spec::PAYLOAD_OXUM_KEY,
        PayloadOxum {
            octets: total_octets,
            streams: total_files,
        }
        .render(),
    );
    write_text(sink, spec::BAG_INFO_TXT, &info.serialize()).await?;

    // Payload manifests.
    let mut payload_manifest_files: Vec<(String, String)> = Vec::new();
    for (i, alg) in algorithms.iter().enumerate() {
        let m = Manifest {
            algorithm: *alg,
            kind: ManifestKind::Payload,
            entries: payload_entries[i].clone(),
        };
        let filename = m.filename();
        let text = m.serialize();
        write_text(sink, &filename, &text).await?;
        payload_manifest_files.push((filename, text));
    }

    if options.include_tag_manifests {
        // Tag manifests cover every tag file written so far: bagit.txt,
        // bag-info.txt, and each payload manifest. They do NOT cover
        // themselves.
        let bagit_text = declaration.serialize();
        let bag_info_text = info.serialize();
        let tag_files: Vec<(String, String)> = std::iter::once((
            spec::BAGIT_TXT.to_string(),
            bagit_text,
        ))
        .chain(std::iter::once((
            spec::BAG_INFO_TXT.to_string(),
            bag_info_text,
        )))
        .chain(payload_manifest_files.into_iter())
        .collect();

        for alg in &algorithms {
            let mut entries = Vec::with_capacity(tag_files.len());
            for (name, text) in &tag_files {
                let mut h = alg.hasher();
                h.update(text.as_bytes());
                entries.push(ManifestEntry {
                    checksum: h.finalize_hex(),
                    path: name.clone(),
                });
            }
            let m = Manifest {
                algorithm: *alg,
                kind: ManifestKind::Tag,
                entries,
            };
            write_text(sink, &m.filename(), &m.serialize()).await?;
        }
    }

    Ok(())
}

async fn write_text<D: BagSink + ?Sized>(
    sink: &mut D,
    path: &str,
    text: &str,
) -> BagResult<()> {
    let mut w = sink.create(path).await?;
    w.write_chunk(text.as_bytes()).await?;
    w.close().await?;
    Ok(())
}
