//! Native `TySh` / EngineData interchange. See docs/psd-interchange.md for
//! the supported subset and public, independently implemented references.
//! `PsTx` remains the lossless Schist representation; `ScTx` records imported
//! state so untouched native typography survives without normalization.
mod engine;

use schist_core::{Layer, RawBlock};
use schist_psd_descriptor::{Builder, Value as DValue};
use schist_text_engine::{Align, StyleRun, TextSpec};
use serde_json::{json, Value};

const TEXT: [u8; 4] = *b"PsTx";
const SNAPSHOT: [u8; 4] = *b"ScTx";
const MAX_TEXT: usize = 1_000_000;

fn stored(layer: &Layer) -> Option<Value> {
    serde_json::from_slice(&layer.extras.iter().find(|b| b.key == TEXT)?.data).ok()
}

/// Native blocks from unsupported or untouched imported text remain exact.
pub(crate) fn preserve_native(layer: &Layer) -> bool {
    let Some(current) = stored(layer) else {
        return true;
    };
    layer
        .extras
        .iter()
        .find(|b| b.key == SNAPSHOT)
        .and_then(|b| serde_json::from_slice::<Value>(&b.data).ok())
        .is_some_and(|snapshot| snapshot == current)
}

fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}
fn number(v: &Value, key: &str, default: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(default)
}
fn boolean(v: &Value, key: &str, default: bool) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(default)
}
fn merge(base: &Value, over: &Value) -> Value {
    let mut result = base.as_object().cloned().unwrap_or_default();
    if let Some(over) = over.as_object() {
        result.extend(over.clone());
    }
    Value::Object(result)
}

fn feature_supported(spec: &TextSpec, raw: &Value) -> bool {
    spec.path.is_none()
        && !requires_bidi_interchange(&spec.text)
        && spec.features.iter().all(|f| matches!(f.tag.as_str(), "kern" | "liga" | "dlig" | "smcp") && f.value <= 1)
        // These future-compatible checks keep separate writing-mode work safe:
        // vertical/path text is still preserved losslessly in PsTx until the
        // matching native paragraph semantics are implemented.
        && raw.get("writing_mode").and_then(Value::as_str).is_none_or(|m| m == "Horizontal")
        && raw.get("direction").and_then(Value::as_str).is_none_or(|m| m != "RightToLeft")
}

// Native paragraph direction is a separate mapping from the Type tool's
// writing-mode support. Until it is encoded, keep directional scripts and
// controls in lossless private/native data rather than changing their layout.
fn requires_bidi_interchange(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(ch as u32,
        0x0590..=0x08ff | 0x200e..=0x200f | 0x202a..=0x202e |
        0x2066..=0x2069 | 0xfb1d..=0xfdff | 0xfe70..=0xfeff |
        0x10800..=0x10fff | 0x1e800..=0x1eeff)
    })
}

