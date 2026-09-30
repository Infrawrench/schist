//! The separation pass: a document's page, turned into plates.
//!
//! This is the driver that ties the stages together. It walks a page's
//! objects front to back, reduces each to coverage, decides which plates
//! that coverage lands on, and accumulates with the right knockout or
//! overprint rule. Then it applies the manager's ink decisions and
//! reports what a prepress provider would object to.
//!
//! The result is a set of [`PlateCoverage`] buffers, and optionally
//! [`schist_core::InkChannel`] plates for delivery as PSD/PSB or a
//! prepress file.

use schist_core::InkChannel;
use schist_layout::compose::compose_object;
use schist_layout::ink::InkManager;
use schist_layout::model::{LayoutDocument, LayoutObject, PlacedObject};

use crate::build::{CmykSource, NaiveBuild};
use crate::coverage::{InkMode, PlateCoverage, Separation};
use crate::geometry::{OutputSettings, PagePixel};
use crate::plan::PlatePlan;
use crate::raster::{coats_for, graphic_coverage, GraphicSource, NoGraphics};
use crate::report::PreflightReport;

/// A page separated into plates.
pub struct SeparatedPage {
    pub page: usize,
    pub plan: PlatePlan,
    pub separation: Separation,
    pub report: PreflightReport,
}

impl SeparatedPage {
    /// The plates as `schist-core` channels, ready for PSD/PSB delivery or
    /// a prepress writer.
    ///
    /// Each plate keeps its real ink definition where the plan has one,
    /// and is marked as a spot so the PSD writer emits a DisplayInfo
    /// entry rather than an ordinary alpha channel. Solidity is left at
    /// zero: it is a display property and never changes coverage.
    pub fn to_ink_channels(&self) -> Vec<InkChannel> {
        self.plan
            .plates
            .iter()
            .enumerate()
            .map(|(index, plate)| {
                let mut channel = InkChannel::spot(plate.name.clone(), plate.preview_rgb);
                // A process plate is not a spot. Saying otherwise would
                // make the PSD writer emit a DisplayInfo entry for cyan.
                channel.info.spot = plate.kind == crate::plan::PlateKind::Spot;
                if let Some(coverage) = self.separation.plate(index) {
                    copy_into(&mut channel, coverage);
                }
                channel
            })
            .collect()
    }

    /// The flat process composite, for a preview or a soft proof.
    pub fn composite(&self) -> &crate::coverage::CompositeCoverage {
        self.separation.composite()
    }
}

fn copy_into(channel: &mut InkChannel, coverage: &PlateCoverage) {
    for (i, value) in coverage.data.iter().enumerate() {
        let x = coverage.rect.left + (i as i32) % coverage.rect.width();
        let y = coverage.rect.top + (i as i32) / coverage.rect.width();
        if *value != 0.0 {
            channel.pixels.set(x, y, *value);
        }
    }
}

/// Separate one page of a document.
pub fn separate_page(
    doc: &LayoutDocument,
    page: usize,
    settings: OutputSettings,
    source: &dyn GraphicSource,
) -> Option<SeparatedPage> {
    separate_page_with(doc, page, settings, source, false)
}

/// Separate one page, with a caller-supplied CMYK source.
///
/// This is what an editor with ICC machinery should use: the layout
/// model's own conversion is preview-grade and, because it always zeroes
/// one channel, makes under-colour removal a silent no-op.
pub fn separate_page_built(
    doc: &LayoutDocument,
    page: usize,
    settings: OutputSettings,
    source: &dyn GraphicSource,
    builds: &dyn CmykSource,
) -> Option<SeparatedPage> {
    let plan = PlatePlan::with_build(&doc.all_inks(), &doc.ink_manager, builds);
    let page_def = doc.pages.get(page)?;
    let frame = PagePixel::rect(settings, page_def, settings.output_box(page_def));
    let mut separation = Separation::new(plan.plates.len(), frame);
    let mut report = layout_report(doc, page, source, &plan);
    let mut working = plan.clone();
    for placed in doc.page_objects(page) {
        if let Some(link) = missing_link(&placed) {
            report.missing_link(&link);
            continue;
        }
        if let Some(path) = paint_object(
            &mut separation,
            &mut working,
            doc,
            &placed,
            settings,
            page_def,
            source,
        ) {
            report.unavailable_graphic(path);
        }
    }
    finish(separation, &plan, &doc.ink_manager, false, report)
}

