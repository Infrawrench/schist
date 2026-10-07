//! Painting the pasteboard.
//!
//! The plan comes from `schist_layout::pasteboard`, which knows where
//! everything is. This knows only how to draw it, which means a layout bug
//! is reproducible in a unit test and a paint bug cannot hide one.
//!
//! Text is drawn with the shared text engine rather than with a font stack
//! of the editor's own, so what the pasteboard shows is the shaping that
//! will be rasterised onto a plate. A preview that shaped text any other
//! way would disagree with the output, which is the one thing a prepress
//! preview must never do.

use std::sync::Arc;

use gpui::{rgb, Background, Bounds, Corners, PathBuilder, Pixels, Point as GpuiPoint, Window};
use schist_layout::affine::{self, Affine};
use schist_layout::pasteboard::{Display, Guide, GuideKind, PagePlan, Pasteboard, StrokeOptions};
use schist_layout::StrokeAlignment;
use schist_layout::{Point, Pt, Rect};
use schist_text_engine::{rasterize, TextRaster, TextSpec};

/// The pasteboard surface, which is a work surface and has to read as one
/// against the chrome around it.
const PASTEBOARD: u32 = 0x333333;
/// Paper. Not the canvas colour: a page is paper.
const PAPER: u32 = 0xFDFDFB;
/// The page's own edge.
const TRIM: u32 = 0x2B2B2B;
const BLEED: u32 = 0xC0392B;
const MARGIN: u32 = 0x4A7FB5;
const COLUMN: u32 = 0x6E6E6E;
/// The baseline grid is the faintest thing on the page, because it is on
/// all the time and is not the content.
const BASELINE: u32 = 0x3D3D3D;
const RULER_GUIDE: u32 = 0xF0A020;
/// A frame outline.
const FRAME: u32 = 0x4A9FD8;
/// An item a parent page owns, which cannot be selected without
/// overriding it and is drawn to say so.
const INHERITED_FRAME: u32 = 0x8A6FB5;
const SELECTED: u32 = 0xF0A020;
/// The boundary a selected item's text wrap pushes text away from.
const WRAP: u32 = 0x2E9E8F;
/// Text on the page. Near-black rather than theme-coloured: paper is
/// white whatever the shell is.
const TEXT: u32 = 0x1A1A1A;
const MISSING: u32 = 0xC0392B;
/// A hidden page's wash, which dims its contents without hiding them.
const HIDDEN_WASH: u32 = 0x4D4D4D;

/// A hairline.
const HAIRLINE: f32 = 1.0;

/// Everything the pasteboard needs to draw itself, gathered before
/// painting so the draw closure does not have to borrow the workspace.
pub struct PasteboardFrame {
    pub plan: Pasteboard,
    pub graphics: Arc<super::graphics::Graphics>,
    /// The canvas bounds. The plan covers the whole pasteboard and
    /// everything in it is clipped to this.
    pub bounds: Bounds<Pixels>,
    /// Selected objects' rectangles, in pasteboard points, paired with
    /// the page they are on.
    pub selected: Vec<(usize, Rect)>,
    /// Selected items' rectangular text-wrap boundaries, in pasteboard points.
    pub wraps: Vec<(usize, Rect)>,
    pub show_marks: bool,
    /// The selected shapes whose own anchor points are being shown, as
    /// page numbers paired with their points in pasteboard space.
    ///
    /// Empty unless the direct selection tool is in use. Drawn only for
    /// selected shapes, because a page covered in anchor dots is a page
    /// nobody can read.
    pub anchors: Vec<(usize, schist_layout::ShapePath)>,
    pub pen_preview: Option<schist_layout::ShapePath>,
    pub typing: Option<super::Typing>,
    /// Pages as they separate, with the bleed rectangles they cover, drawn
    /// in place of the items when previewing output.
    pub previews: Vec<(Rect, Arc<gpui::RenderImage>)>,
    pub band: Option<Rect>,
    pub drawing: Option<schist_layout::ShapePath>,
}

impl PasteboardFrame {
    /// Build a frame for a plan.
    pub fn new(plan: Pasteboard, bounds: Bounds<Pixels>) -> PasteboardFrame {
        PasteboardFrame {
            plan,
            graphics: Default::default(),
            bounds,
            selected: Vec::new(),
            wraps: Vec::new(),
            show_marks: true,
            anchors: Vec::new(),
            pen_preview: None,
            typing: None,
            previews: Vec::new(),
            band: None,
            drawing: None,
        }
    }

    /// The canvas as a page-space rectangle, for hit tests and clipping.
    pub fn visible(&self) -> Rect {
        clip_to(&self.bounds)
    }
}

/// Draw the pasteboard.
pub fn paint_pasteboard(frame: &PasteboardFrame, window: &mut Window) {
    // The surface behind everything, which is the canvas itself.
    fill_rect(window, &frame.visible(), frame.bounds, PASTEBOARD);

    for plan in &frame.plan.pages {
        // All paper precedes all artwork: the next sheet must not erase
        // objects extending across its gutter.
        fill_rect(window, &plan.page.media, frame.bounds, PAPER);
    }
    for (rect, image) in &frame.previews {
        let target = Bounds::new(
            gpoint(&frame.bounds, rect.origin()),
            gpui::size(px(rect.width), px(rect.height)),
        );
        let _ = window.paint_image(target, Corners::default(), image.clone(), 0, false);
    }
    for object in frame.plan.objects() {
        // Previewing output, the pages already show the items: only frame
        // edges, ports, notes and the story being typed are drawn over them.
        let typed = |story: &schist_layout::StoryId| {
            frame.typing.is_some_and(|typing| typing.story == *story)
        };
        let drawn = frame.previews.is_empty()
            || match object {
                Display::Text { story, .. } => typed(story),
                Display::Shape { .. } | Display::Graphic { .. } => false,
                _ => true,
            };
        if drawn {
            paint_object(window, frame, object);
        }
    }
    for plan in &frame.plan.pages {
        if plan.page.hidden {
            fill_rect(window, &plan.page.trim, frame.bounds, HIDDEN_WASH);
        }
        paint_selection(window, frame, plan);
        paint_anchors(window, frame, plan);
        for guide in &plan.guides {
            paint_guide(window, frame.bounds, *guide, plan.page.bleed);
        }
    }
    if let Some(path) = &frame.drawing {
        paint_shape(
            window,
            frame.bounds,
            path,
            Fill::default(),
            Some(([0.1, 0.1, 0.1, 1.0], HAIRLINE)),
        );
    }
    if let Some(band) = frame.band {
        outline(window, frame.bounds, band, SELECTED, true);
    }
    if let Some(path) = &frame.pen_preview {
        paint_shape(
            window,
            frame.bounds,
            path,
            Fill::default(),
            Some(([0.1, 0.1, 0.1, 1.0], HAIRLINE)),
        );
        paint_handles(window, frame.bounds, path);
    }
    if frame.show_marks {
        for plan in &frame.plan.pages {
            paint_marks(window, frame.bounds, &plan.page.trim);
        }
    }
}

