//! The Color Lookup adjustment: a loaded LUT file and how to feed it.
//!
//! The table travels with the layer. Its original file bytes are kept,
//! both so a PSD can embed them the way Photoshop does and so nothing is
//! lost to re-sampling; in the layer's JSON they are zlib-compressed and
//! base64-encoded, and the parsed table is shared behind an `Arc`, so
//! cloning parameters for a preview or a history entry costs nothing.

use super::*;
use crate::lut::{parse_3dl, parse_cube, Lut, Lut3d, LutError, MAX_FILE_BYTES};
use base64::Engine as _;
use std::sync::Arc;

/// The file format a table was loaded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LutFormat {
    #[serde(rename = "cube")]
    Cube,
    #[serde(rename = "3dl")]
    ThreeDl,
}

impl LutFormat {
    /// From a file extension, case-insensitively.
    pub fn from_extension(ext: &str) -> Option<LutFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "cube" => Some(LutFormat::Cube),
            "3dl" => Some(LutFormat::ThreeDl),
            _ => None,
        }
    }
}

/// How document values are presented to the table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LutInput {
    /// The document's own encoded values, as Photoshop applies a LUT.
    #[default]
    Document,
    /// Linear light: sRGB-decoded before the lookup and re-encoded after,
    /// for tables built for scene-linear data.
    Linear,
}

impl LutInput {
    pub const ALL: [LutInput; 2] = [LutInput::Document, LutInput::Linear];

    pub fn index(self) -> usize {
        match self {
            LutInput::Document => 0,
            LutInput::Linear => 1,
        }
    }
}

struct TableInner {
    format: LutFormat,
    /// Shared rather than owned: the browser keeps picked files in an
    /// `Arc` of its own, and a large table should not be copied twice.
    source: Arc<Vec<u8>>,
    /// `source`, zlib-compressed and base64-encoded: what the JSON holds.
    packed: String,
    lut: Lut,
}

/// A parsed LUT plus the file it came from. Cheap to clone.
#[derive(Clone)]
pub struct LutTable(Arc<TableInner>);

impl LutTable {
    /// Parse a LUT file's bytes.
    pub fn load(format: LutFormat, source: impl Into<Arc<Vec<u8>>>) -> Result<LutTable, LutError> {
        let source = source.into();
        let lut = match format {
            LutFormat::Cube => parse_cube(&source)?,
            LutFormat::ThreeDl => parse_3dl(&source)?,
        };
        let packed = base64::engine::general_purpose::STANDARD
            .encode(miniz_oxide::deflate::compress_to_vec_zlib(&source, 6));
        Ok(LutTable(Arc::new(TableInner {
            format,
            source,
            packed,
            lut,
        })))
    }

    /// A table made in Schist rather than loaded, stored as a `.cube`.
    pub fn from_cube(title: &str, cube: &Lut3d) -> LutTable {
        LutTable::load(LutFormat::Cube, crate::lut::write_cube(title, cube))
            .expect("a written cube parses")
    }

    pub fn format(&self) -> LutFormat {
        self.0.format
    }

    /// The original file bytes.
    pub fn source(&self) -> &[u8] {
        &self.0.source
    }

    pub fn lut(&self) -> &Lut {
        &self.0.lut
    }
}

impl PartialEq for LutTable {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.format == other.0.format && *self.0.source == *other.0.source)
    }
}

impl std::fmt::Debug for LutTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LutTable")
            .field("format", &self.0.format)
            .field("bytes", &self.0.source.len())
            .field("size", &self.0.lut.size_label())
            .finish()
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PackedTable<'a> {
    format: LutFormat,
    #[serde(borrow)]
    zlib: std::borrow::Cow<'a, str>,
}

