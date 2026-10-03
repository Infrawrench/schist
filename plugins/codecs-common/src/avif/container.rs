//! The HEIF/MIAF box structure an AVIF still lives in (ISO/IEC 14496-12,
//! 23008-12 and the AV1 Image File Format specification), read and
//! written by hand.
//!
//! The reader resolves exactly what a still image needs: the primary
//! item (an `av01` coded image or a `grid` of them), its alpha auxiliary
//! item, the colour boxes and the clean-aperture / rotation / mirror
//! transforms. The writer emits the smallest file the specification
//! allows: one colour item, an optional alpha item, both colour boxes
//! when there is an ICC profile.
//!
//! `avif-serialize` would write the container, but has no way to embed
//! an ICC profile, and a wide-gamut document exported without one reads
//! as sRGB everywhere else.

use anyhow::{bail, ensure, Context as _};

/// The `nclx` colour box: H.273 code points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nclx {
    pub primaries: u16,
    pub transfer: u16,
    pub matrix: u16,
    pub full_range: bool,
}

/// One coded image: an AV1 bitstream, or a grid of them.
#[derive(Debug, Default)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// `None` for a single coded image, else the tiles in raster order.
    pub grid: Option<Grid>,
    /// The AV1 bitstream of a single coded image.
    pub data: Vec<u8>,
    /// Its `av1C` record and `pixi` (bits, channels), as stored.
    pub config: Vec<u8>,
    pub pixi: Option<(u8, u8)>,
    pub nclx: Option<Nclx>,
    pub icc: Option<Vec<u8>>,
    /// Transforms in the order they apply.
    pub transforms: Vec<Transform>,
}

#[derive(Debug)]
pub struct Grid {
    pub rows: u32,
    pub columns: u32,
    pub tiles: Vec<Image>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    /// Crop to `width`x`height` at (`left`, `top`).
    Crop {
        left: u32,
        top: u32,
        width: u32,
        height: u32,
    },
    /// Anti-clockwise rotation by this many quarter turns.
    Rotate(u8),
    /// Mirror; `true` flips left-to-right, `false` top-to-bottom.
    Mirror(bool),
}

/// The primary image with its alpha plane, if one is attached.
#[derive(Debug)]
pub struct Avif {
    pub color: Image,
    pub alpha: Option<Image>,
    /// The colour samples were multiplied by alpha before coding.
    pub premultiplied: bool,
}

/// Whether the file names AVIF among its brands.
pub fn is_avif(bytes: &[u8]) -> bool {
    let Some((b"ftyp", body, _)) = next_box(bytes) else {
        return false;
    };
    // Major brand, minor version, then compatible brands.
    let major = body.first_chunk::<4>();
    let compatible = body.get(8..).unwrap_or_default().as_chunks::<4>().0;
    major
        .into_iter()
        .chain(compatible)
        .any(|brand| brand == b"avif" || brand == b"avis")
}

/// Split the next box off `bytes`: (type, body, rest).
fn next_box(bytes: &[u8]) -> Option<(&[u8; 4], &[u8], &[u8])> {
    let size = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as u64;
    let kind: &[u8; 4] = bytes.get(4..8)?.try_into().ok()?;
    let (header, size) = match size {
        0 => (8, bytes.len() as u64),
        1 => (16, u64::from_be_bytes(bytes.get(8..16)?.try_into().ok()?)),
        n => (8, n),
    };
    if size < header as u64 || size > bytes.len() as u64 {
        return None;
    }
    let size = size as usize;
    Some((kind, &bytes[header..size], &bytes[size..]))
}

/// Iterate the boxes in `bytes`; a truncated tail ends the iteration.
fn boxes(mut bytes: &[u8]) -> impl Iterator<Item = (&[u8; 4], &[u8])> {
    std::iter::from_fn(move || {
        let (kind, body, rest) = next_box(bytes)?;
        bytes = rest;
        Some((kind, body))
    })
}

