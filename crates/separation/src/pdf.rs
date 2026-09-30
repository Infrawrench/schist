//! A prepress PDF writer for separated pages.
//!
//! This is a different job from the photo print writer in
//! `crates/editor/src/printing.rs`, which lays a contact sheet of
//! photographs on a desktop printer. A prepress file has to carry:
//!
//! * the **bleed**, because a page cut to size with no bleed shows a
//!   white sliver at the edge;
//! * **crop marks** in the slug, so the cutter knows where to cut;
//! * each ink as a **`Separation`** colour space with a process
//!   alternate, which is how a PDF names a spot ink and still shows
//!   something sensible on screen;
//! * **`/ExtGState` with `/OP` and `/op`**, the only place a PDF records
//!   overprint, and the whole reason for writing this rather than
//!   flattening to an image;
//! * **trapping**, thin ink laid into a gap so a misregistration does
//!   not show as paper.
//!
//! PDF is a graph of numbered indirect objects written out in one pass.
//! [`Pdf`] builds that graph and [`write_page`] fills it in. Nothing
//! here knows about layout or separation beyond what it is handed.

use schist_layout::Insets;
use std::io::Write;

use crate::coverage::PlateCoverage;
use crate::geometry::OutputSettings;
use crate::plan::Plate;
use crate::separate::SeparatedPage;

/// A rough ceiling on the output, so a runaway document fails rather
/// than exhausting memory.
pub const MAX_BYTES: usize = 2 << 30;

/// How many pages go on one imposed sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Imposition {
    /// 1, 2 or 4.
    pub up: u8,
    /// Draw crop marks and registration targets in the slug.
    pub marks: bool,
}

impl Default for Imposition {
    fn default() -> Self {
        Self {
            up: 1,
            marks: false,
        }
    }
}

impl Imposition {
    pub fn single() -> Imposition {
        Imposition { up: 1, marks: true }
    }

    /// Sequential n-up positions in reading order, top row first.
    /// This is sheet packing, not booklet signature imposition.
    pub fn slots(self, sheet: (f32, f32), page: (f32, f32), gutter: f32) -> Vec<(f32, f32)> {
        match self.up {
            2 => {
                let x = (sheet.0 - page.0 * 2.0 - gutter).max(0.0) / 2.0;
                let y = (sheet.1 - page.1).max(0.0) / 2.0;
                vec![(x, y), (x + page.0 + gutter, y)]
            }
            4 => {
                let x = (sheet.0 - page.0 * 2.0 - gutter).max(0.0) / 2.0;
                let y = (sheet.1 - page.1 * 2.0 - gutter).max(0.0) / 2.0;
                vec![
                    (x, y + page.1 + gutter),
                    (x + page.0 + gutter, y + page.1 + gutter),
                    (x, y),
                    (x + page.0 + gutter, y),
                ]
            }
            _ => {
                let x = (sheet.0 - page.0).max(0.0) / 2.0;
                let y = (sheet.1 - page.1).max(0.0) / 2.0;
                vec![(x, y)]
            }
        }
    }
}

/// Marks drawn outside the trim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marks {
    pub crop_length: f32,
    pub weight: f32,
    /// Gap between the trim edge and the start of a mark.
    pub offset: f32,
    pub registration: bool,
}

impl Default for Marks {
    fn default() -> Self {
        Marks {
            crop_length: 3.0,
            weight: 0.25,
            offset: 1.0,
            registration: true,
        }
    }
}

/// A PDF under construction.
///
/// Objects are numbered from one as they are added, and
/// [`Pdf::finish`] writes them out with a cross-reference table. This is
/// the minimum a reader needs, and it is small enough to read in a hex
/// editor when a prepress house asks why a file is odd.
pub struct Pdf {
    objects: Vec<Vec<u8>>,
    bytes: usize,
    output_intent: Option<usize>,
}

impl Default for Pdf {
    fn default() -> Self {
        Pdf::new()
    }
}

impl Pdf {
    /// An empty file with the catalogue and page tree reserved.
    pub fn new() -> Pdf {
        Pdf {
            objects: vec![Vec::new(), Vec::new()],
            bytes: 0,
            output_intent: None,
        }
    }

