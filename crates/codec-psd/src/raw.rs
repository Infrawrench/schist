//! Schist camera-raw payloads in a private additional-layer-info block.
//!
//! PSD has no portable representation for an editable camera capture.
//! `ScRw` keeps the original file and Schist's development settings beside
//! the rendered layer pixels. Other PSD readers ignore the private block;
//! Schist can reopen it and render from the sensor data again.
//!
//! Local adjustments live in a second block, `ScRm`, beside it. Keeping
//! them separate leaves `ScRw` readable by versions that predate masks,
//! which keep the unknown block verbatim and so do not lose them.
//! Colour grading has a third block, `ScCg`, for the same reason.

use schist_core::raw::{ColorGrading, GradeWheel};
use schist_core::{Layer, LocalMask, MaskRaster, MaskShape, RawDevelopment, RawSettings};
use std::sync::Arc;

/// Private block key: "Sc" for Schist, "Rw" for camera raw.
pub const RAW_BLOCK_KEY: [u8; 4] = *b"ScRw";
/// Private block key for a development's local adjustment masks.
pub const MASKS_BLOCK_KEY: [u8; 4] = *b"ScRm";
const MASKS_VERSION: u32 = 1;

/// Private block key for a development's colour grading wheels.
pub const GRADING_BLOCK_KEY: [u8; 4] = *b"ScCg";

const VERSION: u32 = 1;
const SETTINGS_LEN: usize = 15;
const GRADING_VERSION: u32 = 1;
const GRADING_LEN: usize = 14;
/// A malformed block must not be able to make the reader allocate without
/// limit. This is well above the largest current still-camera capture.
pub(crate) const MAX_SOURCE_BYTES: usize = 1 << 30;

