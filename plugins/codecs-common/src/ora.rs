//! OpenRaster (`.ora`) layered interchange, implemented from the public
//! OpenRaster specification (openraster.org, baseline 0.0.5/0.0.6).
//!
//! A package is a zip with a stored `mimetype` first, `stack.xml`
//! describing the layer tree top to bottom, one PNG per layer,
//! `mergedimage.png`, and `Thumbnails/thumbnail.png`.
use anyhow::{ensure, Result};
use schist_codec_idml::container;
use schist_codec_idml::xml::Element;
use schist_color::{ColorMode, Depth};
use schist_core::{BlendMode, Document, IntRect, Layer, LayerKind};
use schist_i18n::{t, tf};
use schist_plugin_api::CodecPlugin;

use crate::archive::{self, Raster};
use crate::kra;
use crate::layered::{self, invalid, MAX_BYTES, MAX_LAYERS};

pub const MIMETYPE: &str = "image/openraster";

pub struct OraCodec;

impl CodecPlugin for OraCodec {
    fn id(&self) -> &'static str {
        "codec.ora"
    }
    fn name(&self) -> &'static str {
        t("codec.ora.name")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["ora"]
    }
    fn probe(&self, bytes: &[u8]) -> bool {
        archive::stored_mimetype(bytes) == Some(MIMETYPE.as_bytes())
    }
    fn import(&self, bytes: &[u8]) -> Result<Document> {
        read(bytes)
    }
    fn can_export(&self) -> bool {
        true
    }
    fn export(&self, doc: &Document) -> Result<Vec<u8>> {
        layered::check_ink_channels(doc)?;
        write(doc)
    }
}

/// The flattened render a package carries, for previews.
pub fn merged_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let package = archive::open(bytes).ok()?;
    package.get("mergedimage.png").map(<[u8]>::to_vec)
}

/// Attributes this codec interprets and regenerates; anything else is
/// preserved verbatim.
const KNOWN: &[&str] = &[
    "src",
    "name",
    "x",
    "y",
    "opacity",
    "visibility",
    "composite-op",
    "selected",
    "edit-locked",
    "isolation",
];

/// An OpenRaster composite-op as a Schist blend mode. `exact` is false
/// when the mapping only approximates the operator, in which case the
/// original is preserved for a later OpenRaster save.
fn blend(op: &str) -> (BlendMode, bool) {
    use BlendMode::*;
    let mode = match op {
        "svg:src-over" => Normal,
        "svg:multiply" => Multiply,
        "svg:screen" => Screen,
        "svg:overlay" => Overlay,
        "svg:darken" => Darken,
        "svg:lighten" => Lighten,
        "svg:color-dodge" => ColorDodge,
        "svg:color-burn" => ColorBurn,
        "svg:hard-light" => HardLight,
        "svg:soft-light" => SoftLight,
        "svg:difference" => Difference,
        "svg:exclusion" => Exclusion,
        "svg:color" => Color,
        "svg:luminosity" => Luminosity,
        "svg:hue" => Hue,
        "svg:saturation" => Saturation,
        "svg:plus" => return (LinearDodge, false),
        _ => {
            if let Some(id) = op.strip_prefix("krita:") {
                if let Some(mode) = kra::blend(id) {
                    return (mode, kra::blend_id(mode) == Some(id));
                }
            }
            log::warn!("openraster: composite-op {op:?} is not supported; using normal");
            return (Normal, false);
        }
    };
    (mode, true)
}