impl serde::Serialize for LutTable {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        PackedTable {
            format: self.0.format,
            zlib: std::borrow::Cow::Borrowed(&self.0.packed),
        }
        .serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for LutTable {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let packed = PackedTable::deserialize(d)?;
        let compressed = base64::engine::general_purpose::STANDARD
            .decode(packed.zlib.as_bytes())
            .map_err(D::Error::custom)?;
        let source =
            miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&compressed, MAX_FILE_BYTES)
                .map_err(|e| D::Error::custom(format!("LUT data: {e:?}")))?;
        LutTable::load(packed.format, source).map_err(D::Error::custom)
    }
}

/// Settings of a Color Lookup layer.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ColorLookup {
    /// The table's name as the layer shows it: normally its file name.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub input: LutInput,
    /// Nothing loaded yet: the layer leaves pixels alone.
    #[serde(default)]
    pub table: Option<LutTable>,
}

impl ColorLookup {
    pub fn apply(&self, px: Rgba) -> Rgba {
        let Some(table) = &self.table else {
            return px;
        };
        let rgb = [px.r, px.g, px.b];
        let [r, g, b] = match self.input {
            // The table's domain clamps out-of-range input itself.
            LutInput::Document => table.lut().apply(rgb),
            LutInput::Linear => table.lut().apply(rgb.map(srgb_decode)).map(srgb_encode),
        };
        let clamp = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        Rgba {
            r: clamp(r),
            g: clamp(g),
            b: clamp(b),
            a: px.a,
        }
    }

    /// Shader coefficients: the input mode, the shaper's and the lattice's
    /// headers, then both tables, so the GPU walks exactly the data the
    /// CPU does instead of a resampled copy.
    pub(crate) fn coeffs(&self) -> Vec<f32> {
        let mut out = vec![self.input.index() as f32];
        let lut = self.table.as_ref().map(LutTable::lut);
        let shaper = lut.and_then(|l| l.shaper.as_ref());
        let cube = lut.and_then(|l| l.cube.as_ref());
        out.push(shaper.map_or(0.0, |s| s.size() as f32));
        out.extend(shaper.map_or([0.0; 3], |s| s.domain_min));
        out.extend(shaper.map_or([1.0; 3], |s| s.domain_max));
        out.push(cube.map_or(0.0, |c| c.size as f32));
        out.extend(cube.map_or([0.0; 3], |c| c.domain_min));
        out.extend(cube.map_or([1.0; 3], |c| c.domain_max));
        if let Some(shaper) = shaper {
            out.extend(shaper.table.iter().flatten());
        }
        if let Some(cube) = cube {
            out.extend(cube.table.iter().flatten());
        }
        out
    }

    /// The whole adjustment sampled on a fresh `size`³ lattice, input
    /// handling included. Used where only a plain `.cube` will do.
    pub fn baked(&self, size: usize) -> Lut3d {
        let mut cube = Lut3d::identity(size);
        for v in &mut cube.table {
            let out = self.apply(Rgba::new(v[0], v[1], v[2], 1.0));
            *v = [out.r, out.g, out.b];
        }
        cube
    }
}

