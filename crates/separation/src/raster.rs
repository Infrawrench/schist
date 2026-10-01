//! Stage 2: turning layout objects into coverage masks.
//!
//! Each kind of object on a page reduces to the same question -- how much
//! of this pixel does it cover -- and then to a list of inks saying how
//! much of each plate that coverage carries. Shapes come from the vector
//! rasteriser, text from the text engine, and placed graphics from
//! whatever the caller has loaded for them.
//!
//! Placed graphics are the one kind this crate cannot resolve on its own:
//! a `Link` is a path, and reading it is the editor's job. That is the
//! [`GraphicSource`] trait. A caller with nothing to offer can use
//! [`NoGraphics`]. The separation pass reports its unresolved graphics
//! as unavailable, so a partial page cannot pass preflight.

use schist_core::IntRect;
use schist_layout::compose::{line_spec, ComposedLine};
use schist_layout::geometry::Rect;
use schist_layout::ink::Ink;
use schist_layout::model::{GraphicFit, LayoutObject, Link, PlacedObject};
use schist_layout::LayoutDocument;

use crate::coverage::{Coat, Coverage, InkMode};
use crate::geometry::{OutputSettings, PagePixel};
use crate::plan::PlatePlan;

/// A placed graphic, resolved to pixels.
pub trait GraphicSource {
    /// Loaded source metadata takes precedence over stale IDML link metadata.
    fn info(&self, link: &Link) -> Option<schist_layout::GraphicInfo> {
        link.info
    }
    /// The coverage of a graphic placed at `dest` in output pixels.
    ///
    /// Crop uses normalized source coordinates; dpi keeps Original fitting
    /// independent of the selected output resolution. Apply the inner image
    /// transform before clipping to the destination frame.
    fn sample(&self, link: &Link, placement: &GraphicPlacement) -> Option<PlacedGraphic>;
}

#[derive(Debug, Clone, Copy)]
pub struct GraphicPlacement {
    pub dest: IntRect,
    pub crop: Option<Rect>,
    pub fit: GraphicFit,
    pub scale: f32,
    pub dpi: f32,
    /// Normalized image transform applied after fitting, before frame clipping.
    pub image_transform: schist_core::Affine,
}

/// A placed graphic's pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedGraphic {
    pub rect: IntRect,
    /// Coverage bytes over `rect`, row-major.
    pub coverage: Vec<u8>,
    /// The process build per pixel, for the composite. A colour image
    /// separated by the caller; a solid fill is one value repeated.
    pub cmyk: Vec<[f32; 4]>,
}

impl PlacedGraphic {
    /// A solid rectangle of one process colour, which is what a frame
    /// with a solid placeholder behind it amounts to.
    pub fn solid(rect: IntRect, cmyk: [f32; 4]) -> PlacedGraphic {
        let area = rect.width().max(0) as usize * rect.height().max(0) as usize;
        PlacedGraphic {
            rect,
            coverage: vec![255; area],
            cmyk: vec![cmyk; area],
        }
    }

    pub fn into_coverage(self) -> Coverage {
        Coverage {
            rect: self.rect,
            data: self.coverage,
        }
    }
}

/// A source with nothing to offer, so every graphic reads as unavailable.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoGraphics;

impl GraphicSource for NoGraphics {
    fn sample(&self, _link: &Link, _placement: &GraphicPlacement) -> Option<PlacedGraphic> {
        None
    }
}

/// One object's appearance: the mask, and what it puts on each plate.
pub struct Appearance {
    pub coverage: Coverage,
    /// Plate index and the weight that coverage carries.
    pub coats: Vec<Coat>,
    /// Process build of each coat, same order, for the composite.
    pub build: Vec<[f32; 4]>,
    pub mode: InkMode,
    pub opacity: f32,
}

impl Appearance {
    pub fn is_empty(&self) -> bool {
        self.coverage.is_empty() || self.coats.is_empty()
    }
}