fn blend_op(mode: BlendMode) -> String {
    use BlendMode::*;
    match mode {
        Normal | PassThrough => "svg:src-over",
        Multiply => "svg:multiply",
        Screen => "svg:screen",
        Overlay => "svg:overlay",
        Darken => "svg:darken",
        Lighten => "svg:lighten",
        ColorDodge => "svg:color-dodge",
        ColorBurn => "svg:color-burn",
        HardLight => "svg:hard-light",
        SoftLight => "svg:soft-light",
        Difference => "svg:difference",
        Color => "svg:color",
        Luminosity => "svg:luminosity",
        Hue => "svg:hue",
        Saturation => "svg:saturation",
        LinearDodge => "svg:plus",
        // Not in the baseline list: Krita's extension namespace, which
        // Krita itself writes for these and reads back.
        other => return format!("krita:{}", kra::blend_id(other).unwrap_or("normal")),
    }
    .into()
}

fn number(el: &Element, key: &str) -> Option<f64> {
    el.attr(key)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

fn truthy(el: &Element, key: &str) -> bool {
    matches!(el.attr(key).map(str::trim), Some("true" | "1"))
}

struct Reader<'a> {
    package: &'a container::Package,
    remaining: usize,
    count: usize,
    rasters: Vec<(Raster, String)>,
    unsupported: bool,
    namespaces: Vec<(String, String)>,
}

/// A node of the stack before pixels are placed: decoding first lets the
/// document depth follow the deepest PNG.
enum Node {
    Layer {
        attrs: Vec<(String, String)>,
        raster: Option<usize>,
    },
    Stack {
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    },
}

impl Reader<'_> {
    fn stack(&mut self, el: &Element, depth: usize) -> Result<Vec<Node>> {
        ensure!(depth < 64, "{}", t("codec.layered.too_large"));
        let mut out = Vec::new();
        for child in &el.children {
            match child.name.as_str() {
                "layer" => {
                    self.count += 1;
                    ensure!(self.count <= MAX_LAYERS, "{}", t("codec.layered.too_large"));
                    let src = child.attr("src").ok_or_else(invalid)?;
                    let raster = match self.package.get(src) {
                        Some(bytes) => match archive::decode(bytes, &mut self.remaining) {
                            Ok(raster) => {
                                self.rasters.push((raster, src.to_string()));
                                Some(self.rasters.len() - 1)
                            }
                            // An SVG or other non-raster layer source:
                            // Schist cannot render it, so the merged image
                            // stands in for it below.
                            Err(e) if image::guess_format(bytes).is_err() => {
                                log::warn!("openraster: layer source {src:?} is not a raster: {e}");
                                self.unsupported = true;
                                continue;
                            }
                            Err(e) => return Err(e),
                        },
                        None => return Err(invalid()),
                    };
                    out.push(Node::Layer {
                        attrs: child.attributes.clone(),
                        raster,
                    });
                }
                "stack" => {
                    self.count += 1;
                    ensure!(self.count <= MAX_LAYERS, "{}", t("codec.layered.too_large"));
                    out.push(Node::Stack {
                        attrs: child.attributes.clone(),
                        children: self.stack(child, depth + 1)?,
                    });
                }
                other => {
                    // `text` and `filter` elements are permitted by the
                    // schema but not part of baseline rendering.
                    log::warn!("openraster: <{other}> is not supported");
                    self.unsupported = true;
                }
            }
        }
        Ok(out)
    }

    fn build(
        &mut self,
        nodes: Vec<Node>,
        depth: Depth,
        active: &mut Option<schist_core::LayerId>,
    ) -> Result<Vec<Layer>> {
        let mut out = Vec::with_capacity(nodes.len());
        // stack.xml lists the topmost first; Schist stores bottom first.
        for node in nodes.into_iter().rev() {
            let (attrs, mut layer) = match node {
                Node::Layer { attrs, raster } => {
                    let el = Element {
                        name: "layer".into(),
                        attributes: attrs.clone(),
                        ..Default::default()
                    };
                    let name = el.attr("name").unwrap_or_default().to_string();
                    let x = number(&el, "x").unwrap_or(0.0).round();
                    let y = number(&el, "y").unwrap_or(0.0).round();
                    ensure!(
                        x.abs() <= 1e6 && y.abs() <= 1e6,
                        "{}",
                        t("codec.layered.too_large")
                    );
                    let layer = match raster {
                        Some(i) => archive::raster_layer(
                            name,
                            &self.rasters[i].0,
                            x as i32,
                            y as i32,
                            depth,
                            &mut self.remaining,
                        )?,
                        None => Layer::new_raster(name),
                    };
                    (attrs, layer)
                }
                Node::Stack { attrs, children } => {
                    let el = Element {
                        name: "stack".into(),
                        attributes: attrs.clone(),
                        ..Default::default()
                    };
                    let mut layer = Layer::new_group(el.attr("name").unwrap_or_default());
                    let built = self.build(children, depth, active)?;
                    if let LayerKind::Group(g) = &mut layer.kind {
                        g.children = built;
                    }
                    (attrs, layer)
                }
            };
            let el = Element {
                attributes: attrs,
                ..Default::default()
            };
            layer.visible = el.attr("visibility").map(str::trim) != Some("hidden");
            layer.opacity = number(&el, "opacity").unwrap_or(1.0).clamp(0.0, 1.0) as f32;
            layer.locked = truthy(&el, "edit-locked");
            let op = el.attr("composite-op").unwrap_or("svg:src-over").trim();
            let (mode, exact) = blend(op);
            layer.blend = mode;
            if layer.is_group()
                && el.attr("isolation").map(str::trim) == Some("auto")
                && mode == BlendMode::Normal
                && exact
            {
                // A non-isolated group with the default operator is
                // what Schist (and Photoshop) call pass through.
                layer.blend = BlendMode::PassThrough;
            }
            if truthy(&el, "selected") {
                *active = Some(layer.id);
            }
            let mut keep: Vec<(String, String)> = el
                .attributes
                .iter()
                .filter(|(k, _)| !KNOWN.contains(&k.as_str()))
                .cloned()
                .collect();
            if !exact {
                keep.push(("composite-op".into(), op.to_string()));
            }
            // A prefixed attribute is only meaningful with its namespace.
            let prefixes: Vec<String> = keep
                .iter()
                .filter_map(|(k, _)| k.split_once(':').map(|(p, _)| p.to_string()))
                .filter(|p| p != "xmlns")
                .collect();
            for prefix in prefixes {
                let decl = format!("xmlns:{prefix}");
                if keep.iter().any(|(k, _)| *k == decl) {
                    continue;
                }
                if let Some((_, uri)) = self.namespaces.iter().find(|(k, _)| *k == decl) {
                    keep.push((decl, uri.clone()));
                }
            }
            archive::preserve(&mut layer, &keep);
            out.push(layer);
        }
        Ok(out)
    }
}