/// Serialize a RAW-backed layer, or `None` for an ordinary layer.
pub fn write_raw(layer: &Layer) -> Option<Vec<u8>> {
    let raw = layer.raw.as_deref()?;
    if raw.source.is_empty() || raw.source.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let source_len = u32::try_from(raw.source.len()).ok()?;
    let mut out = Vec::with_capacity(8 + SETTINGS_LEN * 4 + raw.source.len());
    out.extend_from_slice(&VERSION.to_be_bytes());
    out.extend_from_slice(&source_len.to_be_bytes());
    for value in settings_values(raw.settings.sanitized()) {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend_from_slice(&raw.source);
    Some(out)
}

/// Parse a private RAW payload. A malformed payload is ignored because the
/// PSD still contains the last rendered pixels for the layer.
pub fn read_raw(data: &[u8]) -> Option<RawDevelopment> {
    let mut cursor = Cursor { data, at: 0 };
    if cursor.u32()? != VERSION {
        return None;
    }
    let source_len = cursor.u32()? as usize;
    if source_len == 0 || source_len > MAX_SOURCE_BYTES {
        return None;
    }
    let mut values = [0.0f32; SETTINGS_LEN];
    for value in &mut values {
        *value = cursor.f32()?;
        if !value.is_finite() {
            return None;
        }
    }
    let source = cursor.take(source_len)?;
    if cursor.at != data.len() {
        return None;
    }
    Some(RawDevelopment {
        source: Arc::from(source),
        settings: settings_from_values(values).sanitized(),
        masks: Vec::new(),
    })
}

/// Serialize a development's masks, or `None` when it has none.
///
/// The recipe is JSON. Each detected component's raster follows it, in
/// the order the components appear, as its size and zlib-compressed
/// bytes; a component not yet detected is a zero width.
pub fn write_masks(layer: &Layer) -> Option<Vec<u8>> {
    let raw = layer.raw.as_deref()?;
    if raw.masks.is_empty() {
        return None;
    }
    let json = serde_json::to_vec(&raw.masks).ok()?;
    let mut out = Vec::with_capacity(8 + json.len());
    out.extend_from_slice(&MASKS_VERSION.to_be_bytes());
    out.extend_from_slice(&u32::try_from(json.len()).ok()?.to_be_bytes());
    out.extend_from_slice(&json);
    for shape in detected(&raw.masks) {
        let MaskShape::Detected { raster, .. } = shape else {
            continue;
        };
        match raster.as_deref().filter(|r| r.is_valid()) {
            Some(raster) => {
                let packed = miniz_oxide::deflate::compress_to_vec_zlib(&raster.data, 6);
                out.extend_from_slice(&raster.width.to_be_bytes());
                out.extend_from_slice(&raster.height.to_be_bytes());
                out.extend_from_slice(&u32::try_from(packed.len()).ok()?.to_be_bytes());
                out.extend_from_slice(&packed);
            }
            None => out.extend_from_slice(&0u32.to_be_bytes()),
        }
    }
    Some(out)
}

/// Parse a masks block. Malformed input is ignored, like `ScRw`: the
/// rendered pixels are still in the file.
pub fn read_masks(data: &[u8]) -> Option<Vec<LocalMask>> {
    let mut cursor = Cursor { data, at: 0 };
    if cursor.u32()? != MASKS_VERSION {
        return None;
    }
    let json_len = cursor.u32()? as usize;
    let mut masks: Vec<LocalMask> = serde_json::from_slice(cursor.take(json_len)?).ok()?;
    if masks.len() > schist_core::raw_masks::MAX_MASKS {
        return None;
    }
    for shape in detected_mut(&mut masks) {
        let MaskShape::Detected { raster, .. } = shape else {
            continue;
        };
        let width = cursor.u32()?;
        if width == 0 {
            continue;
        }
        let height = cursor.u32()?;
        let packed_len = cursor.u32()? as usize;
        let packed = cursor.take(packed_len)?;
        if width > schist_core::raw_masks::MAX_RASTER_SIDE
            || height > schist_core::raw_masks::MAX_RASTER_SIDE
        {
            return None;
        }
        let expected = width as usize * height as usize;
        let bytes =
            miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(packed, expected.max(1))
                .ok()?;
        let decoded = MaskRaster {
            width,
            height,
            data: bytes,
        };
        if !decoded.is_valid() {
            return None;
        }
        *raster = Some(Arc::new(decoded));
    }
    if cursor.at != data.len() {
        return None;
    }
    Some(schist_core::raw_masks::sanitize_masks(masks))
}

fn detected(masks: &[LocalMask]) -> impl Iterator<Item = &MaskShape> {
    masks
        .iter()
        .flat_map(|m| m.components.iter().map(|c| &c.shape))
        .filter(|s| matches!(s, MaskShape::Detected { .. }))
}

fn detected_mut(masks: &mut [LocalMask]) -> impl Iterator<Item = &mut MaskShape> {
    masks
        .iter_mut()
        .flat_map(|m| m.components.iter_mut().map(|c| &mut c.shape))
        .filter(|s| matches!(s, MaskShape::Detected { .. }))
}

/// Serialize a RAW-backed layer's colour grading, or `None` when it is
/// the default (which is what a reader assumes when the block is absent).
pub fn write_grading(layer: &Layer) -> Option<Vec<u8>> {
    let raw = layer.raw.as_deref()?;
    let g = raw.settings.grading.sanitized();
    if g == ColorGrading::default() {
        return None;
    }
    let mut out = Vec::with_capacity(4 + GRADING_LEN * 4);
    out.extend_from_slice(&GRADING_VERSION.to_be_bytes());
    for wheel in g.wheels() {
        for value in [wheel.hue, wheel.saturation, wheel.luminance] {
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
    out.extend_from_slice(&g.blending.to_be_bytes());
    out.extend_from_slice(&g.balance.to_be_bytes());
    Some(out)
}

/// Parse a grading block. Malformed or newer payloads are `None`, and the
/// caller keeps such a block verbatim.
pub fn read_grading(data: &[u8]) -> Option<ColorGrading> {
    let mut cursor = Cursor { data, at: 0 };
    if cursor.u32()? != GRADING_VERSION {
        return None;
    }
    let mut v = [0.0f32; GRADING_LEN];
    for value in &mut v {
        *value = cursor.f32()?;
        if !value.is_finite() {
            return None;
        }
    }
    if cursor.at != data.len() {
        return None;
    }
    let wheel = |i: usize| GradeWheel {
        hue: v[i],
        saturation: v[i + 1],
        luminance: v[i + 2],
    };
    Some(
        ColorGrading {
            shadows: wheel(0),
            midtones: wheel(3),
            highlights: wheel(6),
            global: wheel(9),
            blending: v[12],
            balance: v[13],
        }
        .sanitized(),
    )
}

/// Fold a layer's `ScCg` block into its development, dropping the block
/// once it has been read: the writer regenerates it from the settings.
pub(crate) fn attach_grading(
    raw: Option<Box<RawDevelopment>>,
    extras: &mut Vec<schist_core::RawBlock>,
) -> Option<Box<RawDevelopment>> {
    let mut raw = raw?;
    if let Some(at) = extras.iter().position(|b| b.key == GRADING_BLOCK_KEY) {
        if let Some(grading) = read_grading(&extras[at].data) {
            raw.settings.grading = grading;
            extras.remove(at);
        }
    }
    Some(raw)
}

fn settings_values(s: RawSettings) -> [f32; SETTINGS_LEN] {
    [
        s.temperature,
        s.tint,
        s.exposure,
        s.contrast,
        s.highlights,
        s.shadows,
        s.whites,
        s.blacks,
        s.clarity,
        s.dehaze,
        s.vibrance,
        s.saturation,
        s.sharpening,
        s.noise,
        s.vignette,
    ]
}

fn settings_from_values(v: [f32; SETTINGS_LEN]) -> RawSettings {
    RawSettings {
        temperature: v[0],
        tint: v[1],
        exposure: v[2],
        contrast: v[3],
        highlights: v[4],
        shadows: v[5],
        whites: v[6],
        blacks: v[7],
        clarity: v[8],
        dehaze: v[9],
        vibrance: v[10],
        saturation: v[11],
        sharpening: v[12],
        noise: v[13],
        vignette: v[14],
        grading: ColorGrading::default(),
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let bytes = self.data.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_payload_round_trips_source_and_settings() {
        let mut layer = Layer::new_raster("capture");
        let settings = RawSettings {
            temperature: 18.0,
            tint: -7.0,
            exposure: 1.25,
            highlights: -32.0,
            shadows: 41.0,
            sharpening: 63.0,
            ..RawSettings::default()
        };
        layer.raw = Some(Box::new(RawDevelopment {
            source: Arc::from(&b"raw camera bytes"[..]),
            settings,
            masks: Vec::new(),
        }));

        let payload = write_raw(&layer).expect("RAW payload");
        let decoded = read_raw(&payload).expect("valid RAW payload");
        assert_eq!(decoded.source.as_ref(), b"raw camera bytes");
        assert_eq!(decoded.settings, settings);
    }

    #[test]
    fn malformed_raw_payload_is_ignored() {
        let mut layer = Layer::new_raster("capture");
        layer.raw = Some(Box::new(RawDevelopment {
            source: Arc::from(&b"source"[..]),
            settings: RawSettings::default(),
            masks: Vec::new(),
        }));
        let payload = write_raw(&layer).unwrap();

        assert!(read_raw(&payload[..payload.len() - 1]).is_none());
        let mut trailing = payload.clone();
        trailing.push(0);
        assert!(read_raw(&trailing).is_none());
        let mut nan = payload;
        nan[8..12].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(read_raw(&nan).is_none());
    }

    fn masked_layer() -> Layer {
        use schist_core::{BrushStroke, DetectedKind, MaskCombine, MaskComponent};
        let mut layer = Layer::new_raster("capture");
        let mut sky = LocalMask::new(MaskShape::Detected {
            kind: DetectedKind::Sky,
            raster: Some(Arc::new(
                MaskRaster::from_coverage(3, 2, &[1.0, 0.5, 0.0, 0.25, 0.75, 1.0]).unwrap(),
            )),
        });
        sky.adjustments.exposure = -0.75;
        sky.adjustments.dehaze = 30.0;
        sky.components.push(MaskComponent {
            shape: MaskShape::default_linear(),
            combine: MaskCombine::Intersect,
            invert: false,
        });
        let mut brushed = LocalMask::new(MaskShape::Brush {
            strokes: vec![BrushStroke {
                erase: false,
                size: 0.05,
                feather: 40.0,
                flow: 60.0,
                points: vec![[0.1, 0.2], [0.4, 0.45]],
            }],
        });
        brushed.invert = true;
        brushed.adjustments.saturation = -20.0;
        // Not yet detected: saved as a recipe and detected again later.
        brushed.components.push(MaskComponent {
            shape: MaskShape::Detected {
                kind: DetectedKind::Subject,
                raster: None,
            },
            combine: MaskCombine::Subtract,
            invert: true,
        });
        layer.raw = Some(Box::new(RawDevelopment {
            source: Arc::from(&b"raw camera bytes"[..]),
            settings: RawSettings::default(),
            masks: vec![sky, brushed],
        }));
        layer
    }

    #[test]
    fn masks_round_trip_with_their_detections() {
        let layer = masked_layer();
        let payload = write_masks(&layer).expect("masks payload");
        let decoded = read_masks(&payload).expect("valid masks payload");
        assert_eq!(decoded, layer.raw.as_ref().unwrap().masks);
        assert!(write_masks(&Layer::new_raster("plain")).is_none());
    }

    #[test]
    fn malformed_masks_payload_is_ignored() {
        let payload = write_masks(&masked_layer()).unwrap();
        assert!(read_masks(&payload[..payload.len() - 1]).is_none());
        let mut trailing = payload.clone();
        trailing.push(0);
        assert!(read_masks(&trailing).is_none());
        let mut version = payload;
        version[3] = 9;
        assert!(read_masks(&version).is_none());
    }

    #[test]
    fn grading_round_trips_in_its_own_block() {
        let mut layer = Layer::new_raster("capture");
        let mut settings = RawSettings::default();
        layer.raw = Some(Box::new(RawDevelopment {
            source: Arc::from(&b"source"[..]),
            settings,
            masks: Vec::new(),
        }));
        assert!(
            write_grading(&layer).is_none(),
            "default grading is implied"
        );
        settings.grading.shadows = GradeWheel {
            hue: 210.0,
            saturation: 35.0,
            luminance: -12.0,
        };
        settings.grading.global.saturation = 8.0;
        settings.grading.blending = 72.0;
        settings.grading.balance = -20.0;
        layer.raw.as_mut().unwrap().settings = settings;
        let payload = write_grading(&layer).expect("grading payload");
        assert_eq!(read_grading(&payload), Some(settings.grading));

        assert!(read_grading(&payload[..payload.len() - 1]).is_none());
        let mut trailing = payload.clone();
        trailing.push(0);
        assert!(read_grading(&trailing).is_none());
        let mut newer = payload.clone();
        newer[3] = 2;
        assert!(read_grading(&newer).is_none());
        let mut nan = payload;
        nan[4..8].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(read_grading(&nan).is_none());
    }
}