/// The coats and process build for one ink, against a plan.
///
/// A separated ink lays one plate at full strength. A process ink lays
/// each channel at the strength of that channel, which is the whole
/// point of storing a swatch in CMYK: a colour whose build is
/// 100/28/100/6 lays 100% cyan, 28% magenta, 100% yellow and 6% black,
/// and not four solid plates. Weighting them all equally is how a
/// process mix turns into a flood of ink.
pub fn coats_for(plan: &mut PlatePlan, ink: &Ink) -> (Vec<Coat>, Vec<[f32; 4]>) {
    let mut entries = plan.coats_for(ink);
    if entries.is_empty() && ink.spot && plan.is_separated(ink) {
        // The plan had not seen this spot, so it needs a plate before it
        // can be laid down. Adding it here is what makes a swatch
        // dragged in mid-document actually print.
        plan.add_spot(ink);
        entries = plan.coats_for(ink);
    }
    let coats = entries
        .iter()
        .filter(|(_, weight)| *weight > 0.0)
        .map(|(plate, weight)| Coat {
            plate: *plate,
            weight: weight.clamp(0.0, 1.0),
        })
        .collect::<Vec<_>>();
    // Each process coat contributes only its own channel. Repeating the
    // entire build per coat multiplies mixed colours by their ink sum.
    let build = coats
        .iter()
        .map(|coat| {
            if let Some(channel) = plan.process.iter().position(|p| *p == coat.plate) {
                let mut build = [0.0; 4];
                build[channel] = 1.0;
                build
            } else {
                Ink::process("", plan.plates[coat.plate].preview_rgb).to_cmyk()
            }
        })
        .collect();
    (coats, build)
}

/// Tint scales resolved ink amounts, never the shape's knockout coverage.
/// Alias resolution, ICC conversion and black generation precede tinting.
pub fn tinted_coats_for(plan: &mut PlatePlan, ink: &Ink, tint: f32) -> (Vec<Coat>, Vec<[f32; 4]>) {
    let (mut coats, build) = coats_for(plan, ink);
    let tint = if ink.tint.is_some() {
        1.0
    } else {
        schist_layout::ink::bounded_tint(tint)
    };
    for coat in &mut coats {
        coat.weight *= tint;
    }
    (coats, build)
}

/// The coverage of a shape: its fill, then its stroke.
///
/// `shape`'s points are **relative to `bounds`' origin**, the same
/// convention as the pasteboard. Resizing updates the path. Coordinates are in points
/// and are scaled into the page's pixel grid here, because the
/// rasteriser works in pixels and a path left in points would come out
/// three-and-a-half times too small at 300 dpi.
pub fn shape_coverage(
    shape: &schist_layout::ShapePath,
    bounds: Rect,
    settings: OutputSettings,
    page: &schist_layout::Page,
    stroke_width: f32,
) -> Coverage {
    shape_mask(
        shape,
        (bounds, schist_core::Affine::IDENTITY),
        settings,
        page,
        stroke_width,
        true,
    )
}

/// Stroke coverage alone, including the half-width outside tight curve
/// bounds. An open curve has caps and never acquires an implicit fill.
pub fn stroke_coverage(
    shape: &schist_layout::ShapePath,
    bounds: Rect,
    settings: OutputSettings,
    page: &schist_layout::Page,
    width: f32,
) -> Coverage {
    shape_mask(
        shape,
        (bounds, schist_core::Affine::IDENTITY),
        settings,
        page,
        width,
        false,
    )
}

/// Stroke in the original frame, then transform its outline, so nonuniform
/// scale and shear affect both the path and the physical stroke consistently.
pub fn placed_shape_coverage(
    placed: &PlacedObject,
    settings: OutputSettings,
    page: &schist_layout::Page,
    stroke: bool,
) -> Coverage {
    let LayoutObject::Shape {
        path, stroke_width, ..
    } = &placed.object
    else {
        return Coverage::new(IntRect::EMPTY);
    };
    shape_mask(
        path,
        (placed.bounds, placed.content_transform()),
        settings,
        page,
        if stroke { *stroke_width } else { 0.0 },
        !stroke,
    )
}

