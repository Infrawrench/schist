mod common;
use common::{Psd, Res, L};
use schist_codec_psd::{read_psd, write_psd_with};
use schist_color::{ColorMode, Depth};
use schist_core::{Document, InkChannel, Layer};

fn fixture(depth: Depth, psb: bool, layered: bool) -> Vec<u8> {
    let mut psd = Psd::rgb8(3, 1);
    psd.version = if psb { 2 } else { 1 };
    psd.depth = (depth.bytes_per_channel() * 8) as u16;
    psd.channels = if layered { 6 } else { 5 };
    psd.negative_count = layered;
    if layered {
        psd.layers.push(L {
            rect: (0, 0, 1, 3),
            raw_planes: Some(vec![
                (0, samples(&[0.25; 3], depth)),
                (1, samples(&[0.5; 3], depth)),
                (2, samples(&[0.75; 3], depth)),
                (-1, samples(&[0.5; 3], depth)),
            ]),
            ..L::default()
        });
    }
    // A preserved ordinary alpha followed by a named spot: independent,
    // manually encoded DisplayInfo, UTF-16 name and identifier resources.
    let mut display = vec![0, 0, 0, 1];
    display.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 50, 0]);
    display.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 35, 2]);
    let mut names = Vec::new();
    for name in ["Selection", "Cyan — 特別"] {
        let codes: Vec<_> = name.encode_utf16().collect();
        names.extend_from_slice(&(codes.len() as u32).to_be_bytes());
        for code in codes {
            names.extend_from_slice(&code.to_be_bytes());
        }
    }
    psd.resources = vec![
        Res {
            id: 1077,
            name: vec![],
            data: display,
        },
        Res {
            id: 1045,
            name: vec![],
            data: names,
        },
        Res {
            id: 1053,
            name: vec![],
            data: [19u32.to_be_bytes(), 27u32.to_be_bytes()].concat(),
        },
        Res {
            id: 3333,
            name: vec![],
            data: b"unknown image resource".to_vec(),
        },
    ];
    let mut bytes = psd.build();
    bytes.truncate(bytes.len() - 2 - psd.channels as usize * 3 * depth.bytes_per_channel());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    for value in [0.25, 0.5, 0.75] {
        bytes.extend(samples(&[value; 3], depth));
    }
    if layered {
        bytes.extend(samples(&[0.5; 3], depth));
    }
    bytes.extend(samples(&[0.0, 0.25, 1.0], depth)); // ordinary alpha
    bytes.extend(samples(&[1.0, 0.25, 0.0], depth)); // no ink, 75% ink, full ink
    bytes
}
fn samples(values: &[f32], depth: Depth) -> Vec<u8> {
    values
        .iter()
        .flat_map(|&v| match depth {
            Depth::Eight => vec![(v * 255.0).round() as u8],
            Depth::Sixteen => ((v * 65535.0).round() as u16).to_be_bytes().to_vec(),
            Depth::ThirtyTwo => v.to_be_bytes().to_vec(),
        })
        .collect()
}

#[test]
fn independent_spot_fixture_retains_alpha_and_ink_at_every_depth_in_psd_and_psb() {
    for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
        for psb in [false, true] {
            for layered in [false, true] {
                let source = read_psd(&fixture(depth, psb, layered)).unwrap();
                assert_eq!(source.ink_channels.len(), 2);
                assert!(!source.ink_channels[0].info.spot);
                let ink = &source.ink_channels[1];
                assert!(ink.info.spot);
                assert_eq!(ink.info.name, "Cyan — 特別");
                assert_eq!(ink.info.id, 27);
                assert_eq!(ink.info.color, [0.0, 1.0, 1.0]);
                assert_eq!(ink.info.solidity, 0.35);
                assert_eq!(ink.pixels.value(0, 0), 0.0);
                assert!((ink.pixels.value(1, 0) - 0.75).abs() < 0.002);
                assert_eq!(ink.pixels.value(2, 0), 1.0);
                let bytes = write_psd_with(&source, psb).unwrap();
                let reopened = read_psd(&bytes).unwrap();
                assert_eq!(reopened.ink_channels.len(), 2);
                for (before, after) in source.ink_channels.iter().zip(&reopened.ink_channels) {
                    assert_eq!(before.info.name, after.info.name);
                    assert_eq!(before.info.original_display, after.info.original_display);
                    for x in 0..3 {
                        assert_eq!(before.pixels.value(x, 0), after.pixels.value(x, 0));
                    }
                }
                assert!(reopened
                    .preserved_resources
                    .iter()
                    .any(|r| r.id == 3333 && r.data == b"unknown image resource"));
            }
        }
    }
}

