//! Adobe ACO swatches, ASE exchange palettes, and ACB color books.
//!
//! Layout references (no SDK headers):
//! https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/
//! https://ates.dev/pages/acb-spec/
//! https://github.com/nsfmc/swatch
//!
//! Source components are retained; RGB previews are approximations for
//! CMYK/spot colors, not ink separations or a substitute for an ICC profile.

use crate::{convert, Rgba};
use serde::{Deserialize, Serialize};

pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_COLORS: usize = 65_535;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Color {
    Rgb([f32; 3]),
    Hsb([f32; 3]),
    Cmyk([f32; 4]),
    Lab([f32; 3]),
    Gray(f32),
}

impl Color {
    pub fn to_rgb(&self) -> Rgba {
        match *self {
            Self::Rgb([r, g, b]) => Rgba::new(r, g, b, 1.0),
            Self::Gray(v) => Rgba::new(v, v, v, 1.0),
            Self::Cmyk(v) => convert::cmyk_to_rgb(v, 1.0),
            Self::Lab(v) => convert::lab_d50_to_rgb(v, 1.0),
            Self::Hsb([h, s, v]) => {
                let h = h.rem_euclid(1.0) * 6.0;
                let c = v * s;
                let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
                let m = v - c;
                let [r, g, b] = match h as u8 {
                    0 => [c, x, 0.0],
                    1 => [x, c, 0.0],
                    2 => [0.0, c, x],
                    3 => [0.0, x, c],
                    4 => [x, 0.0, c],
                    _ => [c, 0.0, x],
                };
                Rgba::new(r + m, g + m, b + m, 1.0)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub color: Color,
    pub group: String,
    pub spot: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub swatches: Vec<Swatch>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    TooLarge,
    Empty,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Diagnostic text; the app maps these categories to localized UI.
        write!(f, "palette: {self:?}")
    }
}

impl std::error::Error for Error {}

/// Detect by signature, rather than trusting a filename extension.
/// Reject the whole import on damage or unsupported color components;
/// silently dropping entries would make a named color book misleading.
pub fn decode(data: &[u8], fallback_name: &str) -> Result<Palette, Error> {
    if data.len() > MAX_FILE_BYTES {
        return Err(Error::TooLarge);
    }
    let mut r = Reader(data);
    let mut palette = if data.starts_with(b"8BCB") {
        acb(&mut r)?
    } else if data.starts_with(b"ASEF") {
        ase(&mut r)?
    } else if data.starts_with(&[0, 1]) || data.starts_with(&[0, 2]) {
        aco(&mut r)?
    } else {
        return Err(Error::Unsupported);
    };
    r.end()?;
    if palette.swatches.is_empty() {
        return Err(Error::Empty);
    }
    if palette.name.trim().is_empty() {
        palette.name = fallback_name.to_string();
    }
    Ok(palette)
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let bytes = self.0.get(..count).ok_or(Error::Invalid)?;
        self.0 = &self.0[count..];
        Ok(bytes)
    }

    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn float(&mut self, min: f32, max: f32) -> Result<f32, Error> {
        let v = f32::from_bits(self.u32()?);
        if !v.is_finite() || !(min..=max).contains(&v) {
            return Err(Error::Invalid);
        }
        Ok(v)
    }

    fn text(&mut self, units: u32, terminated: bool) -> Result<String, Error> {
        if units > 4096 {
            return Err(Error::TooLarge);
        }
        let size = usize::try_from(units).map_err(|_| Error::TooLarge)?;
        let bytes = self.take(size.checked_mul(2).ok_or(Error::TooLarge)?)?;
        let mut chars: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_be_bytes(c))
            .collect();
        if terminated && chars.pop() != Some(0) {
            return Err(Error::Invalid);
        }
        // ACB strings normally have no terminator, but some writers include one.
        if !terminated && chars.last() == Some(&0) {
            chars.pop();
        }
        String::from_utf16(&chars).map_err(|_| Error::Invalid)
    }

    fn text32(&mut self, terminated: bool) -> Result<String, Error> {
        let n = self.u32()?;
        self.text(n, terminated)
    }

    fn text16(&mut self) -> Result<String, Error> {
        let n = self.u16()?;
        self.text(n as u32, true)
    }