fn shape_mask(
    shape: &schist_layout::ShapePath,
    placement: (Rect, schist_core::Affine),
    settings: OutputSettings,
    page: &schist_layout::Page,
    width: f32,
    fill: bool,
) -> Coverage {
    let (bounds, transform) = placement;
    let origin = PagePixel::of(settings, page, bounds.origin());
    let mut path = to_pixels(
        shape.flatten(
            0.25 / (settings.scale() * schist_layout::affine::stretch(transform).max(1.0)),
        ),
        origin.x,
        origin.y,
        settings.scale(),
    );
    let mut stroked = (width > 0.0).then(|| {
        schist_vector::stroke_path(
            &path,
            schist_vector::StrokeStyle {
                width: width * settings.scale(),
                ..Default::default()
            },
        )
    });
    let matrix =
        schist_layout::affine::in_view(transform, settings.scale(), schist_layout::Point::ZERO);
    let map = |path: &mut schist_vector::Path| {
        for sub in &mut path.subpaths {
            for point in sub {
                *point = matrix.apply(point.0, point.1);
            }
        }
    };
    map(&mut path);
    if let Some(stroke) = &mut stroked {
        map(stroke);
    }
    let page_box = settings.to_pixels_rect(settings.output_box(page));
    let rect = stroked
        .as_ref()
        .map(|stroke| stroke.bounds().union(&path.bounds()))
        .unwrap_or_else(|| path.bounds())
        .intersect(&page_box);
    if rect.is_empty() {
        return Coverage::new(IntRect::EMPTY);
    }
    let rule = if shape.even_odd {
        schist_vector::FillRule::EvenOdd
    } else {
        schist_vector::FillRule::NonZero
    };
    // Only explicitly closed contours have a fill in the layout model.
    let mut closed = schist_vector::Path::default();
    for (i, points) in path.subpaths.iter().enumerate() {
        if path.is_closed(i) {
            closed.push_closed(points.clone());
        }
    }
    let mut coverage = Coverage {
        rect,
        data: if fill {
            schist_vector::rasterize(&closed, rect, rule)
        } else {
            vec![0; rect.width() as usize * rect.height() as usize]
        },
    };
    if let Some(stroke) = stroked {
        let mask = schist_vector::rasterize(&stroke, rect, schist_vector::FillRule::NonZero);
        for (a, b) in coverage.data.iter_mut().zip(mask) {
            *a = (*a).max(b);
        }
    }
    coverage
}

/// The coverage of one composed line of text.
///
/// The story is passed in rather than looked up: a document can compose
/// several stories at once, and a line does not carry enough information
/// to say which one it came from.
pub fn line_coverage(
    line: &ComposedLine,
    story: &schist_layout::Story,
    doc: &LayoutDocument,
    settings: OutputSettings,
    page: &schist_layout::Page,
) -> Option<Coverage> {
    if line.forced_break {
        return None;
    }
    // A break line stands for a point with no glyphs, and an empty line
    // is not worth rasterising.
    if line.end <= line.start && line.generated.is_none() {
        return None;
    }
    let spec = line_spec(line, story, doc);
    let align = spec.align;
    let writing = spec.writing_mode;
    let raster = schist_text_engine::rasterize(&scale_spec(spec, settings.scale()))?;
    if raster.bounds.is_empty() {
        return None;
    }
    let frame = PagePixel::rect(settings, page, line.bounds);
    let offset = schist_layout::compose::aligned_origin(
        Rect::new(
            0.0,
            0.0,
            line.bounds.width * settings.scale(),
            line.bounds.height * settings.scale(),
        ),
        raster.layout_width,
        align,
        writing,
    );
    let dx = offset.x.round() as i32;
    let dy = offset.y.round() as i32;
    let rect = IntRect::new(
        frame.left + dx + raster.bounds.left,
        frame.top + dy + raster.bounds.top,
        frame.left + dx + raster.bounds.right,
        frame.top + dy + raster.bounds.bottom,
    );
    Some(Coverage {
        rect,
        data: raster.coverage,
    })
}