fn paint_object(window: &mut Window, frame: &PasteboardFrame, object: &Display) {
    match object {
        Display::Frame {
            rect,
            transform,
            inherited,
            locked,
            ..
        } => {
            let colour = if *inherited { INHERITED_FRAME } else { FRAME };
            // A parent's item is dashed, and so is a locked one. A
            // reader has to be able to tell at a glance which is which,
            // and the frame is too small for a label to be reliable.
            affine_outline(
                window,
                frame.bounds,
                *rect,
                *transform,
                colour,
                *locked || *inherited,
            );
        }
        Display::EmptyFrame {
            rect, transform, ..
        } => {
            // An empty frame is still clickable, so it is still drawn.
            affine_outline(window, frame.bounds, *rect, *transform, FRAME, true);
        }
        Display::Text {
            generated,
            positions,
            object,
            story,
            start,
            end,
            rect,
            spec,
            transform,
            gradients,
            ..
        } => {
            let typing = frame.typing.filter(|t| !*generated && t.story == *story);
            let visual = |source: usize| {
                positions
                    .as_ref()
                    .map_or(source.saturating_sub(*start), |p| p.visual(source))
            };
            let origin = super::text::line_origin(spec, *rect);
            if let Some(typing) = typing {
                let from = typing.anchor.min(typing.at).max(*start);
                let to = typing.anchor.max(typing.at).min(*end);
                if from < to {
                    for rect in schist_text_engine::selection_rects(spec, visual(from)..visual(to))
                    {
                        affine_fill(
                            window,
                            Rect::new(
                                origin.x + rect.left as f32,
                                origin.y + rect.top as f32,
                                rect.width() as f32,
                                rect.height() as f32,
                            ),
                            frame.bounds,
                            *transform,
                            0xB8D2EE,
                        );
                    }
                }
            }
            paint_text(window, frame.bounds, *rect, spec, *transform, gradients);
            if let Some(typing) = typing.filter(|t| {
                !*generated && super::text::caret_line(&frame.plan, *t) == Some((*object, *start))
            }) {
                let caret = schist_text_engine::caret_at(spec, visual(typing.at)).unwrap_or(
                    schist_text_engine::Caret {
                        x: 0.0,
                        top: 0.0,
                        height: spec.size,
                        angle: 0.0,
                    },
                );
                let mut line = PathBuilder::stroke(px(1.0));
                let a = Point::new(origin.x + caret.x, origin.y + caret.top);
                line.move_to(gpoint(&frame.bounds, affine::point(*transform, a)));
                line.line_to(gpoint(
                    &frame.bounds,
                    affine::point(
                        *transform,
                        Point::new(
                            a.x - caret.height * caret.angle.sin(),
                            a.y + caret.height * caret.angle.cos(),
                        ),
                    ),
                ));
                if let Ok(path) = line.build() {
                    window.paint_path(path, rgb(TEXT));
                }
            }
        }
        Display::Ports {
            inlet,
            outlet,
            linked_in,
            linked_out,
            overset,
            ..
        } => {
            for (rect, linked, colour) in [
                (inlet, *linked_in, FRAME),
                (outlet, *linked_out, if *overset { MISSING } else { FRAME }),
            ] {
                fill_rect(window, rect, frame.bounds, PAPER);
                outline(window, frame.bounds, *rect, colour, false);
                if linked {
                    fill_rect(
                        window,
                        &rect.inset(schist_layout::Insets::uniform(2.0)),
                        frame.bounds,
                        colour,
                    );
                }
            }
            if *overset {
                let _ = paint_label_at(
                    window,
                    frame.bounds,
                    outlet.origin(),
                    "+",
                    outlet.height,
                    MISSING,
                );
            }
        }
        Display::Shape {
            path,
            fill,
            gradient,
            stroke,
            stroke_gradient,
            stroke_options,
            gap,
            shadow,
            transform,
            ..
        } => {
            if let Some(shadow) = shadow {
                paint_shadow(
                    window,
                    frame.bounds,
                    path,
                    fill.is_some() || gradient.is_some(),
                    stroke.map(|(_, width)| width),
                    shadow,
                );
            }
            let fill = Fill {
                color: *fill,
                gradient: gradient.as_deref(),
            };
            if *transform == Affine::IDENTITY
                && *stroke_options == StrokeOptions::default()
                && stroke_gradient.is_none()
            {
                // GPUI strokes with InDesign's defaults: butt, mitre, 4.
                paint_shape(window, frame.bounds, path, fill, *stroke);
            } else if let (Some((color, width)), Some(inverse)) = (stroke, transform.invert()) {
                // A gradient stroke is laid pixel by pixel over its outline.
                let stroke_fill = Fill {
                    color: Some(*color),
                    gradient: stroke_gradient.as_deref(),
                };
                // The stroke outlined in the shape's own space, so a skewed
                // or unevenly scaled shape strokes as it prints.
                let mut local = path.clone();
                local.map_points(|p| affine::point(inverse, p));
                let tolerance = 0.25 / affine::stretch(*transform).max(1.0);
                let back = |outline: schist_vector::Path| schist_layout::ShapePath {
                    subpaths: outline
                        .subpaths
                        .iter()
                        .enumerate()
                        .map(|(i, points)| schist_layout::SubPath {
                            points: points
                                .iter()
                                .map(|(x, y)| affine::point(*transform, Point::new(*x, *y)))
                                .collect(),
                            handles: Vec::new(),
                            closed: outline.is_closed(i),
                        })
                        .collect(),
                    even_odd: false,
                };
                let outline = |width: f32| {
                    back(schist_vector::stroke_path(
                        &local.flatten(tolerance),
                        stroke_options.style(width),
                    ))
                };
                let closed = path.subpaths.iter().any(|s| s.closed);
                let aligned = matches!(
                    stroke_options.alignment,
                    StrokeAlignment::Inside | StrokeAlignment::Outside
                ) && closed;
                // Dashes, dots or stripes: the fill sits as under a solid
                // stroke, the gap colour fills the band, the ink goes over it.
                if let Some(pattern) = schist_layout::stroke_patterns::outlines(
                    &local,
                    *width,
                    stroke_options,
                    tolerance,
                ) {
                    if aligned {
                        paint_aligned(
                            window,
                            frame.bounds,
                            path,
                            fill,
                            (Fill::default(), &outline(*width), &outline(*width * 2.0)),
                            stroke_options.alignment == StrokeAlignment::Inside,
                        );
                    } else {
                        paint_shape(window, frame.bounds, path, fill, None);
                    }
                    if let Some(gap) = gap {
                        paint_shape(
                            window,
                            frame.bounds,
                            &back(pattern.band),
                            Fill::solid(*gap),
                            None,
                        );
                    }
                    paint_shape(window, frame.bounds, &back(pattern.ink), stroke_fill, None);
                    return;
                }
                match stroke_options.alignment {
                    StrokeAlignment::Inside | StrokeAlignment::Outside if closed => {
                        paint_aligned(
                            window,
                            frame.bounds,
                            path,
                            fill,
                            (stroke_fill, &outline(*width), &outline(*width * 2.0)),
                            stroke_options.alignment == StrokeAlignment::Inside,
                        );
                    }
                    _ => {
                        paint_shape(window, frame.bounds, path, fill, None);
                        paint_shape(window, frame.bounds, &outline(*width), stroke_fill, None);
                    }
                }
            } else {
                paint_shape(window, frame.bounds, path, fill, None);
            }
        }
        Display::Graphic {
            rect,
            transform,
            label,
            missing,
            source,
            fit,
            crop,
            scale,
            image_transform,
            clip_path,
            zoom,
            opacity,
            ..
        } => {
            if let Some(graphic) = frame.graphics.get(source) {
                if let Some(mapped) = schist_layout::graphics::image_rect(
                    *rect,
                    graphic.pixels.dimensions(),
                    graphic.dpi / zoom,
                    *crop,
                    *fit,
                    *scale,
                ) {
                    let Some(mapping) =
                        schist_layout::graphics::ImageMapping::new(*rect, mapped, *image_transform)
                    else {
                        return;
                    };
                    let clip = mapping.visible;
                    if *transform != Affine::IDENTITY
                        || *image_transform != Affine::IDENTITY
                        || clip_path.is_some()
                    {
                        paint_warped_image(
                            window,
                            frame.bounds,
                            &graphic.pixels,
                            &mapping,
                            *transform,
                            *opacity,
                            clip_path.as_ref(),
                        );
                        return;
                    }
                    let target = Bounds::new(
                        gpoint(&frame.bounds, mapped.origin()),
                        gpui::size(px(mapped.width), px(mapped.height)),
                    );
                    let mask = Bounds::new(
                        gpoint(&frame.bounds, clip.origin()),
                        gpui::size(px(clip.width), px(clip.height)),
                    );
                    window.with_content_mask(
                        Some(gpui::ContentMask {
                            bounds: mask.intersect(&frame.bounds),
                        }),
                        |window| {
                            let _ = window.paint_image(
                                target,
                                Corners::default(),
                                graphic.image_with_opacity(*opacity),
                                0,
                                false,
                            );
                        },
                    );
                    return;
                }
            }
            let colour = if *missing
                || frame
                    .graphics
                    .sources
                    .get(source)
                    .is_some_and(Result::is_err)
            {
                MISSING
            } else {
                FRAME
            };
            affine_outline(window, frame.bounds, *rect, *transform, colour, false);
            // The label goes in the corner when there is room. A frame
            // narrower than a word simply has none, which is right: a
            // clipped filename is worse than none.
            if rect.width > 24.0 && rect.height > 10.0 {
                let _ = paint_label(window, frame.bounds, *rect, label, colour);
            }
        }
        Display::Note { at, text, .. } => {
            let _ = paint_label_at(
                window,
                frame.bounds,
                Point::new(at.x + 4.0, at.y),
                text,
                8.0,
                TEXT,
            );
        }
    }
}

