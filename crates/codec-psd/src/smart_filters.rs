//! A deliberately bounded bridge between Schist filter stacks and PSD smart
//! filters. The descriptor/linked-file layout follows the public ag-psd reader
//! and writer (MIT), not SDK headers. Original unsupported blocks stay opaque.
//!
//! https://github.com/Agamnentzar/ag-psd/blob/master/src/additionalInfo.ts
//! (`SoLd`, `createLnkHandler`, `serializeFilterFXItem`).

use schist_color::{ColorMode, Depth};
use schist_core::{
    blit_rgba_f32,
    filter_stack::{self, FilterEffect, FilterStack},
    Affine, Document, IntRect, Layer, RawBlock, TileMap,
};
use schist_psd_descriptor::{parse, parse_prefix, Builder, Descriptor, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Marks a native placement which Schist may regenerate, or remove on bake.
/// Kept distinct from ScFs/ScFo so removing a recipe cannot resurrect SoLd.
pub(crate) const OWNED_KEY: [u8; 4] = *b"ScFi";
const MAX_PIXELS: u64 = 16_000_000;
const MAX_EMBEDDED_BYTES: usize = 512 * 1024 * 1024;

pub(crate) fn owns_native(layer: &Layer) -> bool {
    layer
        .extras
        .iter()
        .any(|b| b.key == OWNED_KEY && b.data == [1])
}

fn is_placement(key: &[u8; 4]) -> bool {
    matches!(key, b"SoLd" | b"SoLE" | b"PlLd")
}

fn export_stack(layer: &Layer, doc: &Document) -> Option<(FilterStack, TileMap, String, Affine)> {
    if doc.mode != ColorMode::Rgb || layer.as_raster().is_none() {
        return None;
    }
    if layer.extras.iter().any(|b| is_placement(&b.key)) && !owns_native(layer) {
        return None;
    }
    let metadata_bytes = &layer
        .extras
        .iter()
        .find(|b| b.key == filter_stack::STACK_KEY)?
        .data;
    if metadata_bytes.len() > 1024 * 1024 {
        return None;
    }
    let mut metadata: serde_json::Value = serde_json::from_slice(metadata_bytes).ok()?;
    // Version 2 is the independently introduced raster-placement extension.
    // Parse that documented extension explicitly so this codec also works when
    // built against the pre-placement core during separate PR review.
    let placement = metadata.get("placement").filter(|v| !v.is_null());
    let matrix = if let Some(smart) = layer.smart.as_deref() {
        smart.transform
    } else if let Some(placement) = placement {
        if metadata.get("version")?.as_u64()? != 2 {
            return None;
        }
        if !matches!(
            placement.get("filter")?.as_str()?,
            "Nearest" | "Bilinear" | "Bicubic"
        ) {
            return None;
        }
        let matrix = placement.get("matrix")?;
        let n = |key| matrix.get(key)?.as_f64().map(|v| v as f32);
        Affine {
            a: n("a")?,
            b: n("b")?,
            c: n("c")?,
            d: n("d")?,
            tx: n("tx")?,
            ty: n("ty")?,
        }
    } else {
        Affine::IDENTITY
    };
    if ![matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
        .into_iter()
        .all(f32::is_finite)
        || (matrix.a as f64 * matrix.d as f64 - matrix.b as f64 * matrix.c as f64).abs() < 1e-10
    {
        return None;
    }
    if metadata.get("version")?.as_u64()? == 2 {
        metadata["version"] = 1.into();
        metadata.as_object_mut()?.remove("placement");
    }
    let stack: FilterStack = serde_json::from_value(metadata).ok()?;
    stack.validate().ok()?;
    placed_corners(stack.region, matrix)?;
    if stack.effects.is_empty()
        || stack.region.width() as u32 > crate::writer::PSB_MAX_DIM
        || stack.region.height() as u32 > crate::writer::PSB_MAX_DIM
        || stack.region.width() as u64 * stack.region.height() as u64 > MAX_PIXELS
        || stack.effects.iter().any(|f| encode_effect(f).is_none())
    {
        return None;
    }
    let source = filter_stack::read_source(layer).ok()?;
    let bounds = source.content_bounds();
    // A region-limited stack can leave pixels outside the region untouched.
    // Cropping them into a smart object's source would change its appearance.
    if source.mode() != ColorMode::Rgb
        || bounds.left < stack.region.left
        || bounds.top < stack.region.top
        || bounds.right > stack.region.right
        || bounds.bottom > stack.region.bottom
    {
        return None;
    }
    let id = source_id(layer, stack.region, doc)?;
    Some((stack, source, id, matrix))
}

fn placed_corners(r: IntRect, matrix: Affine) -> Option<[f64; 8]> {
    let mut corners = [
        r.left as f64,
        r.top as f64,
        r.right as f64,
        r.top as f64,
        r.right as f64,
        r.bottom as f64,
        r.left as f64,
        r.bottom as f64,
    ];
    for corner in corners.as_chunks_mut::<2>().0 {
        let (x, y) = (corner[0], corner[1]);
        corner[0] = matrix.a as f64 * x + matrix.c as f64 * y + matrix.tx as f64;
        corner[1] = matrix.b as f64 * x + matrix.d as f64 * y + matrix.ty as f64;
    }
    corners
        .iter()
        .all(|v| v.is_finite() && v.abs() < i32::MAX as f64)
        .then_some(corners)
}

/// Content-derived GUIDs let unchanged sources reuse their preserved lnk2
/// entries; new source bytes never accidentally resolve to an old embedding.
fn source_id(layer: &Layer, bounds: IntRect, doc: &Document) -> Option<String> {
    let source = &layer
        .extras
        .iter()
        .find(|b| b.key == filter_stack::SOURCE_KEY)?
        .data;
    let mut a = 0xcbf29ce484222325u64;
    let mut b = 0x84222325cbf29ce4u64;
    let geometry: Vec<u8> = [
        bounds.left,
        bounds.top,
        bounds.right,
        bounds.bottom,
        doc.depth.bytes_per_channel() as i32,
    ]
    .into_iter()
    .flat_map(i32::to_be_bytes)
    .collect();
    for &v in source
        .iter()
        .chain(geometry.iter())
        .chain(doc.resolution_dpi.to_bits().to_be_bytes().iter())
        .chain(doc.icc_profile.as_deref().unwrap_or_default().iter())
    {
        a = (a ^ v as u64).wrapping_mul(0x100000001b3);
        b = (b ^ v as u64).wrapping_mul(0x100000001b3).rotate_left(7);
    }
    Some(format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0xfff,
        b >> 52,
        b & 0xffffffffffff
    ))
}

/// Native placed-object descriptor, including editable filterFX recipes.
/// The caller writes ScFi beside it and removes stale owned placement blocks.
pub(crate) fn write_placed(layer: &Layer, doc: &Document) -> Option<Vec<u8>> {
    let (stack, _, id, matrix) = export_stack(layer, doc)?;
    let r = stack.region;
    let (w, h) = (r.width() as f64, r.height() as f64);
    let corners = placed_corners(r, matrix)?;
    let mut desc = Builder::new("null");
    desc.text("Idnt", &id)
        .text("placed", &id)
        .integer("PgNm", 1)
        .integer("totalPages", 1)
        .object("frameStep", fraction())
        .object("duration", fraction())
        .integer("frameCount", 0)
        .integer("Annt", 16)
        .integer("Type", 2)
        .doubles("Trnf", &corners)
        .doubles("nonAffineTransform", &corners);
    let mut bounds = Builder::new("Rctn");
    bounds
        .pixels("Top ", 0.0)
        .pixels("Left", 0.0)
        .pixels("Btom", h)
        .pixels("Rght", w);
    let mut warp = Builder::new("warp");
    warp.enumerated("warpStyle", "warpStyle", "warpNone")
        .double("warpValue", 0.0)
        .double("warpPerspective", 0.0)
        .double("warpPerspectiveOther", 0.0)
        .enumerated("warpRotate", "Ornt", "Hrzn")
        .object("bounds", bounds)
        .integer("uOrder", 0)
        .integer("vOrder", 0);
    let mut size = Builder::new("Pnt ");
    size.double("Wdth", w).double("Hght", h);
    desc.object("warp", warp)
        .object("Sz  ", size)
        .unit("Rslt", "#Rsl", doc.resolution_dpi as f64);
    let mut fx = Builder::new("filterFXStyle");
    fx.bool("enab", true)
        .bool("validAtPosition", true)
        .bool("filterMaskEnable", false)
        .bool("filterMaskLinked", false)
        .bool("filterMaskExtendWithWhite", true)
        .object_list(
            "filterFXList",
            stack
                .effects
                .iter()
                .rev()
                .map(encode_effect)
                .collect::<Option<_>>()?,
        );
    // PSD lists the topmost (last-applied) smart filter first.
    desc.object("filterFX", fx);
    let mut out = b"soLD".to_vec();
    out.extend_from_slice(&4u32.to_be_bytes());
    out.extend_from_slice(&desc.finish_versioned());
    Some(out)
}

fn fraction() -> Builder {
    let mut b = Builder::new("null");
    b.integer("numerator", 0).integer("denominator", 600);
    b
}

fn parameter(effect: &FilterEffect, key: &str, min: f64, max: f64) -> Option<f64> {
    let value = *effect.values.get(key)? as f64;
    (value.is_finite() && (min..=max).contains(&value)).then_some(value)
}

fn encode_effect(effect: &FilterEffect) -> Option<Builder> {
    let (filter, id, keys): (Builder, i32, &[&str]) = match effect.id.as_str() {
        "filter.gaussian_blur" => {
            let mut filter = Builder::new("GsnB");
            filter.pixels("Rds ", parameter(effect, "radius", 0.0, 100.0)?);
            (filter, 1198747202, &["radius"])
        }
        "filter.box_blur" => {
            let mut filter = Builder::new("boxblur");
            filter.pixels("Rds ", parameter(effect, "radius", 0.0, 100.0)?.round());
            (filter, 697, &["radius"])
        }
        "filter.motion_blur" => {
            let mut filter = Builder::new("MtnB");
            let angle = parameter(effect, "angle", -180.0, 180.0)?;
            // Native Motion Blur's angle is a signed integer descriptor value.
            if angle.fract() != 0.0 {
                return None;
            }
            // Photoshop expresses the angle counterclockwise; Schist's image
            // coordinates increase downward, hence the sign change.
            filter
                .pixels("Dstn", parameter(effect, "distance", 1.0, 200.0)?.round())
                .integer("Angl", -angle as i32);
            (filter, 1299476034, &["distance", "angle"])
        }
        "filter.median" => {
            let mut filter = Builder::new("Mdn ");
            filter.pixels("Rds ", parameter(effect, "radius", 1.0, 10.0)?.round());
            (filter, 1298427424, &["radius"])
        }
        "filter.high_pass" => {
            let mut filter = Builder::new("HghP");
            filter.pixels("Rds ", parameter(effect, "radius", 0.1, 250.0)?);
            (filter, 1214736464, &["radius"])
        }
        "filter.unsharp_mask" | "filter.sharpen" => {
            let simple = effect.id == "filter.sharpen";
            let radius = if simple {
                1.0
            } else {
                parameter(effect, "radius", 0.1, 50.0)?
            };
            let amount = parameter(effect, "amount", 0.0, if simple { 300.0 } else { 500.0 })?;
            let threshold = if simple {
                0.0
            } else {
                parameter(effect, "threshold", 0.0, 255.0)?
            };
            if threshold.fract() != 0.0 {
                return None;
            }
            let mut filter = Builder::new("UnsM");
            filter
                .pixels("Rds ", radius)
                .percent("Amnt", amount)
                .integer("Thsh", threshold as i32);
            (
                filter,
                1433301837,
                if simple {
                    &["amount"]
                } else {
                    &["radius", "amount", "threshold"]
                },
            )
        }
        _ => return None,
    };
    if effect.values.keys().any(|k| !keys.contains(&k.as_str())) {
        return None;
    }
    let name = match effect.id.as_str() {
        "filter.gaussian_blur" => schist_i18n::t("filter.gaussian_blur.name"),
        "filter.box_blur" => schist_i18n::t("filter.box_blur.name"),
        "filter.motion_blur" => schist_i18n::t("filter.motion_blur.name"),
        "filter.median" => schist_i18n::t("filter.median.name"),
        "filter.high_pass" => schist_i18n::t("filter.high_pass.name"),
        "filter.sharpen" => schist_i18n::t("filter.sharpen.name"),
        _ => schist_i18n::t("filter.unsharp_mask.name"),
    };
    let mut blend = Builder::new("blendOptions");
    blend
        .percent("Opct", 100.0)
        .enumerated("Md  ", "BlnM", "Nrml");
    let mut item = Builder::new("filterFX");
    item.text("Nm  ", name)
        .object("blendOptions", blend)
        .bool("enab", effect.enabled)
        .bool("hasoptions", true)
        .color(
            "FrgC",
            effect.foreground[0] as f64 * 255.0,
            effect.foreground[1] as f64 * 255.0,
            effect.foreground[2] as f64 * 255.0,
        )
        .color(
            "BckC",
            effect.background[0] as f64 * 255.0,
            effect.background[1] as f64 * 255.0,
            effect.background[2] as f64 * 255.0,
        )
        .object("Fltr", filter)
        .integer("filterID", id);
    Some(item)
}

/// Additional embedded sources for native placements. Existing opaque lnk2
/// blocks remain untouched, including entries no longer referenced after bake.
pub(crate) fn write_links(doc: &Document) -> Option<RawBlock> {
    let mut known: HashSet<String> = linked_sources(&doc.preserved_layer_info)
        .into_keys()
        .collect();
    let mut data = Vec::new();
    for layer in doc.tree.iter() {
        let Some((stack, source, id, _)) = export_stack(layer, doc) else {
            continue;
        };
        if !known.insert(id.clone()) {
            continue;
        }
        let r = stack.region;
        let mut embedded =
            Document::new(&layer.name, r.width() as u32, r.height() as u32, doc.depth);
        embedded.resolution_dpi = doc.resolution_dpi;
        embedded.icc_profile = doc.icc_profile.clone();
        let mut src_layer = Layer::new_raster(&layer.name);
        let mut rgba = Vec::with_capacity(r.width() as usize * r.height() as usize * 4);
        for y in r.top..r.bottom {
            for x in r.left..r.right {
                let p = source.pixel(x, y);
                rgba.extend_from_slice(&[p.r, p.g, p.b, p.a]);
            }
        }
        blit_rgba_f32(
            &mut src_layer.as_raster_mut()?.tiles,
            doc.depth,
            embedded.canvas_rect(),
            &rgba,
        );
        embedded.push_layer(src_layer);
        let bytes = crate::writer::write_psd(&embedded).ok()?;
        let mut entry = b"liFD".to_vec();
        entry.extend_from_slice(&2u32.to_be_bytes());
        entry.push(id.len() as u8);
        entry.extend_from_slice(id.as_bytes());
        unicode(&mut entry, &format!("{}.psd", layer.name));
        entry.extend_from_slice(b"8BPS8BIM");
        entry.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        entry.push(0); // no file-open descriptor
        entry.extend_from_slice(&bytes);
        data.extend_from_slice(&(entry.len() as u64).to_be_bytes());
        data.extend_from_slice(&entry);
        data.resize((data.len() + 3) & !3, 0);
    }
    if data.is_empty() {
        return None;
    }
    // Tagged-block readers commonly retain one block per key. Merge existing
    // entries so adding a source cannot hide earlier objects in those readers.
    let mut merged = Vec::new();
    for block in doc
        .preserved_layer_info
        .iter()
        .filter(|b| b.key == *b"lnk2")
    {
        merged.extend_from_slice(&block.data);
        merged.resize((merged.len() + 3) & !3, 0);
    }
    merged.extend_from_slice(&data);
    Some(RawBlock {
        key: *b"lnk2",
        data: merged,
    })
}

fn unicode(out: &mut Vec<u8>, value: &str) {
    let units: Vec<_> = value.encode_utf16().chain(Some(0)).collect();
    out.extend_from_slice(&(units.len() as u32).to_be_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_be_bytes());
    }
}

