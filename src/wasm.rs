//! `wasm-bindgen` bridge. Compiled only for `wasm32` targets.
//!
//! Exposes two service classes to JavaScript:
//!
//! - [`Validator`] — wraps [`crate::validate::validate_with`]
//! - [`BagBuilder`] — wraps [`crate::create::create`]
//!
//! Both take JS objects that play the role of [`crate::io::BagSource`] /
//! [`crate::io::BagSink`]. The JS contract is:
//!
//! ```js
//! // Source (read side)
//! const source = {
//!   async list() { return ["path/a.txt", "path/b.bin"]; },
//!   async size(path) { return file.size; },     // optional, enables fast=true
//!   async open(path) {
//!     return {
//!       // Pull-style. Return { done: true } at EOF, otherwise { value: Uint8Array }.
//!       async next() { return { value: chunk, done: false }; }
//!     };
//!   },
//! };
//!
//! // Sink (write side)
//! const sink = {
//!   async create(path) {
//!     return {
//!       async write(chunk /* Uint8Array */) { /* ... */ },
//!       async close() { /* ... */ },
//!     };
//!   },
//! };
//! ```

use crate::bag_info::BagInfo;
use crate::create::{create as core_create, CreateOptions};
use crate::error::{BagError, BagResult};
use crate::hash::Algorithm;
use crate::io::{BagSink, BagSource, FileReader, FileWriter};
use crate::validate::{validate_with, ValidateOptions, ValidationReport};
use async_trait::async_trait;
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn bagit_version() -> String {
    crate::spec::BAGIT_VERSION.to_string()
}

// ---------- JS <-> Rust glue helpers ----------

fn js_err(v: JsValue) -> BagError {
    BagError::Io(
        v.as_string()
            .or_else(|| {
                Reflect::get(&v, &JsValue::from_str("message"))
                    .ok()
                    .and_then(|m| m.as_string())
            })
            .unwrap_or_else(|| format!("{v:?}")),
    )
}

fn bag_err_to_js(e: BagError) -> JsValue {
    JsValue::from_str(&e.to_string())
}

async fn await_promise_like(value: JsValue) -> BagResult<JsValue> {
    // Tolerate hosts that return a plain value or a Promise — Promise.resolve
    // wraps either uniformly.
    let promise = Promise::resolve(&value);
    JsFuture::from(promise).await.map_err(js_err)
}

fn get_method(obj: &JsValue, name: &str) -> BagResult<Function> {
    let val = Reflect::get(obj, &JsValue::from_str(name)).map_err(js_err)?;
    val.dyn_into::<Function>()
        .map_err(|_| BagError::Io(format!("host object has no `{name}()` method")))
}

fn opt_method(obj: &JsValue, name: &str) -> Option<Function> {
    Reflect::get(obj, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into::<Function>().ok())
}

// ---------- JS-backed BagSource ----------

struct JsBagSource {
    obj: JsValue,
}

struct JsFileReader {
    reader: JsValue,
    next: Function,
}

#[async_trait(?Send)]
impl FileReader for JsFileReader {
    async fn next_chunk(&mut self) -> BagResult<Option<Vec<u8>>> {
        let raw = self.next.call0(&self.reader).map_err(js_err)?;
        let resolved = await_promise_like(raw).await?;
        // `resolved` is `{ value: Uint8Array?, done: bool }` per our contract.
        let done = Reflect::get(&resolved, &JsValue::from_str("done"))
            .map_err(js_err)?
            .as_bool()
            .unwrap_or(false);
        if done {
            return Ok(None);
        }
        let value = Reflect::get(&resolved, &JsValue::from_str("value")).map_err(js_err)?;
        if value.is_undefined() || value.is_null() {
            return Ok(Some(Vec::new()));
        }
        let array: Uint8Array = value
            .dyn_into()
            .map_err(|_| BagError::Io("reader.next() value must be a Uint8Array".into()))?;
        Ok(Some(array.to_vec()))
    }
}

#[async_trait(?Send)]
impl BagSource for JsBagSource {
    async fn list(&self) -> BagResult<Vec<String>> {
        let f = get_method(&self.obj, "list")?;
        let result = await_promise_like(f.call0(&self.obj).map_err(js_err)?).await?;
        let arr: Array = result
            .dyn_into()
            .map_err(|_| BagError::Io("list() must return an array of strings".into()))?;
        let mut out = Vec::with_capacity(arr.length() as usize);
        for v in arr.iter() {
            out.push(
                v.as_string()
                    .ok_or_else(|| BagError::Io("list() entries must be strings".into()))?,
            );
        }
        Ok(out)
    }