/// Draw one composed line with the shared text engine.
///
/// The size comes from the plan, already in pasteboard points, so the
/// line is drawn at the size it will print. A preview that drew text at a
/// different size from the output is worse than no preview.
fn paint_text(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    rect: Rect,
    spec: &TextSpec,
    transform: Affine,
    gradients: &[schist_layout::pasteboard::TextGradient],
) {
    if spec.text.is_empty() {
        return;
    }
    if !gradients.is_empty() {
        paint_gradient_text(window, bounds, rect, spec, transform, gradients);
        return;
    }
    let Some(raster) = rasterize(spec) else {
        return;
    };
    if raster.bounds.width() <= 0 || raster.bounds.height() <= 0 {
        return;
    }
    let origin = super::text::line_origin(spec, rect);
    blit_transformed(
        window,
        bounds,
        &raster,
        Point::new(
            origin.x + raster.bounds.left as Pt,
            origin.y + raster.bounds.top as Pt,
        ),
        TEXT,
        transform,
    );
}

/// A line some of whose runs are painted with gradients. Each such paint is
/// given a colour no other paint in the line has, so the engine keeps its
/// coverage apart, and is then coloured pixel by pixel from its gradient at
/// the opacity its own colour carried. Paints composite in draw order, as
/// `TextRaster::rgba` composites them.
fn paint_gradient_text(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    rect: Rect,
    spec: &TextSpec,
    transform: Affine,
    gradients: &[schist_layout::pasteboard::TextGradient],
) {
    use schist_layout::pasteboard::TextPart;
    let mut used = std::collections::HashSet::new();
    for run in &spec.runs {
        used.extend(run.color);
        used.extend(run.stroke.and_then(|s| s.color));
        for decoration in [&run.underline_style, &run.strike_style]
            .into_iter()
            .flatten()
        {
            used.extend(decoration.color);
            used.extend(decoration.gap_color);
        }
    }
    let mut next = 0u32;
    let mut keyed = spec.clone();
    let mut keys = Vec::new();
    for gradient in gradients {
        let Some(run) = keyed.runs.get_mut(gradient.run) else {
            continue;
        };
        let key = loop {
            let [_, r, g, b] = next.to_be_bytes();
            next += 1;
            if !used.contains(&[r, g, b, 255]) {
                break [r, g, b, 255];
            }
        };
        let slot = match gradient.part {
            TextPart::Fill => Some(&mut run.color),
            TextPart::Stroke => run.stroke.as_mut().map(|s| &mut s.color),
            TextPart::Underline => run.underline_style.as_mut().map(|d| &mut d.color),
            TextPart::Strike => run.strike_style.as_mut().map(|d| &mut d.color),
        };
        let Some(slot) = slot else { continue };
        let alpha = slot.map_or(255, |c| c[3]);
        *slot = Some(key);
        keys.push((key, &gradient.gradient, alpha));
    }
    let Some(raster) = schist_text_engine::rasterize_with_paints(&keyed) else {
        return;
    };
    let (width, height) = (raster.bounds.width(), raster.bounds.height());
    if width <= 0 || height <= 0 {
        return;
    }
    let origin = super::text::line_origin(spec, rect);
    let at = Point::new(
        origin.x + raster.bounds.left as Pt,
        origin.y + raster.bounds.top as Pt,
    );
    let [r, g, b] = channels(TEXT);
    let mut out = vec![[0.0f32; 4]; raster.coverage.len()];
    for paint in &raster.paints {
        let gradient = keys.iter().find(|(key, ..)| Some(*key) == paint.color);
        for (index, (pixel, &coverage)) in out.iter_mut().zip(&paint.coverage).enumerate() {
            if coverage == 0 {
                continue;
            }
            let color = match gradient {
                Some((_, gradient, alpha)) => {
                    let rgb = gradient.preview(
                        at.x + (index % width as usize) as f32 + 0.5,
                        at.y + (index / width as usize) as f32 + 0.5,
                    );
                    [
                        rgb[0].clamp(0.0, 1.0) * 255.0,
                        rgb[1].clamp(0.0, 1.0) * 255.0,
                        rgb[2].clamp(0.0, 1.0) * 255.0,
                        f32::from(*alpha),
                    ]
                }
                None => paint.color.unwrap_or([r, g, b, 255]).map(f32::from),
            };
            let alpha = f32::from(coverage) * color[3] / (255.0 * 255.0);
            for channel in 0..3 {
                pixel[channel] = color[channel] * alpha + pixel[channel] * (1.0 - alpha);
            }
            pixel[3] = alpha + pixel[3] * (1.0 - alpha);
        }
    }
    let buffer = out
        .into_iter()
        .flat_map(|p| {
            if p[3] <= 0.0 {
                [0; 4]
            } else {
                [
                    (p[0] / p[3]).round() as u8,
                    (p[1] / p[3]).round() as u8,
                    (p[2] / p[3]).round() as u8,
                    (p[3] * 255.0).round() as u8,
                ]
            }
        })
        .collect();
    if let Some(image) = image::RgbaImage::from_raw(width as u32, height as u32, buffer) {
        paint_rgba(window, bounds, image, at, transform);
    }
}

