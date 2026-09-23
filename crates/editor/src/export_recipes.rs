//! Portable export recipes and their deterministic rendering/naming rules.
use anyhow::{ensure, Context as _, Result};
use schist_core::{Document, IntRect, Layer};
use schist_i18n::{t, tf};
use serde::{Deserialize, Serialize};
#[cfg(any(test, not(target_arch = "wasm32")))]
use std::path::Path;
use std::path::PathBuf;

mod finishing;
pub use finishing::{Finishing, Placement, TargetProfile};

pub const MAX_OUTPUTS: usize = 16;
pub const FLAT_CODECS: &[&str] = &["codec.png", "codec.jpeg", "codec.webp", "codec.tiff"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    #[default]
    Document,
    Artboards,
    Slices,
}
impl Scope {
    pub fn label(self) -> &'static str {
        t(match self {
            Self::Document => "export_recipes.document",
            Self::Artboards => "export_recipes.artboards",
            Self::Slices => "export_recipes.slices",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub codec: String,
    /// Zero preserves the original dimensions; otherwise shrink to this longest edge.
    pub max_edge: u32,
    pub quality: u8,
    pub template: String,
    #[serde(default)]
    pub finishing: Finishing,
}
impl Default for Output {
    fn default() -> Self {
        Self {
            codec: "codec.png".into(),
            max_edge: 0,
            quality: 90,
            template: "{name}-{region}-{index}".into(),
            finishing: Finishing::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub name: String,
    pub destination: PathBuf,
    pub scope: Scope,
    pub outputs: Vec<Output>,
    /// Captured at dispatch; untagged RGB uses the current application working profile.
    #[serde(skip)]
    pub working_icc: Option<Vec<u8>>,
}
impl Default for Recipe {
    fn default() -> Self {
        Self {
            name: t("export_recipes.starter").into(),
            destination: PathBuf::new(),
            scope: Scope::Document,
            working_icc: None,
            outputs: vec![
                Output::default(),
                Output {
                    codec: "codec.jpeg".into(),
                    max_edge: 1600,
                    ..Default::default()
                },
                Output {
                    codec: "codec.webp".into(),
                    max_edge: 512,
                    ..Default::default()
                },
            ],
        }
    }
}
impl Recipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty(),
            "{}",
            t("export_recipes.name_required")
        );
        ensure!(
            !self.outputs.is_empty() && self.outputs.len() <= MAX_OUTPUTS,
            "{}",
            t("export_recipes.outputs_required")
        );
        for output in &self.outputs {
            ensure!(
                FLAT_CODECS.contains(&output.codec.as_str()),
                "{}",
                tf!("export_recipes.unsupported_codec", codec = output.codec)
            );
            ensure!(
                output.max_edge <= 32768 && (1..=100).contains(&output.quality),
                "{}",
                t("export_recipes.invalid_size")
            );
            output.finishing.validate()?;
            output.filename("image", "canvas", 1, 1, 1)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub version: u32,
    pub recipes: Vec<Recipe>,
    pub selected: usize,
}
impl Default for Book {
    fn default() -> Self {
        Self {
            version: 1,
            recipes: vec![Recipe::default()],
            selected: 0,
        }
    }
}
impl Book {
    pub fn parse(text: &str) -> Result<Self> {
        let mut book: Self =
            serde_json::from_str(text).context(t("export_recipes.invalid_library"))?;
        ensure!(book.version == 1, "{}", t("export_recipes.invalid_library"));
        for recipe in &book.recipes {
            recipe.validate()?;
        }
        book.selected = book.selected.min(book.recipes.len().saturating_sub(1));
        Ok(book)
    }
    pub fn load() -> Result<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        let text = match std::fs::read_to_string(book_path()?) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        #[cfg(target_arch = "wasm32")]
        let text = crate::web::local_get("schist.export-recipes.v1");
        text.map(|text| Self::parse(&text))
            .transpose()
            .map(|book| book.unwrap_or_default())
    }
    pub fn save(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Write as _;
            let path = book_path()?;
            let dir = path.parent().unwrap();
            std::fs::create_dir_all(dir)?;
            let mut file = tempfile::NamedTempFile::new_in(dir)?;
            file.write_all(text.as_bytes())?;
            file.as_file().sync_all()?;
            file.persist(path)?;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let storage = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .ok_or_else(|| anyhow::anyhow!("{}", t("export_recipes.storage_failed")))?;
            storage
                .set_item("schist.export-recipes.v1", &text)
                .map_err(|_| anyhow::anyhow!("{}", t("export_recipes.storage_failed")))?;
        }
        Ok(())
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn book_path() -> Result<PathBuf> {
    schist_app_settings::schist_folder()
        .map(|dir| dir.join("export-recipes.json"))
        .ok_or_else(|| anyhow::anyhow!("{}", t("export_recipes.storage_failed")))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Editor {
    pub cloud_assets: Vec<schist_cloud::Asset>,
    /// Identity shared by clones of one modal session.
    pub session: std::sync::Arc<()>,
    pub book: Book,
    pub selected: Option<usize>,
    pub draft: Recipe,
    pub output: usize,
    pub show_finishing: bool,
    pub photos: Vec<PathBuf>,
    pub error: Option<String>,
}
impl Editor {
    pub fn new(book: Book, photos: Vec<PathBuf>) -> Self {
        let selected = (!book.recipes.is_empty()).then_some(book.selected);
        let draft = selected
            .and_then(|i| book.recipes.get(i))
            .cloned()
            .unwrap_or_default();
        let show_finishing = draft
            .outputs
            .first()
            .is_some_and(|output| output.finishing != Finishing::default());
        Self {
            session: std::sync::Arc::new(()),
            book,
            cloud_assets: Vec::new(),
            selected,
            draft,
            output: 0,
            show_finishing,
            photos,
            error: None,
        }
    }
    pub fn save(&mut self) -> Result<()> {
        self.draft.validate()?;
        // Typed relative folders must keep pointing at the same destination
        // when a later launch starts in a different working directory.
        #[cfg(not(target_arch = "wasm32"))]
        if self.draft.destination.is_relative() && self.draft.destination.is_dir() {
            self.draft.destination = std::fs::canonicalize(&self.draft.destination)?;
        }
        let mut book = self.book.clone();
        let index = self.selected.unwrap_or(book.recipes.len());
        ensure!(
            !book
                .recipes
                .iter()
                .enumerate()
                .any(|(i, recipe)| i != index && recipe.name.trim() == self.draft.name.trim()),
            "{}",
            t("export_recipes.duplicate_name")
        );
        if index == book.recipes.len() {
            book.recipes.push(self.draft.clone());
        } else {
            book.recipes[index] = self.draft.clone();
        }
        book.selected = index;
        book.save()?;
        self.book = book;
        self.selected = Some(index);
        self.error = None;
        Ok(())
    }
}

impl Output {
    pub fn dimensions(&self, w: u32, h: u32) -> (u32, u32) {
        let longest = w.max(h);
        if self.max_edge == 0 || longest <= self.max_edge {
            return (w, h);
        }
        let scale = self.max_edge as f64 / longest as f64;
        (
            ((w as f64 * scale).round() as u32).max(1),
            ((h as f64 * scale).round() as u32).max(1),
        )
    }
    pub fn filename(
        &self,
        name: &str,
        region: &str,
        w: u32,
        h: u32,
        index: usize,
    ) -> Result<String> {
        // Parse the template, then substitute: braces in a source filename are literal.
        let mut rest = self.template.as_str();
        let mut rendered = String::new();
        while let Some(at) = rest.find('{') {
            ensure!(
                !rest[..at].contains('}'),
                "{}",
                t("export_recipes.invalid_template")
            );
            rendered.push_str(&rest[..at]);
            let end = rest[at..]
                .find('}')
                .ok_or_else(|| anyhow::anyhow!("{}", t("export_recipes.invalid_template")))?
                + at;
            rendered.push_str(&match &rest[at + 1..end] {
                "name" => name.to_owned(),
                "region" => region.to_owned(),
                "width" => w.to_string(),
                "height" => h.to_string(),
                "index" => index.to_string(),
                _ => anyhow::bail!("{}", t("export_recipes.invalid_template")),
            });
            rest = &rest[end + 1..];
        }
        ensure!(
            !rest.contains('}'),
            "{}",
            t("export_recipes.invalid_template")
        );
        rendered.push_str(rest);
        ensure!(
            !rendered.trim().is_empty(),
            "{}",
            t("export_recipes.invalid_template")
        );
        Ok(safe_stem(&rendered))
    }
}
fn safe_stem(name: &str) -> String {
    let mut safe = String::new();
    for c in name.chars() {
        let c = if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
            c
        } else {
            '-'
        };
        if safe.len() + c.len_utf8() > 160 {
            break;
        }
        safe.push(c);
    }
    let safe = safe.trim_matches(['.', ' ', '-']);
    let safe = if safe.is_empty() { "export" } else { safe };
    let first = safe.split('.').next().unwrap_or(safe).to_ascii_uppercase();
    let reserved = matches!(first.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || first
            .strip_prefix("COM")
            .or_else(|| first.strip_prefix("LPT"))
            .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"));
    if reserved {
        format!("_{safe}")
    } else {
        safe.into()
    }
}

pub fn snapshot(doc: &Document) -> Document {
    let mut result = Document::new(doc.title.clone(), doc.width, doc.height, doc.depth);
    result.tree = doc.tree.clone();
    result.mode = doc.mode;
    result.ink_channels = doc.ink_channels.clone();
    result.ink_channels_loaded = doc.ink_channels_loaded;
    result.icc_profile = doc.icc_profile.clone();
    result.resolution_dpi = doc.resolution_dpi;
    result.artboards = doc.artboards.clone();
    result.slices = doc.slices.clone();
    result.path = doc.path.clone();
    result
}
pub fn regions(doc: &Document, scope: Scope) -> Result<Vec<(String, IntRect)>> {
    let regions: Vec<_> = match scope {
        Scope::Document => vec![("canvas".into(), doc.canvas_rect())],
        Scope::Artboards => doc
            .artboards
            .iter()
            .map(|r| (r.name.clone(), r.rect))
            .collect(),
        Scope::Slices => doc
            .slices
            .iter()
            .map(|r| (r.name.clone(), r.rect))
            .collect(),
    }
    .into_iter()
    .filter_map(|(name, rect)| {
        let rect = rect.intersect(&doc.canvas_rect());
        (!rect.is_empty()).then_some((name, rect))
    })
    .collect();
    ensure!(!regions.is_empty(), "{}", t("export_recipes.no_regions"));
    Ok(regions)
}
pub fn render(doc: &Document, rect: IntRect, output: &Output) -> Document {
    let rgba = schist_compositor::composite_region_f32(doc, rect);
    let (w, h) = (rect.width() as u32, rect.height() as u32);
    let mut result = Document::new(doc.title.clone(), w, h, doc.depth);
    // Native CMYK/Lab composites have already been converted to sRGB.
    result.icc_profile = if matches!(
        doc.mode,
        schist_color::ColorMode::Cmyk | schist_color::ColorMode::Lab
    ) {
        None
    } else {
        doc.icc_profile.clone()
    };
    result.resolution_dpi = doc.resolution_dpi;
    let mut layer = Layer::new_raster(t("common.background_layer"));
    schist_core::blit_rgba_f32(
        &mut layer.as_raster_mut().unwrap().tiles,
        result.depth,
        IntRect::from_size(w, h),
        &rgba,
    );
    result.push_layer(layer);
    let (w, h) = output.dimensions(w, h);
    if (w, h) != (result.width, result.height) {
        schist_tools_transform::resize_image(&mut result, w, h, schist_core::Filter::Bicubic);
    }
    result
}

/// Atomically claim a fresh name. Existing files and symlinks are never replaced.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_copy(dir: &Path, stem: &str, extension: &str, bytes: &[u8]) -> Result<PathBuf> {
    use std::io::Write as _;
    ensure!(
        extension.chars().all(|c| c.is_ascii_alphanumeric()) && !extension.is_empty(),
        "{}",
        t("export_recipes.invalid_extension")
    );
    let stem = safe_stem(stem);
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    for index in 1..=100_000 {
        let name = if index == 1 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem}-{index}.{extension}")
        };
        let path = dir.join(name);
        match file.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                file = error.file
            }
            Err(error) => return Err(error.error.into()),
        }
    }
    anyhow::bail!("{}", t("export_recipes.too_many_collisions"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_plugin_api::CodecPlugin;
    #[test]
    fn templates_cannot_escape_destination_or_reparse_source_names() {
        let output = Output {
            template: "../../{name}/{region}-{width}".into(),
            ..Default::default()
        };
        let name = output.filename("{height}", "C:\\bad", 9, 4, 1).unwrap();
        assert!(!name.contains('/') && !name.contains('\\'));
        assert_eq!(Path::new(&name).components().count(), 1);
        assert_eq!(safe_stem("CON"), "_CON");
        assert!(Output {
            template: "{unknown}".into(),
            ..output
        }
        .filename("a", "b", 1, 1, 1)
        .is_err());
    }
    #[test]
    fn sizing_preserves_aspect_and_never_upscales() {
        let output = Output {
            max_edge: 1600,
            ..Default::default()
        };
        assert_eq!(output.dimensions(4000, 3000), (1600, 1200));
        assert_eq!(output.dimensions(3000, 4000), (1200, 1600));
        assert_eq!(output.dimensions(400, 300), (400, 300));
        assert_eq!(output.dimensions(1, 50000), (1, 1600));
    }
    #[test]
    fn recipe_library_roundtrips_and_rejects_bad_versions() {
        let book = Book::default();
        assert_eq!(
            Book::parse(&serde_json::to_string(&book).unwrap())
                .unwrap()
                .recipes,
            book.recipes
        );
        assert!(Book::parse(r#"{"version":2,"recipes":[],"selected":0}"#).is_err());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn collisions_preserve_original_and_every_output() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("photo.png");
        std::fs::write(&original, b"original").unwrap();
        let a = write_copy(dir.path(), "photo", "png", b"one").unwrap();
        let b = write_copy(dir.path(), "photo", "png", b"two").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read(original).unwrap(), b"original");
        assert_eq!(std::fs::read(a).unwrap(), b"one");
        assert_eq!(std::fs::read(b).unwrap(), b"two");
    }
    #[test]
    fn scopes_clip_regions_and_reject_empty_selections() {
        let mut doc = Document::new("regions", 10, 10, schist_color::Depth::Eight);
        assert!(regions(&doc, Scope::Slices).is_err());
        doc.slices.push(schist_core::Slice {
            name: "partial".into(),
            rect: IntRect::new(-5, -5, 5, 5),
            user: true,
        });
        doc.slices.push(schist_core::Slice {
            name: "outside".into(),
            rect: IntRect::new(20, 20, 30, 30),
            user: true,
        });
        assert_eq!(
            regions(&doc, Scope::Slices).unwrap(),
            vec![("partial".into(), IntRect::new(0, 0, 5, 5))]
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn concurrent_exports_never_replace_each_others_files() {
        let dir = tempfile::tempdir().unwrap();
        let copies: Vec<_> = (0..8)
            .map(|index| {
                let path = dir.path().to_owned();
                std::thread::spawn(move || {
                    (index, write_copy(&path, "same", "png", &[index]).unwrap())
                })
            })
            .collect();
        for copy in copies {
            let (index, path) = copy.join().unwrap();
            assert_eq!(std::fs::read(path).unwrap(), [index]);
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 8);
    }
    #[test]
    fn deep_render_keeps_sub_eight_bit_samples_and_native_modes_drop_native_profiles() {
        let mut doc = Document::new("deep", 2, 1, schist_color::Depth::Sixteen);
        let mut layer = Layer::new_raster("pixels");
        let pixels = vec![
            1000.0 / 65535.0,
            2000.0 / 65535.0,
            3000.0 / 65535.0,
            1.0,
            1001.0 / 65535.0,
            2001.0 / 65535.0,
            3001.0 / 65535.0,
            1.0,
        ];
        schist_core::blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            doc.depth,
            doc.canvas_rect(),
            &pixels,
        );
        doc.push_layer(layer);
        let flat = render(&doc, doc.canvas_rect(), &Output::default());
        assert_eq!(flat.depth, schist_color::Depth::Sixteen);
        let bytes = schist_codecs_common::PngCodec
            .export_with(
                &flat,
                &schist_plugin_api::ExportOptions {
                    bit_depth: 16,
                    dither: false,
                    ..Default::default()
                },
            )
            .unwrap();
        let png = image::load_from_memory(&bytes).unwrap().to_rgba16();
        assert_eq!(png.get_pixel(0, 0).0, [1000, 2000, 3000, 65535]);
        assert_eq!(png.get_pixel(1, 0).0, [1001, 2001, 3001, 65535]);
        for mode in [schist_color::ColorMode::Cmyk, schist_color::ColorMode::Lab] {
            let mut native = Document::new("native", 1, 1, schist_color::Depth::Sixteen);
            native.mode = mode;
            native.icc_profile = Some(vec![1, 2, 3]);
            let flat = render(&native, native.canvas_rect(), &Output::default());
            assert_eq!(flat.mode, schist_color::ColorMode::Rgb);
            assert!(
                flat.icc_profile.is_none(),
                "native profiles must not label RGB export pixels"
            );
        }
    }
    #[test]
    fn real_codecs_export_regions_at_recipe_sizes_without_mutating_source() {
        let mut doc = Document::new("photo", 20, 10, schist_color::Depth::Eight);
        let mut layer = Layer::new_raster("pixels");
        let pixels = [255, 40, 10, 255].repeat(200);
        schist_core::blit_rgba8(
            &mut layer.as_raster_mut().unwrap().tiles,
            doc.depth,
            doc.canvas_rect(),
            &pixels,
        );
        doc.push_layer(layer);
        doc.artboards.push(schist_core::Artboard {
            name: "board".into(),
            rect: IntRect::new(5, 0, 15, 10),
        });
        let (_, rect) = regions(&doc, Scope::Artboards).unwrap().remove(0);
        let codecs: Vec<Box<dyn CodecPlugin>> = vec![
            Box::new(schist_codecs_common::PngCodec),
            Box::new(schist_codecs_common::JpegCodec),
            Box::new(schist_codecs_common::WebPCodec),
            Box::new(schist_codecs_common::TiffCodec),
        ];
        for codec in codecs {
            let output = Output {
                max_edge: 5,
                ..Default::default()
            };
            let result = render(&doc, rect, &output);
            let bytes = codec.export_with(&result, &Default::default()).unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (5, 5));
        }
        assert_eq!((doc.width, doc.height), (20, 10));
        assert_eq!(
            schist_compositor::composite_region_rgba8(&doc, doc.canvas_rect()),
            pixels
        );
    }
}
