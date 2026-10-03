//! Turning an adjustment layer's stored data into [`Params`].
//!
//! The compositor resolves a layer's parameters once per tile. For every
//! other kind that is a few hundred bytes of JSON; a Color Lookup layer
//! carries its whole table, which is far too much to decode and parse
//! hundreds of times a frame. Large payloads are therefore remembered:
//! a small cache keyed by the exact bytes (compared in full, so a hit can
//! never be a different table) hands back the parsed parameters, whose
//! table is shared behind an `Arc`.

use super::*;
use schist_core::AdjustmentData;
use std::sync::{Arc, Mutex};

/// Payloads below this are parsed every time, as they always were.
const CACHE_FROM: usize = 16 * 1024;
/// Distinct large payloads remembered: a few LUT layers plus the one
/// being previewed.
const CACHE_ENTRIES: usize = 8;

struct Entry {
    json: bool,
    kind: AdjustmentKind,
    bytes: Arc<[u8]>,
    params: Params,
}

static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// An adjustment layer's parameters: our canonical JSON when the user has
/// edited it, otherwise the preserved PSD payload.
pub fn resolve(data: &AdjustmentData) -> Params {
    match data.params_json.as_deref() {
        Some(json) => cached(
            true,
            data.kind,
            json.as_bytes(),
            || match serde_json::from_str::<Params>(json) {
                Ok(p) => p,
                Err(err) => {
                    log::warn!("adjustment params unreadable: {err}");
                    parse_psd(data.kind, &data.raw)
                }
            },
        ),
        None => cached(false, data.kind, &data.raw, || {
            parse_psd(data.kind, &data.raw)
        }),
    }
}

fn cached(
    json: bool,
    kind: AdjustmentKind,
    bytes: &[u8],
    parse: impl FnOnce() -> Params,
) -> Params {
    if bytes.len() < CACHE_FROM {
        return parse();
    }
    let find = |cache: &[Entry]| {
        cache
            .iter()
            .position(|e| e.json == json && e.kind == kind && *e.bytes == *bytes)
    };
    {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = find(&cache) {
            // Most recently used last.
            let entry = cache.remove(i);
            let params = entry.params.clone();
            cache.push(entry);
            return params;
        }
    }
    // Parse outside the lock: tiles composite in parallel, and a first
    // parse of a large table should not stall layers that are cached.
    let params = parse();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if find(&cache).is_none() {
        if cache.len() >= CACHE_ENTRIES {
            cache.remove(0);
        }
        cache.push(Entry {
            json,
            kind,
            bytes: Arc::from(bytes),
            params: params.clone(),
        });
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lut::Lut3d;

    fn lookup_json(seed: f32) -> String {
        let mut cube = Lut3d::identity(17);
        for v in &mut cube.table {
            v[0] = (v[0] * seed).min(1.0);
        }
        serde_json::to_string(&Params::ColorLookup(ColorLookup {
            name: format!("{seed}"),
            input: LutInput::Document,
            table: Some(LutTable::from_cube("t", &cube)),
        }))
        .unwrap()
    }

    #[test]
    fn large_payloads_resolve_to_their_own_table() {
        let (a, b) = (lookup_json(0.5), lookup_json(0.25));
        assert!(a.len() >= CACHE_FROM, "fixture should exercise the cache");
        let data = |json: &String| AdjustmentData {
            kind: AdjustmentKind::ColorLookup,
            raw: Vec::new(),
            params_json: Some(json.clone()),
        };
        let px = Rgba::new(1.0, 0.5, 0.5, 1.0);
        for _ in 0..3 {
            assert!((resolve(&data(&a)).apply(px).r - 0.5).abs() < 1e-3);
            assert!((resolve(&data(&b)).apply(px).r - 0.25).abs() < 1e-3);
        }
        assert_eq!(resolve(&data(&a)), serde_json::from_str(&a).unwrap());
    }

    #[test]
    fn small_payloads_still_parse_directly() {
        let data = AdjustmentData {
            kind: AdjustmentKind::Invert,
            raw: Vec::new(),
            params_json: Some("not json".into()),
        };
        assert_eq!(resolve(&data), Params::Invert);
    }
}