fn read(bytes: &[u8]) -> Result<Document> {
    let package = archive::open(bytes)?;
    ensure!(
        package.get("mimetype") == Some(MIMETYPE.as_bytes()),
        "{}",
        t("codec.layered.invalid")
    );
    let root = archive::parse_xml(package.get("stack.xml").ok_or_else(invalid)?)?;
    ensure!(root.name == "image", "{}", t("codec.layered.invalid"));
    let parse_dim = |key| {
        root.attr(key)
            .and_then(|v| v.trim().parse::<u32>().ok())
            .ok_or_else(invalid)
    };
    let (w, h) = (parse_dim("w")?, parse_dim("h")?);
    layered::size(w, h, 16)?;
    let stack = root.child("stack").ok_or_else(invalid)?;
    let mut reader = Reader {
        package: &package,
        remaining: MAX_BYTES,
        count: 0,
        rasters: Vec::new(),
        unsupported: false,
        namespaces: root
            .attributes
            .iter()
            .filter(|(k, _)| k.starts_with("xmlns:"))
            .cloned()
            .collect(),
    };
    let nodes = reader.stack(stack, 0)?;
    let merged = package
        .get("mergedimage.png")
        .map(|png| archive::decode(png, &mut reader.remaining))
        .transpose()
        .unwrap_or_else(|e| {
            log::warn!("openraster: mergedimage.png did not decode: {e}");
            None
        });
    let deep = reader.rasters.iter().any(|(r, _)| r.deep)
        || (reader.rasters.is_empty() && merged.as_ref().is_some_and(|m| m.deep));
    let depth = if deep { Depth::Sixteen } else { Depth::Eight };
    let mut doc = Document::new(t("codec.ora.name"), w, h, depth);
    if let Some(dpi) = number(&root, "xres").filter(|v| *v > 0.0 && *v < 1e6) {
        doc.resolution_dpi = dpi as f32;
    }
    doc.icc_profile = merged
        .as_ref()
        .and_then(|m| m.icc.clone())
        .or_else(|| reader.rasters.iter().find_map(|(r, _)| r.icc.clone()));
    let mut active = None;
    doc.tree.layers = reader.build(nodes, depth, &mut active)?;
    if reader.unsupported {
        match &merged {
            Some(merged) => {
                let name = tf!("codec.layered.merged_layer", format = t("codec.ora.name"));
                let layer =
                    archive::merged_layer(&name, merged, w, h, depth, &mut reader.remaining)?;
                doc.tree.layers.push(layer);
            }
            None => return Err(invalid()),
        }
    }
    doc.active_layer = active;
    Ok(layered::finish(doc))
}

