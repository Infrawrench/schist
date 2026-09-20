//! Extra merged planes and public PSD alpha-name/DisplayInfo resources.
//! Layout references: Adobe's public file-format specification and the
//! psd-tools project's published DisplayInfo documentation (no Adobe headers).
use schist_core::{Document, InkChannel, InkChannelInfo, InkTiles, PreservedResource, RawBlock};

pub const RESOURCE_IDS: [u16; 6] = [1006, 1007, 1045, 1053, 1067, 1077];
pub const STATE_KEY: [u8; 4] = *b"ScIn";

fn resource(doc: &Document, id: u16) -> Option<&[u8]> {
    doc.preserved_resources
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.data.as_slice())
}

/// Alternate Spot Colors: version/count, then (channel ID, 10-byte Color).
fn alternate(doc: &Document, id: u32) -> Option<&[u8]> {
    let bytes = resource(doc, 1067)?;
    if bytes.len() < 4 || bytes[..2] != 1u16.to_be_bytes() {
        return None;
    }
    let count = u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
    bytes[4..]
        .chunks_exact(14)
        .take(count)
        .find(|e| u32::from_be_bytes(e[..4].try_into().unwrap()) == id)
        .map(|e| &e[4..])
}

fn display_color(e: &[u8]) -> Option<[f32; 3]> {
    if e.len() < 10 {
        return None;
    }
    let word = |n: usize| u16::from_be_bytes([e[n], e[n + 1]]);
    let c = [word(2), word(4), word(6), word(8)].map(|v| v as f32 / 65535.0);
    Some(match word(0) {
        0 => [c[0], c[1], c[2]],
        2 => {
            let p = schist_color::NativePixel {
                mode: schist_color::ColorMode::Cmyk,
                color: c.map(|v| 1.0 - v),
                alpha: 1.0,
            }
            .to_rgba();
            [p.r, p.g, p.b]
        }
        7 => {
            let p = schist_color::convert::lab_d50_to_rgb(
                [
                    word(2) as f32 / 100.0,
                    word(4) as i16 as f32 / 100.0,
                    word(6) as i16 as f32 / 100.0,
                ],
                1.0,
            );
            [p.r, p.g, p.b]
        }
        8 => [word(2) as f32 / 10000.0; 3],
        _ => return None,
    })
}

pub fn read(doc: &mut Document, planes: &[Vec<f32>]) {
    let unicode = resource(doc, 1045).unwrap_or_default();
    let mut names = Vec::new();
    let mut remaining = unicode;
    while remaining.len() >= 4 {
        let count = u32::from_be_bytes(remaining[..4].try_into().unwrap()) as usize;
        remaining = &remaining[4..];
        let Some(bytes) = count.checked_mul(2).and_then(|n| remaining.get(..n)) else {
            break;
        };
        names.push(String::from_utf16_lossy(
            &bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        ));
        remaining = &remaining[bytes.len()..];
    }
    if names.is_empty() {
        let mut bytes = resource(doc, 1006).unwrap_or_default();
        while let Some((&count, rest)) = bytes.split_first() {
            let Some(name) = rest.get(..count as usize) else {
                break;
            };
            names.push(
                name.iter()
                    .map(|&b| {
                        if b < 128 {
                            b as char
                        } else {
                            MAC_ROMAN[(b - 128) as usize]
                        }
                    })
                    .collect(),
            );
            bytes = &rest[count as usize..];
        }
    }
    let ids: Vec<u32> = resource(doc, 1053)
        .unwrap_or_default()
        .chunks_exact(4)
        .map(|c| u32::from_be_bytes(c.try_into().unwrap()))
        .collect();
    let entries: Vec<Vec<u8>> =
        if let Some(bytes) = resource(doc, 1077).filter(|b| b.starts_with(&1u32.to_be_bytes())) {
            bytes[4..].chunks_exact(13).map(|c| c.to_vec()).collect()
        } else {
            resource(doc, 1007)
                .unwrap_or_default()
                .chunks_exact(14)
                .map(|c| c[..13].to_vec())
                .collect()
        };
    let visibility: Vec<(u32, bool)> = doc
        .preserved_layer_info
        .iter()
        .find(|b| b.key == STATE_KEY)
        .and_then(|b| serde_json::from_slice(&b.data).ok())
        .unwrap_or_default();
    let mut used = std::collections::HashSet::new();
    for (index, plane) in planes.iter().enumerate() {
        let mut id = ids.get(index).copied().unwrap_or(index as u32 + 1);
        while id == 0 || used.contains(&id) {
            id = id.wrapping_add(1);
        }
        used.insert(id);
        let entry = entries.get(index);
        let spot = entry.is_some_and(|e| e[12] == 2);
        let color = entry
            .and_then(|e| display_color(e))
            .or_else(|| alternate(doc, id).and_then(display_color))
            .unwrap_or([0.5; 3]);
        let solidity = entry.map_or(1.0, |e| {
            u16::from_be_bytes([e[10], e[11]]).min(100) as f32 / 100.0
        });
        let mut channel = InkChannel {
            info: InkChannelInfo {
                id,
                name: names
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| format!("Alpha {}", index + 1)),
                spot,
                color,
                solidity,
                visible: visibility
                    .iter()
                    .find(|(saved, _)| *saved == id)
                    .map_or(true, |(_, v)| *v),
                original_display: entry.cloned(),
            },
            pixels: InkTiles::default(),
        };
        for (i, value) in plane.iter().enumerate() {
            channel.pixels.set(
                (i % doc.width as usize) as i32,
                (i / doc.width as usize) as i32,
                if spot { 1.0 - value } else { *value },
            );
        }
        doc.ink_channels.push(channel);
    }
}