pub(crate) fn write_type(layer: &Layer) -> Option<Vec<u8>> {
    if preserve_native(layer) {
        return None;
    }
    let stored = stored(layer)?;
    let spec: TextSpec = serde_json::from_value(stored.get("spec")?.clone()).ok()?;
    if !feature_supported(&spec, &stored["spec"])
        || spec.text.len() > MAX_TEXT
        || !spec.size.is_finite()
        || spec.size <= 0.0
        || !spec.tracking.is_finite()
        || !spec.line_height.is_finite()
    {
        return None;
    }
    if spec.runs.iter().any(|r| {
        r.start > r.end
            || !spec.text.is_char_boundary(r.start)
            || !spec.text.is_char_boundary(r.end)
            || r.size.is_some_and(|s| !s.is_finite() || s <= 0.0)
    }) {
        return None;
    }
    let origin = stored.get("origin")?.as_array()?;
    let (x, y) = (
        finite(origin.first()?.as_f64()?)?,
        finite(origin.get(1)?.as_f64()?)?,
    );
    let color = stored.get("color")?.as_array()?;
    if color.len() != 4 {
        return None;
    }
    let color: Vec<f64> = color
        .iter()
        .map(|v| v.as_u64().filter(|n| *n <= 255).map(|n| n as f64 / 255.0))
        .collect::<Option<_>>()?;
    let raster = schist_text_engine::measure(&spec)?;
    let baseline = raster.first_baseline as f64;
    let width = spec.wrap_width.unwrap_or(raster.width) as f64;
    if !width.is_finite() || width < 0.0 {
        return None;
    }
    let height = (raster.height as f64).max(baseline + raster.line_advance as f64);
    let alignment = match spec.align {
        Align::Left => 0,
        Align::Right => 1,
        Align::Center => 2,
    };
    let align_factor = match spec.align {
        Align::Left => 0.0,
        Align::Center => 0.5,
        Align::Right => 1.0,
    };
    let box_left = (raster.width as f64 - width) * align_factor;
    let point_shift = if spec.wrap_width.is_some() {
        0.0
    } else {
        match spec.align {
            Align::Left => 0.0,
            Align::Right => width,
            Align::Center => width / 2.0,
        }
    };

    let mut fonts =
        vec![json!({"Name": "AdobeInvisFont", "Script": 0, "FontType": 0, "Synthetic": 0})];
    let mut styles = Vec::new();
    let mut lengths = Vec::new();
    let mut boundaries = vec![0, spec.text.len()];
    for run in &spec.runs {
        boundaries.extend([run.start, run.end]);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    if boundaries.len() == 1 {
        boundaries.push(0);
    }
    for pair in boundaries.windows(2) {
        let style = spec.style_at(pair[0]);
        let style_metrics = schist_text_engine::measure(&TextSpec {
            text: "M".into(),
            family: style.family.clone(),
            bold: style.bold,
            italic: style.italic,
            size: style.size,
            line_height: spec.line_height,
            ..TextSpec::default()
        })?;
        let ps_name = schist_text_engine::postscript_name(&style.family, style.bold, style.italic)
            .or_else(|| schist_text_engine::postscript_name(&style.family, false, false))
            .unwrap_or_else(|| style.family.clone());
        let font = fonts
            .iter()
            .position(|font| font["Name"].as_str() == Some(&ps_name))
            .unwrap_or_else(|| {
                fonts.push(json!({"Name": ps_name, "Script": 0, "FontType": 0, "Synthetic": 0}));
                fonts.len() - 1
            });
        let style_data = json!({
            "Font": font, "FontSize": style.size,
            "FauxBold": style.bold && schist_text_engine::postscript_name(&style.family, true, style.italic).is_none(),
            "FauxItalic": style.italic && schist_text_engine::postscript_name(&style.family, style.bold, true).is_none(),
            "AutoLeading": false, "Leading": style_metrics.line_advance,
            "HorizontalScale": 1.0, "VerticalScale": 1.0,
            "Tracking": spec.tracking as f64 / style.size as f64 * 1000.0,
            "AutoKerning": spec.feature("kern", true), "Kerning": 0,
            "BaselineShift": 0.0, "FontCaps": if spec.feature("smcp", false) {2} else {0},
            "FontBaseline": 0, "Underline": false, "Strikethrough": false,
            "Ligatures": spec.feature("liga", false), "DLigatures": spec.feature("dlig", false),
            "FillColor": {"Type": 1, "Values": [color[3], color[0], color[1], color[2]]},
            "StrokeFlag": false, "FillFlag": true, "FillFirst": true,
            "Language": 0, "NoBreak": false, "StyleRunAlignment": 2, "BaselineDirection": 2
        });
        styles.push(json!({"StyleSheet": {"StyleSheetData": style_data}}));
        lengths.push(spec.text[pair[0]..pair[1]].encode_utf16().count());
    }
    *lengths.last_mut()? += 1; // EngineData's mandatory terminal paragraph mark.
    let plain = spec.text.replace('\n', "\r");
    let editor_text = format!("{plain}\r");
    let paragraph = json!({
        "Justification": alignment, "FirstLineIndent": 0.0, "StartIndent": 0.0, "EndIndent": 0.0,
        "SpaceBefore": 0.0, "SpaceAfter": 0.0, "AutoHyphenate": false,
        "WordSpacing": [0.8, 1.0, 1.33], "LetterSpacing": [0.0, 0.0, 0.0], "GlyphSpacing": [1.0, 1.0, 1.0],
        "AutoLeading": 1.2, "LeadingType": 0, "EveryLineComposer": false
    });
    let paragraph_run = json!({"ParagraphSheet": {"DefaultStyleSheet": 0, "Properties": paragraph}, "Adjustments": {"Axis": [1, 0, 1], "XY": [0, 0]}});
    let paragraph_lengths: Vec<usize> = editor_text
        .split_inclusive('\r')
        .map(|s| s.encode_utf16().count())
        .collect();
    let paragraph_runs = vec![paragraph_run.clone(); paragraph_lengths.len()];
    let shape = if spec.wrap_width.is_some() {
        json!({"ShapeType": 1, "BoxBounds": [box_left, -baseline, box_left + width, height - baseline], "Base": {"ShapeType": 1, "TransformPoint0": [1,0], "TransformPoint1": [0,1], "TransformPoint2": [0,0]}})
    } else {
        json!({"ShapeType": 0, "PointBase": [0,0], "Base": {"ShapeType": 0, "TransformPoint0": [1,0], "TransformPoint1": [0,1], "TransformPoint2": [0,0]}})
    };
    let resources = json!({
        "FontSet": fonts, "TheNormalStyleSheet": 0, "TheNormalParagraphSheet": 0,
        "StyleSheetSet": [{"Name": "Normal RGB", "StyleSheetData": styles[0]["StyleSheet"]["StyleSheetData"]}],
        "ParagraphSheetSet": [{"Name": "Normal RGB", "DefaultStyleSheet": 0, "Properties": paragraph}],
        "KinsokuSet": [], "MojiKumiSet": [], "SuperscriptSize": 0.583, "SuperscriptPosition": 0.333,
        "SubscriptSize": 0.583, "SubscriptPosition": 0.333, "SmallCapSize": 0.7
    });
    let data = json!({"EngineDict": {
        "Editor": {"Text": editor_text},
        "StyleRun": {"DefaultRunData": {"StyleSheet": {"StyleSheetData": {}}}, "RunArray": styles, "RunLengthArray": lengths, "IsJoinable": 2},
        "ParagraphRun": {"DefaultRunData": paragraph_run, "RunArray": paragraph_runs, "RunLengthArray": paragraph_lengths, "IsJoinable": 1},
        "GridInfo": {"GridIsOn": false, "ShowGrid": false, "GridSize": 18.0, "GridLeading": 22.0, "GridColor": {"Type": 1, "Values": [1.0,0.0,0.0,1.0]}, "GridLeadingFillColor": {"Type": 1, "Values": [1.0,0.0,0.0,1.0]}, "AlignLineHeightToGridFlags": false},
        "AntiAlias": 4, "UseFractionalGlyphWidths": true,
        "Rendered": {"Version": 1, "Shapes": {"WritingDirection": 0, "Children": [{"ShapeType": shape["ShapeType"], "Procession": 0, "Lines": {"WritingDirection": 0, "Children": []}, "Cookie": {"Photoshop": shape}}]}}
    }, "ResourceDict": resources, "DocumentResources": resources});
    let mut text = Builder::new("TxLr");
    text.text("Txt ", &plain)
        .enumerated("textGridding", "textGridding", "None")
        .enumerated("Ornt", "Ornt", "Hrzn")
        .enumerated("AntA", "Annt", "antiAliasSharp")
        .integer("TextIndex", 0)
        .raw("EngineData", &engine::encode(&data));
    let mut warp = Builder::new("warp");
    warp.enumerated("warpStyle", "warpStyle", "warpNone")
        .double("warpValue", 0.0)
        .double("warpPerspective", 0.0)
        .double("warpPerspectiveOther", 0.0)
        .enumerated("warpRotate", "Ornt", "Hrzn");
    let mut out = 1u16.to_be_bytes().to_vec();
    for value in [1.0, 0.0, 0.0, 1.0, x + point_shift, y + baseline] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend_from_slice(&50u16.to_be_bytes());
    out.extend(text.finish_versioned());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend(warp.finish_versioned());
    for value in [
        0.0f32,
        -baseline as f32,
        width as f32,
        (height - baseline) as f32,
    ] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    Some(out)
}

/// Promote only type settings the current Type tool can represent; cached
/// pixels always remain authoritative until the user edits the text.
pub(crate) fn import_type(extras: &mut Vec<RawBlock>) {
    if extras.iter().any(|b| b.key == TEXT) {
        return;
    }
    let Some(native) = extras.iter().find(|b| b.key == *b"TySh") else {
        return;
    };
    if let Some(stored) = read_type(&native.data) {
        if let Ok(bytes) = serde_json::to_vec(&stored) {
            extras.push(RawBlock {
                key: TEXT,
                data: bytes.clone(),
            });
            extras.push(RawBlock {
                key: SNAPSHOT,
                data: bytes,
            });
        }
    }
}

fn read_type(bytes: &[u8]) -> Option<Value> {
    if bytes.get(..2)? != 1u16.to_be_bytes()
        || bytes.get(50..52)? != 50u16.to_be_bytes()
        || bytes.get(52..56)? != 16u32.to_be_bytes()
    {
        return None;
    }
    let mut transform = [0.0f64; 6];
    for (i, value) in transform.iter_mut().enumerate() {
        *value = finite(f64::from_be_bytes(
            bytes.get(2 + i * 8..10 + i * 8)?.try_into().ok()?,
        ))?;
    }
    let [sx, skew_y, skew_x, sy, tx, ty] = transform;
    // Arbitrary affine and warp geometry cannot currently be edited by the
    // Type tool. Preserve its native descriptor instead of misplacing glyphs.
    if sx <= 0.0 || (sx - sy).abs() > 0.00001 || skew_x.abs() > 0.00001 || skew_y.abs() > 0.00001 {
        return None;
    }
    let (descriptor, consumed) = schist_psd_descriptor::parse_prefix(bytes.get(56..)?)?;
    if !matches!(descriptor.get("Ornt"), Some(DValue::Enum(_, value)) if value == "Hrzn") {
        return None;
    }
    let warp_start = 56 + consumed;
    if bytes.get(warp_start..warp_start + 2)? != 1u16.to_be_bytes()
        || bytes.get(warp_start + 2..warp_start + 6)? != 16u32.to_be_bytes()
    {
        return None;
    }
    let warp = schist_psd_descriptor::parse(bytes.get(warp_start + 6..)?)?;
    if !matches!(warp.get("warpStyle"), Some(DValue::Enum(_, value)) if value == "warpNone") {
        return None;
    }
    let data = engine::parse(descriptor.get("EngineData")?.as_raw()?)?;
    let engine = &data["EngineDict"];
    let fonts = data["ResourceDict"]["FontSet"].as_array()?;
    let paragraphs = engine["ParagraphRun"]["RunArray"].as_array()?;
    let first = &paragraphs.first()?["ParagraphSheet"]["Properties"];
    let auto_leading = number(first, "AutoLeading", 1.2);
    if !auto_leading.is_finite() || !(0.1..=100.0).contains(&auto_leading) {
        return None;
    }
    let text = descriptor.get("Txt ")?.as_text()?.replace('\r', "\n");
    if text.len() > MAX_TEXT || requires_bidi_interchange(&text) {
        return None;
    }
    let expected_editor = format!("{}\r", text.replace('\n', "\r"));
    if engine["Editor"]["Text"].as_str()? != expected_editor {
        return None;
    }
    let lengths = engine["StyleRun"]["RunLengthArray"].as_array()?;
    let runs = engine["StyleRun"]["RunArray"].as_array()?;
    if lengths.len() != runs.len() || runs.is_empty() || runs.len() > 4096 {
        return None;
    }
    let base = &data["ResourceDict"]["StyleSheetSet"][0]["StyleSheetData"];
    let mut spec = TextSpec {
        text,
        ..TextSpec::default()
    };
    // Convert indices once, rather than scanning the entire story for each
    // run. Interior surrogate positions deliberately have no byte boundary.
    let mut boundaries = vec![None; spec.text.encode_utf16().count() + 2];
    let mut units = 0;
    for (byte, ch) in spec.text.char_indices() {
        boundaries[units] = Some(byte);
        units += ch.len_utf16();
    }
    boundaries[units] = Some(spec.text.len());
    boundaries[units + 1] = Some(spec.text.len());
    let mut offset_utf16 = 0usize;
    let mut color = None;
    let mut leading: Option<f64> = None;
    let mut tracking = None;
    let mut features = None;
    for (i, (run, len)) in runs.iter().zip(lengths).enumerate() {
        let style = merge(base, &run["StyleSheet"]["StyleSheetData"]);
        if boolean(&style, "StrokeFlag", false)
            || !boolean(&style, "FillFlag", true)
            || boolean(&style, "Underline", false)
            || boolean(&style, "Strikethrough", false)
            || boolean(&style, "NoBreak", false)
            || number(&style, "Kerning", 0.0) != 0.0
            || number(&style, "CharacterDirection", 0.0) != 0.0
            || number(&style, "BaselineShift", 0.0) != 0.0
            || number(&style, "FontBaseline", 0.0) != 0.0
            || number(&style, "HorizontalScale", 1.0) != 1.0
            || number(&style, "VerticalScale", 1.0) != 1.0
        {
            return None;
        }
        let index = number(&style, "Font", 0.0) as usize;
        let font_name = fonts.get(index)?["Name"].as_str()?;
        let (family, mut bold, mut italic) = schist_text_engine::family_from_postscript(font_name)
            .unwrap_or_else(|| {
                (
                    font_name.to_string(),
                    font_name.contains("Bold"),
                    font_name.contains("Italic") || font_name.contains("Oblique"),
                )
            });
        bold |= boolean(&style, "FauxBold", false);
        italic |= boolean(&style, "FauxItalic", false);
        let size = finite(number(&style, "FontSize", 12.0) * sx)? as f32;
        if !(0.1..=100_000.0).contains(&size) {
            return None;
        }
        let fill = &style["FillColor"];
        if number(fill, "Type", 1.0) != 1.0 {
            return None;
        }
        let values = fill.get("Values").and_then(Value::as_array);
        let rgba = if let Some(values) = values {
            if values.len() != 4 {
                return None;
            }
            let q = |i: usize| -> Option<u8> {
                Some((values[i].as_f64()?.clamp(0.0, 1.0) * 255.0).round() as u8)
            };
            [q(1)?, q(2)?, q(3)?, q(0)?]
        } else {
            [0, 0, 0, 255]
        };
        if color.is_some_and(|c| c != rgba) {
            return None;
        }
        color = Some(rgba);
        let metrics = schist_text_engine::measure(&TextSpec {
            text: "M".into(),
            family: family.clone(),
            bold,
            italic,
            size,
            ..TextSpec::default()
        })?;
        let advance = if boolean(&style, "AutoLeading", true) {
            auto_leading * size as f64
        } else {
            number(&style, "Leading", 0.0) * sy
        };
        let run_leading = Some(advance / metrics.line_advance as f64);
        if i > 0 {
            match (leading, run_leading) {
                (Some(a), Some(b)) if (a - b).abs() <= 0.001 => {}
                (None, None) => {}
                _ => return None,
            }
        }
        leading = run_leading;
        let run_tracking = number(&style, "Tracking", 0.0) * size as f64 / 1000.0;
        if tracking.is_some_and(|t: f64| (t - run_tracking).abs() > 0.001) {
            return None;
        }
        tracking = Some(run_tracking);
        let run_features = [
            boolean(&style, "AutoKerning", true),
            boolean(&style, "Ligatures", true),
            boolean(&style, "DLigatures", false),
            number(&style, "FontCaps", 0.0) == 2.0,
        ];
        if features.is_some_and(|f| f != run_features)
            || ![0.0, 2.0].contains(&number(&style, "FontCaps", 0.0))
        {
            return None;
        }
        features = Some(run_features);
        let count = len.as_f64()?;
        if count < 0.0 || count.fract() != 0.0 || count > MAX_TEXT as f64 {
            return None;
        }
        let end_utf16 = offset_utf16.checked_add(count as usize)?;
        let (start, end) = (
            boundaries.get(offset_utf16).copied().flatten()?,
            boundaries.get(end_utf16).copied().flatten()?,
        );
        if i == 0 {
            spec.family = family.clone();
            spec.bold = bold;
            spec.italic = italic;
            spec.size = size;
        }
        if start < end {
            spec.runs.push(StyleRun {
                start,
                end,
                family: Some(family),
                bold: Some(bold),
                italic: Some(italic),
                size: Some(size),
            });
        }
        offset_utf16 = end_utf16;
    }
    if offset_utf16 != expected_editor.encode_utf16().count() {
        return None;
    }
    spec.tracking = tracking? as f32;
    for paragraph in paragraphs {
        let props = &paragraph["ParagraphSheet"]["Properties"];
        if number(props, "Justification", 0.0) != number(first, "Justification", 0.0)
            || number(props, "AutoLeading", 1.2) != auto_leading
            || boolean(props, "EveryLineComposer", false)
            || [
                "FirstLineIndent",
                "StartIndent",
                "EndIndent",
                "SpaceBefore",
                "SpaceAfter",
            ]
            .iter()
            .any(|key| number(props, key, 0.0) != 0.0)
        {
            return None;
        }
    }
    spec.align = match number(first, "Justification", 0.0) {
        0.0 => Align::Left,
        1.0 => Align::Right,
        2.0 => Align::Center,
        _ => return None,
    };
    let shape = &engine["Rendered"]["Shapes"]["Children"][0]["Cookie"]["Photoshop"];
    if number(&engine["Rendered"]["Shapes"], "WritingDirection", 0.0) != 0.0 {
        return None;
    }
    if let Some(children) = engine["Rendered"]["Shapes"]["Children"].as_array() {
        if children.len() != 1 {
            return None;
        }
    }
    fn pair_matches(value: &Value, expected: [f64; 2]) -> bool {
        value.is_null()
            || value.as_array().is_some_and(|array| {
                array.len() == 2
                    && array
                        .iter()
                        .zip(expected)
                        .all(|(v, e)| v.as_f64() == Some(e))
            })
    }
    if ![0.0, 1.0].contains(&number(shape, "ShapeType", 0.0))
        || !pair_matches(&shape["PointBase"], [0.0, 0.0])
        || !pair_matches(&shape["Base"]["TransformPoint0"], [1.0, 0.0])
        || !pair_matches(&shape["Base"]["TransformPoint1"], [0.0, 1.0])
        || !pair_matches(&shape["Base"]["TransformPoint2"], [0.0, 0.0])
    {
        return None;
    }
    let mut box_offset = (0.0, 0.0);
    let mut box_height = None;
    if number(shape, "ShapeType", 0.0) == 1.0 {
        if paragraphs
            .iter()
            .any(|p| boolean(&p["ParagraphSheet"]["Properties"], "AutoHyphenate", false))
        {
            return None;
        }
        let bounds = shape["BoxBounds"].as_array()?;
        if bounds.len() != 4 {
            return None;
        }
        let (left, top, right) = (
            bounds.first()?.as_f64()?,
            bounds.get(1)?.as_f64()?,
            bounds.get(2)?.as_f64()?,
        );
        let bottom = bounds.get(3)?.as_f64()?;
        if !left.is_finite()
            || !top.is_finite()
            || !right.is_finite()
            || right <= left
            || right - left > 1_000_000.0
            || !bottom.is_finite()
            || bottom <= top
        {
            return None;
        }
        spec.wrap_width = Some(((right - left) * sx) as f32);
        box_offset = (left * sx, top * sy);
        box_height = Some((bottom - top) * sy);
    }
    if let Some(leading) = leading {
        if !leading.is_finite() || !(0.1..=1000.0).contains(&leading) {
            return None;
        }
        spec.line_height = leading as f32;
    }
    if let Some(features) = features {
        for (tag, enabled) in ["kern", "liga", "dlig", "smcp"].into_iter().zip(features) {
            spec.set_feature(tag, enabled);
        }
    }
    let raster = schist_text_engine::measure(&spec)?;
    // The local model has a wrap width but no clipping/overset box height.
    if box_height.is_some_and(|height| raster.height as f64 > height + 1.0) {
        return None;
    }
    let point_shift = if spec.wrap_width.is_some() {
        0.0
    } else {
        match spec.align {
            Align::Left => 0.0,
            Align::Right => raster.width as f64,
            Align::Center => raster.width as f64 / 2.0,
        }
    };
    let origin = if spec.wrap_width.is_some() {
        let factor = match spec.align {
            Align::Left => 0.0,
            Align::Center => 0.5,
            Align::Right => 1.0,
        };
        [
            tx + box_offset.0 + (spec.wrap_width? as f64 - raster.width as f64) * factor,
            ty + box_offset.1,
        ]
    } else {
        [tx - point_shift, ty - raster.first_baseline as f64]
    };
    if origin
        .iter()
        .any(|v| !v.is_finite() || v.abs() > i32::MAX as f64)
    {
        return None;
    }
    Some(
        json!({"spec": spec, "origin": [origin[0].round() as i32, origin[1].round() as i32], "color": color?}),
    )
}