/// A big-endian cursor over a box body.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> anyhow::Result<&'a [u8]> {
        ensure!(self.0.len() >= n, "AVIF box is truncated");
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn u8(&mut self) -> anyhow::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> anyhow::Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> anyhow::Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    /// An unsigned integer of 0, 4 or 8 bytes, as `iloc` sizes them.
    fn sized(&mut self, size: u8) -> anyhow::Result<u64> {
        Ok(match size {
            0 => 0,
            4 => self.u32()? as u64,
            8 => u64::from_be_bytes(self.take(8)?.try_into()?),
            other => bail!("AVIF iloc field of {other} bytes"),
        })
    }
    /// Version and flags of a FullBox.
    fn full(&mut self) -> anyhow::Result<(u8, u32)> {
        let word = self.u32()?;
        Ok(((word >> 24) as u8, word & 0x00FF_FFFF))
    }
    fn fourcc(&mut self) -> anyhow::Result<[u8; 4]> {
        Ok(self.take(4)?.try_into()?)
    }
}

#[derive(Default)]
struct Location {
    construction: u16,
    base: u64,
    extents: Vec<(u64, u64)>,
}

/// A property and whether the item marked it essential.
struct Property<'a> {
    kind: [u8; 4],
    body: &'a [u8],
}

/// Everything `meta` says, before it is resolved into images.
#[derive(Default)]
struct Meta<'a> {
    primary: u32,
    locations: Vec<(u32, Location)>,
    types: Vec<(u32, [u8; 4])>,
    /// (type, from, to)
    references: Vec<([u8; 4], u32, Vec<u32>)>,
    properties: Vec<Property<'a>>,
    /// item -> property indices (1-based) in association order.
    associations: Vec<(u32, Vec<u16>)>,
    idat: &'a [u8],
}

/// Files describing more items than this are not stills anyone made.
const MAX_ITEMS: usize = 4096;

/// Parse an AVIF still into its primary image and alpha plane.
pub fn read(file: &[u8]) -> anyhow::Result<Avif> {
    ensure!(is_avif(file), "not an AVIF file");
    let meta = boxes(file)
        .find(|(kind, _)| *kind == b"meta")
        .map(|(_, body)| body)
        .context("AVIF file has no meta box")?;
    let meta = parse_meta(meta)?;

    let color = resolve(&meta, file, meta.primary, 0)?;
    // The alpha plane is an auxiliary item that names the primary
    // image as the one it belongs to.
    let alpha_id = meta
        .references
        .iter()
        .filter(|(kind, _, to)| kind == b"auxl" && to.contains(&meta.primary))
        .map(|(_, from, _)| *from)
        .find(|id| is_alpha(&meta, *id));
    let alpha = alpha_id.map(|id| resolve(&meta, file, id, 0)).transpose()?;
    let premultiplied = alpha_id.is_some_and(|alpha| {
        meta.references
            .iter()
            .any(|(kind, from, to)| kind == b"prem" && *from == meta.primary && to.contains(&alpha))
    });
    Ok(Avif {
        color,
        alpha,
        premultiplied,
    })
}