/// One rasterized text paint, with tint distinct from coverage and opacity.
pub struct TextPaint {
    pub coverage: Coverage,
    pub ink: Ink,
    pub opacity: f32,
    pub overprint: bool,
    pub tint: f32,
}

/// A composed line split into its actual ink paints without reshaping each
/// substring. Ink IDs travel through the text rasterizer as opaque colors,
/// preserving spot identity even when two inks have the same RGB preview.
pub fn line_paints(
    line: &ComposedLine,
    story: &schist_layout::Story,
    doc: &LayoutDocument,
    settings: OutputSettings,
    page: &schist_layout::Page,
) -> Vec<TextPaint> {
    if line.forced_break || (line.end <= line.start && line.generated.is_none()) {
        return Vec::new();
    }
    let base = line
        .paragraph
        .character(doc.styles.resolve_character(&doc.default_character_style));
    let mut inks = Vec::new();
    let mut spec = line_spec(line, story, doc);
    let ranges: Vec<_> = story
        .ranges
        .iter()
        .filter(|r| r.start < line.end && r.end > line.start)
        .collect();
    // Composition appends its paragraph fallback after the local style ranges.
    for (index, run) in spec.runs.iter_mut().enumerate() {
        let style = line
            .generated
            .as_ref()
            .map(|g| g.character.clone())
            .unwrap_or_else(|| {
                ranges.get(index).map_or_else(
                    || base.clone(),
                    |range| {
                        doc.styles
                            .resolve_character(&range.style)
                            .with_paint_defaults(&base)
                    },
                )
            });
        let mut paint_id = |ink: Ink, overprint: bool, tint: f32| {
            let paint = (ink, style.opacity.unwrap_or(1.0), overprint, tint);
            let index = inks
                .iter()
                .position(|existing| existing == &paint)
                .unwrap_or_else(|| {
                    inks.push(paint);
                    inks.len() - 1
                });
            Some((index as u32).to_le_bytes())
        };
        run.color = paint_id(
            style.fill.clone().unwrap_or_else(Ink::black),
            style.overprint_fill.unwrap_or(false),
            style.fill_tint.unwrap_or(1.0),
        );
        if let Some(stroke) = &mut run.stroke {
            stroke.color = paint_id(
                style.stroke.clone().unwrap_or_else(Ink::black),
                style.overprint_stroke.unwrap_or(false),
                style.stroke_tint.unwrap_or(1.0),
            );
        }
        for (rendered, definition) in [
            (&mut run.underline_style, &style.underline_style),
            (&mut run.strike_style, &style.strike_style),
        ] {
            if let Some(rendered) = rendered {
                if let Some((ink, tint, overprint)) = definition.paint(&style) {
                    rendered.color = paint_id(ink, overprint, tint);
                }
                if let Some((ink, tint, overprint)) = definition.gap_paint(&style) {
                    rendered.gap_color = paint_id(ink, overprint, tint);
                }
            }
        }
    }
    spec.word_spacing = line.word_space.unwrap_or(0.0);
    spec.wrap_width = None;
    let align = spec.align;
    let writing = spec.writing_mode;
    let Some(raster) =
        schist_text_engine::rasterize_with_paints(&scale_spec(spec, settings.scale()))
    else {
        return Vec::new();
    };
    let frame = PagePixel::rect(settings, page, line.bounds);
    let offset = schist_layout::compose::aligned_origin(
        Rect::new(
            0.0,
            0.0,
            line.bounds.width * settings.scale(),
            line.bounds.height * settings.scale(),
        ),
        raster.layout_width,
        align,
        writing,
    );
    let dx = offset.x.round() as i32;
    let dy = offset.y.round() as i32;
    let rect = IntRect::new(
        frame.left + dx + raster.bounds.left,
        frame.top + dy + raster.bounds.top,
        frame.left + dx + raster.bounds.right,
        frame.top + dy + raster.bounds.bottom,
    );
    raster
        .paints
        .into_iter()
        .map(|paint| {
            let index = paint.color.map(u32::from_le_bytes).unwrap_or(0) as usize;
            let (ink, opacity, overprint, tint) = &inks[index];
            TextPaint {
                coverage: Coverage {
                    rect,
                    data: paint.coverage,
                },
                ink: ink.clone(),
                opacity: *opacity,
                overprint: *overprint,
                tint: *tint,
            }
        })
        .collect()
}