/// Promote only complete, unmasked, normal-blend stacks with integer translated
/// PSD/PSB sources. Cached layer pixels remain the authority until a user edits.
pub(crate) fn import_document(doc: &mut Document) {
    if doc.mode != ColorMode::Rgb {
        return;
    }
    let linked = linked_sources(&doc.preserved_layer_info);
    let mut promoted = Vec::new();
    for layer in doc.tree.iter() {
        if filter_stack::has_stack(layer) || layer.as_raster().is_none() {
            continue;
        }
        let Some((stack, id)) = read_placement(layer) else {
            continue;
        };
        let Some(bytes) = linked.get(&id) else {
            continue;
        };
        let Some(source) =
            decode_embedded_source(bytes, stack.region, doc.depth, doc.icc_profile.as_deref())
        else {
            continue;
        };
        if let Ok(mut extras) = stack.blocks(layer, &source) {
            extras.retain(|b| b.key != OWNED_KEY);
            extras.push(RawBlock {
                key: OWNED_KEY,
                data: vec![1],
            });
            promoted.push((layer.id, extras));
        }
    }
    for (id, extras) in promoted {
        if let Some(layer) = doc.tree.find_mut(id) {
            layer.extras = extras;
        }
    }
}

fn decode_embedded_source(
    bytes: &[u8],
    region: IntRect,
    depth: Depth,
    parent_profile: Option<&[u8]>,
) -> Option<TileMap> {
    if bytes.len() > MAX_EMBEDDED_BYTES || bytes.get(..4)? != b"8BPS" {
        return None;
    }
    let h = u32::from_be_bytes(bytes.get(14..18)?.try_into().ok()?);
    let w = u32::from_be_bytes(bytes.get(18..22)?.try_into().ok()?);
    if w != region.width() as u32 || h != region.height() as u32 || w as u64 * h as u64 > MAX_PIXELS
    {
        return None;
    }
    // Parsing nested source files never recursively promotes their smart objects.
    let src = crate::reader::read_psd_without_native_filters(bytes).ok()?;
    if src.mode != ColorMode::Rgb
        || src.depth != depth
        || src.icc_profile.as_deref().unwrap_or_default() != parent_profile.unwrap_or_default()
    {
        return None;
    }
    let pixels = schist_compositor::composite_region_f32_cpu(&src, src.canvas_rect());
    let mut result = TileMap::new();
    blit_rgba_f32(&mut result, depth, region, &pixels);
    Some(result)
}