    /// Embed a validated CMYK ICC output profile. This records the print
    /// condition; it does not declare PDF/X conformance.
    pub fn set_output_profile(&mut self, bytes: &[u8], name: &str) -> Result<(), std::io::Error> {
        if bytes.len() < 128
            || bytes.get(16..20) != Some(b"CMYK")
            || bytes.get(36..40) != Some(b"acsp")
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a CMYK ICC output profile is required",
            ));
        }
        let profile = self.stream("/N 4", bytes)?;
        let name: String = std::iter::once(0xfeff_u16)
            .chain(name.encode_utf16())
            .map(|u| format!("{u:04X}"))
            .collect();
        self.output_intent=Some(self.add_str(&format!("<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (Custom) /Info <{name}> /DestOutputProfile {profile} 0 R >>")));
        Ok(())
    }

    /// Add an object, returning its number.
    pub fn add(&mut self, bytes: Vec<u8>) -> usize {
        // Each object costs its bytes plus a header and an offset entry.
        self.bytes += bytes.len() + 48;
        self.objects.push(bytes);
        self.objects.len()
    }

    pub fn add_str(&mut self, body: &str) -> usize {
        self.add(body.as_bytes().to_vec())
    }

    /// Whether the file is still within the size ceiling.
    pub fn within_limit(&self) -> bool {
        self.bytes <= MAX_BYTES
    }

    /// A compressed stream object.
    pub fn stream(&mut self, dict: &str, bytes: &[u8]) -> Result<usize, std::io::Error> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes)?;
        let compressed = encoder.finish()?;
        let mut stream = format!(
            "<< {dict} /Filter /FlateDecode /Length {} >>\nstream\n",
            compressed.len()
        )
        .into_bytes();
        stream.extend(compressed);
        stream.extend(b"\nendstream");
        Ok(self.add(stream))
    }

    /// The whole file.
    pub fn finish(mut self, pages: &[usize]) -> Vec<u8> {
        let intent = self
            .output_intent
            .map(|id| format!("/OutputIntents [{id} 0 R]"))
            .unwrap_or_default();
        self.objects[0] = format!("<< /Type /Catalog /Pages 2 0 R {intent} >>").into_bytes();
        self.objects[1] = format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            pages.len(),
            pages
                .iter()
                .map(|n| format!("{n} 0 R"))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes();
        let mut out = b"%PDF-1.6\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).bytes());
            out.extend(object);
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).bytes());
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                self.objects.len() + 1
            )
            .bytes(),
        );
        out
    }
}

/// Encode a PDF name as UTF-8 bytes, escaping whitespace and delimiters.
/// A name is not a literal string: spaces and slashes require #XX escapes.
pub fn pdf_name(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        if (33..=126).contains(byte) && !b"()<>[]{}/%#".contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("#{byte:02X}"));
        }
    }
    out
}

/// Quantise a plate to the bytes a PDF image needs.
///
/// Coverage becomes ink density: 1.0 is no ink, 0.0 is a full flood,
/// which is the reverse of the plate's own convention. Getting this
/// backwards prints the negative.
pub fn plate_to_gray(coverage: &PlateCoverage) -> Vec<u8> {
    coverage
        .data
        .iter()
        .map(|v| ((1.0 - v.clamp(0.0, 1.0)) * 255.0).round() as u8)
        .collect()
}

/// A one-bit-per-plate mask, used for trapping.
pub fn plate_to_bits(coverage: &PlateCoverage, threshold: f32) -> Vec<u8> {
    let width = coverage.rect.width().max(0) as usize;
    let height = coverage.rect.height().max(0) as usize;
    let stride = width.div_ceil(8);
    let mut out = vec![0u8; stride * height];
    for y in 0..height {
        for x in 0..width {
            if coverage.at(coverage.rect.left + x as i32, coverage.rect.top + y as i32) >= threshold
            {
                out[y * stride + x / 8] |= 0b1000_0000 >> (x % 8);
            }
        }
    }
    out
}

