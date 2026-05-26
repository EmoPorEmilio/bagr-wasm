//! Host I/O abstraction.
//!
//! The browser hands us files via async callbacks. The Rust core is written
//! against these traits so we can plug a `wasm-bindgen` bridge on `wasm32`
//! and an in-memory fake in native tests.

use crate::error::BagResult;
use async_trait::async_trait;

/// A streaming source for a single file in a bag.
#[async_trait(?Send)]
pub trait FileReader {
    /// Pull the next chunk. `Ok(None)` signals EOF.
    async fn next_chunk(&mut self) -> BagResult<Option<Vec<u8>>>;
}

/// A streaming sink for writing a single file.
#[async_trait(?Send)]
pub trait FileWriter {
    async fn write_chunk(&mut self, chunk: &[u8]) -> BagResult<()>;
    async fn close(&mut self) -> BagResult<()>;
}

/// The host-side view of a bag-in-progress, used during validation. The
/// validator lists the bag, then asks the host to open each file as needed.
#[async_trait(?Send)]
pub trait BagSource {
    /// Every file path in the bag, relative to the bag root, using forward
    /// slashes. Order is not significant.
    async fn list(&self) -> BagResult<Vec<String>>;

    /// Open a streaming reader for the file at `path`.
    async fn open(&self, path: &str) -> BagResult<Box<dyn FileReader + '_>>;

    /// File size in bytes, if the host can answer cheaply (e.g. browser
    /// `File.size`). The default implementation streams the file and counts;
    /// `fast` validation overrides this with `File.size` in the wasm bridge.
    async fn size(&self, path: &str) -> BagResult<u64> {
        let mut reader = self.open(path).await?;
        let mut n: u64 = 0;
        while let Some(chunk) = reader.next_chunk().await? {
            n += chunk.len() as u64;
        }
        Ok(n)
    }
}

/// The host-side view of a destination during bag creation.
#[async_trait(?Send)]
pub trait BagSink {
    async fn create(&mut self, path: &str) -> BagResult<Box<dyn FileWriter + '_>>;
}

// ---------- In-memory fake, used by tests and convenient for fixtures ----------

use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct MemoryBag {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl MemoryBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<String>, bytes: impl Into<Vec<u8>>) {
        self.files.insert(path.into(), bytes.into());
    }
}

pub struct MemoryReader {
    bytes: Vec<u8>,
    done: bool,
}

#[async_trait(?Send)]
impl FileReader for MemoryReader {
    async fn next_chunk(&mut self) -> BagResult<Option<Vec<u8>>> {
        if self.done {
            Ok(None)
        } else {
            self.done = true;
            Ok(Some(std::mem::take(&mut self.bytes)))
        }
    }
}

#[async_trait(?Send)]
impl BagSource for MemoryBag {
    async fn list(&self) -> BagResult<Vec<String>> {
        Ok(self.files.keys().cloned().collect())
    }

    async fn open(&self, path: &str) -> BagResult<Box<dyn FileReader + '_>> {
        let bytes = self
            .files
            .get(path)
            .ok_or_else(|| crate::error::BagError::Io(format!("missing file: {path}")))?
            .clone();
        Ok(Box::new(MemoryReader { bytes, done: false }))
    }

    async fn size(&self, path: &str) -> BagResult<u64> {
        self.files
            .get(path)
            .map(|b| b.len() as u64)
            .ok_or_else(|| crate::error::BagError::Io(format!("missing file: {path}")))
    }
}

pub struct MemoryWriter<'a> {
    target: &'a mut Vec<u8>,
}

#[async_trait(?Send)]
impl<'a> FileWriter for MemoryWriter<'a> {
    async fn write_chunk(&mut self, chunk: &[u8]) -> BagResult<()> {
        self.target.extend_from_slice(chunk);
        Ok(())
    }
    async fn close(&mut self) -> BagResult<()> {
        Ok(())
    }
}

#[async_trait(?Send)]
impl BagSink for MemoryBag {
    async fn create(&mut self, path: &str) -> BagResult<Box<dyn FileWriter + '_>> {
        let slot = self.files.entry(path.to_string()).or_default();
        slot.clear();
        Ok(Box::new(MemoryWriter { target: slot }))
    }
}