fn read_placement(layer: &Layer) -> Option<(FilterStack, String)> {
    // FEid/FXid carry filter masks/render metadata that this bridge cannot edit.
    if layer
        .extras
        .iter()
        .any(|b| matches!(&b.key, b"FEid" | b"FXid" | b"FMsk"))
    {
        return None;
    }
    let block = layer
        .extras
        .iter()
        .find(|b| matches!(&b.key, b"SoLd" | b"SoLE"))?;
    if block.data.len() > 1024 * 1024 || block.data.get(..4)? != b"soLD" {
        return None;
    }
    let version = u32::from_be_bytes(block.data.get(4..8)?.try_into().ok()?);
    if !matches!(version, 4 | 5) || block.data.get(8..12)? != 16u32.to_be_bytes() {
        return None;
    }
    let desc = parse(block.data.get(12..)?)?;
    if desc.number("Type") != Some(2.0) || desc.get("quiltWarp").is_some() {
        return None;
    }
    if desc.get("ClMg").is_some()
        || desc.get("Crop").is_some()
        || desc.number("PgNm").is_some_and(|v| v != 1.0)
        || desc.number("totalPages").is_some_and(|v| v != 1.0)
        || desc.number("comp").is_some_and(|v| v != -1.0)
        || desc
            .get("compInfo")
            .and_then(Value::as_object)
            .is_some_and(|info| info.number("compID").is_some_and(|v| v != -1.0))
    {
        return None;
    }
    let warp = desc.get("warp")?.as_object()?;
    if !matches!(warp.get("warpStyle"), Some(Value::Enum(t, v)) if t == "warpStyle" && v == "warpNone")
    {
        return None;
    }
    let size = desc.get("Sz  ")?.as_object()?;
    let (w, h) = (size.number("Wdth")?, size.number("Hght")?);
    if w < 1.0 || h < 1.0 || w.fract() != 0.0 || h.fract() != 0.0 || w * h > MAX_PIXELS as f64 {
        return None;
    }
    let corners: Vec<_> = desc
        .get("Trnf")?
        .as_list()?
        .iter()
        .map(Value::as_f64)
        .collect::<Option<_>>()?;
    if corners.len() != 8
        || corners
            .iter()
            .any(|v| !v.is_finite() || v.fract() != 0.0 || v.abs() > 1_000_000.0)
    {
        return None;
    }
    let (x, y) = (corners[0], corners[1]);
    if corners != [x, y, x + w, y, x + w, y + h, x, y + h] {
        return None;
    }
    if let Some(non_affine) = desc.get("nonAffineTransform") {
        let values: Vec<_> = non_affine
            .as_list()?
            .iter()
            .map(Value::as_f64)
            .collect::<Option<_>>()?;
        if values != corners {
            return None;
        }
    }
    let fx = desc.get("filterFX")?.as_object()?;
    if fx.get("filterMaskEnable").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let master_enabled = fx.get("enab")?.as_bool()?;
    let filters = fx.get("filterFXList")?.as_list()?;
    if filters.is_empty() || filters.len() > filter_stack::MAX_EFFECTS {
        return None;
    }
    let mut stack = FilterStack::new(IntRect::new(
        x as i32,
        y as i32,
        (x + w) as i32,
        (y + h) as i32,
    ));
    for entry in filters.iter().rev() {
        let mut effect = decode_effect(entry.as_object()?)?;
        effect.enabled &= master_enabled;
        stack.effects.push(effect);
    }
    stack.validate().ok()?;
    let id = desc.get("Idnt")?.as_text()?.to_owned();
    Some((stack, id))
}