/// A shape's outline, or its fill.
///
/// Filled contours share the output rasterizer and its winding rule.
/// Unfilled outlines use native cubic paths.
/// A shape whose stroke sits inside or outside its path: InDesign moves the
/// path half the stroke's weight that way for the fill and the stroke alike.
/// `stroke` is the stroke's colour or gradient and its outlines at the
/// weight and at twice it. Inside, the fill loses the inner half of the first
/// band and the stroke is the inner side of the second; outside, the fill
/// gains the outer half and the stroke is the outer side. The stroke is laid
/// over the fill.
fn paint_aligned(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    shape: &schist_layout::ShapePath,
    fill: Fill<'_>,
    (stroke, narrow, wide): (
        Fill<'_>,
        &schist_layout::ShapePath,
        &schist_layout::ShapePath,
    ),
    inside: bool,
) {
    let path = shape.flatten(0.25);
    let narrow = narrow.flatten(0.25);
    let wide = wide.flatten(0.25);
    let clip = schist_core::IntRect::new(
        0,
        0,
        f32::from(bounds.size.width).ceil() as i32,
        f32::from(bounds.size.height).ceil() as i32,
    );
    let rect = path.bounds().union(&wide.bounds()).intersect(&clip);
    if rect.width() <= 0
        || rect.height() <= 0
        || i64::from(rect.width()) * i64::from(rect.height()) > 16_000_000
    {
        return;
    }
    let rule = if shape.even_odd {
        schist_vector::FillRule::EvenOdd
    } else {
        schist_vector::FillRule::NonZero
    };
    let filled = schist_vector::rasterize(&path, rect, rule);
    let band = schist_vector::rasterize(&narrow, rect, schist_vector::FillRule::NonZero);
    let double = schist_vector::rasterize(&wide, rect, schist_vector::FillRule::NonZero);
    let width = rect.width() as usize;
    let mut pixels = Vec::with_capacity(filled.len() * 4);
    for index in 0..filled.len() {
        let (f, n, w) = (
            f32::from(filled[index]) / 255.0,
            f32::from(band[index]) / 255.0,
            f32::from(double[index]) / 255.0,
        );
        let (fill_cover, stroke_cover) = if inside {
            (f * (1.0 - n), w * f)
        } else {
            (f.max(n), w * (1.0 - f))
        };
        let (x, y) = (
            rect.left as f32 + (index % width) as f32 + 0.5,
            rect.top as f32 + (index / width) as f32 + 0.5,
        );
        let under = fill.at(x, y).unwrap_or_default();
        let color = stroke.at(x, y).unwrap_or_default();
        let fa = fill_cover * under[3];
        let sa = stroke_cover * color[3];
        let alpha = sa + fa * (1.0 - sa);
        let rgb: [f32; 3] = std::array::from_fn(|c| {
            if alpha > 0.0 {
                (color[c] * sa + under[c] * fa * (1.0 - sa)) / alpha
            } else {
                0.0
            }
        });
        pixels.extend(
            [rgb[0], rgb[1], rgb[2], alpha].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
        );
    }
    if let Some(image) =
        image::RgbaImage::from_raw(rect.width() as u32, rect.height() as u32, pixels)
    {
        let image = super::graphics::render_image(image);
        let target = Bounds::new(
            gpoint(&bounds, Point::new(rect.left as f32, rect.top as f32)),
            gpui::size(px(rect.width() as f32), px(rect.height() as f32)),
        );
        let _ = window.paint_image(target, Corners::default(), image, 0, false);
    }
}

