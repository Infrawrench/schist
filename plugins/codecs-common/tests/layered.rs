use schist_codecs_common::{CommonCodecsPlugin, KraCodec, OraCodec, PdnCodec, XcfCodec};
use schist_color::Depth;
use schist_core::{blit_rgba8, blit_rgba_f32, BlendMode, Document, IntRect, Layer, LayerKind};
use schist_plugin_api::{CodecPlugin, PluginManifest, PluginRegistry};

const PDN3: &[u8] = include_bytes!("../../../fixtures/layered/paintnet-3.pdn");
const PDN4: &[u8] = include_bytes!("../../../fixtures/layered/paintnet-4.pdn");
const XCF: &[u8] = include_bytes!("../../../fixtures/layered/gimp-rle.xcf");
const XCF16: &[u8] = include_bytes!("../../../fixtures/layered/gimp-group-16.xcf");

fn sample_doc() -> Document {
    let mut doc = Document::new("Interchange", 67, 65, Depth::Eight);
    let mut background = Layer::new_raster("Background");
    let mut rgba = vec![0; 67 * 65 * 4];
    for (i, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        pixel.copy_from_slice(&[(i % 251) as u8, (i % 127) as u8, (i % 63) as u8, 255]);
    }
    blit_rgba8(
        &mut background.as_raster_mut().unwrap().tiles,
        doc.depth,
        doc.canvas_rect(),
        &rgba,
    );
    let mut top = Layer::new_raster("青 • top");
    blit_rgba8(
        &mut top.as_raster_mut().unwrap().tiles,
        doc.depth,
        IntRect::from_xywh(63, 63, 2, 2),
        &[20, 40, 60, 128].repeat(4),
    );
    top.visible = false;
    top.opacity = 128.0 / 255.0;
    top.blend = BlendMode::Multiply;
    doc.tree.layers = vec![background, top];
    doc
}

fn assert_layers(a: &[Layer], b: &[Layer], tolerance: f32) {
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(b) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.visible, b.visible);
        assert!((a.opacity - b.opacity).abs() <= tolerance);
        assert_eq!(a.blend, b.blend);
        assert_eq!(a.is_group(), b.is_group());
        if let (Some(a), Some(b)) = (a.children(), b.children()) {
            assert_layers(a, b, tolerance);
        } else {
            let a = &a.as_raster().unwrap().tiles;
            let b = &b.as_raster().unwrap().tiles;
            for y in -2..67 {
                for x in -2..69 {
                    let a = a.pixel(x, y);
                    let b = b.pixel(x, y);
                    for (a, b) in [a.r, a.g, a.b, a.a].into_iter().zip([b.r, b.g, b.b, b.a]) {
                        assert!((a - b).abs() <= tolerance, "pixel ({x}, {y}): {a} != {b}");
                    }
                }
            }
        }
    }
}

#[test]
fn native_codecs_register_for_content_and_case_insensitive_extensions() {
    let mut registry = PluginRegistry::new();
    CommonCodecsPlugin.register(&mut registry);
    for (magic, ext, id) in [
        (b"PDN3".as_slice(), "PDN", "codec.pdn"),
        (b"gimp xcf ".as_slice(), "XCF", "codec.xcf"),
    ] {
        assert_eq!(registry.codec_for(magic, None).unwrap().id(), id);
        let codec = registry.codec_for(&[], Some(ext)).unwrap();
        assert_eq!(codec.id(), id);
        assert!(codec.can_export());
    }
    let ora = OraCodec.export(&sample_doc()).unwrap();
    assert_eq!(registry.codec_for(&ora, None).unwrap().id(), "codec.ora");
    assert_eq!(
        registry.codec_for(&[], Some("ORA")).unwrap().id(),
        "codec.ora"
    );
    for ext in ["kra", "KRZ"] {
        let codec = registry.codec_for(&[], Some(ext)).unwrap();
        assert_eq!(codec.id(), "codec.kra");
        assert!(!codec.can_export());
    }
    // A Krita package is recognised by its stored mimetype, not its name.
    let kra = schist_codec_idml::container::write(&[(
        "mimetype".into(),
        b"application/x-krita".to_vec(),
    )]);
    assert_eq!(registry.codec_for(&kra, None).unwrap().id(), "codec.kra");
}

