//! Output uses an immutable layout/graphics snapshot and a bounded sheet at
//! a time. The raster editor document and layout undo stack are untouched.
use super::graphics::Graphics;
use schist_i18n::{t, tf};
use schist_layout::LayoutDocument;
use schist_separation::{
    pdf::{Imposition, Marks, PageOutput, Pdf},
    GraphicSource, OutputSettings,
};
use std::{path::Path, sync::Arc};

#[derive(Clone)]
pub struct Options {
    pub dpi: u16,
    pub up: u8,
    pub marks: bool,
    pub hidden: bool,
    pub profile: Option<(String, Arc<Vec<u8>>)>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            dpi: 300,
            up: 1,
            marks: true,
            hidden: false,
            profile: None,
        }
    }
}

struct ManagedGraphics<'a> {
    graphics: &'a Graphics,
    transform: Option<&'a schist_colormgmt::NativeColorTransform>,
}

struct OutputBuilds(Vec<(schist_layout::Ink, [f32; 4])>);
impl schist_separation::CmykSource for OutputBuilds {
    fn build(&self, ink: &schist_layout::Ink) -> [f32; 4] {
        self.0
            .iter()
            .find(|(i, _)| i == ink)
            .map(|(_, build)| *build)
            .unwrap_or_else(|| ink.to_cmyk())
    }
}
impl GraphicSource for ManagedGraphics<'_> {
    fn info(&self, link: &schist_layout::Link) -> Option<schist_layout::GraphicInfo> {
        self.graphics.info(link)
    }
    fn sample(
        &self,
        link: &schist_layout::Link,
        placement: &schist_separation::GraphicPlacement,
    ) -> Option<schist_separation::PlacedGraphic> {
        self.graphics
            .sample_managed(link, placement, self.transform)
    }
}