#[test]
fn authored_channels_export_without_burning_ink_into_process_planes() {
    for mode in [ColorMode::Rgb, ColorMode::Cmyk] {
        let mut doc = Document::new("spot", 1, 1, Depth::Sixteen);
        doc.mode = mode;
        doc.push_layer(Layer::new_raster("Process"));
        let mut channel = InkChannel::spot("Varnish".into(), [1.0, 0.0, 0.0]);
        channel.info.visible = false;
        channel.pixels.set(0, 0, 0.875);
        doc.ink_channels.push(channel);
        doc.ink_preview = schist_core::InkPreview::Overprint;
        for psb in [false, true] {
            let bytes = write_psd_with(&doc, psb).unwrap();
            assert_eq!(
                u16::from_be_bytes(bytes[12..14].try_into().unwrap()) as usize,
                mode.channels() + 2
            );
            let reopened = read_psd(&bytes).unwrap();
            assert!(!reopened.ink_channels[0].info.visible);
            assert!((reopened.ink_channels[0].pixels.value(0, 0) - 0.875).abs() < 0.0001);
            assert_eq!(
                reopened.tree.layers[0]
                    .as_raster()
                    .unwrap()
                    .tiles
                    .native_pixel(0, 0)
                    .alpha,
                0.0
            );
            assert_eq!(reopened.ink_preview, schist_core::InkPreview::Process);
        }
    }
}

#[test]
fn plate_only_document_and_delete_last_plate_round_trip() {
    let mut doc = Document::new("plates", 1, 1, Depth::Eight);
    let mut channel = InkChannel::spot("Ink".into(), [0.0; 3]);
    channel.pixels.set(0, 0, 1.0);
    doc.ink_channels.push(channel);
    let mut reopened = read_psd(&write_psd_with(&doc, false).unwrap()).unwrap();
    assert_eq!(reopened.ink_channels.len(), 1);
    assert_eq!(reopened.ink_channels[0].pixels.value(0, 0), 1.0);
    let mut edit = reopened.begin_edit("delete");
    edit.change_ink_channels(|c| c.clear());
    edit.commit();
    let deleted = read_psd(&write_psd_with(&reopened, false).unwrap()).unwrap();
    assert!(deleted.ink_channels.is_empty());
    assert!(!deleted
        .preserved_resources
        .iter()
        .any(|r| [1045, 1053, 1077].contains(&r.id)));
}

#[test]
fn zipped_spot_planes_and_unknown_metadata_keep_samples_and_original_bytes() {
    for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
        for prediction in [false, true] {
            let mut bytes = fixture(depth, true, true);
            let data_len = 6 * 3 * depth.bytes_per_channel();
            let raw = bytes.split_off(bytes.len() - data_len);
            bytes.truncate(bytes.len() - 2);
            bytes.extend_from_slice(&(if prediction { 3u16 } else { 2u16 }).to_be_bytes());
            bytes.extend(schist_codec_psd::zip::encode_channel(
                &raw,
                6,
                3 * depth.bytes_per_channel(),
                depth,
                prediction,
            ));
            let doc = read_psd(&bytes).unwrap();
            assert_eq!(doc.ink_channels.len(), 2);
            assert!((doc.ink_channels[1].pixels.value(1, 0) - 0.75).abs() < 0.002);
        }
    }
    let mut doc = read_psd(&fixture(Depth::Eight, false, true)).unwrap();
    // Retain unknown display-space bytes while explicitly editing just the name.
    let original = vec![0, 42, 1, 2, 3, 4, 5, 6, 7, 8, 0, 35, 2];
    doc.ink_channels[1].info.original_display = Some(original.clone());
    let mut edit = doc.begin_edit("rename");
    edit.change_ink_channels(|c| c[1].info.name = "Renamed ink".into());
    edit.commit();
    let again = read_psd(&write_psd_with(&doc, false).unwrap()).unwrap();
    assert_eq!(again.ink_channels[1].info.original_display, Some(original));
    assert_eq!(again.ink_channels[1].info.name, "Renamed ink");
    assert!(again.preserved_layer_info.iter().any(|b| b.key == *b"ScIr"));
}