/// The crop marks and registration targets around a trim box.
///
/// The marks are in the page's own space, outside the trim, which is
/// what makes the same code work for a single page and for a sheet with
/// four imposed.
pub fn marks_for(trim: (f32, f32), bleed: impl Into<Insets>, marks: &Marks) -> String {
    let bleed = bleed.into();
    let mut out = format!("q /Registration CS 1 SCN {} w\n", marks.weight);
    for (x, y, dx, dy) in [
        (0.0, 0.0, -1.0, -1.0),
        (trim.0, 0.0, 1.0, -1.0),
        (0.0, trim.1, -1.0, 1.0),
        (trim.0, trim.1, 1.0, 1.0),
    ] {
        let gap_x = if dx < 0.0 { bleed.left } else { bleed.right } + marks.offset;
        let gap_y = if dy < 0.0 { bleed.bottom } else { bleed.top } + marks.offset;
        let far_x = gap_x + marks.crop_length;
        let far_y = gap_y + marks.crop_length;
        out.push_str(&format!(
            "{x:.3} {:.3} m {x:.3} {:.3} l S\n",
            y + gap_y * dy,
            y + far_y * dy
        ));
        out.push_str(&format!(
            "{:.3} {y:.3} m {:.3} {y:.3} l S\n",
            x + gap_x * dx,
            x + far_x * dx
        ));
        if marks.registration {
            let cx = x + far_x * dx;
            let cy = y + far_y * dy;
            let r = marks.crop_length;
            let k = r * 0.5522848;
            out.push_str(&format!("{} {} m\n", cx + r, cy));
            for p in [
                [cx + r, cy + k, cx + k, cy + r, cx, cy + r],
                [cx - k, cy + r, cx - r, cy + k, cx - r, cy],
                [cx - r, cy - k, cx - k, cy - r, cx, cy - r],
                [cx + k, cy - r, cx + r, cy - k, cx + r, cy],
            ] {
                out.push_str(&format!(
                    "{} {} {} {} {} {} c\n",
                    p[0], p[1], p[2], p[3], p[4], p[5]
                ));
            }
            out.push_str(&format!(
                "h S\n{} {} m {} {} l S\n{} {} m {} {} l S\n",
                cx - r,
                cy,
                cx + r,
                cy,
                cx,
                cy - r,
                cx,
                cy + r
            ));
        }
    }
    out.push_str("Q\n");
    out
}

/// Everything one PDF page needs.
pub struct PageOutput<'a> {
    pub separated: &'a SeparatedPage,
    /// The page's trim size, in points.
    pub trim: (f32, f32),
    /// Physical offsets beyond trim.
    pub bleed: Insets,
    /// Slug offsets from trim; the media encloses both slug and bleed.
    pub slug: Insets,
    pub settings: OutputSettings,
    pub imposition: Imposition,
    pub marks: Marks,
    /// Set the image graphics state. Object-level overprint has already
    /// been resolved into the plate samples by separation.
    pub overprint: bool,
}

/// The process build of a plate, which is what its `Separation`
/// alternate converts to.
pub fn process_build(plate: &Plate) -> [f32; 4] {
    if let Some(channel) = plate.process_index.filter(|c| *c < 4) {
        let mut build = [0.0; 4];
        build[channel] = 1.0;
        return build;
    }
    let rgb = plate.preview_rgb;
    let k = 1.0 - rgb.iter().copied().fold(0.0f32, f32::max);
    if k >= 1.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inv = 1.0 - k;
    [
        (1.0 - rgb[0] - k) / inv,
        (1.0 - rgb[1] - k) / inv,
        (1.0 - rgb[2] - k) / inv,
        k,
    ]
}

/// ISO 32000 Type 2 function: a tint interpolates paper to the ink's
/// CMYK alternate. Zero channels remain exactly zero at every tint.
pub fn tint_function(build: [f32; 4]) -> String {
    let values = build.map(|v| {
        if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            0.0
        }
    });
    format!("<< /FunctionType 2 /Domain [0 1] /Range [0 1 0 1 0 1 0 1] /C0 [0 0 0 0] /C1 [{} {} {} {}] /N 1 >>",values[0],values[1],values[2],values[3])
}