/// The coverage of a placed graphic, if its source can be resolved.
pub fn graphic_coverage(
    placed: &PlacedObject,
    settings: OutputSettings,
    page: &schist_layout::Page,
    source: &dyn GraphicSource,
) -> Option<PlacedGraphic> {
    let LayoutObject::GraphicFrame {
        link,
        fit,
        crop,
        scale,
        image_transform,
        clip_path,
        ..
    } = &placed.object
    else {
        return None;
    };
    let dest = PagePixel::rect(settings, page, placed.bounds);
    if dest.is_empty() {
        return None;
    }
    let graphic = source.sample(
        link,
        &GraphicPlacement {
            dest,
            crop: *crop,
            fit: *fit,
            scale: *scale,
            image_transform: *image_transform,
            dpi: settings.resolution_dpi,
        },
    )?;
    let mut graphic = warp_graphic(graphic, placed, settings, page)?;
    if let Some(path) = clip_path {
        let matrix = schist_layout::affine::in_view(
            placed.content_transform(),
            settings.scale(),
            schist_layout::Point::ZERO,
        );
        let mask =
            schist_layout::graphics::clip_coverage(path, rect_points(dest), matrix, graphic.rect);
        for (alpha, clip) in graphic.coverage.iter_mut().zip(mask) {
            *alpha = ((*alpha as u16 * clip as u16 + 127) / 255) as u8;
        }
    }
    Some(graphic)
}

fn rect_points(rect: IntRect) -> Rect {
    Rect::new(
        rect.left as f32,
        rect.top as f32,
        rect.width() as f32,
        rect.height() as f32,
    )
}

pub fn warp_coverage(
    coverage: Coverage,
    placed: &PlacedObject,
    settings: OutputSettings,
    page: &schist_layout::Page,
) -> Coverage {
    let matrix = schist_layout::affine::in_view(
        placed.content_transform(),
        settings.scale(),
        schist_layout::Point::ZERO,
    );
    if matrix == schist_core::Affine::IDENTITY || coverage.rect.is_empty() {
        return coverage;
    }
    let source = rect_points(coverage.rect);
    let Some(warp) = schist_layout::affine::Warp::new(
        source,
        matrix,
        rect_points(settings.to_pixels_rect(settings.output_box(page))),
        64 * 1024 * 1024,
    ) else {
        return Coverage::new(IntRect::EMPTY);
    };
    let mut out = Coverage::new(warp.rect);
    for y in warp.rect.top..warp.rect.bottom {
        for x in warp.rect.left..warp.rect.right {
            let (_, alpha) = schist_layout::affine::sample::<0>(
                source,
                (coverage.rect.width() as u32, coverage.rect.height() as u32),
                warp.source(x, y),
                |i| {
                    (
                        [],
                        coverage.data.get(i).copied().unwrap_or(0) as f32 / 255.0,
                    )
                },
            );
            out.data[((y - warp.rect.top) * warp.rect.width() + x - warp.rect.left) as usize] =
                (alpha * 255.0).round() as u8;
        }
    }
    out
}