fn parse_meta(body: &[u8]) -> anyhow::Result<Meta<'_>> {
    let mut reader = Reader(body);
    reader.full()?;
    let mut meta = Meta::default();
    for (kind, body) in boxes(reader.0) {
        let mut r = Reader(body);
        match kind {
            b"pitm" => {
                let (version, _) = r.full()?;
                meta.primary = if version == 0 {
                    r.u16()? as u32
                } else {
                    r.u32()?
                };
            }
            b"iloc" => parse_iloc(&mut r, &mut meta)?,
            b"iinf" => {
                let (version, _) = r.full()?;
                let _count = if version == 0 {
                    r.u16()? as u32
                } else {
                    r.u32()?
                };
                for (kind, body) in boxes(r.0) {
                    if kind != b"infe" {
                        continue;
                    }
                    let mut r = Reader(body);
                    let (version, _) = r.full()?;
                    if version < 2 {
                        continue;
                    }
                    let id = if version == 2 {
                        r.u16()? as u32
                    } else {
                        r.u32()?
                    };
                    let _protection = r.u16()?;
                    meta.types.push((id, r.fourcc()?));
                    ensure!(meta.types.len() <= MAX_ITEMS, "AVIF has too many items");
                }
            }
            b"iref" => {
                let (version, _) = r.full()?;
                for (kind, body) in boxes(r.0) {
                    let mut r = Reader(body);
                    let id = |r: &mut Reader| -> anyhow::Result<u32> {
                        Ok(if version == 0 {
                            r.u16()? as u32
                        } else {
                            r.u32()?
                        })
                    };
                    let from = id(&mut r)?;
                    let count = r.u16()?;
                    let to = (0..count)
                        .map(|_| id(&mut r))
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    meta.references.push((*kind, from, to));
                    ensure!(
                        meta.references.len() <= MAX_ITEMS,
                        "AVIF has too many references"
                    );
                }
            }
            b"iprp" => {
                for (kind, body) in boxes(body) {
                    match kind {
                        b"ipco" => {
                            meta.properties = boxes(body)
                                .map(|(kind, body)| Property { kind: *kind, body })
                                .collect();
                        }
                        b"ipma" => {
                            let mut r = Reader(body);
                            let (version, flags) = r.full()?;
                            let count = r.u32()?;
                            ensure!(count as usize <= MAX_ITEMS, "AVIF has too many items");
                            for _ in 0..count {
                                let id = if version < 1 {
                                    r.u16()? as u32
                                } else {
                                    r.u32()?
                                };
                                let n = r.u8()?;
                                let mut indices = Vec::with_capacity(n as usize);
                                for _ in 0..n {
                                    // The top bit is "essential"; every
                                    // property is honoured here anyway.
                                    indices.push(if flags & 1 != 0 {
                                        r.u16()? & 0x7FFF
                                    } else {
                                        (r.u8()? & 0x7F) as u16
                                    });
                                }
                                meta.associations.push((id, indices));
                            }
                        }
                        _ => {}
                    }
                }
            }
            b"idat" => meta.idat = body,
            _ => {}
        }
    }
    ensure!(meta.primary != 0, "AVIF file names no primary item");
    Ok(meta)
}

fn parse_iloc(r: &mut Reader, meta: &mut Meta) -> anyhow::Result<()> {
    let (version, _) = r.full()?;
    ensure!(version <= 2, "AVIF iloc version {version}");
    let sizes = r.u16()?;
    let offset_size = (sizes >> 12) as u8;
    let length_size = ((sizes >> 8) & 0xF) as u8;
    let base_size = ((sizes >> 4) & 0xF) as u8;
    let index_size = if version >= 1 { (sizes & 0xF) as u8 } else { 0 };
    let count = if version < 2 {
        r.u16()? as u32
    } else {
        r.u32()?
    };
    ensure!(count as usize <= MAX_ITEMS, "AVIF has too many items");
    for _ in 0..count {
        let id = if version < 2 {
            r.u16()? as u32
        } else {
            r.u32()?
        };
        let construction = if version >= 1 { r.u16()? & 0xF } else { 0 };
        let _data_reference = r.u16()?;
        let base = r.sized(base_size)?;
        let extents = r.u16()?;
        let mut location = Location {
            construction,
            base,
            extents: Vec::with_capacity(extents.min(64) as usize),
        };
        for _ in 0..extents {
            if index_size > 0 {
                r.sized(index_size)?;
            }
            let offset = r.sized(offset_size)?;
            let length = r.sized(length_size)?;
            location.extents.push((offset, length));
        }
        meta.locations.push((id, location));
    }
    Ok(())
}

fn item_type(meta: &Meta, id: u32) -> Option<[u8; 4]> {
    meta.types.iter().find(|(i, _)| *i == id).map(|(_, t)| *t)
}

fn properties<'m>(meta: &'m Meta, id: u32) -> impl Iterator<Item = &'m Property<'m>> {
    meta.associations
        .iter()
        .filter(move |(i, _)| *i == id)
        .flat_map(|(_, indices)| indices.iter())
        .filter_map(|&index| meta.properties.get((index as usize).checked_sub(1)?))
}

/// Whether item `id` declares itself an alpha plane.
fn is_alpha(meta: &Meta, id: u32) -> bool {
    properties(meta, id).any(|p| {
        p.kind == *b"auxC" && {
            let urn = p.body.get(4..).unwrap_or_default();
            let urn = urn.split(|b| *b == 0).next().unwrap_or_default();
            urn == b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha"
                || urn == b"urn:mpeg:hevc:2015:auxid:1"
        }
    })
}