fn decode_effect(item: &Descriptor) -> Option<FilterEffect> {
    let blend = item.get("blendOptions")?.as_object()?;
    if blend.number("Opct") != Some(100.0)
        || !matches!(blend.get("Md  "), Some(Value::Enum(t, v)) if t == "BlnM" && v == "Nrml")
    {
        return None;
    }
    let filter = item.get("Fltr")?.as_object()?;
    let (id, native_id, keys): (&str, f64, &[&str]) = match filter.class.as_str() {
        "GsnB" => ("filter.gaussian_blur", 1198747202.0, &["Rds "]),
        "boxblur" => ("filter.box_blur", 697.0, &["Rds "]),
        "MtnB" => ("filter.motion_blur", 1299476034.0, &["Dstn", "Angl"]),
        "Mdn " => ("filter.median", 1298427424.0, &["Rds "]),
        "HghP" => ("filter.high_pass", 1214736464.0, &["Rds "]),
        "UnsM" => (
            "filter.unsharp_mask",
            1433301837.0,
            &["Rds ", "Amnt", "Thsh"],
        ),
        _ => return None,
    };
    if item.number("filterID") != Some(native_id)
        || filter.items.keys().any(|key| !keys.contains(&key.as_str()))
    {
        return None;
    }
    let pixel = |key| match filter.get(key)? {
        Value::Unit(unit, v) if unit == "#Pxl" => Some(*v as f32),
        _ => None,
    };
    let mut values = if filter.class == "MtnB" {
        BTreeMap::from([
            ("distance".into(), pixel("Dstn")?),
            ("angle".into(), -filter.number("Angl")? as f32),
        ])
    } else {
        BTreeMap::from([("radius".to_string(), pixel("Rds ")?)])
    };
    if filter.class == "UnsM" {
        values.insert("amount".into(), filter.number("Amnt")? as f32);
        values.insert("threshold".into(), filter.number("Thsh")? as f32);
    }
    let effect = FilterEffect {
        id: id.into(),
        enabled: item.get("enab")?.as_bool()?,
        values,
        foreground: decode_color(item.get("FrgC")?.as_object()?)?,
        background: decode_color(item.get("BckC")?.as_object()?)?,
    };
    encode_effect(&effect)?; // applies the same parameter/domain bounds on import
    Some(effect)
}

