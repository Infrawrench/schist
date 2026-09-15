//! Paint.NET PDN3: an NRBF document graph followed by chunked BGRA surfaces.
//! The pixel blocks are data, not objects to be deserialized by a runtime.
mod nrbf;

use std::{
    collections::HashMap,
    io::{Read, Write},
};

use anyhow::{ensure, Result};
use schist_color::{ColorMode, Depth};
use schist_core::{blit_rgba8, BlendMode, Document, Layer};
use schist_i18n::t;
use schist_plugin_api::CodecPlugin;

use crate::layered::{self, invalid, unsupported, Reader, MAX_BYTES, MAX_LAYERS};
use nrbf::{Graph, Object, Value};

pub struct PdnCodec;

impl CodecPlugin for PdnCodec {
    fn id(&self) -> &'static str {
        "codec.pdn"
    }
    fn name(&self) -> &'static str {
        t("codec.pdn.name")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["pdn"]
    }
    fn probe(&self, bytes: &[u8]) -> bool {
        bytes.starts_with(b"PDN3")
    }
    fn import(&self, bytes: &[u8]) -> Result<Document> {
        read(bytes)
    }
    fn can_export(&self) -> bool {
        true
    }
    fn export(&self, doc: &Document) -> Result<Vec<u8>> {
        write(doc)
    }
}

fn unsigned(n: i64) -> Result<u32> {
    u32::try_from(n).map_err(|_| invalid())
}

fn chunks(r: &mut Reader<'_>, length: usize) -> Result<Vec<u8>> {
    let compression = r.byte()?;
    ensure!(
        compression <= 1,
        "{}",
        unsupported(t("codec.pdn.feature.compression"))
    );
    let chunk_size = r.be32()? as usize;
    ensure!(
        chunk_size > 0 && length <= MAX_BYTES,
        "{}",
        t("codec.layered.invalid")
    );
    let count = length.div_ceil(chunk_size);
    ensure!(
        count <= (r.bytes.len() - r.pos) / 8,
        "{}",
        t("codec.layered.invalid")
    );
    let mut seen = vec![false; count];
    let mut out = vec![0; length];
    for _ in 0..count {
        let index = r.be32()? as usize;
        let compressed_length = r.be32()? as usize;
        ensure!(
            index < count && !seen[index],
            "{}",
            t("codec.layered.invalid")
        );
        seen[index] = true;
        let start = index * chunk_size;
        let len = chunk_size.min(length - start);
        let data = r.take(compressed_length)?;
        if compression == 0 {
            let mut decoded = Vec::new();
            // Read one extra byte so overlong compressed chunks fail too.
            flate2::read::GzDecoder::new(data)
                .take(len as u64 + 1)
                .read_to_end(&mut decoded)
                .map_err(|_| invalid())?;
            ensure!(decoded.len() == len, "{}", t("codec.layered.invalid"));
            out[start..start + len].copy_from_slice(&decoded);
        } else {
            ensure!(data.len() == len, "{}", t("codec.layered.invalid"));
            out[start..start + len].copy_from_slice(data);
        }
    }
    Ok(out)
}

fn memory<'a>(
    graph: &'a Graph,
    buffers: &'a HashMap<i32, Vec<u8>>,
    value: &'a Value,
    depth: usize,
) -> Result<&'a [u8]> {
    ensure!(depth < 64, "{}", t("codec.layered.invalid"));
    let Value::Ref(id) = value else {
        return Err(invalid());
    };
    if let Some(data) = buffers.get(id) {
        return Ok(data);
    }
    let block = graph.object(value)?;
    if block.boolean("hasParent")? {
        let parent = memory(graph, buffers, block.field("parentBlock")?, depth + 1)?;
        let offset = block
            .int("parentOffset64")
            .or_else(|_| block.int("parentOffset"))?;
        let offset = usize::try_from(offset).map_err(|_| invalid())?;
        let len = usize::try_from(block.int("length64").or_else(|_| block.int("length"))?)
            .map_err(|_| invalid())?;
        return parent
            .get(offset..offset.checked_add(len).ok_or_else(invalid)?)
            .ok_or_else(invalid);
    }
    match graph.resolve(block.field("pointerData")?)? {
        Value::Bytes(data) => Ok(data),
        _ => Err(invalid()),
    }
}