    async fn open(&self, path: &str) -> BagResult<Box<dyn FileReader + '_>> {
        let f = get_method(&self.obj, "open")?;
        let raw = f
            .call1(&self.obj, &JsValue::from_str(path))
            .map_err(js_err)?;
        let reader = await_promise_like(raw).await?;
        if !reader.is_object() {
            return Err(BagError::Io(format!(
                "open({path:?}) did not return an object with next()"
            )));
        }
        let next = get_method(&reader, "next")?;
        Ok(Box::new(JsFileReader { reader, next }))
    }

    async fn size(&self, path: &str) -> BagResult<u64> {
        if let Some(f) = opt_method(&self.obj, "size") {
            let raw = f
                .call1(&self.obj, &JsValue::from_str(path))
                .map_err(js_err)?;
            let val = await_promise_like(raw).await?;
            let n = val
                .as_f64()
                .ok_or_else(|| BagError::Io("size() must return a number".into()))?;
            return Ok(n as u64);
        }
        // Fall back to streaming the whole file.
        let mut r = self.open(path).await?;
        let mut n: u64 = 0;
        while let Some(chunk) = r.next_chunk().await? {
            n += chunk.len() as u64;
        }
        Ok(n)
    }

    async fn fetch_url(&self, url: &str) -> BagResult<Option<Box<dyn FileReader + '_>>> {
        let Some(f) = opt_method(&self.obj, "fetch") else {
            return Ok(None);
        };
        let raw = f
            .call1(&self.obj, &JsValue::from_str(url))
            .map_err(js_err)?;
        let reader = await_promise_like(raw).await?;
        if !reader.is_object() {
            return Err(BagError::Io(format!(
                "fetch({url:?}) did not return an object with next()"
            )));
        }
        let next = get_method(&reader, "next")?;
        Ok(Some(Box::new(JsFileReader { reader, next })))
    }
}

// ---------- JS-backed BagSink ----------

struct JsBagSink {
    obj: JsValue,
}

struct JsFileWriter {
    writer: JsValue,
    write: Function,
    close: Function,
}

#[async_trait(?Send)]
impl FileWriter for JsFileWriter {
    async fn write_chunk(&mut self, chunk: &[u8]) -> BagResult<()> {
        // Copy bytes into a JS Uint8Array. We allocate a fresh buffer per
        // chunk so the JS side can hold on to it without us reusing the
        // memory — important for streaming writes that defer beyond `await`.
        let arr = Uint8Array::new_with_length(chunk.len() as u32);
        arr.copy_from(chunk);
        let raw = self
            .write
            .call1(&self.writer, &arr.into())
            .map_err(js_err)?;
        await_promise_like(raw).await?;
        Ok(())
    }