fn decode_color(color: &Descriptor) -> Option<[f32; 4]> {
    if color.class != "RGBC" {
        return None;
    }
    let rgb = [
        color.number("Rd  ")?,
        color.number("Grn ")?,
        color.number("Bl  ")?,
    ];
    if rgb.iter().any(|v| !(0.0..=255.0).contains(v)) {
        return None;
    }
    Some([
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
        1.0,
    ])
}

/// Borrow only embedded data. Each record has its own length, so unsupported
/// versions and external references do not prevent later entries being read.
fn linked_sources(blocks: &[RawBlock]) -> HashMap<String, &[u8]> {
    let mut result = HashMap::new();
    for block in blocks
        .iter()
        .filter(|b| matches!(&b.key, b"lnk2" | b"lnk3" | b"lnkD"))
    {
        let mut cur = Cur {
            bytes: &block.data,
            at: 0,
        };
        while let Some(length) = cur.u64().and_then(|v| usize::try_from(v).ok()) {
            let Some(record) = cur.take(length) else {
                break;
            };
            if let Some((id, bytes)) = read_link(record) {
                result.entry(id).or_insert(bytes);
            }
            if cur.take((4 - length % 4) % 4).is_none() {
                break;
            }
        }
    }
    result
}

fn read_link(bytes: &[u8]) -> Option<(String, &[u8])> {
    let mut c = Cur { bytes, at: 0 };
    if c.take(4)? != b"liFD" {
        return None;
    }
    if !(1..=7).contains(&c.u32()?) {
        return None;
    }
    let id_len = c.take(1)?[0] as usize;
    let id = std::str::from_utf8(c.take(id_len)?).ok()?.to_owned();
    let name_len = c.u32()? as usize;
    c.take(name_len.checked_mul(2)?)?;
    c.take(8)?;
    let length = usize::try_from(c.u64()?).ok()?;
    if length > MAX_EMBEDDED_BYTES {
        return None;
    }
    if c.take(1)?[0] != 0 {
        if c.u32()? != 16 {
            return None;
        }
        let (_, used) = parse_prefix(c.bytes.get(c.at..)?)?;
        c.take(used)?;
    }
    Some((id, c.take(length)?))
}