fn warp_graphic(
    graphic: PlacedGraphic,
    placed: &PlacedObject,
    settings: OutputSettings,
    page: &schist_layout::Page,
) -> Option<PlacedGraphic> {
    let matrix = schist_layout::affine::in_view(
        placed.content_transform(),
        settings.scale(),
        schist_layout::Point::ZERO,
    );
    if matrix == schist_core::Affine::IDENTITY || graphic.rect.is_empty() {
        return Some(graphic);
    }
    let source = rect_points(graphic.rect);
    let warp = schist_layout::affine::Warp::new(
        source,
        matrix,
        rect_points(settings.to_pixels_rect(settings.output_box(page))),
        64 * 1024 * 1024,
    )?;
    let area = warp.rect.width().max(0) as usize * warp.rect.height().max(0) as usize;
    if graphic.coverage.len() != graphic.rect.width() as usize * graphic.rect.height() as usize
        || graphic.coverage.len() != graphic.cmyk.len()
    {
        return None;
    }
    let mut out = PlacedGraphic {
        rect: warp.rect,
        coverage: vec![0; area],
        cmyk: vec![[0.0; 4]; area],
    };
    for y in warp.rect.top..warp.rect.bottom {
        for x in warp.rect.left..warp.rect.right {
            let (color, alpha) = schist_layout::affine::sample(
                source,
                (graphic.rect.width() as u32, graphic.rect.height() as u32),
                warp.source(x, y),
                |i| (graphic.cmyk[i], graphic.coverage[i] as f32 / 255.0),
            );
            let index = ((y - warp.rect.top) * warp.rect.width() + x - warp.rect.left) as usize;
            out.cmyk[index] = color;
            out.coverage[index] = (alpha * 255.0).round() as u8;
        }
    }
    Some(out)
}

/// The object's frame coverage, for a graphic whose source is a flat fill.
pub fn frame_coverage(
    placed: &PlacedObject,
    settings: OutputSettings,
    page: &schist_layout::Page,
) -> Coverage {
    Coverage::new(PagePixel::rect(settings, page, placed.bounds))
}

/// Scale a text spec from points into output pixels.
///
/// The text engine's `size` is in pixels, not points, so a spec built
/// for a 170mm measure rasterises an 11 *pixel* tall line. At 150 dpi
/// that is a page of 8pt type, laid out correctly and placed correctly,
/// and utterly wrong to look at. Scaling here rather than resampling the
/// raster afterwards keeps the glyph shapes crisp at plate resolution.
pub fn scale_spec(spec: schist_text_engine::TextSpec, scale: f32) -> schist_text_engine::TextSpec {
    let mut spec = spec;
    spec.size *= scale;
    spec.leading = spec.leading.map(|v| v * scale);
    spec.tracking *= scale;
    spec.word_spacing *= scale;
    if let Some(tabs) = &mut spec.tabs {
        tabs.scaled(scale);
    }
    if let Some(path) = &mut spec.path {
        path.scaled(scale);
    }
    for run in &mut spec.runs {
        run.size = run.size.map(|size| size * scale);
        run.metric_size = run.metric_size.map(|v| v * scale);
        run.baseline_shift = run.baseline_shift.map(|v| v * scale);
        for d in [&mut run.underline_style, &mut run.strike_style]
            .into_iter()
            .flatten()
        {
            d.scaled(scale);
        }
        if let Some(stroke) = &mut run.stroke {
            stroke.width *= scale;
        }
        run.tracking = run.tracking.map(|v| v * scale);
        run.leading = run.leading.map(|v| v * scale);
    }
    spec.wrap_width = spec.wrap_width.map(|w| w * scale);
    spec
}