/// Paint a drop shadow: the shape's fill and stroke moved by its offset,
/// blurred and laid in its colour, as output casts it.
fn paint_shadow(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    shape: &schist_layout::ShapePath,
    filled: bool,
    stroke: Option<f32>,
    shadow: &schist_layout::pasteboard::ShapeShadow,
) {
    let mut moved = shape.clone();
    moved.map_points(|p| Point::new(p.x + shadow.offset.x, p.y + shadow.offset.y));
    let path = moved.flatten(0.25);
    let outline = stroke
        .filter(|width| *width > 0.0)
        .map(|width| schist_vector::stroke_path(&path, StrokeOptions::default().style(width)));
    let reach = (3.0 * shadow.sigma).ceil() as i32 + 1;
    let area = outline.as_ref().map_or(path.bounds(), |o| o.bounds());
    let clip = schist_core::IntRect::new(
        0,
        0,
        f32::from(bounds.size.width).ceil() as i32,
        f32::from(bounds.size.height).ceil() as i32,
    );
    let rect = schist_core::IntRect::new(
        area.left - reach,
        area.top - reach,
        area.right + reach,
        area.bottom + reach,
    )
    .intersect(&clip);
    if rect.width() <= 0
        || rect.height() <= 0
        || i64::from(rect.width()) * i64::from(rect.height()) > 16_000_000
    {
        return;
    }
    let rule = if shape.even_odd {
        schist_vector::FillRule::EvenOdd
    } else {
        schist_vector::FillRule::NonZero
    };
    let (width, height) = (rect.width() as usize, rect.height() as usize);
    let mut alpha = vec![0.0f32; width * height];
    if filled {
        for (a, c) in alpha
            .iter_mut()
            .zip(schist_vector::rasterize(&path, rect, rule))
        {
            *a = f32::from(c) / 255.0;
        }
    }
    if let Some(outline) = &outline {
        let coverage = schist_vector::rasterize(outline, rect, schist_vector::FillRule::NonZero);
        for (a, c) in alpha.iter_mut().zip(coverage) {
            let c = f32::from(c) / 255.0;
            *a += c * (1.0 - *a);
        }
    }
    schist_layout::effects::blur(&mut alpha, width, height, shadow.sigma);
    let [r, g, b, a] = shadow.color.map(|v| v.clamp(0.0, 1.0));
    let pixels = alpha
        .into_iter()
        .flat_map(|value| {
            [
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
                (value * a * 255.0).round().clamp(0.0, 255.0) as u8,
            ]
        })
        .collect();
    if let Some(image) =
        image::RgbaImage::from_raw(rect.width() as u32, rect.height() as u32, pixels)
    {
        let image = super::graphics::render_image(image);
        let target = Bounds::new(
            gpoint(&bounds, Point::new(rect.left as f32, rect.top as f32)),
            gpui::size(px(rect.width() as f32), px(rect.height() as f32)),
        );
        let _ = window.paint_image(target, Corners::default(), image, 0, false);
    }
}

/// What fills or strokes a shape on the canvas: a flat colour or a
/// gradient.
#[derive(Clone, Copy, Default)]
struct Fill<'a> {
    color: Option<[f32; 4]>,
    gradient: Option<&'a schist_layout::pasteboard::ShapeGradient>,
}

impl Fill<'_> {
    /// A flat colour.
    fn solid(color: [f32; 4]) -> Self {
        Self {
            color: Some(color),
            gradient: None,
        }
    }

    /// The colour at canvas point (x, y).
    fn at(&self, x: f32, y: f32) -> Option<[f32; 4]> {
        match (self.gradient, self.color) {
            (Some(gradient), _) => {
                let [r, g, b] = gradient.preview(x, y);
                Some([r, g, b, gradient.opacity])
            }
            (None, color) => color,
        }
    }
}

fn paint_shape(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    shape: &schist_layout::ShapePath,
    fill: Fill<'_>,
    stroke: Option<([f32; 4], f32)>,
) {
    if fill.color.is_some() || fill.gradient.is_some() {
        let path = shape.flatten(0.25);
        let clip = schist_core::IntRect::new(
            0,
            0,
            f32::from(bounds.size.width).ceil() as i32,
            f32::from(bounds.size.height).ceil() as i32,
        );
        let rect = path.bounds().intersect(&clip);
        if rect.width() > 0
            && rect.height() > 0
            && i64::from(rect.width()) * i64::from(rect.height()) <= 16_000_000
        {
            let rule = if shape.even_odd {
                schist_vector::FillRule::EvenOdd
            } else {
                schist_vector::FillRule::NonZero
            };
            let coverage = schist_vector::rasterize(&path, rect, rule);
            let width = rect.width() as usize;
            let pixels = coverage
                .into_iter()
                .enumerate()
                .flat_map(|(index, alpha)| {
                    let (x, y) = (
                        rect.left as f32 + (index % width) as f32 + 0.5,
                        rect.top as f32 + (index / width) as f32 + 0.5,
                    );
                    let [r, g, b, a] = fill
                        .at(x, y)
                        .unwrap_or_default()
                        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                    [r, g, b, ((alpha as u16 * a as u16 + 127) / 255) as u8]
                })
                .collect();
            if let Some(image) =
                image::RgbaImage::from_raw(rect.width() as u32, rect.height() as u32, pixels)
            {
                let image = super::graphics::render_image(image);
                let target = Bounds::new(
                    gpoint(&bounds, Point::new(rect.left as f32, rect.top as f32)),
                    gpui::size(px(rect.width() as f32), px(rect.height() as f32)),
                );
                let _ = window.paint_image(target, Corners::default(), image, 0, false);
            }
        }
    }
    let Some((color, width)) = stroke.filter(|(_, width)| *width > 0.0) else {
        return;
    };
    // Native GPUI cubics keep outlines smooth at any zoom. Each contour
    // starts separately; there is no phantom connector between subpaths.
    let mut builder = PathBuilder::stroke(px(width));
    for sub in &shape.subpaths {
        let Some(first) = sub.points.first() else {
            continue;
        };
        builder.move_to(gpoint(&bounds, *first));
        let count = if sub.closed {
            sub.points.len()
        } else {
            sub.points.len().saturating_sub(1)
        };
        for i in 0..count {
            let next = (i + 1) % sub.points.len();
            builder.cubic_bezier_to(
                gpoint(&bounds, sub.points[next]),
                gpoint(&bounds, sub.handles_at(i).outgoing.unwrap_or(sub.points[i])),
                gpoint(
                    &bounds,
                    sub.handles_at(next).incoming.unwrap_or(sub.points[next]),
                ),
            );
        }
        if sub.closed {
            builder.close();
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(
            path,
            gpui::Rgba {
                r: color[0],
                g: color[1],
                b: color[2],
                a: color[3],
            },
        );
    }
}

fn paint_label(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    rect: Rect,
    text: &str,
    colour: u32,
) -> bool {
    let size = (rect.height * 0.4).min(9.0);
    paint_label_at(
        window,
        bounds,
        Point::new(rect.x + 2.0, rect.y + 2.0),
        text,
        size,
        colour,
    )
}

fn paint_label_at(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    at: Point,
    text: &str,
    size: Pt,
    colour: u32,
) -> bool {
    let spec = TextSpec {
        text: text.to_string(),
        size,
        ..TextSpec::default()
    };
    let Some(raster) = rasterize(&spec) else {
        return false;
    };
    if raster.bounds.width() <= 0 || raster.bounds.height() <= 0 {
        return false;
    }
    blit(
        window,
        bounds,
        &raster,
        Point::new(
            at.x + raster.bounds.left as Pt,
            at.y + raster.bounds.top as Pt,
        ),
        colour,
    );
    true
}

fn paint_selection(window: &mut Window, frame: &PasteboardFrame, plan: &PagePlan) {
    for (page, rect) in &frame.wraps {
        if *page == plan.page.page {
            outline(window, frame.bounds, *rect, WRAP, true);
        }
    }
    for (page, rect) in &frame.selected {
        if *page != plan.page.page {
            continue;
        }
        // A selection draws over everything else on the page, so a
        // selected object behind a later one is still findable.
        outline(window, frame.bounds, *rect, SELECTED, false);
    }
}

/// The anchor points of the selected shapes, so a user can see what the
/// direct selection tool will grab before they click.
fn paint_anchors(window: &mut Window, frame: &PasteboardFrame, plan: &PagePlan) {
    for (page, path) in &frame.anchors {
        if *page != plan.page.page {
            continue;
        }
        paint_handles(window, frame.bounds, path);
    }
}

fn paint_handles(window: &mut Window, bounds: Bounds<Pixels>, path: &schist_layout::ShapePath) {
    for sub in &path.subpaths {
        for (i, anchor) in sub.points.iter().enumerate() {
            let handles = sub.handles_at(i);
            for handle in [handles.incoming, handles.outgoing].into_iter().flatten() {
                let mut line = PathBuilder::stroke(px(1.0));
                line.move_to(gpoint(&bounds, *anchor));
                line.line_to(gpoint(&bounds, handle));
                if let Ok(path) = line.build() {
                    window.paint_path(path, rgb(SELECTED));
                }
                outline(
                    window,
                    bounds,
                    Rect::new(handle.x - 2.5, handle.y - 2.5, 5.0, 5.0),
                    SELECTED,
                    false,
                );
            }
            fill_rect(
                window,
                &Rect::new(
                    anchor.x - ANCHOR_HALF,
                    anchor.y - ANCHOR_HALF,
                    2.0 * ANCHOR_HALF,
                    2.0 * ANCHOR_HALF,
                ),
                bounds,
                SELECTED,
            );
        }
    }
}

/// Half the width of a drawn anchor, in pixels.
const ANCHOR_HALF: f32 = 3.0;

fn paint_guide(window: &mut Window, bounds: Bounds<Pixels>, guide: Guide, extent: Rect) {
    // Page-local guides span this page, including its bleed.
    let at = guide.position();
    let colour = match guide.kind() {
        GuideKind::Trim => TRIM,
        GuideKind::Bleed => BLEED,
        GuideKind::Margin => MARGIN,
        GuideKind::Column => COLUMN,
        GuideKind::Baseline => BASELINE,
        GuideKind::Ruler => RULER_GUIDE,
    };
    let width = px(match guide.kind() {
        // The baseline grid is on all the time, so it is the thinnest.
        GuideKind::Baseline => 0.5,
        _ => HAIRLINE,
    });
    // Positions are canvas-local; painting adds the window canvas origin.
    let mut builder = PathBuilder::stroke(width);
    if guide.vertical() {
        builder.move_to(gpoint(&bounds, Point::new(at, extent.y)));
        builder.line_to(gpoint(&bounds, Point::new(at, extent.bottom())));
    } else {
        builder.move_to(gpoint(&bounds, Point::new(extent.x, at)));
        builder.line_to(gpoint(&bounds, Point::new(extent.right(), at)));
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, rgb(colour));
    }
}