pub struct PdfExport {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
fn pdf(
    document: &LayoutDocument,
    graphics: &Graphics,
    options: &Options,
) -> anyhow::Result<Vec<u8>> {
    pdf_report(document, graphics, options).map(|output| output.bytes)
}

pub fn pdf_report(
    document: &LayoutDocument,
    graphics: &Graphics,
    options: &Options,
) -> anyhow::Result<PdfExport> {
    anyhow::ensure!(
        matches!(options.up, 1 | 2 | 4) && (72..=600).contains(&options.dpi),
        t("design.output_invalid")
    );
    let transform = options
        .profile
        .as_ref()
        .map(|(_, bytes)| {
            schist_colormgmt::NativeColorTransform::new(schist_color::ColorMode::Cmyk, Some(bytes))
        })
        .transpose()?;
    let source = ManagedGraphics {
        graphics,
        transform: transform.as_ref(),
    };
    let mut builds = OutputBuilds(Vec::new());
    for ink in document.all_inks() {
        let build = if let Some(cmyk) = ink.source_cmyk {
            cmyk
        } else if let Some(transform) = &transform {
            transform.rgb_to_cmyk_checked(&[ink.preview_rgb])?[0]
        } else {
            ink.to_cmyk()
        };
        builds.0.push((ink, build));
    }
    let mut document = document.clone();
    for object in document.objects.iter_mut().chain(
        document
            .parents
            .iter_mut()
            .flat_map(|p| p.objects.iter_mut().map(|o| &mut o.object)),
    ) {
        if let schist_layout::LayoutObject::GraphicFrame {
            link,
            embedded: false,
            ..
        } = &mut object.object
        {
            link.present = graphics.get(&link.path).is_some();
        }
    }
    let settings = OutputSettings::at(f32::from(options.dpi));
    let imposition = Imposition {
        up: options.up,
        marks: options.marks,
    };
    let indices: Vec<_> = document
        .pages
        .iter()
        .enumerate()
        .filter(|(_, p)| options.hidden || !p.hidden)
        .map(|(i, _)| i)
        .collect();
    anyhow::ensure!(!indices.is_empty(), t("design.output_no_pages"));
    let mut pdf = Pdf::new();
    if let Some((name, bytes)) = &options.profile {
        pdf.set_output_profile(bytes, name)?;
    }
    let mut pages = Vec::new();
    let mut warnings = Vec::new();
    let channels = schist_separation::PlatePlan::with_build(
        &document.all_inks(),
        &document.ink_manager,
        &builds,
    )
    .plates
    .len()
        + 4;
    for chunk in indices.chunks(options.up as usize) {
        let mut separated = Vec::new();
        for index in chunk {
            let p = &document.pages[*index];
            let width = f64::from(p.width + 2.0 * p.bleed) * f64::from(settings.scale());
            let height = f64::from(p.height + 2.0 * p.bleed) * f64::from(settings.scale());
            anyhow::ensure!(
                width.is_finite()
                    && height.is_finite()
                    && width > 0.0
                    && height > 0.0
                    && width * height * channels as f64 * 4.0 <= 512.0 * 1024.0 * 1024.0,
                t("design.output_too_large")
            );
            let page = schist_separation::separate_page_built(
                &document, *index, settings, &source, &builds,
            )
            .ok_or_else(|| anyhow::anyhow!(t("design.output_invalid")))?;
            if !page.report.is_printable() {
                let errors = page
                    .report
                    .findings
                    .iter()
                    .filter(|f| f.severity == schist_separation::Severity::Error)
                    .map(|f| f.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                anyhow::bail!(
                    "{}",
                    tf!(
                        "design.output_preflight",
                        page = document.page_number(*index),
                        errors = errors
                    )
                );
            }
            warnings.extend(
                page.report
                    .findings
                    .iter()
                    .filter(|f| f.severity == schist_separation::Severity::Warning)
                    .map(|f| {
                        tf!(
                            "design.output_page_warning",
                            page = document.page_number(*index),
                            message = f.message
                        )
                    }),
            );
            separated.push(page);
        }
        let outputs: Vec<_> = separated
            .iter()
            .zip(chunk)
            .map(|(page, i)| PageOutput {
                separated: page,
                trim: (document.pages[*i].width, document.pages[*i].height),
                bleed: document.pages[*i].bleed,
                settings,
                imposition: Imposition {
                    up: 1,
                    marks: options.marks,
                },
                marks: Marks::default(),
                overprint: true,
            })
            .collect();
        pages.push(schist_separation::pdf::write_sheet(
            &mut pdf, &outputs, imposition,
        )?);
    }
    Ok(PdfExport {
        bytes: pdf.finish(&pages),
        warnings,
    })
}

pub fn package(
    document: &LayoutDocument,
    path: Option<&Path>,
) -> anyhow::Result<schist_codec_idml::package::Packaged> {
    schist_codec_idml::package::build(document, |link| {
        let source = super::graphics::resolve_path(link, path)
            .ok_or_else(|| tf!("design.preflight_missing_link", path = link))?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::read(source).map_err(|e| tf!("design.output_failed", error = e))
        }
        #[cfg(target_arch = "wasm32")]
        {
            crate::web::read_file(&source)
                .map(|bytes| bytes.as_ref().clone())
                .map_err(|e| tf!("design.output_failed", error = e))
        }
    })
    .map_err(anyhow::Error::msg)
}

/// Unique sibling temporary files avoid colliding with the user's files or
/// another export. A failed write/rename cleans up only its own temporary.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let parent = path.parent().unwrap_or(Path::new("."));
    let temporary = parent.join(format!(
        ".schist-output-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_selection_and_sheet_counts_do_not_mutate_the_document() {
        let mut document = schist_layout::blank_a4();
        for _ in 0..4 {
            document.add_page(schist_layout::Page::new("", 40.0, 30.0));
        }
        document.pages[0] = schist_layout::Page::new("1", 40.0, 30.0);
        document.pages[2].hidden = true;
        let before = document.clone();
        for up in [1, 2, 4] {
            for hidden in [false, true] {
                let options = Options {
                    dpi: 72,
                    up,
                    hidden,
                    ..Options::default()
                };
                let bytes = pdf(&document, &Graphics::default(), &options).unwrap();
                let text = String::from_utf8_lossy(&bytes);
                let count = if hidden { 5usize } else { 4 };
                assert!(text.contains(&format!("/Count {} ", count.div_ceil(up as usize))));
                assert_eq!(document, before);
            }
        }
    }
    #[test]
    fn invalid_geometry_or_unavailable_artwork_cannot_produce_a_partial_pdf() {
        let mut doc = schist_layout::blank_a4();
        let options = Options {
            dpi: 72,
            ..Default::default()
        };
        doc.pages[0].width = f32::INFINITY;
        assert!(pdf(&doc, &Graphics::default(), &options).is_err());
        doc.pages[0].width = 200.0;
        schist_layout::authoring::graphic_frame(
            &mut doc,
            &mut Default::default(),
            0,
            schist_layout::Rect::new(0.0, 0.0, 20.0, 20.0),
            "missing.png",
            false,
        )
        .unwrap();
        assert!(pdf(&doc, &Graphics::default(), &options).is_err());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn atomic_output_does_not_clobber_existing_temporary_files_and_cleans_failed_renames() {
        let directory =
            std::env::temp_dir().join(format!("schist-output-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("proof.pdf");
        let unrelated = directory.join("proof.schist-tmp");
        std::fs::write(&unrelated, b"keep me").unwrap();
        for bytes in [b"first".as_slice(), b"second"] {
            write_atomic(&path, bytes).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep me");
        assert!(write_atomic(&directory, b"invalid target").is_err());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn a_system_cmyk_profile_converts_output_and_is_embedded_without_changing_authored_inks() {
        // Read a public ICC profile, never a proprietary program/header.
        let path = "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc";
        let bytes = std::fs::read(path).expect("macOS generic CMYK profile");
        let transform = schist_colormgmt::NativeColorTransform::new(
            schist_color::ColorMode::Cmyk,
            Some(&bytes),
        )
        .unwrap();
        let colors = [
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.2, 0.4, 0.7],
        ];
        let builds = transform.rgb_to_cmyk_checked(&colors).unwrap();
        assert_eq!(builds.len(), colors.len());
        assert!(builds
            .iter()
            .flatten()
            .all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0));
        assert!(builds[0].iter().sum::<f32>() < 0.05);
        assert!(builds[1].iter().sum::<f32>() > 0.9);
        assert!(transform
            .rgb_to_cmyk_checked(&[[f32::NAN, 0.0, 0.0]])
            .is_err());
        let mut doc = schist_layout::blank_a4();
        doc.pages[0] = schist_layout::Page::new("1", 40.0, 30.0);
        let before = doc.clone();
        let output = pdf(
            &doc,
            &Graphics::default(),
            &Options {
                dpi: 72,
                profile: Some(("Generic CMYK".into(), Arc::new(bytes))),
                ..Default::default()
            },
        )
        .unwrap();
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("/OutputIntents ["));
        assert!(text.contains("/DestOutputProfile"));
        assert_eq!(doc, before);
    }
}