fn srgb_decode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_encode(v: f32) -> f32 {
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

// ---- PSD `clrL` ----
//
// Photoshop's Color Lookup block is a u16 version (1), a u32 descriptor
// version (16) and a descriptor. The field names below follow the public,
// MIT-licensed ag-psd implementation (`additionalInfo.ts`, `clrL`) and
// what our own files contain; no Adobe headers or SDK sources were used.
// Only embedded 3D LUT files in `.cube` or `.3dl` form are rendered. The
// other lookup types (abstract and device-link ICC profiles) and the
// `.look` format parse as unsupported, so their bytes stay preserved and
// the layer renders as a no-op.

pub(crate) fn parse_clrl(raw: &[u8]) -> Params {
    if crate::psd::be_u16(raw, 0) != Some(1) {
        return Params::Unsupported;
    }
    let Some(d) = descriptor::parse_versioned(raw) else {
        return Params::Unsupported;
    };
    let enum_value = |key: &str| match d.get(key) {
        Some(descriptor::Value::Enum(_, v)) => Some(v.as_str()),
        _ => None,
    };
    if enum_value("lookupType").is_some_and(|t| t != "3DLUT") {
        return Params::Unsupported;
    }
    // The order flags only describe embedded profile tables in the files
    // we have seen; a blue-first order is something we cannot vouch for.
    if ["dataOrder", "tableOrder"]
        .iter()
        .any(|k| enum_value(k).is_some_and(|v| v != "rgbOrder"))
    {
        return Params::Unsupported;
    }
    let format = match enum_value("LUTFormat") {
        Some("LUTFormatCUBE") | None => LutFormat::Cube,
        Some("LUTFormat3DL") => LutFormat::ThreeDl,
        Some(_) => return Params::Unsupported,
    };
    let Some(bytes) = d.get("LUT3DFileData").and_then(|v| v.as_raw()) else {
        return Params::Unsupported;
    };
    let Ok(table) = LutTable::load(format, bytes.to_vec()) else {
        return Params::Unsupported;
    };
    let name = d
        .get("Nm  ")
        .and_then(|v| v.as_text())
        .or_else(|| d.get("LUT3DFileName").and_then(|v| v.as_text()))
        .unwrap_or_default()
        .to_string();
    Params::ColorLookup(ColorLookup {
        name,
        input: LutInput::Document,
        table: Some(table),
    })
}

/// The `clrL` payload. Photoshop has no input-space setting, so a table
/// used on linear light is written baked into a plain 33³ `.cube`, which
/// renders the same in any reader; Schist then reopens it as that table.
pub(crate) fn encode_clrl(lookup: &ColorLookup) -> Option<Vec<u8>> {
    let table = lookup.table.as_ref()?;
    let (format, bytes) = match lookup.input {
        LutInput::Document => (table.format(), table.source().to_vec()),
        LutInput::Linear => (
            LutFormat::Cube,
            crate::lut::write_cube(&lookup.name, &lookup.baked(33)),
        ),
    };
    let mut b = descriptor::Builder::new("null");
    b.enumerated("lookupType", "colorLookupType", "3DLUT")
        .text("Nm  ", &lookup.name)
        .bool("Dthr", false)
        .enumerated(
            "LUTFormat",
            "LUTFormatType",
            match format {
                LutFormat::Cube => "LUTFormatCUBE",
                LutFormat::ThreeDl => "LUTFormat3DL",
            },
        )
        .enumerated("dataOrder", "colorLookupOrder", "rgbOrder")
        .enumerated("tableOrder", "colorLookupOrder", "rgbOrder")
        .raw("LUT3DFileData", &bytes)
        .text("LUT3DFileName", &lookup.name);
    let mut out = 1u16.to_be_bytes().to_vec();
    out.extend_from_slice(&b.finish_versioned());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invert_cube() -> LutTable {
        let mut cube = Lut3d::identity(9);
        for v in &mut cube.table {
            *v = v.map(|c| 1.0 - c);
        }
        LutTable::from_cube("invert", &cube)
    }

    #[test]
    fn json_round_trip_keeps_the_table_and_is_compact() {
        let mut cube = Lut3d::identity(33);
        for v in &mut cube.table {
            *v = [v[0] * 0.9, v[1].powf(0.8), v[2]];
        }
        let lookup = ColorLookup {
            name: "warm.cube".into(),
            input: LutInput::Linear,
            table: Some(LutTable::from_cube("warm", &cube)),
        };
        let params = Params::ColorLookup(lookup);
        let json = serde_json::to_string(&params).unwrap();
        let source = match &params {
            Params::ColorLookup(l) => l.table.as_ref().unwrap().source().len(),
            _ => unreachable!(),
        };
        assert!(json.len() < source, "{} vs {source}", json.len());
        let back: Params = serde_json::from_str(&json).unwrap();
        assert_eq!(back, params);
    }

    #[test]
    fn corrupt_json_tables_are_errors_not_panics() {
        for json in [
            r#"{"ColorLookup":{"table":{"format":"cube","zlib":"!!"}}}"#,
            r#"{"ColorLookup":{"table":{"format":"cube","zlib":"AAAA"}}}"#,
            r#"{"ColorLookup":{"table":{"format":"lut","zlib":""}}}"#,
        ] {
            assert!(serde_json::from_str::<Params>(json).is_err(), "{json}");
        }
        // No table at all is a valid, inert layer.
        let empty: Params = serde_json::from_str(r#"{"ColorLookup":{}}"#).unwrap();
        let px = Rgba::new(0.3, 0.6, 0.9, 0.5);
        assert_eq!(empty.apply(px), px);
    }

    #[test]
    fn linear_input_wraps_the_table_in_srgb_decoding() {
        let identity = ColorLookup {
            name: String::new(),
            input: LutInput::Linear,
            table: Some(LutTable::from_cube("", &Lut3d::identity(17))),
        };
        let px = Rgba::new(0.2, 0.5, 0.8, 1.0);
        let out = identity.apply(px);
        assert!(
            (out.r - 0.2).abs() < 2e-3 && (out.b - 0.8).abs() < 2e-3,
            "{out:?}"
        );

        let invert = ColorLookup {
            input: LutInput::Linear,
            table: Some(invert_cube()),
            ..Default::default()
        };
        // Mid-grey in sRGB is about 0.21 linear, inverted to 0.79, which
        // re-encodes to about 0.90: not the 0.5 a document-space inversion
        // would give.
        let out = invert.apply(Rgba::new(0.5, 0.5, 0.5, 1.0));
        assert!((out.r - 0.90).abs() < 0.01, "{out:?}");
    }

    #[test]
    fn clrl_round_trips_and_preserves_the_file() {
        let lookup = ColorLookup {
            name: "Invert.cube".into(),
            input: LutInput::Document,
            table: Some(invert_cube()),
        };
        let raw = encode_clrl(&lookup).unwrap();
        assert_eq!(&raw[..2], &[0, 1]);
        match parse_clrl(&raw) {
            Params::ColorLookup(back) => {
                assert_eq!(back, lookup);
                assert_eq!(
                    back.table.unwrap().source(),
                    lookup.table.as_ref().unwrap().source()
                );
            }
            other => panic!("{other:?}"),
        }
        // Linear input is baked for other readers and reopens as a table
        // that renders the same.
        let linear = ColorLookup {
            input: LutInput::Linear,
            ..lookup
        };
        let back = parse_clrl(&encode_clrl(&linear).unwrap());
        for px in [Rgba::new(0.5, 0.5, 0.5, 1.0), Rgba::new(0.1, 0.7, 0.3, 1.0)] {
            let (a, b) = (linear.apply(px), back.apply(px));
            assert!(
                (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01,
                "{a:?} {b:?}"
            );
        }
    }

    #[test]
    fn unrenderable_clrl_blocks_are_unsupported() {
        let mut b = descriptor::Builder::new("null");
        b.enumerated("lookupType", "colorLookupType", "abstractProfile")
            .raw("profile", b"not a lut");
        let mut raw = 1u16.to_be_bytes().to_vec();
        raw.extend_from_slice(&b.finish_versioned());
        assert_eq!(parse_clrl(&raw), Params::Unsupported);
        assert_eq!(parse_clrl(&[]), Params::Unsupported);
        assert_eq!(parse_clrl(&[0, 2, 0, 0, 0, 16]), Params::Unsupported);

        let mut b = descriptor::Builder::new("null");
        b.enumerated("LUTFormat", "LUTFormatType", "LUTFormatCUBE")
            .raw("LUT3DFileData", b"LUT_3D_SIZE 2\n0 0 0\n");
        let mut raw = 1u16.to_be_bytes().to_vec();
        raw.extend_from_slice(&b.finish_versioned());
        assert_eq!(parse_clrl(&raw), Params::Unsupported);
    }
}