    async fn close(&mut self) -> BagResult<()> {
        let raw = self.close.call0(&self.writer).map_err(js_err)?;
        await_promise_like(raw).await?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl BagSink for JsBagSink {
    async fn create(&mut self, path: &str) -> BagResult<Box<dyn FileWriter + '_>> {
        let f = get_method(&self.obj, "create")?;
        let raw = f
            .call1(&self.obj, &JsValue::from_str(path))
            .map_err(js_err)?;
        let writer = await_promise_like(raw).await?;
        if !writer.is_object() {
            return Err(BagError::Io(format!(
                "create({path:?}) did not return an object with write()/close()"
            )));
        }
        let write = get_method(&writer, "write")?;
        let close = get_method(&writer, "close")?;
        Ok(Box::new(JsFileWriter { writer, write, close }))
    }
}

// ---------- Public JS-facing API ----------

/// Service: validate an existing bag.
#[wasm_bindgen]
pub struct Validator;

#[wasm_bindgen]
impl Validator {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Validate `source` (a JS object implementing the source contract).
    ///
    /// `options` may be omitted or `{ fast?: bool, completeness_only?: bool }`.
    /// Resolves to a `{ payload_files, payload_octets, payload_manifests,
    /// tag_manifests, held_files }` object.
    #[wasm_bindgen(js_name = validate)]
    pub async fn validate_js(&self, source: JsValue, options: JsValue) -> Result<JsValue, JsValue> {
        let opts = parse_validate_options(&options).map_err(bag_err_to_js)?;
        let src = JsBagSource { obj: source };
        let report = validate_with(&src, opts).await.map_err(bag_err_to_js)?;
        Ok(report_to_js(&report))
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

/// Service: build a new bag.
#[wasm_bindgen]
pub struct BagBuilder;

#[wasm_bindgen]
impl BagBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Assemble a bag in `sink` from payload files in `source`.
    ///
    /// `options` keys (all optional):
    /// - `algorithms`: `string[]` (subset of `["md5","sha256","sha512"]`).
    /// - `bag_info`: `{ [key: string]: string | string[] }`.
    /// - `bagging_date`: `string` (e.g. `"2026-05-26"`). wasm has no clock,
    ///   so pass `new Date().toISOString().slice(0,10)` from JS.
    /// - `software_agent`: `string`.
    /// - `include_tag_manifests`: `bool` (default true).
    #[wasm_bindgen(js_name = build)]
    pub async fn build_js(
        &self,
        source: JsValue,
        sink: JsValue,
        options: JsValue,
    ) -> Result<JsValue, JsValue> {
        let opts = parse_create_options(&options).map_err(bag_err_to_js)?;
        let src = JsBagSource { obj: source };
        let mut snk = JsBagSink { obj: sink };
        core_create(&src, &mut snk, opts).await.map_err(bag_err_to_js)?;
        Ok(JsValue::TRUE)
    }
}

impl Default for BagBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- Options parsing ----------

fn parse_validate_options(value: &JsValue) -> BagResult<ValidateOptions> {
    if value.is_undefined() || value.is_null() {
        return Ok(ValidateOptions::default());
    }
    let fast = Reflect::get(value, &JsValue::from_str("fast"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let completeness_only = Reflect::get(value, &JsValue::from_str("completeness_only"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(ValidateOptions { fast, completeness_only })
}

fn parse_create_options(value: &JsValue) -> BagResult<CreateOptions> {
    let mut opts = CreateOptions::default();
    if value.is_undefined() || value.is_null() {
        return Ok(opts);
    }

    if let Ok(v) = Reflect::get(value, &JsValue::from_str("algorithms")) {
        if !v.is_undefined() && !v.is_null() {
            let arr: Array = v
                .dyn_into()
                .map_err(|_| BagError::Io("options.algorithms must be an array".into()))?;
            let mut algs = Vec::new();
            for item in arr.iter() {
                let name = item
                    .as_string()
                    .ok_or_else(|| BagError::Io("algorithm entries must be strings".into()))?;
                algs.push(Algorithm::from_name(&name)?);
            }
            if !algs.is_empty() {
                opts.algorithms = algs;
            }
        }
    }

    if let Ok(v) = Reflect::get(value, &JsValue::from_str("include_tag_manifests")) {
        if let Some(b) = v.as_bool() {
            opts.include_tag_manifests = b;
        }
    }

    if let Ok(v) = Reflect::get(value, &JsValue::from_str("bagging_date")) {
        if let Some(s) = v.as_string() {
            opts.bagging_date = Some(s);
        }
    }

    if let Ok(v) = Reflect::get(value, &JsValue::from_str("software_agent")) {
        if let Some(s) = v.as_string() {
            opts.software_agent = Some(s);
        }
    }

    if let Ok(v) = Reflect::get(value, &JsValue::from_str("bag_info")) {
        if v.is_object() && !v.is_null() {
            opts.bag_info = bag_info_from_js(&v)?;
        }
    }

    Ok(opts)
}

fn bag_info_from_js(value: &JsValue) -> BagResult<BagInfo> {
    let mut info = BagInfo::new();
    let keys = Object::keys(value.unchecked_ref::<Object>());
    for key in keys.iter() {
        let key_str = key
            .as_string()
            .ok_or_else(|| BagError::Io("bag_info keys must be strings".into()))?;
        let val = Reflect::get(value, &key).map_err(js_err)?;
        if let Some(s) = val.as_string() {
            info.entries.push((key_str, s));
        } else if let Ok(arr) = val.clone().dyn_into::<Array>() {
            for item in arr.iter() {
                let s = item.as_string().ok_or_else(|| {
                    BagError::Io(format!("bag_info[{key_str:?}] entries must be strings"))
                })?;
                info.entries.push((key_str.clone(), s));
            }
        } else {
            return Err(BagError::Io(format!(
                "bag_info[{key_str:?}] must be a string or string[]"
            )));
        }
    }
    Ok(info)
}

// ---------- Report rendering ----------

fn report_to_js(r: &ValidationReport) -> JsValue {
    let obj = Object::new();
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("payload_files"),
        &JsValue::from_f64(r.payload_files as f64),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("payload_octets"),
        &JsValue::from_f64(r.payload_octets as f64),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("held_files"),
        &JsValue::from_f64(r.held_files as f64),
    );
    let pm = Array::new();
    for n in &r.payload_manifests {
        pm.push(&JsValue::from_str(n));
    }
    let _ = Reflect::set(&obj, &JsValue::from_str("payload_manifests"), &pm);
    let tm = Array::new();
    for n in &r.tag_manifests {
        tm.push(&JsValue::from_str(n));
    }
    let _ = Reflect::set(&obj, &JsValue::from_str("tag_manifests"), &tm);
    obj.into()
}