struct Writer<'a> {
    doc: &'a Document,
    parts: Vec<(String, Vec<u8>)>,
    xml: String,
    namespaces: Vec<(String, String)>,
    remaining: usize,
    count: usize,
}

impl Writer<'_> {
    fn layers(&mut self, layers: &[Layer], indent: usize) -> Result<()> {
        ensure!(indent < 64, "{}", t("codec.layered.too_large"));
        for layer in layers.iter().rev() {
            self.count += 1;
            ensure!(self.count <= MAX_LAYERS, "{}", t("codec.layered.too_large"));
            layered::check_layer(layer, true, false)?;
            let pad = "  ".repeat(indent + 2);
            let mut attrs: Vec<(String, String)> = vec![("name".into(), layer.name.clone())];
            let preserved = archive::preserved(layer);
            let mut op = blend_op(layer.blend);
            if let Some((_, original)) = preserved.iter().find(|(k, _)| k == "composite-op") {
                // An operator Schist only approximates comes back as it
                // was, unless the user has since changed the mode.
                if blend(original).0 == layer.blend {
                    op = original.clone();
                }
            }
            let opacity = (layer.opacity * layer.fill_opacity).clamp(0.0, 1.0);
            let common = [
                (
                    "visibility",
                    if layer.visible { "visible" } else { "hidden" }.to_string(),
                ),
                ("opacity", format!("{opacity:.6}")),
                ("composite-op", op),
            ];
            let tag = if let Some(children) = layer.children() {
                attrs.extend(common.map(|(k, v)| (k.to_string(), v)));
                attrs.push((
                    "isolation".into(),
                    if layer.blend == BlendMode::PassThrough {
                        "auto"
                    } else {
                        "isolate"
                    }
                    .into(),
                ));
                self.open("stack", layer, attrs, preserved, &pad, false);
                self.layers(children, indent + 1)?;
                self.xml.push_str(&format!("{pad}</stack>\n"));
                continue;
            } else {
                let tiles = &layer.as_raster().ok_or_else(invalid)?.tiles;
                let bounds = tiles.content_bounds();
                let bounds = if bounds.is_empty() {
                    IntRect::from_xywh(0, 0, 1, 1)
                } else {
                    bounds
                };
                let (w, h) = (bounds.width() as u32, bounds.height() as u32);
                layered::budget(&mut self.remaining, layered::size(w, h, 16)?)?;
                let pixels = archive::layer_pixels(layer, bounds);
                let src = format!("data/layer{}.png", self.count);
                let png = archive::encode_png(
                    w,
                    h,
                    &pixels,
                    self.doc.depth != Depth::Eight,
                    self.doc.icc_profile.as_deref(),
                )?;
                self.parts.push((src.clone(), png));
                attrs.push(("src".into(), src));
                attrs.push(("x".into(), bounds.left.to_string()));
                attrs.push(("y".into(), bounds.top.to_string()));
                attrs.extend(common.map(|(k, v)| (k.to_string(), v)));
                "layer"
            };
            self.open(tag, layer, attrs, preserved, &pad, true);
        }
        Ok(())
    }

    fn open(
        &mut self,
        tag: &str,
        layer: &Layer,
        mut attrs: Vec<(String, String)>,
        preserved: Vec<(String, String)>,
        pad: &str,
        empty: bool,
    ) {
        if self.doc.active_layer == Some(layer.id) {
            attrs.push(("selected".into(), "true".into()));
        }
        if layer.locked {
            attrs.push(("edit-locked".into(), "true".into()));
        }
        for (key, value) in preserved {
            if key.starts_with("xmlns:") {
                if !self.namespaces.iter().any(|(k, _)| *k == key) {
                    self.namespaces.push((key, value));
                }
            } else if !KNOWN.contains(&key.as_str()) && !attrs.iter().any(|(k, _)| *k == key) {
                attrs.push((key, value));
            }
        }
        self.xml.push_str(pad);
        self.xml.push('<');
        self.xml.push_str(tag);
        for (key, value) in attrs {
            self.xml
                .push_str(&format!(" {key}=\"{}\"", archive::escape(&value)));
        }
        self.xml.push_str(if empty { "/>\n" } else { ">\n" });
    }
}