/// The bytes item `id` occupies, its extents concatenated.
fn item_data(meta: &Meta, file: &[u8], id: u32) -> anyhow::Result<Vec<u8>> {
    let location = meta
        .locations
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, l)| l)
        .with_context(|| format!("AVIF item {id} has no location"))?;
    let source = match location.construction {
        0 => file,
        1 => meta.idat,
        other => bail!("AVIF item construction method {other}"),
    };
    let mut data = Vec::new();
    for &(offset, length) in &location.extents {
        let start = location
            .base
            .checked_add(offset)
            .filter(|s| *s <= source.len() as u64)
            .context("AVIF item lies outside the file")? as usize;
        let end = if length == 0 {
            source.len()
        } else {
            start
                .checked_add(length as usize)
                .filter(|e| *e <= source.len())
                .context("AVIF item lies outside the file")?
        };
        data.extend_from_slice(&source[start..end]);
    }
    Ok(data)
}

/// Resolve item `id` into an image with its colour boxes and transforms.
fn resolve(meta: &Meta, file: &[u8], id: u32, depth: u32) -> anyhow::Result<Image> {
    ensure!(depth < 2, "AVIF grid nests another grid");
    let mut image = Image::default();
    for property in properties(meta, id) {
        let mut r = Reader(property.body);
        match &property.kind {
            b"ispe" => {
                r.full()?;
                image.width = r.u32()?;
                image.height = r.u32()?;
            }
            b"colr" => match &r.fourcc()? {
                b"nclx" => {
                    image.nclx = Some(Nclx {
                        primaries: r.u16()?,
                        transfer: r.u16()?,
                        matrix: r.u16()?,
                        full_range: r.u8()? & 0x80 != 0,
                    })
                }
                b"prof" | b"rICC" => image.icc = Some(r.0.to_vec()),
                _ => {}
            },
            b"clap" => {
                let mut rational = || -> anyhow::Result<f64> {
                    let n = r.u32()? as i32 as f64;
                    let d = r.u32()? as i32 as f64;
                    ensure!(d != 0.0, "AVIF clap has a zero denominator");
                    Ok(n / d)
                };
                let (w, h) = (rational()?, rational()?);
                let (dx, dy) = (rational()?, rational()?);
                image
                    .transforms
                    .push(clean_aperture(image.width, image.height, w, h, dx, dy)?);
            }
            b"av1C" => image.config = property.body.to_vec(),
            b"pixi" => {
                r.full()?;
                let channels = r.u8()?;
                image.pixi = Some((r.u8()?, channels));
            }
            b"irot" => image.transforms.push(Transform::Rotate(r.u8()? & 3)),
            // Mode 0 mirrors top-to-bottom, mode 1 left-to-right.
            b"imir" => image.transforms.push(Transform::Mirror(r.u8()? & 1 == 1)),
            _ => {}
        }
    }
    match &item_type(meta, id).with_context(|| format!("AVIF item {id} has no type"))? {
        b"av01" => image.data = item_data(meta, file, id)?,
        b"grid" => {
            let data = item_data(meta, file, id)?;
            let mut r = Reader(&data);
            let _version = r.u8()?;
            let flags = r.u8()?;
            let rows = r.u8()? as u32 + 1;
            let columns = r.u8()? as u32 + 1;
            let (width, height) = if flags & 1 != 0 {
                (r.u32()?, r.u32()?)
            } else {
                (r.u16()? as u32, r.u16()? as u32)
            };
            image.width = width;
            image.height = height;
            let tiles = meta
                .references
                .iter()
                .find(|(kind, from, _)| kind == b"dimg" && *from == id)
                .map(|(_, _, to)| to.as_slice())
                .unwrap_or_default();
            ensure!(
                tiles.len() == (rows * columns) as usize,
                "AVIF grid of {rows}x{columns} names {} tiles",
                tiles.len()
            );
            let tiles = tiles
                .iter()
                .map(|&tile| resolve(meta, file, tile, depth + 1))
                .collect::<anyhow::Result<Vec<_>>>()?;
            // Tiles inherit the grid's colour description where they
            // carry none of their own.
            if image.nclx.is_none() {
                image.nclx = tiles.first().and_then(|t| t.nclx);
            }
            if image.icc.is_none() {
                image.icc = tiles.first().and_then(|t| t.icc.clone());
            }
            image.grid = Some(Grid {
                rows,
                columns,
                tiles,
            });
        }
        other => bail!(
            "AVIF primary item is of type {:?}",
            String::from_utf8_lossy(other)
        ),
    }
    ensure!(
        image.width > 0 && image.height > 0,
        "AVIF item {id} has no size"
    );
    Ok(image)
}