pub fn resources(doc: &Document) -> Vec<PreservedResource> {
    if doc.ink_channels.is_empty() {
        return Vec::new();
    }
    let (mut names, mut ids, mut display) = (Vec::new(), Vec::new(), 1u32.to_be_bytes().to_vec());
    for channel in &doc.ink_channels {
        let info = &channel.info;
        let utf16: Vec<_> = info.name.encode_utf16().collect();
        names.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
        for c in utf16 {
            names.extend_from_slice(&c.to_be_bytes());
        }
        ids.extend_from_slice(&info.id.to_be_bytes());
        if let Some(raw) = info.original_display.as_ref().filter(|b| b.len() == 13) {
            display.extend_from_slice(raw);
        } else {
            display.extend_from_slice(&0u16.to_be_bytes()); // RGB display colour
            for c in info.color {
                display.extend_from_slice(
                    &((c.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes(),
                );
            }
            display.extend_from_slice(&0u16.to_be_bytes());
            display.extend_from_slice(
                &((info.solidity.clamp(0.0, 1.0) * 100.0).round() as u16).to_be_bytes(),
            );
            display.push(if info.spot { 2 } else { 0 });
        }
    }
    let mut alternate_entries = Vec::new();
    for channel in doc.ink_channels.iter().filter(|c| c.info.spot) {
        let info = &channel.info;
        let raw = if info.original_display.is_some() {
            alternate(doc, info.id)
        } else {
            None
        };
        let mut color = Vec::new();
        if let Some(raw) = raw {
            color.extend_from_slice(raw);
        } else if let Some(original) = info.original_display.as_ref() {
            if original.len() < 10 || display_color(original).is_none() {
                continue;
            }
            color.extend_from_slice(&original[..10]);
        } else {
            color.extend_from_slice(&0u16.to_be_bytes());
            for value in info.color {
                color.extend_from_slice(
                    &((value.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes(),
                );
            }
            color.extend_from_slice(&0u16.to_be_bytes());
        }
        alternate_entries.extend_from_slice(&info.id.to_be_bytes());
        alternate_entries.extend(color);
    }
    let mut entries = vec![(1045, names), (1053, ids), (1077, display)];
    if !alternate_entries.is_empty() {
        let mut data = 1u16.to_be_bytes().to_vec();
        data.extend_from_slice(&((alternate_entries.len() / 14) as u16).to_be_bytes());
        data.extend(alternate_entries);
        entries.push((1067, data));
    }
    entries
        .into_iter()
        .map(|(id, data)| PreservedResource {
            id,
            name: vec![0, 0],
            data,
        })
        .collect()
}

pub fn state(doc: &Document) -> Option<RawBlock> {
    (!doc.ink_channels.is_empty()).then(|| RawBlock {
        key: STATE_KEY,
        data: serde_json::to_vec(
            &doc.ink_channels
                .iter()
                .map(|c| (c.info.id, c.info.visible))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    })
}

/// Retain the exact imported metadata when rewriting understood resources.
/// This also keeps unfamiliar DisplayInfo versions and legacy name encodings.
pub fn resource_backup(doc: &Document) -> Option<RawBlock> {
    if doc.preserved_layer_info.iter().any(|b| b.key == *b"ScIr") {
        return None;
    }
    let originals: Vec<_> = doc
        .preserved_resources
        .iter()
        .filter(|r| RESOURCE_IDS.contains(&r.id))
        .map(|r| (r.id, &r.name, &r.data))
        .collect();
    (!originals.is_empty() && (doc.ink_channels_loaded || !doc.ink_channels.is_empty())).then(
        || RawBlock {
            key: *b"ScIr",
            data: serde_json::to_vec(&originals).unwrap(),
        },
    )
}

// High half of the standard Macintosh Roman character encoding.
const MAC_ROMAN: [char; 128] = [
    '\u{c4}', '\u{c5}', '\u{c7}', '\u{c9}', '\u{d1}', '\u{d6}', '\u{dc}', '\u{e1}', '\u{e0}',
    '\u{e2}', '\u{e4}', '\u{e3}', '\u{e5}', '\u{e7}', '\u{e9}', '\u{e8}', '\u{ea}', '\u{eb}',
    '\u{ed}', '\u{ec}', '\u{ee}', '\u{ef}', '\u{f1}', '\u{f3}', '\u{f2}', '\u{f4}', '\u{f6}',
    '\u{f5}', '\u{fa}', '\u{f9}', '\u{fb}', '\u{fc}', '\u{2020}', '\u{b0}', '\u{a2}', '\u{a3}',
    '\u{a7}', '\u{2022}', '\u{b6}', '\u{df}', '\u{ae}', '\u{a9}', '\u{2122}', '\u{b4}', '\u{a8}',
    '\u{2260}', '\u{c6}', '\u{d8}', '\u{221e}', '\u{b1}', '\u{2264}', '\u{2265}', '\u{a5}',
    '\u{b5}', '\u{2202}', '\u{2211}', '\u{220f}', '\u{3c0}', '\u{222b}', '\u{aa}', '\u{ba}',
    '\u{3a9}', '\u{e6}', '\u{f8}', '\u{bf}', '\u{a1}', '\u{ac}', '\u{221a}', '\u{192}', '\u{2248}',
    '\u{2206}', '\u{ab}', '\u{bb}', '\u{2026}', '\u{a0}', '\u{c0}', '\u{c3}', '\u{d5}', '\u{152}',
    '\u{153}', '\u{2013}', '\u{2014}', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '\u{f7}',
    '\u{25ca}', '\u{ff}', '\u{178}', '\u{2044}', '\u{20ac}', '\u{2039}', '\u{203a}', '\u{fb01}',
    '\u{fb02}', '\u{2021}', '\u{b7}', '\u{201a}', '\u{201e}', '\u{2030}', '\u{c2}', '\u{ca}',
    '\u{c1}', '\u{cb}', '\u{c8}', '\u{cd}', '\u{ce}', '\u{cf}', '\u{cc}', '\u{d3}', '\u{d4}',
    '\u{f8ff}', '\u{d2}', '\u{da}', '\u{db}', '\u{d9}', '\u{131}', '\u{2c6}', '\u{2dc}', '\u{af}',
    '\u{2d8}', '\u{2d9}', '\u{2da}', '\u{b8}', '\u{2dd}', '\u{2db}', '\u{2c7}',
];