    fn end(&self) -> Result<(), Error> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Error::Invalid)
        }
    }
}

fn aco(r: &mut Reader<'_>) -> Result<Palette, Error> {
    let version = r.u16()?;
    let mut swatches = aco_section(r, version)?;
    if version == 1 && !r.0.is_empty() {
        if r.u16()? != 2 {
            return Err(Error::Unsupported);
        }
        let named = aco_section(r, 2)?;
        if named.len() != swatches.len() {
            return Err(Error::Invalid);
        }
        swatches = named;
    }
    Ok(Palette {
        name: String::new(),
        swatches,
    })
}

fn aco_section(r: &mut Reader<'_>, version: u16) -> Result<Vec<Swatch>, Error> {
    if !matches!(version, 1 | 2) {
        return Err(Error::Unsupported);
    }
    let count = r.u16()? as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        let space = r.u16()?;
        let v = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
        let unit = |i: usize| v[i] as f32 / 65535.0;
        let color = match space {
            0 => Color::Rgb([unit(0), unit(1), unit(2)]),
            1 => Color::Hsb([unit(0), unit(1), unit(2)]),
            2 => Color::Cmyk(v.map(|c| 1.0 - c as f32 / 65535.0)),
            7 if v[0] <= 10000
                && (-12800..=12700).contains(&(v[1] as i16))
                && (-12800..=12700).contains(&(v[2] as i16)) =>
            {
                Color::Lab([
                    v[0] as f32 / 100.0,
                    v[1] as i16 as f32 / 100.0,
                    v[2] as i16 as f32 / 100.0,
                ])
            }
            8 if v[0] <= 10000 => Color::Gray(v[0] as f32 / 10000.0),
            7 | 8 => return Err(Error::Invalid),
            // Legacy ink-library IDs contain opaque references, not RGB.
            _ => return Err(Error::Unsupported),
        };
        let name = if version == 2 {
            r.text32(true)?
        } else {
            String::new()
        };
        out.push(Swatch {
            name,
            color,
            group: String::new(),
            spot: false,
        });
    }
    Ok(out)
}

fn book_text(text: String) -> String {
    let text = if text.starts_with("$$$/") {
        text.split_once('=')
            .map_or(text.as_str(), |(_, value)| value)
    } else {
        &text
    };
    text.replace("^R", "®").replace("^C", "©")
}

fn acb(r: &mut Reader<'_>) -> Result<Palette, Error> {
    r.take(4)?;
    if r.u16()? != 1 {
        return Err(Error::Unsupported);
    }
    r.u16()?; // book ID
    let name = book_text(r.text32(false)?);
    let prefix = book_text(r.text32(false)?);
    let suffix = book_text(r.text32(false)?);
    r.text32(false)?; // description
    let count = r.u16()?;
    r.u16()?; // colors per page
    r.u16()?; // key color on page
    let space = r.u16()?;
    if !matches!(space, 0 | 2 | 7) {
        return Err(Error::Unsupported);
    }
    let mut swatches = Vec::new();
    let mut text_bytes = 0;
    for _ in 0..count {
        let raw_name = book_text(r.text32(false)?);
        r.take(6)?; // catalog code
        let v = r.take(if space == 2 { 4 } else { 3 })?;
        if raw_name.is_empty() {
            continue;
        } // page padding
        let color = match space {
            0 => Color::Rgb([
                v[0] as f32 / 255.0,
                v[1] as f32 / 255.0,
                v[2] as f32 / 255.0,
            ]),
            2 => Color::Cmyk([v[0], v[1], v[2], v[3]].map(|c| 1.0 - c as f32 / 255.0)),
            _ => Color::Lab([
                v[0] as f32 * 100.0 / 255.0,
                v[1] as f32 - 128.0,
                v[2] as f32 - 128.0,
            ]),
        };
        let name = format!("{prefix}{raw_name}{suffix}");
        text_bytes += name.len();
        if text_bytes > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        swatches.push(Swatch {
            name,
            color,
            group: String::new(),
            spot: true,
        });
    }
    if !r.0.is_empty() {
        let spot = match r.take(8)? {
            b"spflspot" => true,
            b"spflproc" => false,
            _ => return Err(Error::Invalid),
        };
        for swatch in &mut swatches {
            swatch.spot = spot;
        }
    }
    Ok(Palette { name, swatches })
}