/// The integer crop a `clap` box describes: a `w`x`h` window centred
/// `dx`,`dy` from the image's centre.
fn clean_aperture(
    width: u32,
    height: u32,
    w: f64,
    h: f64,
    dx: f64,
    dy: f64,
) -> anyhow::Result<Transform> {
    let left = (width as f64 - 1.0) / 2.0 + dx - (w - 1.0) / 2.0;
    let top = (height as f64 - 1.0) / 2.0 + dy - (h - 1.0) / 2.0;
    let whole = |v: f64| v.fract() == 0.0 && v >= 0.0;
    ensure!(
        whole(left) && whole(top) && whole(w) && whole(h) && w >= 1.0 && h >= 1.0,
        "AVIF clean aperture is not a whole-pixel crop"
    );
    ensure!(
        left + w <= width as f64 && top + h <= height as f64,
        "AVIF clean aperture lies outside the image"
    );
    Ok(Transform::Crop {
        left: left as u32,
        top: top as u32,
        width: w as u32,
        height: h as u32,
    })
}

/// One coded plane for `write`.
pub struct Coded<'a> {
    /// The AV1 bitstream (a temporal unit of OBUs).
    pub data: &'a [u8],
    /// The `av1C` record's body: the four fixed bytes, plus any
    /// configuration OBUs.
    pub config: &'a [u8],
    /// Bits per sample and channel count, for `pixi`.
    pub depth: u8,
    pub channels: u8,
}