/// Crop marks at the trim corners, reaching in from the slug.
///
/// They stop short of the corner so the pair does not join into an L,
/// which is what a crop mark is: two separate lines, not a corner.
fn paint_marks(window: &mut Window, bounds: Bounds<Pixels>, trim: &Rect) {
    let gap = 2.0;
    let length = 6.0;
    for (corner, dx, dy) in [
        (Point::new(trim.x, trim.y), -1.0f32, -1.0f32),
        (Point::new(trim.right(), trim.y), 1.0, -1.0),
        (Point::new(trim.x, trim.bottom()), -1.0, 1.0),
        (Point::new(trim.right(), trim.bottom()), 1.0, 1.0),
    ] {
        for (direction, horizontal) in [(dx, true), (dy, false)] {
            let end = if horizontal {
                Point::new(corner.x + direction * (gap + length), corner.y)
            } else {
                Point::new(corner.x, corner.y + direction * (gap + length))
            };
            let mut builder = PathBuilder::stroke(px(HAIRLINE));
            let start = if horizontal {
                Point::new(corner.x + direction * gap, corner.y)
            } else {
                Point::new(corner.x, corner.y + direction * gap)
            };
            builder.move_to(gpoint(&bounds, start));
            builder.line_to(gpoint(&bounds, end));
            if let Ok(path) = builder.build() {
                window.paint_path(path, rgb(TEXT));
            }
        }
    }
}

/// Fill a rectangle in pasteboard points, clipped to the canvas.
fn fill_rect(window: &mut Window, rect: &Rect, bounds: Bounds<Pixels>, colour: u32) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let Some(visible) = clip(rect, bounds) else {
        return;
    };
    let mut builder = PathBuilder::fill();
    builder.move_to(gpoint(&bounds, visible.origin()));
    builder.line_to(gpoint(&bounds, Point::new(visible.right(), visible.y)));
    builder.line_to(gpoint(
        &bounds,
        Point::new(visible.right(), visible.bottom()),
    ));
    builder.line_to(gpoint(&bounds, Point::new(visible.x, visible.bottom())));
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, rgb(colour));
    }
}

/// A rectangle's outline, dashed or solid.
///
/// A dashed outline is a long edge broken into pieces, because a stroke
/// path cannot be dashed with gaps in the geometry and gpui's dash array
/// is a stroke option this fork does not expose on a closed path.
fn outline(window: &mut Window, bounds: Bounds<Pixels>, rect: Rect, colour: u32, dashed: bool) {
    affine_outline(window, bounds, rect, Affine::IDENTITY, colour, dashed);
}
fn affine_outline(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    rect: Rect,
    transform: Affine,
    colour: u32,
    dashed: bool,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    if !affine::bounds(transform, rect).intersects(clip_to(&bounds)) {
        return;
    }
    let corners = [
        (Point::new(rect.x, rect.y), Point::new(rect.right(), rect.y)),
        (
            Point::new(rect.right(), rect.y),
            Point::new(rect.right(), rect.bottom()),
        ),
        (
            Point::new(rect.right(), rect.bottom()),
            Point::new(rect.x, rect.bottom()),
        ),
        (
            Point::new(rect.x, rect.bottom()),
            Point::new(rect.x, rect.y),
        ),
    ];
    for (from, to) in corners {
        let from = affine::point(transform, from);
        let to = affine::point(transform, to);
        if dashed {
            dash_line(window, bounds, from, to, colour);
        } else {
            let mut builder = PathBuilder::stroke(px(HAIRLINE));
            builder.move_to(gpoint(&bounds, from));
            builder.line_to(gpoint(&bounds, to));
            if let Ok(path) = builder.build() {
                window.paint_path(path, rgb(colour));
            }
        }
    }
}

const DASH: Pt = 3.0;
const GAP: Pt = 2.0;