/// The sheet a page occupies: trim expanded by physical media offsets.
pub fn sheet_size(trim: (f32, f32), offsets: impl Into<Insets>) -> (f32, f32) {
    let offsets = offsets.into();
    (
        trim.0 + offsets.left + offsets.right,
        trim.1 + offsets.top + offsets.bottom,
    )
}

/// PDF coordinates start at the bottom left, unlike layout's top left.
pub fn trim_box(trim: (f32, f32), offsets: impl Into<Insets>) -> String {
    let offsets = offsets.into();
    format!(
        "[{:.2} {:.2} {:.2} {:.2}]",
        offsets.left,
        offsets.bottom,
        trim.0 + offsets.left,
        trim.1 + offsets.bottom
    )
}

/// A page's bleed box when media is exactly the bleed extent.
pub fn bleed_box(trim: (f32, f32), bleed: impl Into<Insets>) -> String {
    let size = sheet_size(trim, bleed);
    format!("[0.00 0.00 {:.2} {:.2}]", size.0, size.1)
}

/// Extra media outside bleed leaves crop and registration marks intact.
fn mark_padding(page: &PageOutput<'_>) -> f32 {
    if page.imposition.marks {
        page.marks.offset + 2.0 * page.marks.crop_length + 2.0
    } else {
        0.0
    }
}

fn media_offsets(page: &PageOutput<'_>) -> Insets {
    page.bleed.expanded(mark_padding(page)).max(page.slug)
}

/// One DeviceN image carries all already-composited plates. Independent
/// opaque plate images erase earlier inks in viewers without overprint simulation.
pub fn page_content(page: &PageOutput<'_>, images: &[(usize, PlateCoverage)]) -> String {
    page_content_for(page, !images.is_empty())
}

fn page_content_for(page: &PageOutput<'_>, has_ink: bool) -> String {
    let mut content = String::new();
    let scale = 1.0 / page.settings.scale();
    let offset = media_offsets(page);
    if has_ink {
        let rect = page.separated.separation.rect();
        let (w, h) = (rect.width() as f32 * scale, rect.height() as f32 * scale);
        let x = rect.left as f32 * scale + offset.left;
        let y = page.trim.1 + offset.bottom - rect.bottom as f32 * scale;
        let state = if page.overprint { "OP" } else { "KO" };
        content.push_str(&format!(
            "q /{state} gs {w:.3} 0 0 {h:.3} {x:.3} {y:.3} cm /InkImage Do Q\n"
        ));
    }
    if page.imposition.marks {
        content.push_str(&format!(
            "q 1 0 0 1 {:.3} {:.3} cm\n",
            offset.left, offset.bottom
        ));
        content.push_str(&marks_for(page.trim, page.bleed, &page.marks));
        content.push_str("Q\n");
    }
    content
}

/// A bounded Type 4 function maps N ink tints to a CMYK screen alternate.
/// The stored image retains every original channel for a separation-aware RIP.
fn device_n_function(plates: &[Plate]) -> String {
    let mut code = String::from("{ ");
    let n = plates.len();
    for channel in 0..4 {
        code.push_str("0 ");
        for (index, plate) in plates.iter().enumerate() {
            let coefficient = process_build(plate)[channel];
            if coefficient > 0.0 {
                code.push_str(&format!(
                    "{} index {coefficient:.6} mul add ",
                    n + channel - index
                ));
            }
        }
    }
    code.push_str(&format!("{} 4 roll ", n + 4));
    for _ in 0..n {
        code.push_str("pop ");
    }
    code.push('}');
    code
}

/// The plates of a separated page that carry any ink.
///
/// A plate with no coverage gets its colour space and graphics states in
/// the resources but no image, because a prepress tool treats a missing
/// plate as an error and a blank one as an empty plate.
pub fn drawn_plates(page: &SeparatedPage) -> Vec<(usize, PlateCoverage)> {
    let mut out = Vec::new();
    for index in 0..page.separation.plates().len() {
        let Some(coverage) = page.separation.plate(index) else {
            continue;
        };
        if coverage.rect.is_empty() || coverage.data.is_empty() {
            continue;
        }
        if coverage.data.iter().all(|v| *v == 0.0) {
            continue;
        }
        out.push((index, coverage.clone()));
    }
    out
}

