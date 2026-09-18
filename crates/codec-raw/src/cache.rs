//! Reuse decoded sensor samples while white balance/exposure are edited.
use crate::{RawData, RawImage, Result};
use std::sync::{Arc, Mutex, OnceLock};

struct Entry {
    bytes: Arc<[u8]>,
    raw: Arc<RawImage>,
}
static CACHE: OnceLock<Mutex<Option<Entry>>> = OnceLock::new();

/// Decode with a bounded, single-image cache. Exact byte comparison makes
/// modified files safe even when their names and metadata have not changed.
/// Returned samples are shared immutably across concurrent developments.
pub fn decode_cached(bytes: &[u8]) -> Result<Arc<RawImage>> {
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Some(entry) = cache.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        if entry.bytes.as_ref() == bytes {
            return Ok(entry.raw.clone());
        }
    }
    let raw = Arc::new(crate::decode(bytes)?);
    let samples = match &raw.data {
        RawData::U16(v) => v.len().saturating_mul(2),
        RawData::F32(v) => v.len().saturating_mul(4),
    };
    let size = samples
        .saturating_add(bytes.len())
        .saturating_add(raw.preview.as_ref().map_or(0, Vec::len));
    let entry = (size <= 128 * 1024 * 1024).then(|| Entry {
        bytes: Arc::from(bytes),
        raw: raw.clone(),
    });
    *cache.lock().unwrap_or_else(|p| p.into_inner()) = entry;
    Ok(raw)
}

/// Release retained file bytes and sensor samples, for memory-pressure handling.
pub fn clear_decode_cache() {
    if let Some(cache) = CACHE.get() {
        *cache.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}