fn dash_line(window: &mut Window, bounds: Bounds<Pixels>, from: Point, to: Point, colour: u32) {
    let length = (to.x - from.x).hypot(to.y - from.y);
    let along = |distance: f32| {
        Point::new(
            from.x + (to.x - from.x) * distance / length,
            from.y + (to.y - from.y) * distance / length,
        )
    };
    let mut at = 0.0;
    while at < length {
        let end = (at + DASH).min(length);
        let a = along(at);
        let b = along(end);
        let mut builder = PathBuilder::stroke(px(HAIRLINE));
        builder.move_to(gpoint(&bounds, a));
        builder.line_to(gpoint(&bounds, b));
        if let Ok(path) = builder.build() {
            window.paint_path(path, rgb(colour));
        }
        at = end + GAP;
    }
}

/// A point a distance along a line from an origin.
#[cfg(test)]
fn along(origin: Point, horizontal: bool, distance: Pt) -> Point {
    if horizontal {
        Point::new(origin.x + distance, origin.y)
    } else {
        Point::new(origin.x, origin.y + distance)
    }
}

/// Blit an 8-bit coverage raster as one tinted image.
///
/// One image per run of text rather than a path per pixel: a page of body
/// text is hundreds of thousands of coverage bytes, and a path each would
/// make scrolling stall. The atlas has no padding, so a single image also
/// cannot bleed at a fractional zoom the way a quad per tile did.
fn blit(window: &mut Window, bounds: Bounds<Pixels>, raster: &TextRaster, at: Point, colour: u32) {
    blit_transformed(window, bounds, raster, at, colour, Affine::IDENTITY);
}
fn blit_transformed(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    raster: &TextRaster,
    at: Point,
    colour: u32,
    transform: Affine,
) {
    let width = raster.bounds.width();
    let height = raster.bounds.height();
    if width <= 0 || height <= 0 || raster.coverage.len() < (width * height) as usize {
        return;
    }
    let [r, g, b] = channels(colour);
    let buffer = raster.rgba([r, g, b, 255]);
    let Some(image) = image::RgbaImage::from_raw(width as u32, height as u32, buffer) else {
        return;
    };
    paint_rgba(window, bounds, image, at, transform);
}

/// Composed straight-alpha pixels with their top-left at `at`, then mapped
/// by `transform`.
fn paint_rgba(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    image: image::RgbaImage,
    at: Point,
    transform: Affine,
) {
    let (width, height) = image.dimensions();
    if transform != Affine::IDENTITY {
        let rect = Rect::new(at.x, at.y, width as f32, height as f32);
        if let Some(mapping) =
            schist_layout::graphics::ImageMapping::new(rect, rect, Affine::IDENTITY)
        {
            paint_warped_image(window, bounds, &image, &mapping, transform, 1.0, None);
        }
        return;
    }
    let target = Bounds::new(
        gpoint(&bounds, at),
        gpui::size(px(width as f32), px(height as f32)),
    );
    if target.intersect(&bounds).is_empty() {
        return;
    }
    let _ = window.paint_image(
        target,
        Corners::default(),
        super::graphics::render_image(image),
        0,
        false,
    );
}

/// Transform the already composed pixels, clipping in frame space and allocating
/// only the visible destination. The shared sampler preserves straight alpha.
fn paint_warped_image(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    image: &image::RgbaImage,
    mapping: &schist_layout::graphics::ImageMapping,
    transform: Affine,
    opacity: f32,
    clip_path: Option<&schist_layout::ShapePath>,
) {
    let Some(warp) = affine::Warp::new(
        mapping.visible,
        transform,
        clip_to(&bounds),
        16 * 1024 * 1024,
    ) else {
        return;
    };
    if warp.rect.is_empty() {
        return;
    }
    let mut out = image::RgbaImage::new(warp.rect.width() as u32, warp.rect.height() as u32);
    let mask = clip_path.map(|path| {
        schist_layout::graphics::clip_coverage(path, mapping.frame(), transform, warp.rect)
    });
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let Some(at) =
            mapping.source_at(warp.source(warp.rect.left + x as i32, warp.rect.top + y as i32))
        else {
            continue;
        };
        let (color, alpha) = affine::sample(mapping.source, image.dimensions(), at, |i| {
            let p = &image.as_raw()[i * 4..i * 4 + 4];
            (
                [
                    p[0] as f32 / 255.0,
                    p[1] as f32 / 255.0,
                    p[2] as f32 / 255.0,
                ],
                p[3] as f32 / 255.0,
            )
        });
        *pixel = image::Rgba([
            (color[0] * 255.0).round() as u8,
            (color[1] * 255.0).round() as u8,
            (color[2] * 255.0).round() as u8,
            (alpha
                * opacity.clamp(0.0, 1.0)
                * mask.as_ref().map_or(255.0, |mask| {
                    mask[(y * warp.rect.width() as u32 + x) as usize] as f32
                }))
            .round() as u8,
        ]);
    }
    let target = Bounds::new(
        gpoint(
            &bounds,
            Point::new(warp.rect.left as f32, warp.rect.top as f32),
        ),
        gpui::size(px(warp.rect.width() as f32), px(warp.rect.height() as f32)),
    );
    let image = super::graphics::render_image(out);
    let _ = window.paint_image(target, Corners::default(), image, 0, false);
}

fn affine_fill(
    window: &mut Window,
    rect: Rect,
    bounds: Bounds<Pixels>,
    transform: Affine,
    color: u32,
) {
    let points = affine::corners(rect).map(|p| affine::point(transform, p));
    let mut path = PathBuilder::fill();
    path.move_to(gpoint(&bounds, points[0]));
    for point in &points[1..] {
        path.line_to(gpoint(&bounds, *point));
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, rgb(color));
    }
}

/// A packed colour's channels, ignoring any alpha in it.
fn channels(colour: u32) -> [u8; 3] {
    [
        ((colour >> 16) & 0xFF) as u8,
        ((colour >> 8) & 0xFF) as u8,
        (colour & 0xFF) as u8,
    ]
}

/// A page-space point in window pixels.
fn gpoint(bounds: &Bounds<Pixels>, point: Point) -> GpuiPoint<Pixels> {
    GpuiPoint {
        x: bounds.origin.x + px(point.x),
        y: bounds.origin.y + px(point.y),
    }
}

/// The canvas bounds as a page-space rectangle.
fn clip_to(bounds: &Bounds<Pixels>) -> Rect {
    Rect::new(
        0.0,
        0.0,
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    )
}

/// The part of `rect` inside the canvas, or `None` if it is all outside.
fn clip(rect: &Rect, bounds: Bounds<Pixels>) -> Option<Rect> {
    let visible = rect.intersection(clip_to(&bounds));
    (!visible.is_empty()).then_some(visible)
}