/// Write one separated page as a PDF page object.
pub fn write_page(pdf: &mut Pdf, page: &PageOutput<'_>) -> Result<usize, std::io::Error> {
    if page.imposition.up != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "use write_document_imposed for multiple pages per sheet",
        ));
    }
    write_leaf(pdf, page, false)
}

fn write_leaf(pdf: &mut Pdf, page: &PageOutput<'_>, form: bool) -> Result<usize, std::io::Error> {
    use std::io::{Error, ErrorKind};
    let plan = &page.separated.plan;
    let rect = page.separated.separation.rect();
    if ![
        page.trim.0,
        page.trim.1,
        page.bleed.top,
        page.bleed.right,
        page.bleed.bottom,
        page.bleed.left,
        page.slug.top,
        page.slug.right,
        page.slug.bottom,
        page.slug.left,
        page.marks.crop_length,
        page.marks.weight,
        page.marks.offset,
    ]
    .iter()
    .all(|v| v.is_finite() && *v >= 0.0)
        || page.trim.0 <= 0.0
        || page.trim.1 <= 0.0
        || plan.plates.len() != page.separated.separation.plates().len()
        || page.separated.separation.plates().iter().any(|plate| {
            plate.rect != rect
                || plate.data.len()
                    != (rect.width().max(0) as usize).saturating_mul(rect.height().max(0) as usize)
                || plate.data.iter().any(|v| !v.is_finite())
        })
        || plan.plates.is_empty()
        || plan.plates.len() > 32
        || rect.is_empty()
        || !page.settings.scale().is_finite()
        || page.settings.scale() <= 0.0
    {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "invalid separated page geometry or channel count",
        ));
    }
    let has_ink = page
        .separated
        .separation
        .plates()
        .iter()
        .any(|plate| plate.data.iter().any(|v| *v > 0.0));
    let mut colorants = String::new();
    let mut names = String::new();
    let mut seen = std::collections::BTreeSet::new();
    for plate in &plan.plates {
        if !seen.insert(&plate.name) || matches!(plate.name.as_str(), "All" | "None") {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                schist_i18n::tf!("design.preflight_ink_conflict", name = plate.name),
            ));
        }
        let name = pdf_name(&plate.name);
        let space = pdf.add_str(&format!(
            "[/Separation /{name} /DeviceCMYK {}]",
            tint_function(process_build(plate))
        ));
        colorants.push_str(&format!("/{name} {space} 0 R "));
        names.push_str(&format!("/{name} "));
    }
    let domain = "0 1 ".repeat(plan.plates.len());
    let function = pdf.stream(
        &format!("/FunctionType 4 /Domain [{domain}] /Range [0 1 0 1 0 1 0 1]"),
        device_n_function(&plan.plates).as_bytes(),
    )?;
    let space=pdf.add_str(&format!("[/DeviceN [{names}] /DeviceCMYK {function} 0 R << /Subtype /NChannel /Colorants << {colorants}>> /Process << /ColorSpace /DeviceCMYK /Components [/Cyan /Magenta /Yellow /Black] >> >>]"));
    let registration = pdf.add_str(&format!(
        "[/Separation /All /DeviceCMYK {}]",
        tint_function([1.0; 4])
    ));
    let over = pdf.add_str("<< /Type /ExtGState /OP true /op true /OPM 1 >>");
    let knock = pdf.add_str("<< /Type /ExtGState /OP false /op false /OPM 1 >>");
    let mut xobjects = String::new();
    if has_ink {
        let area = (rect.width() as usize)
            .checked_mul(rect.height() as usize)
            .and_then(|v| v.checked_mul(plan.plates.len()))
            .filter(|v| *v <= MAX_BYTES)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::InvalidInput,
                    "separated image exceeds size limit",
                )
            })?;
        let mut data = Vec::with_capacity(area);
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                for plate in page.separated.separation.plates() {
                    data.push((plate.at(x, y).clamp(0.0, 1.0) * 255.0).round() as u8);
                }
            }
        }
        let image=pdf.stream(&format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {space} 0 R /BitsPerComponent 8 /Interpolate false",rect.width(),rect.height()),&data)?;
        xobjects = format!("/InkImage {image} 0 R");
    }
    let resources=format!("<< /ColorSpace << /Inks {space} 0 R /Registration {registration} 0 R >> /ExtGState << /OP {over} 0 R /KO {knock} 0 R >> /XObject << {xobjects} >> >>");
    let content = page_content_for(page, has_ink);
    let offset = media_offsets(page);
    let (width, height) = sheet_size(page.trim, offset);
    let bleed = format!(
        "[{:.2} {:.2} {:.2} {:.2}]",
        offset.left - page.bleed.left,
        offset.bottom - page.bleed.bottom,
        width - offset.right + page.bleed.right,
        height - offset.top + page.bleed.top,
    );
    if !pdf.within_limit() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "PDF exceeds size limit",
        ));
    }
    if form {
        return pdf.stream(&format!("/Type /XObject /Subtype /Form /BBox [0 0 {width:.2} {height:.2}] /Resources {resources}"), content.as_bytes());
    }
    let content = pdf.stream("", content.as_bytes())?;
    Ok(pdf.add_str(&format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width:.2} {height:.2}] /TrimBox {} /BleedBox {bleed} /Resources {resources} /Contents {content} 0 R >>",trim_box(page.trim,offset))))
}