/// Write a single-image AVIF: `color` with an optional `alpha` plane.
pub fn write(
    width: u32,
    height: u32,
    color: &Coded,
    alpha: Option<&Coded>,
    nclx: Nclx,
    icc: Option<&[u8]>,
    xmp: Option<&[u8]>,
) -> Vec<u8> {
    let full_box = |kind: &[u8; 4], version: u8, flags: u32, body: &[u8]| -> Vec<u8> {
        let mut inner = (((version as u32) << 24) | flags).to_be_bytes().to_vec();
        inner.extend_from_slice(body);
        plain_box(kind, &inner)
    };

    let mut ftyp = b"avif".to_vec();
    ftyp.extend(0u32.to_be_bytes());
    ftyp.extend(b"avifmif1miaf");
    let ftyp = plain_box(b"ftyp", &ftyp);

    let mut hdlr = 0u32.to_be_bytes().to_vec();
    hdlr.extend(b"pict");
    hdlr.extend([0u8; 12]);
    hdlr.push(0);
    let hdlr = full_box(b"hdlr", 0, 0, &hdlr);
    let pitm = full_box(b"pitm", 0, 0, &1u16.to_be_bytes());

    let items: Vec<(u16, &Coded)> = std::iter::once((1, color))
        .chain(alpha.map(|a| (2, a)))
        .collect();

    let xmp_id = items.len() as u16 + 1;
    let mut iinf = ((items.len() + xmp.is_some() as usize) as u16)
        .to_be_bytes()
        .to_vec();
    for (id, _) in &items {
        let mut infe = id.to_be_bytes().to_vec();
        infe.extend(0u16.to_be_bytes());
        infe.extend(b"av01");
        infe.push(0);
        // The alpha plane is hidden: it is not an image to show alone.
        let flags = u32::from(*id != 1);
        iinf.extend(full_box(b"infe", 2, flags, &infe));
    }
    if xmp.is_some() {
        // XMP is a MIME item that describes the primary image.
        let mut infe = xmp_id.to_be_bytes().to_vec();
        infe.extend(0u16.to_be_bytes());
        infe.extend(b"mime");
        infe.push(0);
        infe.extend(b"application/rdf+xml\0");
        iinf.extend(full_box(b"infe", 2, 0, &infe));
    }
    let iinf = full_box(b"iinf", 0, 0, &iinf);

    let reference = |kind: &[u8; 4], from: u16| {
        let mut body = from.to_be_bytes().to_vec();
        body.extend(1u16.to_be_bytes());
        body.extend(1u16.to_be_bytes());
        plain_box(kind, &body)
    };
    let mut references = Vec::new();
    if alpha.is_some() {
        references.extend(reference(b"auxl", 2));
    }
    if xmp.is_some() {
        references.extend(reference(b"cdsc", xmp_id));
    }
    let iref = (!references.is_empty()).then(|| full_box(b"iref", 0, 0, &references));

    // Properties, 1-based in the order they are pushed.
    let mut ipco: Vec<Vec<u8>> = Vec::new();
    let mut push = |property: Vec<u8>| -> u8 {
        ipco.push(property);
        ipco.len() as u8
    };
    let mut ispe = width.to_be_bytes().to_vec();
    ispe.extend(height.to_be_bytes());
    let ispe = push(full_box(b"ispe", 0, 0, &ispe));
    let mut associations: Vec<(u16, Vec<(u8, bool)>)> = Vec::new();
    for (id, coded) in &items {
        let mut pixi = vec![coded.channels];
        pixi.extend(std::iter::repeat_n(coded.depth, coded.channels as usize));
        let pixi = push(full_box(b"pixi", 0, 0, &pixi));
        let av1c = push(plain_box(b"av1C", coded.config));
        let mut list = vec![(ispe, false), (av1c, true), (pixi, false)];
        if *id == 1 {
            let mut colr = b"nclx".to_vec();
            colr.extend(nclx.primaries.to_be_bytes());
            colr.extend(nclx.transfer.to_be_bytes());
            colr.extend(nclx.matrix.to_be_bytes());
            colr.push(if nclx.full_range { 0x80 } else { 0 });
            list.push((push(plain_box(b"colr", &colr)), false));
            if let Some(icc) = icc {
                let mut colr = b"prof".to_vec();
                colr.extend_from_slice(icc);
                list.push((push(plain_box(b"colr", &colr)), false));
            }
        } else {
            let urn = b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0";
            list.push((push(full_box(b"auxC", 0, 0, urn)), true));
        }
        associations.push((*id, list));
    }
    let ipco = plain_box(b"ipco", &ipco.concat());
    let mut ipma = (associations.len() as u32).to_be_bytes().to_vec();
    for (id, list) in &associations {
        ipma.extend(id.to_be_bytes());
        ipma.push(list.len() as u8);
        for (index, essential) in list {
            ipma.push(index | if *essential { 0x80 } else { 0 });
        }
    }
    let ipma = full_box(b"ipma", 0, 0, &ipma);
    let iprp = plain_box(b"iprp", &[ipco, ipma].concat());

    // iloc's offsets are absolute, so its size must be known before
    // they are: it is fixed (4-byte offsets and lengths, one extent an
    // item), and everything before mdat is laid out first with zeroes.
    let payloads: Vec<(u16, &[u8])> = items
        .iter()
        .map(|(id, coded)| (*id, coded.data))
        .chain(xmp.map(|xmp| (xmp_id, xmp)))
        .collect();
    let iloc = |offsets: &[u32]| -> Vec<u8> {
        let mut body = 0x4400u16.to_be_bytes().to_vec();
        body.extend((payloads.len() as u16).to_be_bytes());
        for ((id, data), offset) in payloads.iter().zip(offsets) {
            body.extend(id.to_be_bytes());
            body.extend(0u16.to_be_bytes());
            body.extend(1u16.to_be_bytes());
            body.extend(offset.to_be_bytes());
            body.extend((data.len() as u32).to_be_bytes());
        }
        full_box(b"iloc", 0, 0, &body)
    };
    let meta = |iloc: Vec<u8>| -> Vec<u8> {
        let mut body = [hdlr.clone(), pitm.clone(), iloc, iinf.clone()].concat();
        if let Some(iref) = &iref {
            body.extend_from_slice(iref);
        }
        body.extend_from_slice(&iprp);
        full_box(b"meta", 0, 0, &body)
    };
    let head = ftyp.len() + meta(iloc(&vec![0; payloads.len()])).len();
    // mdat's own header precedes the first item.
    let mut offset = (head + 8) as u32;
    let offsets: Vec<u32> = payloads
        .iter()
        .map(|(_, data)| {
            let at = offset;
            offset += data.len() as u32;
            at
        })
        .collect();
    let mut out = ftyp;
    out.extend(meta(iloc(&offsets)));
    let mdat: Vec<u8> = payloads
        .iter()
        .flat_map(|(_, data)| data.iter().copied())
        .collect();
    out.extend(plain_box(b"mdat", &mdat));
    out
}

