//! Bounded, color-managed PDF print jobs. PDF syntax is generated only from
//! numbers and fixed tokens; user text is shaped into raster caption images.
use anyhow::{bail, Result};
use schist_colormgmt::{ColorTransform, Intent, Profile};
use schist_core::{Document, IntRect};
use schist_i18n::t;
use std::{
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub mod layout;
pub use layout::{Frame, Placement, Preset};

pub const MAX_ITEMS: usize = 200;
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_PIXELS: u64 = 40_000_000;
const MAX_EDGE: u32 = 32_768;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub letter: bool,
    pub landscape: bool,
    pub margin: f32,
    pub columns: usize,
    pub rows: usize,
    pub dpi: u32,
    pub actual_size: bool,
    pub captions: bool,
    pub layout: Vec<Placement>,
    pub page_count: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            letter: false,
            landscape: false,
            margin: 10.0,
            columns: 3,
            rows: 4,
            dpi: 300,
            actual_size: false,
            captions: true,
            layout: Vec::new(),
            page_count: 0,
        }
    }
}
impl Options {
    pub fn page_mm(&self) -> (f32, f32) {
        let (w, h) = if self.letter {
            (215.9, 279.4)
        } else {
            (210.0, 297.0)
        };
        if self.landscape {
            (h, w)
        } else {
            (w, h)
        }
    }
    pub fn capacity(&self) -> usize {
        self.columns * self.rows
    }
    pub fn pages(&self, count: usize) -> usize {
        if self.layout.is_empty() && self.page_count == 0 {
            count.div_ceil(self.capacity().max(1))
        } else {
            self.layout
                .iter()
                .map(|p| p.page + 1)
                .max()
                .unwrap_or(1)
                .max(self.page_count)
        }
    }
    pub fn validate(&self, count: usize) -> Result<()> {
        if !(1..=5).contains(&self.columns)
            || !(1..=6).contains(&self.rows)
            || !(72..=600).contains(&self.dpi)
            || !self.margin.is_finite()
            || !(5.0..=30.0).contains(&self.margin)
            || !(1..=MAX_ITEMS).contains(&count)
            || self.layout.len() > MAX_ITEMS
            || self.page_count > MAX_ITEMS
            || self
                .layout
                .iter()
                .any(|p| p.source >= count || p.page >= MAX_ITEMS || !p.frame.valid(self.page_mm()))
        {
            bail!("{}", t("printing.invalid"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Item {
    pub path: std::path::PathBuf,
    pub name: String,
    pub rating: u8,
    pub snapshot: Option<Arc<Document>>,
    pub preview: Option<Arc<gpui::RenderImage>>,
    pub caption: String,
    pub dimensions: Option<(u32, u32, f32)>,
}
impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.name == other.name
            && self.rating == other.rating
            && self.caption == other.caption
            && self.snapshot.as_ref().map(|d| d.id) == other.snapshot.as_ref().map(|d| d.id)
            && match (&self.preview, &other.preview) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub index: usize,
    pub start: gpui::Point<gpui::Pixels>,
    pub frame: Frame,
    pub resize: bool,
}
#[derive(Clone, Debug)]
pub struct Editor {
    pub options: Options,
    pub photos: Vec<Item>,
    pub cancel: Arc<AtomicBool>,
    pub running: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub preset: Preset,
    pub page: usize,
    pub selected: Option<usize>,
    pub drag: Option<Drag>,
    pub preview_bounds: Option<gpui::Bounds<gpui::Pixels>>,
    pub undo: Vec<(Vec<Placement>, usize)>,
    pub redo: Vec<(Vec<Placement>, usize)>,
}
impl PartialEq for Editor {
    fn eq(&self, other: &Self) -> bool {
        self.options == other.options
            && self.photos == other.photos
            && self.running == other.running
            && self.loading == other.loading
            && self.error == other.error
            && self.notice == other.notice
            && self.preset == other.preset
            && self.page == other.page
            && self.selected == other.selected
            && self.drag == other.drag
            && Arc::ptr_eq(&self.cancel, &other.cancel)
    }
}
impl Editor {
    pub fn new(photos: Vec<Item>) -> Self {
        let single = photos.len() == 1 && photos[0].snapshot.is_some();
        let mut options = Options::default();
        if single {
            options.columns = 1;
            options.rows = 1;
            options.captions = false;
        }
        options.layout = layout::grid(&options, 0..photos.len());
        options.page_count = options.pages(photos.len());
        Self {
            options,
            photos,
            cancel: Arc::new(AtomicBool::new(false)),
            running: false,
            loading: true,
            error: None,
            notice: None,
            preset: if single {
                Preset::Single
            } else {
                Preset::Contact
            },
            page: 0,
            selected: None,
            drag: None,
            preview_bounds: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
    pub fn remember(&mut self) {
        if self.undo.last().map(|s| (&s.0, s.1))
            != Some((&self.options.layout, self.options.page_count))
        {
            self.undo
                .push((self.options.layout.clone(), self.options.page_count));
            if self.undo.len() > 50 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
    }
    pub fn history(&mut self, undo: bool) {
        let (from, to) = if undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        if let Some((layout, pages)) = from.pop() {
            to.push((
                std::mem::replace(&mut self.options.layout, layout),
                self.options.page_count,
            ));
            self.options.page_count = pages;
            self.selected = None;
            self.preset = Preset::Custom;
            self.page = self
                .page
                .min(self.options.pages(self.photos.len()).saturating_sub(1));
        }
    }
    pub fn apply_preset(&mut self, preset: Preset) {
        self.preset = preset;
        if let Some((cols, rows)) = preset.grid() {
            self.remember();
            self.options.columns = cols;
            self.options.rows = rows;
            let sources: Vec<_> = self.options.layout.iter().map(|p| p.source).collect();
            self.options.layout = layout::grid(&self.options, sources);
            self.options.page_count = self
                .options
                .layout
                .iter()
                .map(|p| p.page + 1)
                .max()
                .unwrap_or(1);
            self.page = self.page.min(self.options.page_count - 1);
            self.selected = None;
        }
    }
    pub fn add(&mut self, source: usize) {
        if source >= self.photos.len() || self.options.layout.len() >= MAX_ITEMS {
            return;
        }
        self.remember();
        let (w, h) = self.options.page_mm();
        self.options.layout.push(Placement {
            source,
            page: self.page,
            frame: Frame {
                x: w * 0.2,
                y: h * 0.2,
                width: w * 0.6,
                height: h * 0.6,
            },
        });
        self.selected = Some(self.options.layout.len() - 1);
        self.preset = Preset::Custom;
    }
    pub fn change_paper(&mut self, old: (f32, f32)) {
        let new = self.options.page_mm();
        for p in &mut self.options.layout {
            p.frame.x *= new.0 / old.0;
            p.frame.width *= new.0 / old.0;
            p.frame.y *= new.1 / old.1;
            p.frame.height *= new.1 / old.1;
            p.frame.width = p.frame.width.max(8.0).min(new.0);
            p.frame.height = p.frame.height.max(8.0).min(new.1);
            p.frame.x = p.frame.x.min(new.0 - p.frame.width);
            p.frame.y = p.frame.y.min(new.1 - p.frame.height);
        }
        // Old geometry belongs to another paper size.
        self.undo.clear();
        self.redo.clear();
    }
}
pub fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("{}", t("common.cancel"));
    }
    Ok(())
}

fn placement(w: u32, h: u32, dpi: f32, box_w: f32, box_h: f32, actual: bool) -> Result<(f32, f32)> {
    if w == 0 || h == 0 || !box_w.is_finite() || !box_h.is_finite() || box_w <= 0.0 || box_h <= 0.0
    {
        bail!("{}", t("printing.invalid"));
    }
    if actual {
        if !dpi.is_finite() || dpi <= 0.0 {
            bail!("{}", t("printing.invalid"));
        }
        let (w, h) = (w as f32 * 25.4 / dpi, h as f32 * 25.4 / dpi);
        if !w.is_finite() || !h.is_finite() || w > box_w + 0.01 || h > box_h + 0.01 {
            bail!("{}", t("printing.does_not_fit"));
        }
        Ok((w, h))
    } else {
        // Fit is a pixel aspect-ratio operation; malformed metadata DPI must
        // not turn its physical placement into NaN or infinity in the PDF.
        let scale = (box_w / w as f32).min(box_h / h as f32);
        Ok((w as f32 * scale, h as f32 * scale))
    }
}

/// Resolve the source interpretation explicitly. Invalid/mismatched profiles are
/// errors, never silently replaced by an sRGB tag. Native process channels
/// are already converted to sRGB by the compositor after profile validation.
fn transform(doc: &Document, working: &Profile) -> Result<ColorTransform> {
    if matches!(
        doc.mode,
        schist_color::ColorMode::Cmyk | schist_color::ColorMode::Lab
    ) {
        // Native compositing converts process channels to sRGB. Validate first
        // because its display path permits a fallback; never apply a CMYK/Lab
        // profile a second time to those resulting RGB numbers.
        schist_colormgmt::NativeColorTransform::new(doc.mode, doc.icc_profile.as_deref())
            .map_err(|_| anyhow::anyhow!("{}", t("printing.profile_error")))?;
        return Ok(ColorTransform::identity());
    }
    let src = match doc.icc_profile.as_deref() {
        Some(bytes) => Profile::from_bytes(bytes)
            .map_err(|_| anyhow::anyhow!("{}", t("printing.profile_error")))?,
        None => working.clone(),
    };
    if src.color_mode() != Some(schist_color::ColorMode::Rgb) {
        bail!("{}", t("printing.profile_error"));
    }
    ColorTransform::new(&src, &Profile::srgb(), Intent::RelativeColorimetric)
        .map_err(|_| anyhow::anyhow!("{}", t("printing.profile_error")))
}
async fn raster_async(
    doc: &Document,
    working: &Profile,
    target: (u32, u32),
    cancel: &AtomicBool,
    yield_executor: Option<&gpui::BackgroundExecutor>,
) -> Result<image::RgbImage> {
    if doc.width > MAX_EDGE
        || doc.height > MAX_EDGE
        || doc.width as u64 * doc.height as u64 > MAX_PIXELS
    {
        bail!("{}", t("printing.limit"));
    }
    let transform = transform(doc, working)?;
    let mut rgb = image::RgbImage::new(doc.width, doc.height);
    for y in (0..doc.height).step_by(32) {
        cooperate(yield_executor, cancel).await?;
        let rect = IntRect {
            left: 0,
            top: y as i32,
            right: doc.width as i32,
            bottom: (y + 32).min(doc.height) as i32,
        };
        let mut strip = schist_compositor::composite_region_f32_cpu(doc, rect);
        if strip.iter().any(|v| !v.is_finite()) {
            bail!("{}", t("printing.invalid"));
        }
        transform.apply(&mut strip);
        let raw: &mut [u8] = rgb.as_mut();
        let out = &mut raw[y as usize * doc.width as usize * 3..];
        for (pixel, dst) in strip
            .as_chunks::<4>()
            .0
            .iter()
            .zip(out.as_chunks_mut::<3>().0.iter_mut())
        {
            let alpha = pixel[3].clamp(0.0, 1.0);
            for c in 0..3 {
                dst[c] = ((pixel[c].clamp(0.0, 1.0) * alpha + 1.0 - alpha) * 255.0).round() as u8;
            }
        }
    }
    check(cancel)?;
    // Never invent image resolution: downsample oversized sources only.
    if doc.width > target.0 || doc.height > target.1 {
        Ok(image::imageops::resize(
            &rgb,
            target.0.min(doc.width).max(1),
            target.1.min(doc.height).max(1),
            image::imageops::FilterType::Lanczos3,
        ))
    } else {
        Ok(rgb)
    }
}

/// Bounded, color-managed thumbnail for the print composer.
pub async fn preview(
    doc: &Document,
    working: &Profile,
    cancel: &AtomicBool,
    executor: Option<&gpui::BackgroundExecutor>,
) -> Result<image::RgbaImage> {
    let scale = 320.0 / doc.width.max(doc.height).max(1) as f32;
    let target = (
        (doc.width as f32 * scale).ceil().max(1.0) as u32,
        (doc.height as f32 * scale).ceil().max(1.0) as u32,
    );
    let rgb = raster_async(doc, working, target, cancel, executor).await?;
    Ok(image::RgbaImage::from_fn(
        rgb.width(),
        rgb.height(),
        |x, y| {
            let p = rgb.get_pixel(x, y);
            image::Rgba([p[2], p[1], p[0], 255])
        },
    ))
}

struct Pdf {
    objects: Vec<Vec<u8>>,
    bytes: usize,
}
impl Pdf {
    fn new() -> Self {
        Self {
            objects: vec![Vec::new(), Vec::new()],
            bytes: 0,
        }
    }
    fn add(&mut self, bytes: Vec<u8>) -> Result<usize> {
        self.bytes += bytes.len();
        if self.bytes > MAX_BYTES {
            bail!("{}", t("printing.limit"));
        }
        self.objects.push(bytes);
        Ok(self.objects.len())
    }
    fn stream(&mut self, dict: &str, bytes: &[u8]) -> Result<usize> {
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
        self.add(stream)
    }
    fn image(&mut self, w: u32, h: u32, color: &str, data: &[u8]) -> Result<usize> {
        self.stream(&format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace {color} /BitsPerComponent 8"), data)
    }
    fn finish(mut self, pages: &[usize]) -> Vec<u8> {
        self.objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
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
        let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, obj) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).bytes());
            out.extend(obj);
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

/// Sequential decoding keeps only one original and its raster in memory.
/// Captions are rasterized with the app's Unicode shaping and font fallback.
pub async fn render_async(
    options: &Options,
    count: usize,
    working: &Profile,
    cancel: &AtomicBool,
    yield_executor: Option<&gpui::BackgroundExecutor>,
    mut source: impl FnMut(usize) -> Result<(Document, String)>,
) -> Result<Vec<u8>> {
    options.validate(count)?;
    let mut pdf = Pdf::new();
    let srgb = Profile::srgb();
    let icc = srgb
        .icc_bytes()
        .ok_or_else(|| anyhow::anyhow!("{}", t("printing.profile_error")))?;
    let profile = pdf.stream("/N 3 /Alternate /DeviceRGB", icc)?;
    let color = format!("[/ICCBased {profile} 0 R]");
    let (pw, ph) = options.page_mm();
    let layout = if options.layout.is_empty() && options.page_count == 0 {
        layout::grid(options, 0..count)
    } else {
        options.layout.clone()
    };
    let mut pages = Vec::new();
    for page in 0..options.pages(count) {
        check(cancel)?;
        let mut commands = String::new();
        let mut resources = String::new();
        for placed in layout.iter().filter(|p| p.page == page) {
            check(cancel)?;
            let (doc, caption) = source(placed.source)?;
            let Frame {
                x,
                y: top,
                width: cw,
                height: ch,
            } = placed.frame;
            let (x, top) = (x + 2.0, top + 2.0);
            let mut caption_h = 0.0;
            let caption_image = if options.captions && !caption.is_empty() {
                if caption.chars().count() > 4096 {
                    bail!("{}", t("printing.caption_limit"));
                }
                let spec = schist_text_engine::TextSpec {
                    text: caption,
                    size: options.dpi as f32 * 8.0 / 72.0,
                    wrap_width: Some((cw - 4.0) * options.dpi as f32 / 25.4),
                    ..Default::default()
                };
                let raster = schist_text_engine::rasterize(&spec)
                    .ok_or_else(|| anyhow::anyhow!("{}", t("printing.font_error")))?;
                let w = raster.bounds.width().max(0) as u32;
                let h = raster.bounds.height().max(0) as u32;
                caption_h = h as f32 * 25.4 / options.dpi as f32;
                if caption_h > ch * 0.45 || w as f32 * 25.4 / options.dpi as f32 > cw - 3.0 {
                    bail!("{}", t("printing.caption_limit"));
                }
                if w > 0 && h > 0 {
                    let data: Vec<_> = raster.coverage.iter().map(|v| 255 - v).collect();
                    Some((pdf.image(w, h, "/DeviceGray", &data)?, w, h))
                } else {
                    None
                }
            } else {
                None
            };
            let image_box_h = ch
                - 4.0
                - if caption_image.is_some() {
                    caption_h + 2.0
                } else {
                    0.0
                };
            let (w, h) = placement(
                doc.width,
                doc.height,
                doc.resolution_dpi,
                cw - 4.0,
                image_box_h,
                options.actual_size,
            )?;
            let target = (
                (w * options.dpi as f32 / 25.4).ceil() as u32,
                (h * options.dpi as f32 / 25.4).ceil() as u32,
            );
            let image = raster_async(&doc, working, target, cancel, yield_executor).await?;
            let id = pdf.image(image.width(), image.height(), &color, image.as_raw())?;
            resources.push_str(&format!("/I{id} {id} 0 R "));
            draw(
                &mut commands,
                id,
                x + (cw - 4.0 - w) / 2.0,
                ph - top - (image_box_h - h) / 2.0 - h,
                w,
                h,
            );
            if let Some((id, w, h)) = caption_image {
                resources.push_str(&format!("/I{id} {id} 0 R "));
                draw(
                    &mut commands,
                    id,
                    x,
                    ph - top - (ch - 4.0),
                    w as f32 * 25.4 / options.dpi as f32,
                    h as f32 * 25.4 / options.dpi as f32,
                );
            }
        }
        let contents = pdf.stream("", commands.as_bytes())?;
        pages.push(pdf.add(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.5} {:.5}] /Resources << /XObject << {resources} >> >> /Contents {contents} 0 R >>", pw * 72.0/25.4, ph * 72.0/25.4).into_bytes())?);
    }
    check(cancel)?;
    let bytes = pdf.finish(&pages);
    cooperate(yield_executor, cancel).await?;
    Ok(bytes)
}
// GPUI's browser executor shares the JS thread. Timer yields allow paint and
// cancellation events; native jobs already run on a background thread.
async fn cooperate(executor: Option<&gpui::BackgroundExecutor>, cancel: &AtomicBool) -> Result<()> {
    if let Some(executor) = executor {
        executor.timer(std::time::Duration::from_millis(1)).await;
    }
    check(cancel)
}

#[cfg(test)]
fn raster(
    doc: &Document,
    working: &Profile,
    target: (u32, u32),
    cancel: &AtomicBool,
) -> Result<image::RgbImage> {
    futures::executor::block_on(raster_async(doc, working, target, cancel, None))
}
#[cfg(test)]
fn render(
    options: &Options,
    count: usize,
    working: &Profile,
    cancel: &AtomicBool,
    source: impl FnMut(usize) -> Result<(Document, String)>,
) -> Result<Vec<u8>> {
    futures::executor::block_on(render_async(options, count, working, cancel, None, source))
}

fn draw(commands: &mut String, id: usize, x: f32, y: f32, w: f32, h: f32) {
    let pt = 72.0 / 25.4;
    commands.push_str(&format!(
        "q {:.5} 0 0 {:.5} {:.5} {:.5} cm /I{id} Do Q\n",
        w * pt,
        h * pt,
        x * pt,
        y * pt
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adding_images_preserves_custom_layout_and_preset_changes_can_be_undone() {
        let source = || Item {
            path: "fixture.png".into(),
            name: "fixture".into(),
            rating: 4,
            snapshot: None,
            preview: None,
            caption: String::new(),
            dimensions: Some((600, 400, 300.0)),
        };
        let mut editor = Editor::new(vec![source()]);
        editor.options.layout[0].frame = Frame {
            x: 12.0,
            y: 18.0,
            width: 60.0,
            height: 40.0,
        };
        let original = editor.options.layout[0];
        editor.photos.push(source());
        editor.add(1);
        assert_eq!(editor.options.layout[0], original);
        let custom = editor.options.layout.clone();
        editor.apply_preset(Preset::Four);
        let arranged = editor.options.layout.clone();
        assert_ne!(arranged, custom);
        editor.history(true);
        assert_eq!(editor.options.layout, custom);
        editor.history(false);
        assert_eq!(editor.options.layout, arranged);
        let old = editor.options.page_mm();
        editor.options.landscape = true;
        editor.change_paper(old);
        assert!(editor.options.validate(editor.photos.len()).is_ok());
    }

    #[test]
    fn custom_pdf_keeps_page_positions_and_repeated_sources() {
        let mut options = Options {
            captions: false,
            page_count: 2,
            ..Default::default()
        };
        options.layout = vec![
            Placement {
                source: 1,
                page: 0,
                frame: Frame {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 40.0,
                },
            },
            Placement {
                source: 1,
                page: 1,
                frame: Frame {
                    x: 60.0,
                    y: 90.0,
                    width: 40.0,
                    height: 40.0,
                },
            },
        ];
        let mut loaded = Vec::new();
        let bytes = render(
            &options,
            2,
            &Profile::srgb(),
            &AtomicBool::new(false),
            |index| {
                loaded.push(index);
                Ok((
                    Document::new("square", 2, 2, schist_color::Depth::Eight),
                    String::new(),
                ))
            },
        )
        .unwrap();
        assert_eq!(
            loaded,
            vec![1, 1],
            "layout order and duplicates are retained"
        );
        let parsed = lopdf::Document::load_mem(&bytes).unwrap();
        let pages = parsed.get_pages();
        assert_eq!(pages.len(), 2);
        for (page, x, top) in [(1, 42.0, 32.0), (2, 62.0, 92.0)] {
            let content =
                lopdf::content::Content::decode(&parsed.get_page_content(pages[&page]).unwrap())
                    .unwrap();
            let matrix = content
                .operations
                .iter()
                .find(|op| op.operator == "cm")
                .unwrap();
            let values: Vec<_> = matrix
                .operands
                .iter()
                .map(|v| v.as_float().unwrap())
                .collect();
            assert!((values[0] - 36.0 * 72.0 / 25.4).abs() < 0.001);
            assert!((values[4] - x * 72.0 / 25.4).abs() < 0.001);
            assert!((values[5] - (297.0 - top - 36.0) * 72.0 / 25.4).abs() < 0.001);
        }
        options.layout[0].source = 2;
        assert!(options.validate(2).is_err());
        options.layout[0].source = 1;
        options.layout[0].frame.x = f32::NAN;
        assert!(options.validate(2).is_err());
    }

    #[test]
    fn pagination_and_physical_size() {
        let o = Options::default();
        assert_eq!(o.pages(25), 3);
        assert_eq!(
            placement(300, 600, 300.0, 100.0, 100.0, true).unwrap(),
            (25.4, 50.8)
        );
        assert!(placement(3000, 6000, 300.0, 100.0, 100.0, true).is_err());
        assert!(placement(1, 1, f32::NAN, 100.0, 100.0, true).is_err());
        assert!(placement(1, 1, f32::MIN_POSITIVE, 100.0, 100.0, true).is_err());
        assert_eq!(
            placement(1, 1, f32::NAN, 100.0, 100.0, false).unwrap(),
            (100.0, 100.0)
        );
        let mut o = o;
        o.landscape = true;
        assert_eq!(o.page_mm(), (297.0, 210.0));
    }
    #[test]
    fn invalid_profiles_never_retagged() {
        let mut doc = Document::new("test", 1, 1, schist_color::Depth::Eight);
        doc.icc_profile = Some(vec![0, 1]);
        assert!(transform(&doc, &Profile::srgb()).is_err());
        doc.icc_profile = None;
        doc.mode = schist_color::ColorMode::Cmyk;
        assert!(transform(&doc, &Profile::srgb()).is_err());
    }
    #[test]
    fn color_transform_changes_pixels_and_transparency_prints_on_white() {
        let mut doc = Document::new("test", 1, 1, schist_color::Depth::Eight);
        let mut layer = schist_core::Layer::new_raster("pixels");
        schist_core::blit_rgba8(
            &mut layer.as_raster_mut().unwrap().tiles,
            doc.depth,
            IntRect::from_size(1, 1),
            &[180, 90, 20, 255],
        );
        doc.push_layer(layer);
        let untagged = raster(&doc, &Profile::srgb(), (1, 1), &AtomicBool::new(false)).unwrap();
        doc.icc_profile = Profile::display_p3().icc_bytes().map(|b| b.to_vec());
        let tagged = raster(&doc, &Profile::srgb(), (1, 1), &AtomicBool::new(false)).unwrap();
        assert_ne!(
            untagged.as_raw(),
            tagged.as_raw(),
            "conversion must change numbers, not only tags"
        );
        doc.icc_profile = None;
        let working = raster(
            &doc,
            &Profile::display_p3(),
            (1, 1),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            tagged.as_raw(),
            working.as_raw(),
            "untagged pixels use the selected working profile"
        );
        let blank = Document::new("blank", 1, 1, schist_color::Depth::Eight);
        assert_eq!(
            raster(&blank, &Profile::srgb(), (1, 1), &AtomicBool::new(false))
                .unwrap()
                .as_raw(),
            &[255, 255, 255]
        );
    }
    #[test]
    fn tagged_lab_native_samples_are_converted_once() {
        let mut profile = moxcms::ColorProfile::new_lab();
        profile.pcs = moxcms::DataColorSpace::Lab;
        let mut doc = Document::new("native", 1, 1, schist_color::Depth::ThirtyTwo);
        doc.mode = schist_color::ColorMode::Lab;
        doc.icc_profile = Some(profile.encode().unwrap());
        let mut layer = schist_core::Layer::new_raster("pixels");
        layer
            .as_raster_mut()
            .unwrap()
            .tiles
            .get_mut_or_insert_mode(schist_core::TileCoord { tx: 0, ty: 0 }, doc.depth, doc.mode)
            .set_native_pixel(
                0,
                schist_color::NativePixel {
                    mode: doc.mode,
                    color: [0.5, 128.0 / 255.0, 128.0 / 255.0, 0.0],
                    alpha: 1.0,
                },
            );
        doc.push_layer(layer);
        let rgb = raster(&doc, &Profile::srgb(), (1, 1), &AtomicBool::new(false)).unwrap();
        assert!(rgb.as_raw().iter().all(|v| *v > 75 && *v < 180));
        let working = raster(
            &doc,
            &Profile::display_p3(),
            (1, 1),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            rgb, working,
            "native composite is sRGB regardless of working RGB profile"
        );
        doc.mode = schist_color::ColorMode::Cmyk;
        assert!(
            transform(&doc, &Profile::srgb()).is_err(),
            "Lab profile must not be used for CMYK samples"
        );
    }
    #[test]
    fn captions_are_not_silently_truncated() {
        let o = Options::default();
        assert!(
            render(&o, 1, &Profile::srgb(), &AtomicBool::new(false), |_| Ok((
                Document::new("test", 1, 1, schist_color::Depth::Eight),
                "a".repeat(4097)
            )))
            .is_err()
        );
    }
    #[test]
    fn cancellation_and_limits() {
        assert!(check(&AtomicBool::new(true)).is_err());
        let mut o = Options {
            columns: 0,
            ..Default::default()
        };
        assert!(o.validate(1).is_err());
        o.columns = 1;
        assert!(o.validate(MAX_ITEMS + 1).is_err());
        let wide = Document::new("wide", MAX_EDGE + 1, 1, schist_color::Depth::Eight);
        assert!(raster(&wide, &Profile::srgb(), (1, 1), &AtomicBool::new(false)).is_err());
    }
    #[test]
    fn actual_size_is_written_in_pdf_points() {
        let o = Options {
            columns: 1,
            rows: 1,
            captions: false,
            actual_size: true,
            ..Default::default()
        };
        let bytes = render(&o, 1, &Profile::srgb(), &AtomicBool::new(false), |_| {
            let mut doc = Document::new("actual", 300, 600, schist_color::Depth::Eight);
            doc.resolution_dpi = 300.0;
            Ok((doc, String::new()))
        })
        .unwrap();
        let parsed = lopdf::Document::load_mem(&bytes).unwrap();
        let page = *parsed.get_pages().values().next().unwrap();
        let content =
            lopdf::content::Content::decode(&parsed.get_page_content(page).unwrap()).unwrap();
        let matrix = content
            .operations
            .iter()
            .find(|op| op.operator == "cm")
            .unwrap();
        assert!((matrix.operands[0].as_float().unwrap() - 72.0).abs() < 0.001);
        assert!((matrix.operands[3].as_float().unwrap() - 144.0).abs() < 0.001);
        assert!(
            (matrix.operands[4].as_float().unwrap() - (210.0 - 25.4) / 2.0 * 72.0 / 25.4).abs()
                < 0.001
        );
        assert!(
            (matrix.operands[5].as_float().unwrap() - (297.0 - 50.8) / 2.0 * 72.0 / 25.4).abs()
                < 0.001
        );
    }
    #[test]
    fn parseable_pdf_has_pages_icc_and_no_executable_captions() {
        let o = Options {
            columns: 1,
            rows: 1,
            captions: true,
            ..Default::default()
        };
        let bytes = render(&o, 2, &Profile::srgb(), &AtomicBool::new(false), |_| {
            Ok((
                Document::new("test", 2, 2, schist_color::Depth::Eight),
                ") /JavaScript <script> café العربية 日本語".into(),
            ))
        })
        .unwrap();
        let parsed = lopdf::Document::load_mem(&bytes).unwrap();
        assert_eq!(parsed.get_pages().len(), 2);
        for page in parsed.get_pages().values() {
            let dict = parsed.get_object(*page).unwrap().as_dict().unwrap();
            let bounds = dict.get(b"MediaBox").unwrap().as_array().unwrap();
            assert_eq!(bounds.len(), 4);
            assert!((bounds[2].as_float().unwrap() - 210.0 * 72.0 / 25.4).abs() < 0.001);
            assert!((bounds[3].as_float().unwrap() - 297.0 * 72.0 / 25.4).abs() < 0.001);
            let resources = dict.get(b"Resources").unwrap().as_dict().unwrap();
            assert_eq!(
                resources.get(b"XObject").unwrap().as_dict().unwrap().len(),
                2
            );
        }
        assert!(!String::from_utf8_lossy(&bytes).contains("JavaScript"));
        assert!(String::from_utf8_lossy(&bytes).contains("/ICCBased"));
        assert!(String::from_utf8_lossy(&bytes).contains("/N 3"));
        let icc = parsed
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .find(|s| s.dict.get(b"N").is_ok())
            .unwrap()
            .decompressed_content()
            .unwrap();
        let embedded = Profile::from_bytes(&icc).unwrap();
        assert_eq!(embedded.color_mode(), Some(schist_color::ColorMode::Rgb));
        let transform =
            ColorTransform::new(&embedded, &Profile::srgb(), Intent::RelativeColorimetric).unwrap();
        let mut samples = [0.12, 0.5, 0.9, 1.0, 0.9, 0.2, 0.4, 1.0];
        let expected = samples;
        transform.apply(&mut samples);
        for (actual, expected) in samples.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 0.001,
                "embedded sRGB changes colors"
            );
        }
    }
}