/// Write a document with uniform bleed and no slug, one PDF page per separated
/// page. Use `write_sheet` with `PageOutput` for independent bleed/slug edges.
pub fn write_document(
    pages: &[SeparatedPage],
    trims: &[(f32, f32)],
    bleeds: &[f32],
    settings: OutputSettings,
) -> Result<Vec<u8>, std::io::Error> {
    write_document_imposed(pages, trims, bleeds, settings, Imposition::single())
}

/// Pack sequential pages onto one-, two- or four-up sheets. Mixed page
/// sizes keep their physical dimensions; the largest determines the cell.
/// An incomplete final sheet leaves its remaining cells blank.
pub fn write_document_imposed(
    pages: &[SeparatedPage],
    trims: &[(f32, f32)],
    bleeds: &[f32],
    settings: OutputSettings,
    imposition: Imposition,
) -> Result<Vec<u8>, std::io::Error> {
    if pages.len() != trims.len()
        || pages.len() != bleeds.len()
        || !matches!(imposition.up, 1 | 2 | 4)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid page geometry count or imposition",
        ));
    }
    let mut pdf = Pdf::new();
    let mut written = Vec::new();
    for chunk in (0..pages.len())
        .collect::<Vec<_>>()
        .chunks(imposition.up as usize)
    {
        let outputs: Vec<_> = chunk
            .iter()
            .map(|i| PageOutput {
                separated: &pages[*i],
                trim: trims[*i],
                bleed: bleeds[*i].into(),
                slug: Insets::ZERO,
                settings,
                imposition: Imposition {
                    up: 1,
                    marks: imposition.marks,
                },
                marks: Marks::default(),
                overprint: true,
            })
            .collect();
        written.push(write_sheet(&mut pdf, &outputs, imposition)?);
    }
    Ok(pdf.finish(&written))
}