/// Rewrite an AVIF this module wrote with an XMP packet attached to its
/// primary image, for export recipes that keep the copyright notice.
pub fn with_xmp(file: &[u8], xmp: &[u8]) -> anyhow::Result<Vec<u8>> {
    let avif = read(file)?;
    fn coded(image: &Image, channels: u8) -> anyhow::Result<Coded<'_>> {
        ensure!(
            image.grid.is_none() && image.transforms.is_empty(),
            "AVIF layout this writer did not produce"
        );
        let (depth, channels) = image.pixi.unwrap_or((8, channels));
        Ok(Coded {
            data: &image.data,
            config: &image.config,
            depth,
            channels,
        })
    }
    let color = coded(&avif.color, 3)?;
    let alpha = avif.alpha.as_ref().map(|a| coded(a, 1)).transpose()?;
    let nclx = avif.color.nclx.unwrap_or(Nclx {
        primaries: 2,
        transfer: 2,
        matrix: 6,
        full_range: true,
    });
    Ok(write(
        avif.color.width,
        avif.color.height,
        &color,
        alpha.as_ref(),
        nclx,
        avif.color.icc.as_deref(),
        Some(xmp),
    ))
}

fn plain_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_files_read_back() {
        let color = Coded {
            data: b"colour-obus",
            config: &[0x81, 0x20, 0x00, 0x00],
            depth: 10,
            channels: 3,
        };
        let alpha = Coded {
            data: b"alpha",
            config: &[0x81, 0x20, 0x10, 0x00],
            depth: 10,
            channels: 1,
        };
        let nclx = Nclx {
            primaries: 2,
            transfer: 2,
            matrix: 6,
            full_range: true,
        };
        let file = write(7, 5, &color, Some(&alpha), nclx, Some(b"icc-bytes"), None);
        assert!(is_avif(&file));
        let avif = read(&file).unwrap();
        assert_eq!((avif.color.width, avif.color.height), (7, 5));
        assert_eq!(avif.color.data, b"colour-obus");
        assert_eq!(avif.color.nclx, Some(nclx));
        assert_eq!(avif.color.icc.as_deref(), Some(&b"icc-bytes"[..]));
        assert_eq!(avif.alpha.unwrap().data, b"alpha");
        assert!(!avif.premultiplied);

        let opaque = write(3, 3, &color, None, nclx, None, None);
        let avif = read(&opaque).unwrap();
        assert!(avif.alpha.is_none() && avif.color.icc.is_none());

        // XMP rides along as an item of its own, and the images survive.
        let tagged = with_xmp(&file, b"<x:xmpmeta/>").unwrap();
        assert!(tagged.windows(12).any(|w| w == b"<x:xmpmeta/>"));
        let avif = read(&tagged).unwrap();
        assert_eq!(avif.color.data, b"colour-obus");
        assert_eq!(avif.color.pixi, Some((10, 3)));
        assert_eq!(avif.alpha.unwrap().data, b"alpha");
    }

    #[test]
    fn brands_are_read_from_the_compatible_list_too() {
        let mut ftyp = b"mif1".to_vec();
        ftyp.extend([0; 4]);
        ftyp.extend(b"miafavif");
        assert!(is_avif(&plain_box(b"ftyp", &ftyp)));
        let mut heic = b"heic".to_vec();
        heic.extend([0; 4]);
        heic.extend(b"mif1heic");
        assert!(!is_avif(&plain_box(b"ftyp", &heic)));
        assert!(!is_avif(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn clean_aperture_centres_the_window() {
        assert_eq!(
            clean_aperture(10, 8, 6.0, 4.0, 0.0, 0.0).unwrap(),
            Transform::Crop {
                left: 2,
                top: 2,
                width: 6,
                height: 4
            }
        );
        assert!(clean_aperture(10, 8, 5.0, 4.0, 0.0, 0.0).is_err());
        assert!(clean_aperture(10, 8, 12.0, 4.0, 0.0, 0.0).is_err());
    }
}