/// Separate one page, optionally baking the ink limit into the plates.
///
/// `enforce_ink_limit` is off by default and should stay that way
/// unless the user has asked for the artwork to be changed: clamping
/// coverage alters what they designed, and a preflight report that
/// silently fixed the page would hide the problem it exists to report.
pub fn separate_page_with(
    doc: &LayoutDocument,
    page: usize,
    settings: OutputSettings,
    source: &dyn GraphicSource,
    enforce_ink_limit: bool,
) -> Option<SeparatedPage> {
    let page_def = doc.pages.get(page)?;
    let plan = PlatePlan::with_build(&doc.all_inks(), &doc.ink_manager, &NaiveBuild);
    let box_rect = settings.output_box(page_def);
    let frame = PagePixel::rect(settings, page_def, box_rect);
    let mut separation = Separation::new(plan.plates.len(), frame);
    let mut report = layout_report(doc, page, source, &plan);
    let mut working = plan.clone();

    for placed in doc.page_objects(page) {
        if let Some(link) = missing_link(&placed) {
            report.missing_link(&link);
            continue;
        }
        if let Some(path) = paint_object(
            &mut separation,
            &mut working,
            doc,
            &placed,
            settings,
            page_def,
            source,
        ) {
            report.unavailable_graphic(path);
        }
    }

    finish(
        separation,
        &plan,
        &doc.ink_manager,
        enforce_ink_limit,
        report,
    )
}

/// Structural failures remain visible even when the rendered portion fits
/// within the ink limit. Overset text must not silently vanish on export.
fn layout_report(
    doc: &LayoutDocument,
    page: usize,
    source: &dyn GraphicSource,
    plan: &PlatePlan,
) -> PreflightReport {
    let mut report = PreflightReport::new(page);
    let mut plate_names = std::collections::BTreeSet::new();
    for plate in &plan.plates {
        if !plate_names.insert(&plate.name) || matches!(plate.name.as_str(), "All" | "None") {
            report.add(
                crate::report::Severity::Error,
                schist_i18n::tf!("design.preflight_ink_conflict", name = plate.name),
            );
        }
    }
    let mut families = std::collections::BTreeSet::new();
    for object in doc.page_objects(page) {
        if !schist_layout::affine::finite(object.content_transform())
            || object.content_transform().invert().is_none()
        {
            report.add(
                crate::report::Severity::Error,
                schist_i18n::tf!("design.preflight_transform", name = object.name),
            );
        }
        if let LayoutObject::TextFrame { story, .. } = &object.object {
            let Some(frame) = compose_object(doc, &object) else {
                continue;
            };
            if frame.lost {
                report.add(
                    crate::report::Severity::Error,
                    schist_i18n::tf!("design.preflight_overset", name = object.name),
                );
            }
            let Some(story) = doc.stories.get(story.0 as usize) else {
                continue;
            };
            for line in frame.lines {
                let spec = schist_layout::compose::spec_for(
                    story,
                    line.start,
                    line.end,
                    &doc.styles,
                    &line.paragraph_style,
                    &doc.default_character_style,
                    line.bounds.width,
                );
                for byte in std::iter::once(0)
                    .chain(spec.runs.iter().flat_map(|r| [r.start, r.end]))
                    .filter(|i| *i < spec.text.len())
                {
                    families.insert(spec.style_at(byte).family);
                }
            }
        }
        if let LayoutObject::GraphicFrame {
            link,
            fit,
            crop,
            scale,
            image_transform,
            ..
        } = &object.object
        {
            let inner = schist_layout::graphics::image_affine(object.bounds, *image_transform);
            let Some(inner) = inner.filter(|m| schist_layout::affine::inverse(*m).is_some()) else {
                report.add(
                    crate::report::Severity::Error,
                    schist_i18n::tf!("design.preflight_transform", name = object.name),
                );
                continue;
            };
            if let Some(info) = source.info(link) {
                if let Some(rect) = schist_layout::graphics::image_rect(
                    object.bounds,
                    (info.width, info.height),
                    info.dpi,
                    *crop,
                    *fit,
                    *scale,
                ) {
                    let Some(dpi) = schist_layout::affine::effective_dpi(
                        object.content_transform().then(&inner),
                        rect,
                        (info.width, info.height),
                    ) else {
                        continue;
                    };
                    if dpi < 150.0 {
                        report.add(
                            crate::report::Severity::Warning,
                            schist_i18n::tf!(
                                "design.preflight_low_resolution",
                                name = object.name,
                                dpi = dpi.round()
                            ),
                        );
                    }
                }
            }
        }
    }
    for family in families {
        if !family.is_empty()
            && !matches!(
                family.as_str(),
                "sans-serif" | "serif" | "monospace" | "system-ui"
            )
            && !schist_text_engine::has_family(&family)
        {
            report.add(
                crate::report::Severity::Error,
                schist_i18n::tf!("design.preflight_missing_font", name = family),
            );
        }
    }
    report
}