fn read(bytes: &[u8]) -> Result<Document> {
    let mut r = Reader::new(bytes);
    ensure!(r.take(4)? == b"PDN3", "{}", t("codec.layered.invalid"));
    let header = r.take(3)?;
    let len = header[0] as usize | (header[1] as usize) << 8 | (header[2] as usize) << 16;
    r.take(len)?; // optional XML thumbnail; all document data is in NRBF
    ensure!(
        r.take(2)? == [0, 1],
        "{}",
        unsupported(t("codec.pdn.feature.container_version"))
    );
    let graph = nrbf::read(&mut r)?;
    let root = Value::Ref(graph.root);
    let source = graph.object(&root)?;
    ensure!(
        source.name == "PaintDotNet.Document",
        "{}",
        t("codec.layered.invalid")
    );
    let (w, h) = (
        unsigned(source.int("width")?)?,
        unsigned(source.int("height")?)?,
    );
    layered::size(w, h, 4)?;
    let list = graph.object(source.field("layers")?)?;
    let count = unsigned(list.int("ArrayList+_size")?)? as usize;
    ensure!(count <= MAX_LAYERS, "{}", t("codec.layered.too_large"));
    let layers = graph
        .array(list.field("ArrayList+_items")?)?
        .get(..count)
        .ok_or_else(invalid)?;
    let mut remaining = MAX_BYTES;
    let mut buffers = HashMap::new();
    // Deferred blocks occur in serialization order, which need not be the
    // layer-list order. Parent blocks can be shared by several surfaces.
    for &id in &graph.order {
        if let Some(Value::Object(block)) = graph.values.get(&id) {
            if block.name == "PaintDotNet.MemoryBlock"
                && block
                    .fields
                    .get("deferred")
                    .is_some_and(|v| matches!(v, Value::Bool(true)))
            {
                let len = usize::try_from(block.int("length64").or_else(|_| block.int("length"))?)
                    .map_err(|_| invalid())?;
                layered::budget(&mut remaining, len)?;
                buffers.insert(id, chunks(&mut r, len)?);
            }
        }
    }
    let mut doc = Document::new(t("codec.pdn.name"), w, h, Depth::Eight);
    for value in layers {
        let source = graph.object(value)?;
        ensure!(
            source.name == "PaintDotNet.BitmapLayer",
            "{}",
            unsupported(t("codec.pdn.feature.layer_type"))
        );
        ensure!(
            source.int("Layer+width")? == w as i64 && source.int("Layer+height")? == h as i64,
            "{}",
            t("codec.layered.invalid")
        );
        let props = graph.object(source.field("Layer+properties")?)?;
        let surface = graph.object(source.field("surface")?)?;
        ensure!(
            surface.int("width")? == w as i64 && surface.int("height")? == h as i64,
            "{}",
            t("codec.layered.invalid")
        );
        let stride = unsigned(surface.int("stride")?)? as usize;
        ensure!(
            stride >= w as usize * 4,
            "{}",
            unsupported(t("codec.pdn.feature.pixel_format"))
        );
        let length = stride.checked_mul(h as usize).ok_or_else(invalid)?;
        let data = memory(&graph, &buffers, surface.field("scan0")?, 0)?;
        ensure!(length == data.len(), "{}", t("codec.layered.invalid"));
        // Count the decoded layer too: repeated references must not bypass
        // the document's total allocation limit.
        layered::budget(
            &mut remaining,
            layered::tile_bytes(doc.canvas_rect(), Depth::Eight, false),
        )?;
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for row in data.chunks_exact(stride) {
            for pixel in row[..w as usize * 4].as_chunks::<4>().0 {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
        }
        let mut layer = Layer::new_raster(graph.text(props.field("name")?)?);
        layer.visible = props.boolean("visible")?;
        let opacity = props.int("opacity")?;
        ensure!(
            (0..=255).contains(&opacity),
            "{}",
            t("codec.layered.invalid")
        );
        layer.opacity = opacity as f32 / 255.0;
        layer.blend = layer_blend(&graph, source, props)?;
        blit_rgba8(
            &mut layer.as_raster_mut().unwrap().tiles,
            Depth::Eight,
            doc.canvas_rect(),
            &rgba,
        );
        doc.tree.layers.push(layer);
    }
    Ok(layered::finish(doc))
}

// Names are the public PaintDotNet.UserBlendOps nested classes. Modern PDNs
// also store the same modes as an enum in LayerProperties.
const MODES: &[(i64, &str, BlendMode)] = &[
    (0, "Normal", BlendMode::Normal),
    (1, "Multiply", BlendMode::Multiply),
    (2, "Additive", BlendMode::LinearDodge),
    (3, "ColorBurn", BlendMode::ColorBurn),
    (4, "ColorDodge", BlendMode::ColorDodge),
    (7, "Overlay", BlendMode::Overlay),
    (8, "Difference", BlendMode::Difference),
    (10, "Lighten", BlendMode::Lighten),
    (11, "Darken", BlendMode::Darken),
    (12, "Screen", BlendMode::Screen),
];

fn layer_blend(graph: &Graph, layer: &Object, props: &Object) -> Result<BlendMode> {
    if let Some(mode) = props.fields.get("blendMode") {
        let id = graph.object(mode)?.int("value__")?;
        return MODES
            .iter()
            .find(|m| m.0 == id)
            .map(|m| m.2)
            .ok_or_else(|| unsupported(t("codec.pdn.feature.blend_mode")));
    }
    let properties = graph.object(layer.field("properties")?)?;
    let op = graph.object(properties.field("blendOp")?)?;
    MODES
        .iter()
        .find(|m| op.name == format!("PaintDotNet.UserBlendOps+{}BlendOp", m.1))
        .map(|m| m.2)
        .ok_or_else(|| unsupported(t("codec.pdn.feature.blend_mode")))
}

fn write(doc: &Document) -> Result<Vec<u8>> {
    use nrbf::Field::*;
    ensure!(
        doc.mode == ColorMode::Rgb && doc.depth == Depth::Eight,
        "{}",
        t("codec.layered.export_depth")
    );
    let length = layered::size(doc.width, doc.height, 4)?;
    ensure!(
        doc.tree.layers.len() <= MAX_LAYERS,
        "{}",
        t("codec.layered.too_large")
    );
    let mut remaining = MAX_BYTES;
    for layer in &doc.tree.layers {
        layered::check_layer(layer, false, false)?;
        layered::budget(&mut remaining, length)?;
        let bounds = layer.as_raster().unwrap().tiles.content_bounds();
        // PDN layers are canvas-sized, so refuse to discard off-canvas paint.
        ensure!(
            bounds.is_empty() || bounds.intersect(&doc.canvas_rect()) == bounds,
            "{}",
            schist_i18n::tf!("codec.layered.export_layer", name = layer.name)
        );
    }
    let mut w = nrbf::Writer::new();
    let list = w.id();
    let version = w.id();
    let items = w.id();
    let metadata = w.id();
    let keys = w.id();
    let values = w.id();
    let hash = w.id();
    let text = w.id();
    let comparer = w.id();
    let compare_info = w.id();
    w.class(
        1,
        "PaintDotNet.Document",
        Some(1),
        &[
            ("isDisposed", Bool(false)),
            ("layers", Ref(list)),
            ("width", Int(doc.width as i32)),
            ("height", Int(doc.height as i32)),
            ("savedWith", Ref(version)),
            ("userMetaData", Ref(metadata)),
        ],
    );
    w.class(
        version,
        "System.Version",
        None,
        &[
            ("_Major", Int(3)),
            ("_Minor", Int(510)),
            ("_Build", Int(4297)),
            ("_Revision", Int(28964)),
        ],
    );
    let layer_ids: Vec<_> = doc.tree.layers.iter().map(|_| w.id()).collect();
    w.class(
        list,
        "PaintDotNet.LayerList",
        Some(1),
        &[
            ("parent", Ref(1)),
            ("ArrayList+_items", Ref(items)),
            ("ArrayList+_size", Int(layer_ids.len() as i32)),
            ("ArrayList+_version", Int(1)),
        ],
    );
    w.array(items, &layer_ids);
    // Empty NameValueCollection, using its .NET 2.0 serialization contract.
    // Paint.NET's backwards-compatible loader accepts this original schema.
    w.class(
        metadata,
        "System.Collections.Specialized.NameValueCollection",
        Some(2),
        &[
            ("ReadOnly", Bool(false)),
            ("HashProvider", Ref(hash)),
            ("Comparer", Ref(comparer)),
            ("Count", Int(0)),
            ("Keys", Ref(keys)),
            ("Values", Ref(values)),
            ("Version", Int(1)),
        ],
    );
    w.string_array(keys);
    w.array(values, &[]);
    w.class(
        hash,
        "System.Collections.CaseInsensitiveHashCodeProvider",
        None,
        &[("m_text", Ref(text))],
    );
    w.class(
        text,
        "System.Globalization.TextInfo",
        None,
        &[
            ("m_listSeparator", Null),
            ("m_isReadOnly", Bool(true)),
            ("customCultureName", Null),
            ("m_nDataItem", Int(202)),
            ("m_useUserOverride", Bool(false)),
            ("m_win32LangID", Int(127)),
        ],
    );
    w.class(
        comparer,
        "System.Collections.CaseInsensitiveComparer",
        None,
        &[("m_compareInfo", Ref(compare_info))],
    );
    w.class(
        compare_info,
        "System.Globalization.CompareInfo",
        None,
        &[
            ("win32LCID", Int(127)),
            ("culture", Int(127)),
            ("m_name", Text("")),
        ],
    );
    for (layer, id) in doc.tree.layers.iter().zip(layer_ids) {
        let mode = MODES
            .iter()
            .find(|m| m.2 == layer.blend)
            .ok_or_else(|| unsupported(t("codec.pdn.feature.blend_mode")))?;
        let props = w.id();
        let bitmap_props = w.id();
        let surface = w.id();
        let memory = w.id();
        let blend = w.id();
        w.class(
            id,
            "PaintDotNet.BitmapLayer",
            Some(1),
            &[
                ("properties", Ref(bitmap_props)),
                ("surface", Ref(surface)),
                ("Layer+isDisposed", Bool(false)),
                ("Layer+width", Int(doc.width as i32)),
                ("Layer+height", Int(doc.height as i32)),
                ("Layer+properties", Ref(props)),
            ],
        );
        w.class(
            props,
            "PaintDotNet.Layer+LayerProperties",
            Some(1),
            &[
                ("name", Text(&layer.name)),
                ("userMetaData", Ref(metadata)),
                ("visible", Bool(layer.visible)),
                ("isBackground", Bool(false)),
                (
                    "opacity",
                    Byte((layer.opacity * layer.fill_opacity * 255.0 + 0.5) as u8),
                ),
            ],
        );
        w.class(
            bitmap_props,
            "PaintDotNet.BitmapLayer+BitmapLayerProperties",
            Some(1),
            &[("blendOp", Ref(blend))],
        );
        w.class(
            blend,
            &format!("PaintDotNet.UserBlendOps+{}BlendOp", mode.1),
            Some(1),
            &[],
        );
        w.class(
            surface,
            "PaintDotNet.Surface",
            Some(3),
            &[
                ("scan0", Ref(memory)),
                ("width", Int(doc.width as i32)),
                ("height", Int(doc.height as i32)),
                ("stride", Int(doc.width as i32 * 4)),
            ],
        );
        w.class(
            memory,
            "PaintDotNet.MemoryBlock",
            Some(3),
            &[
                ("length64", Long(length as i64)),
                ("hasParent", Bool(false)),
                ("deferred", Bool(true)),
            ],
        );
    }
    w.bytes.push(11); // MessageEnd, followed by deferred surfaces
    for layer in &doc.tree.layers {
        let mut bgra = Vec::with_capacity(length);
        for y in 0..doc.height {
            for x in 0..doc.width {
                let p = layer.as_raster().unwrap().tiles.pixel(x as i32, y as i32);
                for v in [p.b, p.g, p.r, p.a] {
                    bgra.push((v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
                }
            }
        }
        let chunk_size = 65536;
        w.bytes.push(0);
        w.bytes
            .extend_from_slice(&(chunk_size as u32).to_be_bytes());
        for (index, chunk) in bgra.chunks(chunk_size).enumerate() {
            let mut gzip =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gzip.write_all(chunk)?;
            let data = gzip.finish()?;
            w.bytes.extend_from_slice(&(index as u32).to_be_bytes());
            w.bytes
                .extend_from_slice(&(data.len() as u32).to_be_bytes());
            w.bytes.extend_from_slice(&data);
        }
    }
    let xml = format!("<pdnImage width=\"{}\" height=\"{}\" layers=\"{}\" savedWithVersion=\"3.510.4297.28964\"><custom /></pdnImage>", doc.width, doc.height, doc.tree.layers.len());
    let mut out = b"PDN3".to_vec();
    out.extend_from_slice(&(xml.len() as u32).to_le_bytes()[..3]);
    out.extend_from_slice(xml.as_bytes());
    out.extend_from_slice(&[0, 1]);
    out.extend_from_slice(&w.bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_chunks(indices: &[u32]) -> Vec<u8> {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&2u32.to_be_bytes());
        for &index in indices {
            bytes.extend_from_slice(&index.to_be_bytes());
            bytes.extend_from_slice(&2u32.to_be_bytes());
            bytes.extend_from_slice(&[index as u8 * 2, index as u8 * 2 + 1]);
        }
        bytes
    }

    #[test]
    fn chunks_follow_their_indices_not_stream_order() {
        let bytes = raw_chunks(&[2, 0, 1]);
        assert_eq!(
            chunks(&mut Reader::new(&bytes), 6).unwrap(),
            [0, 1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn invalid_chunk_indices_and_sizes_are_rejected() {
        for indices in [&[0, 0, 1][..], &[0, 1, 3][..]] {
            assert!(chunks(&mut Reader::new(&raw_chunks(indices)), 6).is_err());
        }
        let mut bytes = raw_chunks(&[0]);
        bytes[1..5].fill(0);
        assert!(chunks(&mut Reader::new(&bytes), 2).is_err());
        let mut bytes = raw_chunks(&[0]);
        bytes[9..13].copy_from_slice(&1u32.to_be_bytes());
        assert!(chunks(&mut Reader::new(&bytes), 2).is_err());
    }

    #[test]
    fn gzip_chunks_must_match_the_declared_output_size() {
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&[1, 2, 3]).unwrap();
        let data = gzip.finish().unwrap();
        let mut bytes = vec![0];
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&data);
        assert!(chunks(&mut Reader::new(&bytes), 2).is_err());
    }
}