/// Re-scale a path from points into a pixel grid anchored at an origin.
fn to_pixels(
    path: schist_vector::Path,
    origin_x: i32,
    origin_y: i32,
    scale: f32,
) -> schist_vector::Path {
    let mut out = schist_vector::Path::default();
    for (i, sub) in path.subpaths.iter().enumerate() {
        let points: Vec<(f32, f32)> = sub
            .iter()
            .map(|(x, y)| (origin_x as f32 + x * scale, origin_y as f32 + y * scale))
            .collect();
        if path.is_closed(i) {
            out.push_closed(points);
        } else {
            out.push_open(points);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::geometry::{mm, Insets, Point};
    use schist_layout::model::{blank_a4, FrameOverflow, ObjectId, StoryId};
    use schist_layout::story::{Point as StoryPoint, Story};
    use schist_layout::StyleSet;

    fn square(size: f32) -> schist_layout::ShapePath {
        let mut shape = schist_layout::ShapePath::default();
        shape.subpaths.push(schist_layout::SubPath {
            handles: Vec::new(),
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(size, 0.0),
                Point::new(size, size),
                Point::new(0.0, size),
            ],
            closed: true,
        });
        shape
    }

    #[test]
    fn a_shape_at_the_page_origin_rasterises_where_it_sits() {
        let page = schist_layout::Page::a4();
        let settings = OutputSettings::at(300.0);
        let coverage = shape_coverage(
            &square(mm(20.0)),
            Rect::new(0.0, 0.0, mm(20.0), mm(20.0)),
            settings,
            &page,
            0.0,
        );
        // The square is filled where it is, not offset by the page.
        assert!(coverage.at(10, 10) > 0.9, "centre not filled");
        assert_eq!(coverage.at(400, 10), 0.0, "outside the square");
    }

    #[test]
    fn shape_geometry_is_scaled_from_points_into_pixels() {
        // A path left in points would come out 300/72 times too small,
        // and the shape would look like a hairline in the corner.
        let page = schist_layout::Page::a4();
        let settings = OutputSettings::at(300.0);
        let coverage = shape_coverage(
            &square(mm(20.0)),
            Rect::new(0.0, 0.0, mm(20.0), mm(20.0)),
            settings,
            &page,
            0.0,
        );
        let half = settings.to_pixels(mm(10.0));
        assert!(coverage.at(half, half) > 0.9, "centre not filled");
        // Just inside the corner is covered, and past the edge is not.
        assert!(coverage.at(half - 4, half - 4) > 0.9);
        let past = settings.to_pixels(mm(25.0));
        assert_eq!(coverage.at(past, past), 0.0, "coverage outside the shape");
    }

    #[test]
    fn a_shape_moves_with_its_frame() {
        let page = schist_layout::Page::a4();
        let settings = OutputSettings::at(300.0);
        // Same geometry, frame in a different place: the shape follows.
        let moved = shape_coverage(
            &square(mm(20.0)),
            Rect::new(mm(100.0), mm(100.0), mm(20.0), mm(20.0)),
            settings,
            &page,
            0.0,
        );
        // The same geometry at a new position lands at the new position.
        let here = settings.to_pixels(mm(105.0));
        let there = settings.to_pixels(mm(10.0));
        assert!(moved.at(here, here) > 0.9, "not at the moved position");
        assert_eq!(moved.at(there, there), 0.0, "still at the old position");
    }

    #[test]
    fn an_empty_frame_rasterises_to_nothing() {
        let page = schist_layout::Page::a4();
        let coverage = shape_coverage(
            &schist_layout::ShapePath::default(),
            Rect::new(0.0, 0.0, mm(20.0), mm(20.0)),
            OutputSettings::at(300.0),
            &page,
            0.0,
        );
        assert!(coverage.is_empty() || coverage.data.iter().all(|b| *b == 0));
    }

    #[test]
    fn a_stroke_adds_coverage_outside_the_fill() {
        let page = schist_layout::Page::a4();
        let settings = OutputSettings::at(300.0);
        // A single open line, so the fill covers nothing and only the
        // stroke is visible.
        let mut shape = schist_layout::ShapePath::default();
        shape.subpaths.push(schist_layout::SubPath {
            handles: Vec::new(),
            points: vec![Point::new(0.0, mm(10.0)), Point::new(mm(40.0), mm(10.0))],
            closed: false,
        });
        let bounds = Rect::new(0.0, 0.0, mm(40.0), mm(20.0));
        let thin = shape_coverage(&shape, bounds, settings, &page, 0.0);
        let thick = shape_coverage(&shape, bounds, settings, &page, 2.0);
        let covered = |c: &Coverage| c.data.iter().filter(|b| **b > 0).count();
        assert!(covered(&thick) > covered(&thin), "stroke added no coverage");
    }

    #[test]
    fn a_source_with_nothing_offers_reports_every_link_missing() {
        let source = NoGraphics;
        let link = Link::new("/nonexistent.psd");
        assert!(source
            .sample(
                &link,
                &GraphicPlacement {
                    dest: IntRect::new(0, 0, 10, 10),
                    crop: None,
                    fit: GraphicFit::Fill,
                    scale: 1.0,
                    image_transform: Default::default(),
                    dpi: 72.0
                }
            )
            .is_none());
    }

    #[test]
    fn a_solid_graphic_is_fully_covered() {
        let r = IntRect::new(2, 3, 6, 9);
        let g = PlacedGraphic::solid(r, [0.0, 1.0, 1.0, 0.0]);
        assert_eq!(g.cmyk.len(), (6 - 2) * (9 - 3));
        assert!(g.coverage.iter().all(|c| *c == 255));
        let coverage = g.into_coverage();
        assert!((coverage.at(3, 4) - 1.0).abs() < 1e-6);
        assert_eq!(coverage.at(0, 0), 0.0);
    }

    #[test]
    fn an_ink_the_plan_has_not_seen_gets_a_plate() {
        let mut plan = PlatePlan::build(
            &[Ink::black()],
            &schist_layout::ink::InkManager::with_defaults(),
        );
        let before = plan.plates.len();
        let spot = Ink::spot("RUSH ADDED", [10.0, 20.0, 30.0]);
        let (coats, build) = coats_for(&mut plan, &spot);
        assert_eq!(plan.plates.len(), before + 1, "no plate was added");
        assert_eq!(coats.len(), 1);
        assert!(plan.has_plate(coats[0].plate));
        // The composite still needs a process build for a spot.
        assert_eq!(build.len(), 1);
        assert_eq!(build[0].len(), 4);
    }

    #[test]
    fn a_process_ink_produces_one_coat_per_channel_it_needs() {
        let mut plan = PlatePlan::build(
            &[Ink::process("Cyan", [0.0, 0.68, 0.94])],
            &schist_layout::ink::InkManager::with_defaults(),
        );
        let (coats, _) = coats_for(&mut plan, &Ink::process("Cyan", [0.0, 0.68, 0.94]));
        assert!(coats.len() >= 2, "a saturated cyan needs two plates");
    }

    #[test]
    fn an_empty_frame_coverage_matches_the_frame() {
        let page = schist_layout::Page::a4();
        let placed = PlacedObject {
            appearance: Default::default(),
            id: ObjectId::next(),
            page: 0,
            bounds: Rect::new(mm(10.0), mm(10.0), mm(50.0), mm(50.0)),
            object: LayoutObject::TextFrame {
                text_path: None,
                story: StoryId(0),
                columns: 1,
                gutter: 0.0,
                insets: Insets::ZERO,
                overflow: FrameOverflow::Clip,
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Frame".into(),
            locked: false,
            overprint: false,
            transparency: 1.0,
        };
        let coverage = frame_coverage(&placed, OutputSettings::at(300.0), &page);
        let settings = OutputSettings::at(300.0);
        assert_eq!(coverage.rect.left, settings.to_pixels(mm(10.0)));
        assert_eq!(coverage.rect.width(), settings.to_pixels(mm(50.0)));
    }

    #[test]
    fn a_forced_break_line_has_no_glyphs_to_rasterise() {
        let mut doc = blank_a4();
        doc.styles = StyleSet::with_defaults();
        let mut story = Story::new();
        story.push_paragraph("Some text to compose.", "Body");
        let id = doc.add_story(story);
        let page = doc.pages[0].clone();
        let thread = schist_layout::compose::compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(170.0), mm(200.0)),
                FrameOverflow::Clip,
                1,
                0.0,
                schist_layout::compose::InsetsLike::default(),
            )],
        );
        let story = doc.story(id).unwrap();
        for line in thread.lines() {
            let c = line_coverage(line, story, &doc, OutputSettings::at(300.0), &page);
            if line.forced_break {
                assert!(c.is_none(), "a break line produced coverage");
            }
        }
        // And a real line does produce some.
        let real: Vec<_> = thread.lines().filter(|l| !l.forced_break).collect();
        assert!(!real.is_empty());
        let c = line_coverage(real[0], story, &doc, OutputSettings::at(300.0), &page)
            .expect("a text line rasterises");
        assert!(c.data.iter().any(|b| *b > 0), "no ink at all");
        let _ = StoryPoint::LineBreak;
    }
}