/// Apply the manager's decisions and produce the report.
fn finish(
    mut separation: Separation,
    plan: &PlatePlan,
    manager: &InkManager,
    enforce_ink_limit: bool,
    report: PreflightReport,
) -> Option<SeparatedPage> {
    // Preflight measures the page as the designer wrote it. Enforcing
    // the ink limit here would quietly rewrite their artwork, and then
    // report the page as within the limit -- hiding the very thing
    // preflight exists to find.
    let effective = manager.separating(
        manager.ucr,
        manager.black_generation,
        if enforce_ink_limit {
            manager.total_area_limit
        } else {
            None
        },
    );
    apply_ink_manager(&mut separation, plan, &effective);
    let report = preflight(&separation, plan, manager, report);

    Some(SeparatedPage {
        page: report.page,
        plan: plan.clone(),
        separation,
        report,
    })
}

/// Separate a page with no graphic sources. Missing links are errors;
/// other graphics are reported as unavailable, including embedded ones.
///
/// Preflight over a document whose linked files have not been fetched
/// yet is the honest thing to do, and it is what a caller that has no
/// decoder gets for free.
pub fn separate_page_without_graphics(
    doc: &LayoutDocument,
    page: usize,
    settings: OutputSettings,
) -> Option<SeparatedPage> {
    separate_page(doc, page, settings, &NoGraphics)
}

/// The path of a placed object whose link is known to be absent.
fn missing_link(placed: &PlacedObject) -> Option<String> {
    let LayoutObject::GraphicFrame { link, embedded, .. } = &placed.object else {
        return None;
    };
    if *embedded || link.present {
        return None;
    }
    Some(link.path.clone())
}