fn write(doc: &Document) -> Result<Vec<u8>> {
    ensure!(
        doc.mode == ColorMode::Rgb,
        "{}",
        t("codec.layered.export_mode")
    );
    layered::size(doc.width, doc.height, 16)?;
    let mut w = Writer {
        doc,
        parts: vec![("mimetype".into(), MIMETYPE.as_bytes().to_vec())],
        xml: String::new(),
        namespaces: Vec::new(),
        remaining: MAX_BYTES,
        count: 0,
    };
    w.layers(&doc.tree.layers, 0)?;

    let canvas = doc.canvas_rect();
    let merged = schist_compositor::composite_region_f32(doc, canvas);
    let deep = doc.depth != Depth::Eight;
    w.parts.push((
        "mergedimage.png".into(),
        archive::encode_png(
            doc.width,
            doc.height,
            &merged,
            deep,
            doc.icc_profile.as_deref(),
        )?,
    ));
    // The thumbnail must be 8-bit and at most 256 on its longer side.
    let image = image::Rgba32FImage::from_raw(doc.width, doc.height, merged).ok_or_else(invalid)?;
    let scale = (256.0 / doc.width.max(doc.height) as f32).min(1.0);
    let (tw, th) = (
        ((doc.width as f32 * scale).round() as u32).max(1),
        ((doc.height as f32 * scale).round() as u32).max(1),
    );
    let thumb = image::imageops::resize(&image, tw, th, image::imageops::FilterType::Triangle);
    w.parts.push((
        "Thumbnails/thumbnail.png".into(),
        archive::encode_png(tw, th, thumb.as_raw(), false, None)?,
    ));

    let mut xml = String::from("<?xml version='1.0' encoding='UTF-8'?>\n");
    xml.push_str(&format!(
        "<image version=\"0.0.5\" w=\"{}\" h=\"{}\" xres=\"{dpi}\" yres=\"{dpi}\"",
        doc.width,
        doc.height,
        dpi = (doc.resolution_dpi.round() as u32).max(1)
    ));
    for (key, value) in &w.namespaces {
        xml.push_str(&format!(" {key}=\"{}\"", archive::escape(value)));
    }
    xml.push_str(">\n  <stack>\n");
    xml.push_str(&w.xml);
    xml.push_str("  </stack>\n</image>\n");
    w.parts.insert(1, ("stack.xml".into(), xml.into_bytes()));
    Ok(container::write(&w.parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(stack: &str, parts: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
        let mut all = vec![
            ("mimetype".to_string(), MIMETYPE.as_bytes().to_vec()),
            ("stack.xml".to_string(), stack.as_bytes().to_vec()),
        ];
        all.extend(parts.into_iter().map(|(n, b)| (n.to_string(), b)));
        container::write(&all)
    }

    fn png(w: u32, h: u32, rgba: [f32; 4]) -> Vec<u8> {
        archive::encode_png(w, h, &rgba.repeat((w * h) as usize), false, None).unwrap()
    }

    #[test]
    fn composite_ops_map_both_ways() {
        for mode in [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::SoftLight,
            BlendMode::Luminosity,
            BlendMode::LinearBurn,
            BlendMode::VividLight,
            BlendMode::HardMix,
            BlendMode::Dissolve,
            BlendMode::DarkerColor,
        ] {
            assert_eq!(blend(&blend_op(mode)).0, mode, "{mode:?}");
        }
        assert_eq!(blend("svg:dst-out"), (BlendMode::Normal, false));
    }

    #[test]
    fn stack_offsets_are_ignored_and_layer_offsets_are_signed() {
        let stack = r#"<image version="0.0.6" w="8" h="8"><stack>
            <stack name="g" x="100" y="100" isolation="auto">
              <layer src="data/a.png" name="a" x="-2" y="3"/>
            </stack></stack></image>"#;
        let bytes = package(stack, vec![("data/a.png", png(2, 2, [1.0, 0.0, 0.0, 1.0]))]);
        let doc = read(&bytes).unwrap();
        let group = &doc.tree.layers[0];
        assert_eq!(group.blend, BlendMode::PassThrough);
        let a = &group.children().unwrap()[0];
        let tiles = &a.as_raster().unwrap().tiles;
        assert_eq!(tiles.pixel(-2, 3).r, 1.0);
        assert_eq!(tiles.content_bounds(), IntRect::from_xywh(-2, 3, 2, 2));
    }

    #[test]
    fn missing_sources_and_bad_documents_are_errors_not_panics() {
        for stack in [
            "<image w=\"4\" h=\"4\"><stack><layer src=\"nope.png\"/></stack></image>",
            "<image w=\"0\" h=\"4\"><stack/></image>",
            "<image w=\"4\"><stack/></image>",
            "<notimage w=\"4\" h=\"4\"><stack/></notimage>",
            "<image w=\"4\" h=\"4\"><stack><layer/></stack></image>",
            "<image w=\"4\" h=\"4\"><stack>",
        ] {
            assert!(read(&package(stack, vec![])).is_err(), "{stack}");
        }
    }

    #[test]
    fn a_non_raster_layer_falls_back_to_the_merged_image() {
        let stack = r#"<image w="4" h="4"><stack>
            <layer src="data/v.svg" name="vector"/>
            <layer src="data/a.png" name="paint"/></stack></image>"#;
        let bytes = package(
            stack,
            vec![
                (
                    "data/v.svg",
                    b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec(),
                ),
                ("data/a.png", png(4, 4, [0.0, 1.0, 0.0, 1.0])),
                ("mergedimage.png", png(4, 4, [0.0, 0.0, 1.0, 1.0])),
            ],
        );
        let doc = read(&bytes).unwrap();
        assert_eq!(doc.tree.layers.len(), 2);
        assert_eq!(doc.tree.layers[0].name, "paint");
        let merged = &doc.tree.layers[1];
        assert!(merged.visible);
        assert_eq!(merged.as_raster().unwrap().tiles.pixel(3, 3).b, 1.0);
    }
}