fn ase(r: &mut Reader<'_>) -> Result<Palette, Error> {
    r.take(4)?;
    if r.u16()? != 1 || r.u16()? != 0 {
        return Err(Error::Unsupported);
    }
    let count = r.u32()? as usize;
    if count > r.0.len() / 6 {
        return Err(Error::Invalid);
    }
    let mut groups = Vec::<String>::new();
    let mut swatches = Vec::new();
    let mut text_bytes = 0;
    for _ in 0..count {
        let kind = r.u16()?;
        let len = r.u32()? as usize;
        let mut block = Reader(r.take(len)?);
        match kind {
            0xc001 => {
                if groups.len() == 16 {
                    return Err(Error::TooLarge);
                }
                groups.push(block.text16()?);
            }
            0xc002 => {
                groups.pop().ok_or(Error::Invalid)?;
            }
            1 => {
                if swatches.len() == MAX_COLORS {
                    return Err(Error::TooLarge);
                }
                let name = block.text16()?;
                let color = match block.take(4)? {
                    b"RGB " => Color::Rgb([
                        block.float(0.0, 1.0)?,
                        block.float(0.0, 1.0)?,
                        block.float(0.0, 1.0)?,
                    ]),
                    b"CMYK" => Color::Cmyk([
                        block.float(0.0, 1.0)?,
                        block.float(0.0, 1.0)?,
                        block.float(0.0, 1.0)?,
                        block.float(0.0, 1.0)?,
                    ]),
                    b"LAB " => Color::Lab([
                        block.float(0.0, 1.0)? * 100.0,
                        block.float(-128.0, 127.0)?,
                        block.float(-128.0, 127.0)?,
                    ]),
                    b"Gray" => Color::Gray(block.float(0.0, 1.0)?),
                    _ => return Err(Error::Unsupported),
                };
                let spot = match block.u16()? {
                    0 | 2 => false,
                    1 => true,
                    _ => return Err(Error::Invalid),
                };
                let group = groups.join(" / ");
                text_bytes += name.len() + group.len();
                if text_bytes > MAX_FILE_BYTES {
                    return Err(Error::TooLarge);
                }
                swatches.push(Swatch {
                    name,
                    color,
                    group,
                    spot,
                });
            }
            _ => continue, // unknown length-delimited extension block
        }
        block.end()?;
    }
    if !groups.is_empty() {
        return Err(Error::Invalid);
    }
    Ok(Palette {
        name: String::new(),
        swatches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(out: &mut Vec<u8>, n: u16) {
        out.extend(n.to_be_bytes());
    }
    fn long(out: &mut Vec<u8>, n: u32) {
        out.extend(n.to_be_bytes());
    }
    fn text(out: &mut Vec<u8>, name: &str, short: bool, terminated: bool) {
        let mut chars: Vec<u16> = name.encode_utf16().collect();
        if terminated {
            chars.push(0);
        }
        if short {
            word(out, chars.len() as u16);
        } else {
            long(out, chars.len() as u32);
        }
        for c in chars {
            word(out, c);
        }
    }

    fn aco_file(version: u16, entries: &[(u16, [u16; 4], &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        word(&mut out, version);
        word(&mut out, entries.len() as u16);
        for (space, values, name) in entries {
            word(&mut out, *space);
            for value in values {
                word(&mut out, *value);
            }
            if version == 2 {
                text(&mut out, name, false, true);
            }
        }
        out
    }

    fn acb_file(space: u16, entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = b"8BCB".to_vec();
        word(&mut out, 1);
        word(&mut out, 42);
        for value in [
            "$$$/test/title=Test Book^R",
            "$$$/test/prefix=Brand ",
            "$$$/test/postfix= C",
            "",
        ] {
            text(&mut out, value, false, false);
        }
        word(&mut out, entries.len() as u16);
        word(&mut out, 2);
        word(&mut out, 1);
        word(&mut out, space);
        for (name, values) in entries {
            text(&mut out, name, false, false);
            out.extend(b"000001");
            out.extend(*values);
        }
        out
    }

    fn ase_color(name: &str, model: &[u8; 4], values: &[f32], kind: u16) -> Vec<u8> {
        let mut out = Vec::new();
        text(&mut out, name, true, true);
        out.extend(model);
        for value in values {
            long(&mut out, value.to_bits());
        }
        word(&mut out, kind);
        out
    }

    fn ase_file(blocks: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let mut out = b"ASEF\0\x01\0\0".to_vec();
        long(&mut out, blocks.len() as u32);
        for (kind, block) in blocks {
            word(&mut out, *kind);
            long(&mut out, block.len() as u32);
            out.extend(block);
        }
        out
    }

    #[test]
    fn aco_v1_v2_and_combined_keep_one_copy_and_unicode_names() {
        let entries = [(0, [65535, 0, 0, 0], "Red 🟥"), (0, [0, 65535, 0, 0], "緑")];
        let v1 = aco_file(1, &entries);
        let v2 = aco_file(2, &entries);
        let combined = [v1.as_slice(), v2.as_slice()].concat();
        let old = decode(&v1, "Custom").unwrap();
        assert_eq!(old.name, "Custom");
        assert_eq!(old.swatches[0].color.to_rgb().to_u8(), [255, 0, 0, 255]);
        assert_eq!(old.swatches[0].name, "");
        let named = decode(&v2, "Custom").unwrap();
        assert_eq!(named.swatches.len(), 2);
        assert_eq!(named.swatches[0].name, "Red 🟥");
        assert_eq!(named.swatches[1].name, "緑");
        assert_eq!(decode(&combined, "Custom").unwrap(), named);
    }

    #[test]
    fn aco_color_spaces_have_distinct_scaling_and_signed_lab() {
        let bytes = aco_file(
            2,
            &[
                (1, [65535, 65535, 65535, 0], "Hue wrap"),
                (2, [0, 65535, 65535, 65535], "Cyan"),
                (8, [5000, 0, 0, 0], "Gray"),
                (7, [5000, (-2500i16) as u16, 1250, 0], "Lab"),
            ],
        );
        let swatches = decode(&bytes, "").unwrap().swatches;
        assert_eq!(swatches[0].color.to_rgb().to_u8(), [255, 0, 0, 255]);
        assert_eq!(swatches[1].color.to_rgb().to_u8(), [0, 255, 255, 255]);
        assert_eq!(swatches[2].color.to_rgb().to_u8(), [128, 128, 128, 255]);
        assert_eq!(swatches[3].color, Color::Lab([50.0, -25.0, 12.5]));
        assert_eq!(
            decode(&aco_file(1, &[(3, [0; 4], "")]), ""),
            Err(Error::Unsupported)
        );
        assert_eq!(
            decode(&aco_file(1, &[(8, [10001, 0, 0, 0], "")]), ""),
            Err(Error::Invalid)
        );
    }

    #[test]
    fn acb_preserves_book_names_affixes_and_ignores_page_padding() {
        let mut bytes = acb_file(0, &[("101", &[255, 0, 128]), ("", &[0, 0, 0])]);
        bytes.extend(b"spflproc");
        let book = decode(&bytes, "file name").unwrap();
        assert_eq!(book.name, "Test Book®");
        assert_eq!(book.swatches.len(), 1);
        assert_eq!(book.swatches[0].name, "Brand 101 C");
        assert_eq!(book.swatches[0].color.to_rgb().to_u8(), [255, 0, 128, 255]);
        assert!(!book.swatches[0].spot);
    }

    #[test]
    fn acb_cmyk_is_inverted_and_lab_has_unsigned_offsets() {
        let cmyk = decode(&acb_file(2, &[("Cyan", &[0, 255, 255, 255])]), "").unwrap();
        assert_eq!(cmyk.swatches[0].color.to_rgb().to_u8(), [0, 255, 255, 255]);
        let lab = decode(
            &acb_file(7, &[("White", &[255, 128, 128]), ("Tint", &[0, 0, 255])]),
            "",
        )
        .unwrap();
        assert!(lab.swatches[0].spot);
        assert_eq!(lab.swatches[0].color, Color::Lab([100.0, 0.0, 0.0]));
        assert_eq!(lab.swatches[0].color.to_rgb().to_u8(), [255; 4]);
        assert_eq!(lab.swatches[1].color, Color::Lab([0.0, -128.0, 127.0]));
    }

    #[test]
    fn d50_lab_matches_reference_srgb_colors() {
        // D50-adapted sRGB primaries, not the D65 Lab values used by
        // document mode conversion. A neutral-only test misses that bug.
        for (lab, expected) in [
            ([54.2917, 80.8125, 69.8851], [255, 0, 0, 255]),
            ([87.8181, -79.2873, 80.9902], [0, 255, 0, 255]),
            ([29.5676, 68.2986, -112.0294], [0, 0, 255, 255]),
            ([50.0, 0.0, 0.0], [119, 119, 119, 255]),
        ] {
            let actual = Color::Lab(lab).to_rgb().to_u8();
            for (a, b) in actual.into_iter().zip(expected) {
                assert!(a.abs_diff(b) <= 1, "{actual:?} != {expected:?}");
            }
        }
    }

    #[test]
    fn ase_reads_groups_spot_flags_and_all_component_models() {
        let mut group = Vec::new();
        text(&mut group, "印刷", true, true);
        let bytes = ase_file(&[
            (0xc001, group),
            (1, ase_color("Red", b"RGB ", &[1.0, 0.0, 0.0], 0)),
            (1, ase_color("Cyan", b"CMYK", &[1.0, 0.0, 0.0, 0.0], 2)),
            (1, ase_color("Lab", b"LAB ", &[0.5, -25.0, 12.5], 1)),
            (0xc002, vec![]),
            (1, ase_color("Gray", b"Gray", &[0.5], 2)),
        ]);
        let colors = decode(&bytes, "Exchange").unwrap().swatches;
        assert_eq!(colors.len(), 4);
        assert_eq!(colors[0].group, "印刷");
        assert_eq!(colors[0].color.to_rgb().to_u8(), [255, 0, 0, 255]);
        assert_eq!(colors[1].color.to_rgb().to_u8(), [0, 255, 255, 255]);
        assert_eq!(colors[2].color, Color::Lab([50.0, -25.0, 12.5]));
        assert!(colors[2].spot);
        assert_eq!(colors[3].color, Color::Gray(0.5));
        assert!(colors[3].group.is_empty());
    }

    #[test]
    fn ase_rejects_nonfinite_values_bad_groups_and_short_blocks() {
        for bad in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(decode(
                &ase_file(&[(1, ase_color("bad", b"RGB ", &[bad, 0.0, 0.0], 0))]),
                ""
            )
            .is_err());
        }
        assert!(decode(&ase_file(&[(0xc002, vec![])]), "").is_err());
        let mut bytes = ase_file(&[(1, ase_color("red", b"RGB ", &[1.0, 0.0, 0.0], 0))]);
        bytes[14..18].copy_from_slice(&2u32.to_be_bytes());
        assert!(decode(&bytes, "").is_err());
    }

    #[test]
    fn truncated_files_are_rejected_at_every_byte_boundary() {
        let fixtures = [
            aco_file(2, &[(0, [65535, 0, 0, 0], "Example")]),
            acb_file(7, &[("123", &[128, 90, 20])]),
            ase_file(&[(1, ase_color("Example", b"RGB ", &[1.0, 0.0, 0.0], 1))]),
        ];
        for file in fixtures {
            assert!(decode(&file, "test").is_ok());
            for end in 0..file.len() {
                assert!(
                    decode(&file[..end], "test").is_err(),
                    "accepted prefix {end}/{}",
                    file.len()
                );
            }
            let mut trailing = file;
            trailing.push(0);
            assert!(decode(&trailing, "test").is_err());
        }
    }

    #[test]
    fn corrupt_counts_names_and_empty_palettes_fail_without_allocating_from_counts() {
        assert_eq!(decode(&aco_file(1, &[]), ""), Err(Error::Empty));
        assert_eq!(decode(&ase_file(&[]), ""), Err(Error::Empty));
        let mut file = aco_file(2, &[(0, [0; 4], "a")]);
        file[14..18].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(decode(&file, "").is_err());
        let mut file = ase_file(&[]);
        file[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(decode(&file, "").is_err());
        let mut file = aco_file(2, &[(0, [0; 4], "a")]);
        file[18..20].copy_from_slice(&0xd800u16.to_be_bytes());
        assert!(decode(&file, "").is_err());
        assert_eq!(
            decode(&vec![0; MAX_FILE_BYTES + 1], ""),
            Err(Error::TooLarge)
        );
    }
}