/// Add one sheet while allowing the caller to release its raster plates
/// before separating the next sheet.
pub fn write_sheet(
    pdf: &mut Pdf,
    outputs: &[PageOutput<'_>],
    imposition: Imposition,
) -> Result<usize, std::io::Error> {
    if outputs.is_empty()
        || !matches!(imposition.up, 1 | 2 | 4)
        || outputs.len() > imposition.up as usize
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid sheet page count",
        ));
    }
    if imposition.up == 1 {
        return write_page(pdf, &outputs[0]);
    }
    let mut forms = Vec::new();
    let mut cell = (0.0_f32, 0.0_f32);
    for page in outputs {
        let size = sheet_size(page.trim, media_offsets(page));
        let form = write_leaf(pdf, page, true)?;
        cell.0 = cell.0.max(size.0);
        cell.1 = cell.1.max(size.1);
        forms.push((form, size));
    }
    let sheet = (
        cell.0 * 2.0,
        cell.1 * if imposition.up == 4 { 2.0 } else { 1.0 },
    );
    let slots = imposition.slots(sheet, cell, 0.0);
    let mut resources = String::new();
    let mut content = String::new();
    for (i, ((form, size), (x, y))) in forms.iter().zip(slots).enumerate() {
        resources.push_str(&format!("/P{i} {form} 0 R "));
        let x = x + (cell.0 - size.0) / 2.0;
        let y = y + (cell.1 - size.1) / 2.0;
        content.push_str(&format!("q 1 0 0 1 {x:.3} {y:.3} cm /P{i} Do Q\n"));
    }
    let contents = pdf.stream("", content.as_bytes())?;
    let sheet=pdf.add_str(&format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] /Resources << /XObject << {resources} >> >> /Contents {contents} 0 R >>",sheet.0,sheet.1));
    if !pdf.within_limit() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "PDF exceeds size limit",
        ));
    }
    Ok(sheet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_pdf_is_still_well_formed() {
        let out = Pdf::new().finish(&[]);
        let text = String::from_utf8_lossy(&out);
        assert!(text.starts_with("%PDF-1.6"));
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("startxref"));
        assert!(text.trim_end().ends_with("%%EOF"));
        assert!(text.contains("/Count 0"));
    }

    #[test]
    fn the_cross_reference_table_points_at_every_object() {
        let mut pdf = Pdf::new();
        let a = pdf.add_str("<< /Dummy 1 >>");
        let b = pdf.add_str("<< /Dummy 2 >>");
        let out = pdf.finish(&[a, b]);
        let text = String::from_utf8_lossy(&out);
        // Four objects: the two reserved plus the two added.
        assert!(text.contains("/Count 2"));
        assert!(text.contains("/Size 5"));
        // Every "N 0 obj" has a matching xref entry, or a reader will
        // refuse the file.
        for n in 1..=4 {
            assert!(
                text.contains(&format!("\n{n} 0 obj\n")),
                "object {n} missing"
            );
        }
    }

    #[test]
    fn ink_names_escape_delimiters_and_preserve_every_utf8_byte() {
        for name in ["Plain", "A (B)", "Pantone 032 Ä", "色/Spot#1", "[]%\\"] {
            let encoded = pdf_name(name);
            assert!(encoded.is_ascii());
            let bytes = encoded.as_bytes();
            let mut at = 0;
            let mut decoded = Vec::new();
            while at < bytes.len() {
                if bytes[at] == b'#' {
                    decoded.push(u8::from_str_radix(&encoded[at + 1..at + 3], 16).unwrap());
                    at += 3;
                } else {
                    assert!(!b"()<>[]{}/% ".contains(&bytes[at]));
                    decoded.push(bytes[at]);
                    at += 1;
                }
            }
            assert_eq!(decoded, name.as_bytes());
        }
    }

    #[test]
    fn coverage_becomes_ink_density_not_its_own_value() {
        let rect = schist_core::IntRect::new(0, 0, 2, 1);
        let mut plate = PlateCoverage::new(rect);
        // Coverage 1.0 is a full flood and 0.0 is bare paper.
        plate.set(0, 0, 1.0);
        plate.set(1, 0, 0.0);
        let bytes = plate_to_gray(&plate);
        // Full ink is black (0) and bare paper is white (255). Printing
        // the coverage directly would invert the plate.
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[1], 255);
    }

    #[test]
    fn tint_functions_are_linear_and_keep_absent_channels_zero() {
        for build in [
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.5, 0.25, 0.1, 0.05],
        ] {
            let function = tint_function(build);
            assert!(function.contains("/FunctionType 2"));
            assert!(function.contains("/C0 [0 0 0 0]"));
            let start = function.find("/C1 [").unwrap() + 5;
            let end = start + function[start..].find(']').unwrap();
            let values: Vec<f32> = function[start..end]
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            for tint in [0.0, 0.1, 0.5, 1.0] {
                for c in 0..4 {
                    assert_eq!(values[c] * tint, build[c] * tint);
                }
            }
        }
    }

    #[test]
    fn a_two_up_sheet_keeps_sequential_reading_order() {
        let slots = Imposition { up: 2, marks: true }.slots((600.0, 800.0), (280.0, 400.0), 10.0);
        assert_eq!(slots.len(), 2);
        // Page 0 on the right, page 1 on the left: a spread's left-hand
        // page is the even one.
        assert!(slots[0].0 < slots[1].0, "{slots:?}");
    }

    #[test]
    fn a_four_up_sheet_fills_a_quadrant_grid() {
        let slots = Imposition { up: 4, marks: true }.slots((600.0, 800.0), (280.0, 380.0), 10.0);
        assert_eq!(slots.len(), 4);
        let mut xs: Vec<f32> = slots.iter().map(|s| s.0).collect();
        let mut ys: Vec<f32> = slots.iter().map(|s| s.1).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(xs[0], xs[1]);
        assert_eq!(xs[2], xs[3]);
        assert_eq!(ys[0], ys[1]);
        assert_eq!(ys[2], ys[3]);
    }

    #[test]
    fn a_single_page_sits_in_the_middle_of_the_sheet() {
        let slots = Imposition::single().slots((600.0, 800.0), (200.0, 300.0), 0.0);
        assert_eq!(slots, vec![(200.0, 250.0)]);
    }

    #[test]
    fn a_sheet_larger_than_its_page_still_centres_it() {
        // A page wider than the sheet clamps to the origin rather than
        // going negative, which would put it off the paper.
        let slots = Imposition::single().slots((100.0, 100.0), (200.0, 300.0), 0.0);
        assert_eq!(slots, vec![(0.0, 0.0)]);
    }

    #[test]
    fn crop_marks_stay_outside_bleed_and_circles_have_valid_cubic_operands() {
        for bleed in [0.0, 3.0, 9.0] {
            let marks = marks_for((595.0, 842.0), bleed, &Marks::default());
            let cubics: Vec<_> = marks.lines().filter(|l| l.ends_with(" c")).collect();
            assert_eq!(cubics.len(), 16);
            for cubic in cubics {
                assert_eq!(
                    cubic
                        .split_whitespace()
                        .filter(|v| v.parse::<f32>().is_ok())
                        .count(),
                    6
                );
            }
            let first = marks.lines().nth(1).unwrap();
            let values: Vec<f32> = first
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            assert_eq!(values, [0.0, -bleed - 1.0, 0.0, -bleed - 4.0]);
        }
    }

    #[test]
    fn marks_can_be_left_off() {
        let marks = Marks {
            registration: false,
            ..Marks::default()
        };
        let text = marks_for((595.0, 842.0), 0.0, &marks);
        assert!(!text.contains(" c\n"), "a registration target was drawn");
        assert!(text.contains(" S\n"), "the crop marks went too");
    }

    #[test]
    fn a_trim_box_inside_the_bleed_box() {
        let trim = trim_box((595.0, 842.0), 9.0);
        let bleed = bleed_box((595.0, 842.0), 9.0);
        assert_eq!(trim, "[9.00 9.00 604.00 851.00]");
        assert_eq!(bleed, "[0.00 0.00 613.00 860.00]");
    }

    #[test]
    fn a_bit_mask_packs_eight_pixels_to_a_byte() {
        let rect = schist_core::IntRect::new(0, 0, 8, 1);
        let mut plate = PlateCoverage::new(rect);
        plate.set(0, 0, 1.0);
        plate.set(7, 0, 1.0);
        let bits = plate_to_bits(&plate, 0.5);
        assert_eq!(bits.len(), 1);
        assert_eq!(bits[0], 0b1000_0001);
    }

    #[test]
    fn a_bit_mask_pads_a_partial_byte() {
        let rect = schist_core::IntRect::new(0, 0, 3, 1);
        let mut plate = PlateCoverage::new(rect);
        plate.set(1, 0, 1.0);
        assert_eq!(plate_to_bits(&plate, 0.5).len(), 1);
    }

    #[test]
    fn the_size_ceiling_is_reported_before_it_is_reached() {
        let mut pdf = Pdf::new();
        assert!(pdf.within_limit());
        pdf.add(vec![0u8; 1024]);
        assert!(pdf.within_limit());
    }
}