#[test]
fn paintnet_3_and_4_files_keep_real_layers_and_pixels() {
    for bytes in [PDN3, PDN4] {
        let doc = PdnCodec.import(bytes).unwrap();
        assert_eq!(
            (doc.width, doc.height, doc.tree.layers.len()),
            (800, 600, 2)
        );
        assert_eq!(doc.tree.layers[0].name, "Background");
        assert_eq!(doc.tree.layers[1].name, "Layer 2");
        assert_eq!(
            doc.tree.layers[0].as_raster().unwrap().tiles.pixel(0, 0).a,
            1.0
        );
        assert!(!doc.dirty);
    }
    let doc = PdnCodec.import(PDN4).unwrap();
    assert_eq!(doc.tree.layers[1].blend, BlendMode::LinearDodge);
    assert_eq!(doc.tree.layers[1].opacity, 161.0 / 255.0);
}

#[test]
fn gimp_rle_tiles_and_negative_offsets() {
    let doc = XcfCodec.import(XCF).unwrap();
    assert_eq!((doc.width, doc.height), (67, 65));
    assert_eq!(doc.depth, Depth::Eight);
    assert_eq!(doc.tree.layers.len(), 2);
    assert_eq!(doc.tree.layers[0].name, "Red background");
    assert_eq!(
        doc.tree.layers[0]
            .as_raster()
            .unwrap()
            .tiles
            .pixel(66, 64)
            .r,
        1.0
    );
    let top = &doc.tree.layers[1];
    assert_eq!(top.name, "Blue offset");
    assert_eq!(top.blend, BlendMode::Multiply);
    assert!((top.opacity - 0.5).abs() < 0.003);
    assert_eq!(top.as_raster().unwrap().tiles.pixel(-1, 1).b, 1.0);
    assert_eq!(top.as_raster().unwrap().tiles.pixel(2, 1).a, 0.0);
}

#[test]
fn gimp_high_precision_group_and_disabled_mask() {
    let doc = XcfCodec.import(XCF16).unwrap();
    assert_eq!(doc.depth, Depth::Sixteen);
    assert_eq!(doc.tree.layers.len(), 2);
    let group = &doc.tree.layers[1];
    assert_eq!(group.name, "Group");
    let layer = &group.children().unwrap()[0];
    assert_eq!(layer.name, "Blue offset");
    let mask = layer.mask.as_ref().unwrap();
    assert!(!mask.enabled);
    // GIMP converts the sRGB foreground gray to linear mask coverage.
    assert!(mask.value(-1, 1).abs_diff(55) <= 1);
    let again = XcfCodec.import(&XcfCodec.export(&doc).unwrap()).unwrap();
    assert_layers(&doc.tree.layers, &again.tree.layers, 0.0001);
    let mask2 = again.tree.layers[1].children().unwrap()[0]
        .mask
        .as_ref()
        .unwrap();
    assert_eq!(mask.enabled, mask2.enabled);
    assert_eq!(mask.value(-1, 1), mask2.value(-1, 1));
}

#[test]
fn native_exports_preserve_layer_order_names_alpha_and_opacity() {
    for codec in [&PdnCodec as &dyn CodecPlugin, &XcfCodec, &OraCodec] {
        let doc = sample_doc();
        let bytes = codec.export(&doc).unwrap();
        assert!(codec.probe(&bytes));
        let again = codec.import(&bytes).unwrap();
        assert_eq!((again.width, again.height), (doc.width, doc.height));
        assert_layers(&doc.tree.layers, &again.tree.layers, 0.0001);
    }
}

#[test]
fn openraster_round_trips_groups_depth_profile_and_selection() {
    let mut doc = XcfCodec.import(XCF16).unwrap();
    if let LayerKind::Group(g) = &mut doc.tree.layers[1].kind {
        g.children[0].mask = None;
    }
    doc.tree.layers[1].blend = BlendMode::Screen;
    doc.tree.layers[1].locked = true;
    doc.icc_profile = None;
    doc.active_layer = Some(doc.tree.layers[0].id);
    let bytes = OraCodec.export(&doc).unwrap();
    let again = OraCodec.import(&bytes).unwrap();
    assert_eq!(again.depth, Depth::Sixteen);
    assert_eq!(again.tree.layers[1].blend, BlendMode::Screen);
    assert!(again.tree.layers[1].locked);
    assert_eq!(again.active_layer, Some(again.tree.layers[0].id));
    assert_layers(&doc.tree.layers, &again.tree.layers, 1.0 / 65535.0);

    // Pass-through groups survive as non-isolated stacks.
    doc.tree.layers[1].blend = BlendMode::PassThrough;
    let again = OraCodec.import(&OraCodec.export(&doc).unwrap()).unwrap();
    assert_eq!(again.tree.layers[1].blend, BlendMode::PassThrough);

    let mut doc = sample_doc();
    doc.resolution_dpi = 300.0;
    doc.icc_profile = Some(sample_profile());
    let again = OraCodec.import(&OraCodec.export(&doc).unwrap()).unwrap();
    assert_eq!(again.resolution_dpi, 300.0);
    assert_eq!(again.icc_profile, doc.icc_profile);
}