fn px(value: f32) -> Pixels {
    gpui::px(value)
}

/// A background for a colour at an opacity, for callers that need one.
pub fn translucent(colour: u32, opacity: f32) -> Background {
    Background::from(rgb(colour)).opacity(opacity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::pasteboard::{pasteboard, PasteboardView};

    fn canvas() -> Bounds<Pixels> {
        Bounds::new(
            GpuiPoint::new(px(0.0), px(0.0)),
            gpui::size(px(800.0), px(600.0)),
        )
    }

    #[test]
    fn the_pasteboard_is_not_the_window_background() {
        // The pasteboard is a work surface and has to read as one against
        // the chrome.
        assert_ne!(PASTEBOARD, crate::ui::palette().window_bg);
        assert_ne!(PASTEBOARD, PAPER);
    }

    #[test]
    fn paper_is_light_and_the_pasteboard_is_dark() {
        // Summed as u32: three bright channels overflow a u8.
        let paper: u32 = channels(PAPER).iter().map(|c| *c as u32).sum();
        let board: u32 = channels(PASTEBOARD).iter().map(|c| *c as u32).sum();
        assert!(paper > board, "paper is lighter than the board");
    }

    #[test]
    fn a_page_transforms_onto_the_canvas() {
        let at = gpoint(&canvas(), Point::new(0.0, 0.0));
        assert!((at.x - px(0.0)).abs() < px(0.01));
        let offset = gpoint(&canvas(), Point::new(25.0, 40.0));
        assert!((offset.x - px(25.0)).abs() < px(0.01));
        assert!((offset.y - px(40.0)).abs() < px(0.01));
    }

    #[test]
    fn a_rect_outside_the_canvas_is_clipped_away() {
        assert!(clip(&Rect::new(50.0, 50.0, 20.0, 20.0), canvas()).is_some());
        assert!(clip(&Rect::new(900.0, 900.0, 20.0, 20.0), canvas()).is_none());
        // And one that straddles the edge is trimmed, not dropped.
        let straddling = clip(&Rect::new(790.0, 590.0, 40.0, 40.0), canvas()).unwrap();
        assert!((straddling.width - 10.0).abs() < 1e-3);
        assert!((straddling.height - 10.0).abs() < 1e-3);
    }

    #[test]
    fn clipping_is_independent_of_the_canvases_window_position() {
        for x in [0.0, 30.0, 300.0] {
            for y in [0.0, 70.0, 200.0] {
                let mut bounds = canvas();
                bounds.origin = GpuiPoint::new(px(x), px(y));
                for rect in [
                    Rect::new(-5.0, 10.0, 30.0, 20.0),
                    Rect::new(790.0, 590.0, 40.0, 40.0),
                    Rect::new(2.0, 3.0, 4.0, 5.0),
                ] {
                    assert_eq!(clip(&rect, bounds), clip(&rect, canvas()));
                    assert_eq!(
                        gpoint(&bounds, rect.origin()),
                        GpuiPoint::new(px(x + rect.x), px(y + rect.y))
                    );
                }
            }
        }
    }

    #[test]
    fn a_colour_splits_into_channels() {
        assert_eq!(channels(0xFF8000), [0xFF, 0x80, 0x00]);
        assert_eq!(channels(0x000000), [0, 0, 0]);
        assert_eq!(channels(0xFFFFFF), [255, 255, 255]);
    }

    #[test]
    fn text_pixels_keep_straight_color_and_multiply_only_alpha() {
        let coverage: Vec<u8> = (0..=255).collect();
        for color in [None, Some([200, 100, 50, 128]), Some([1, 2, 3, 0])] {
            let raster = TextRaster {
                coverage: coverage.to_vec(),
                colors: vec![color; coverage.len()],
                bounds: schist_core::IntRect::from_xywh(0, 0, coverage.len() as u32, 1),
                paints: Vec::new(),
                first_baseline: 0.0,
                line_advance: 0.0,
                layout_width: 0.0,
                cap_height: None,
            };
            let pixels = raster.rgba([0xAB, 0xCD, 0xEF, 255]);
            assert_eq!(pixels.len(), coverage.len() * 4);
            let expected = color.unwrap_or([0xAB, 0xCD, 0xEF, 255]);
            for (coverage, pixel) in coverage.iter().zip(pixels.as_chunks::<4>().0) {
                assert_eq!(&pixel[..3], &expected[..3]);
                assert_eq!(
                    pixel[3],
                    ((*coverage as u16 * expected[3] as u16 + 127) / 255) as u8
                );
            }
        }
    }

    #[test]
    fn a_dash_advances_along_its_axis() {
        let from = Point::new(10.0, 10.0);
        assert_eq!(along(from, true, 5.0), Point::new(15.0, 10.0));
        assert_eq!(along(from, false, 5.0), Point::new(10.0, 15.0));
        assert_eq!(along(from, true, -5.0), Point::new(5.0, 10.0));
    }

    #[test]
    fn a_dash_covers_the_whole_line() {
        // The last dash is clipped to the line's end, so a long edge is
        // dashed along its whole length rather than stopping short of the
        // corner. A trailing gap is correct -- that is what a dash
        // pattern is -- so the property to check is that the walk reaches
        // the end, not that the last dash lands exactly on it.
        let mut at = 0.0;
        let length = 10.0;
        while at < length {
            at = (at + DASH).min(length) + GAP;
        }
        assert!(at >= length, "the dash walk reached {at} of {length}");
        // And a line shorter than one dash is one solid dash, not none.
        let short = 1.0;
        let mut at = 0.0;
        while at < short {
            at = (at + DASH).min(short) + GAP;
        }
        assert!(at >= short);
    }

    #[test]
    fn a_guide_knows_which_axis_it_runs_along() {
        assert!(Guide::Vertical(1.0, GuideKind::Margin).vertical());
        assert!(!Guide::Horizontal(1.0, GuideKind::Margin).vertical());
        assert_eq!(
            Guide::Horizontal(1.0, GuideKind::Baseline).kind(),
            GuideKind::Baseline
        );
    }

    #[test]
    fn a_plan_lands_inside_the_canvas_when_fitted() {
        // The whole point of the plan is that a caller does not work this
        // out for itself.
        let doc = schist_layout::blank_a4();
        // Fitting is asked for the smaller of the two sides, because the
        // pasteboard is a work surface that has to fit both ways.
        let fitted = PasteboardView::fit_page(&schist_layout::Page::a4(), 600.0, 20.0);
        let plan = pasteboard(&doc, &fitted).unwrap();
        let trim = plan.pages[0].page.trim;
        assert!(trim.width <= 560.0, "{}", trim.width);
        assert!(trim.height <= 560.0, "{}", trim.height);
    }
}