struct Cur<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cur<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(length)?;
        let bytes = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_record_lengths_and_truncated_inputs_are_bounded() {
        let file = include_bytes!("../tests/fixtures/native-smart-filters-ag-psd.psd");
        let doc = crate::reader::read_psd_without_native_filters(file).unwrap();
        let block = doc
            .preserved_layer_info
            .iter()
            .find(|b| b.key == *b"lnk2")
            .unwrap();
        assert_eq!(linked_sources(std::slice::from_ref(block)).len(), 1);
        for cut in 0..block.data.len() {
            let truncated = RawBlock {
                key: *b"lnk2",
                data: block.data[..cut].to_vec(),
            };
            let _ = linked_sources(&[truncated]);
        }
        let oversized = RawBlock {
            key: *b"lnk2",
            data: u64::MAX.to_be_bytes().to_vec(),
        };
        assert!(linked_sources(&[oversized]).is_empty());
    }

    #[test]
    fn unsupported_masks_and_effects_do_not_partially_import() {
        let file = include_bytes!("../tests/fixtures/native-smart-filters-ag-psd.psd");
        let doc = crate::reader::read_psd_without_native_filters(file).unwrap();
        let layer = &doc.tree.layers[0];
        assert!(read_placement(layer).is_some());
        for (needle, replacement) in [
            (
                b"filterMaskEnablebool\0".as_slice(),
                b"filterMaskEnablebool\x01".as_slice(),
            ),
            (b"GsnB".as_slice(), b"cust".as_slice()),
        ] {
            let mut unsupported = layer.clone();
            let block = unsupported
                .extras
                .iter_mut()
                .find(|b| b.key == *b"SoLd")
                .unwrap();
            let at = block
                .data
                .windows(needle.len())
                .position(|bytes| bytes == needle)
                .unwrap();
            block.data[at..at + needle.len()].copy_from_slice(replacement);
            assert!(read_placement(&unsupported).is_none());
        }
    }
}