/// A small but structurally valid ICC profile, since PNG encoders check.
fn sample_profile() -> Vec<u8> {
    let mut icc = vec![0u8; 132];
    icc[..4].copy_from_slice(&132u32.to_be_bytes());
    icc[36..40].copy_from_slice(b"acsp");
    icc
}

#[test]
fn openraster_keeps_unknown_attributes_and_operators() {
    let png = {
        let mut doc = Document::new("1px", 1, 1, Depth::Eight);
        let mut l = Layer::new_raster("p");
        blit_rgba8(
            &mut l.as_raster_mut().unwrap().tiles,
            Depth::Eight,
            doc.canvas_rect(),
            &[1, 2, 3, 255],
        );
        doc.tree.layers.push(l);
        let ora = OraCodec.export(&doc).unwrap();
        schist_codec_idml::container::read(&ora)
            .unwrap()
            .get("data/layer1.png")
            .unwrap()
            .to_vec()
    };
    let stack = r#"<?xml version="1.0"?>
<image version="0.0.5" w="4" h="4" xmlns:mypaint="http://mypaint.org/ns/openraster">
 <stack>
  <layer src="data/a.png" name="kept" composite-op="svg:dst-out" mypaint:locked="true" krita:unknown="x &amp; y" edit-locked="true"/>
 </stack>
</image>"#;
    let bytes = schist_codec_idml::container::write(&[
        ("mimetype".into(), b"image/openraster".to_vec()),
        ("stack.xml".into(), stack.as_bytes().to_vec()),
        ("data/a.png".into(), png),
    ]);
    let doc = OraCodec.import(&bytes).unwrap();
    let layer = &doc.tree.layers[0];
    assert_eq!(layer.blend, BlendMode::Normal);
    assert!(layer.locked);
    // The preserved attributes do not block saving to other formats.
    XcfCodec.export(&doc).unwrap();
    let out = OraCodec.export(&doc).unwrap();
    let package = schist_codec_idml::container::read(&out).unwrap();
    let xml = package.text("stack.xml").unwrap();
    for needle in [
        r#"composite-op="svg:dst-out""#,
        r#"mypaint:locked="true""#,
        r#"krita:unknown="x &amp; y""#,
        r#"xmlns:mypaint="http://mypaint.org/ns/openraster""#,
    ] {
        assert!(xml.contains(needle), "{needle} missing from {xml}");
    }
    assert!(package.get("mergedimage.png").is_some());
    let thumb = image::load_from_memory(package.get("Thumbnails/thumbnail.png").unwrap()).unwrap();
    assert_eq!((thumb.width(), thumb.height()), (4, 4));
    // Changing the mode drops the stale operator.
    let mut doc = OraCodec.import(&out).unwrap();
    doc.tree.layers[0].blend = BlendMode::Multiply;
    let xml = String::from_utf8(
        schist_codec_idml::container::read(&OraCodec.export(&doc).unwrap())
            .unwrap()
            .get("stack.xml")
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(xml.contains(r#"composite-op="svg:multiply""#), "{xml}");
}

#[test]
fn openraster_rejects_masks_and_non_rgb_documents() {
    let doc = XcfCodec.import(XCF16).unwrap();
    assert!(
        OraCodec.export(&doc).is_err(),
        "masks must be applied first"
    );
    let mut doc = sample_doc();
    doc.tree.layers[0].clipping = true;
    assert!(OraCodec.export(&doc).is_err());
}

#[test]
fn xcf_float_export_keeps_hdr_values_and_profile() {
    let mut doc = Document::new("Float", 2, 1, Depth::ThirtyTwo);
    let mut layer = Layer::new_raster("HDR");
    blit_rgba_f32(
        &mut layer.as_raster_mut().unwrap().tiles,
        doc.depth,
        doc.canvas_rect(),
        &[1.5, 0.125, -0.1, 1.0, 0.2, 0.4, 0.6, 0.5],
    );
    doc.tree.layers.push(layer);
    doc.resolution_dpi = 300.0;
    doc.icc_profile = Some(vec![1, 2, 3, 4]);
    let again = XcfCodec.import(&XcfCodec.export(&doc).unwrap()).unwrap();
    assert_eq!(again.depth, Depth::ThirtyTwo);
    assert_eq!(again.resolution_dpi, 300.0);
    assert_eq!(again.icc_profile, doc.icc_profile);
    assert_layers(&doc.tree.layers, &again.tree.layers, 0.00001);
}

#[test]
fn xcf_keeps_mask_coverage_beyond_the_painted_pixels() {
    let mut doc = Document::new("Mask extent", 3, 1, Depth::Eight);
    let mut layer = Layer::new_raster("Masked");
    blit_rgba8(
        &mut layer.as_raster_mut().unwrap().tiles,
        doc.depth,
        IntRect::from_size(1, 1),
        &[255, 0, 0, 255],
    );
    let mut mask = schist_core::LayerMask::new_revealing();
    mask.bounds = doc.canvas_rect();
    mask.tiles
        .get_mut_or_insert(schist_core::TileCoord::containing(0, 0))[..3]
        .copy_from_slice(&[255, 128, 0]);
    layer.mask = Some(mask);
    doc.tree.layers.push(layer);
    let again = XcfCodec.import(&XcfCodec.export(&doc).unwrap()).unwrap();
    let mask = again.tree.layers[0].mask.as_ref().unwrap();
    assert_eq!(mask.bounds, doc.canvas_rect());
    assert_eq!(mask.value(1, 0), 128);
    assert_eq!(mask.value(2, 0), 0);
}

#[test]
fn unsupported_exports_fail_before_returning_bytes() {
    let mut doc = sample_doc();
    doc.tree.layers[0].clipping = true;
    assert!(PdnCodec.export(&doc).is_err());
    assert!(XcfCodec.export(&doc).is_err());
    doc.tree.layers[0].clipping = false;
    doc.tree.layers.push(Layer::new_group("Group"));
    assert!(PdnCodec.export(&doc).is_err());
    doc.tree.layers.pop();
    doc.depth = Depth::Sixteen;
    assert!(PdnCodec.export(&doc).is_err());
}

#[test]
fn damaged_files_return_errors() {
    for (codec, bytes) in [(&PdnCodec as &dyn CodecPlugin, PDN4), (&XcfCodec, XCF16)] {
        for n in [0, 1, 4, 9, 13, 20, 28, bytes.len() / 2] {
            assert!(
                codec.import(&bytes[..n]).is_err(),
                "{} accepts truncated file at {n}",
                codec.id()
            );
        }
        // Native GIMP files can end with unused mipmap structures. Truncate
        // a writer output whose final bytes are required compressed pixels.
        let exported = codec.export(&sample_doc()).unwrap();
        assert!(codec.import(&exported[..exported.len() - 1]).is_err());
    }
    let ora = OraCodec.export(&sample_doc()).unwrap();
    for n in (0..ora.len()).step_by(37) {
        assert!(OraCodec.import(&ora[..n]).is_err(), "truncated ORA at {n}");
    }
    let mut flipped = ora.clone();
    for i in (0..flipped.len()).step_by(13) {
        flipped[i] ^= 0xa5;
        let _ = OraCodec.import(&flipped);
        let _ = KraCodec.import(&flipped);
    }
    let mut huge = XCF.to_vec();
    huge[14..18].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(XcfCodec.import(&huge).is_err());
    let mut future = XCF.to_vec();
    future[9..13].copy_from_slice(b"v999");
    assert!(XcfCodec.import(&future).is_err());
}

// Optional artifacts for the independent GIMP/Python interoperability check
// described in docs/layered-formats.md. Normal tests write no files.
#[test]
fn interoperability_exports() {
    let Some(dir) = std::env::var_os("SCHIST_LAYERED_INTEROP_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let doc = sample_doc();
    std::fs::write(dir.join("schist.pdn"), PdnCodec.export(&doc).unwrap()).unwrap();
    std::fs::write(dir.join("schist.xcf"), XcfCodec.export(&doc).unwrap()).unwrap();
    std::fs::write(dir.join("schist.ora"), OraCodec.export(&doc).unwrap()).unwrap();
    let doc = XcfCodec.import(XCF16).unwrap();
    std::fs::write(dir.join("schist-16.xcf"), XcfCodec.export(&doc).unwrap()).unwrap();
    if let LayerKind::Group(_) = doc.tree.layers[1].kind {
    } else {
        panic!("missing group");
    }
}