/// Paint one object onto the plates, returning an unresolved graphic's
/// path so neither separation entry point can silently omit its error.
fn paint_object<'a>(
    separation: &mut Separation,
    plan: &mut PlatePlan,
    doc: &LayoutDocument,
    placed: &'a PlacedObject,
    settings: OutputSettings,
    page: &schist_layout::Page,
    source: &dyn GraphicSource,
) -> Option<&'a str> {
    if !schist_layout::affine::finite(placed.content_transform())
        || placed.content_transform().invert().is_none()
    {
        return None;
    }
    let mode = if placed.overprint {
        InkMode::Overprint
    } else {
        InkMode::Knockout
    };
    let opacity = placed.transparency.clamp(0.0, 1.0);

    match &placed.object {
        LayoutObject::Shape {
            fill,
            stroke,
            fill_overprint,
            stroke_overprint,
            ..
        } => {
            let coverage = crate::raster::placed_shape_coverage(placed, settings, page, false);
            if let Some(ink) = fill {
                let (coats, build) = coats_for(plan, ink);
                let mode = if *fill_overprint {
                    InkMode::Overprint
                } else {
                    mode
                };
                separation.paint(&coverage, &coats, mode, opacity);
                separation.paint_composite(&coverage, &coats, &build, mode, opacity);
            }
            if let Some(ink) = stroke {
                // The stroke is a separate ink on a separate rule: a
                // shape whose fill knocks out and whose stroke
                // overprints is an ordinary way to draw a keyline.
                let stroke_mask =
                    crate::raster::placed_shape_coverage(placed, settings, page, true);
                let (coats, build) = coats_for(plan, ink);
                let mode = if *stroke_overprint {
                    InkMode::Overprint
                } else {
                    mode
                };
                separation.paint(&stroke_mask, &coats, mode, opacity);
                separation.paint_composite(&stroke_mask, &coats, &build, mode, opacity);
            }
        }

        LayoutObject::TextFrame { story, .. } => {
            let composed = compose_object(doc, placed)?;
            let story_def = doc.story(*story)?;
            for line in &composed.lines {
                for (coverage, ink, alpha, overprint) in
                    crate::raster::line_paints(line, story_def, doc, settings, page)
                {
                    let coverage = crate::raster::warp_coverage(coverage, placed, settings, page);
                    let mode = if overprint { InkMode::Overprint } else { mode };
                    let (coats, build) = coats_for(plan, &ink);
                    separation.paint(&coverage, &coats, mode, opacity * alpha);
                    separation.paint_composite(&coverage, &coats, &build, mode, opacity * alpha);
                }
            }
        }

        LayoutObject::GraphicFrame { link, .. } => {
            // An empty destination contributes no ink and never asks the
            // source for pixels; it is not a failed source lookup.
            if PagePixel::rect(settings, page, placed.bounds).is_empty() {
                return None;
            }
            let Some(placed_graphic) = graphic_coverage(placed, settings, page, source) else {
                return Some(&link.path);
            };
            let rect = placed_graphic.rect;
            if !separation.paint_process(
                rect,
                &placed_graphic.coverage,
                &placed_graphic.cmyk,
                plan.process,
                mode,
                opacity,
            ) {
                return Some(&link.path);
            }
        }

        // A group is a container, and a note is a review annotation.
        // Neither puts ink on a plate.
        LayoutObject::Group { .. } | LayoutObject::Note { .. } => {}
    }
    None
}

/// Apply the ink manager's output-time decisions to the process plates.
pub fn apply_ink_manager(separation: &mut Separation, plan: &PlatePlan, manager: &InkManager) {
    if manager.is_identity() {
        return;
    }
    let rect = separation.rect();
    let plates = separation.plates_mut();
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            let mut cmyk = [0.0f32; 4];
            for (i, plate) in plan.process.iter().enumerate() {
                cmyk[i] = plates
                    .get(*plate)
                    .map(|p| p.at(x, y))
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
            }
            let separated = manager.separate_cmyk(cmyk);
            for (i, plate) in plan.process.iter().enumerate() {
                if let Some(p) = plates.get_mut(*plate) {
                    p.set(x, y, separated[i]);
                }
            }
        }
    }
}

/// Everything a prepress provider would want to know about the page.
fn preflight(
    separation: &Separation,
    plan: &PlatePlan,
    manager: &InkManager,
    mut report: PreflightReport,
) -> PreflightReport {
    let rect = separation.rect();
    let process = plan.process;
    let mut worst = 0.0f32;
    let mut over = 0usize;
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            let tac = separation.total_area_at(process, x, y);
            worst = worst.max(tac);
            if let Some(limit) = manager.total_area_limit {
                if tac > limit + 0.001 {
                    over += 1;
                }
            }
        }
    }
    report.note_total_area(worst, manager.total_area_limit, over);
    report
}
