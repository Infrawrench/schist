//! Linked graphic geometry and reversible placement, independent of decoding.
use crate::affine::{self, Affine};
use crate::{
    authoring, snapshot_object, structure, GraphicFit, History, LayoutDocument, LayoutEdit,
    LayoutObject, Link, ObjectId, Page, Rect,
};

/// Convert a normalized inner image transform to the frame's coordinates.
/// The outer object transform applies only after the frame has clipped it.
pub fn image_affine(frame: Rect, normalized: Affine) -> Option<Affine> {
    if ![frame.x, frame.y, frame.width, frame.height]
        .iter()
        .all(|n| n.is_finite())
        || frame.width <= 0.0
        || frame.height <= 0.0
        || !affine::finite(normalized)
    {
        return None;
    }
    if normalized == Affine::IDENTITY {
        return Some(normalized);
    }
    let matrix = Affine::translate(frame.x, frame.y)
        .then(&Affine::scale(frame.width, frame.height))
        .then(&normalized)
        .then(&Affine::scale(1.0 / frame.width, 1.0 / frame.height))
        .then(&Affine::translate(-frame.x, -frame.y));
    affine::finite(matrix).then_some(matrix)
}

/// Shared preview/print mapping. Fitting and the inner image transform move
/// the source pixel grid; the frame alone clips it, as in native IDML.
pub struct ImageMapping {
    pub source: Rect,
    pub visible: Rect,
    frame: Rect,
    inverse: Affine,
}

impl ImageMapping {
    pub fn frame(&self) -> Rect {
        self.frame
    }
    pub fn new(frame: Rect, source: Rect, normalized: Affine) -> Option<Self> {
        let matrix = image_affine(frame, normalized)?;
        let inverse = affine::inverse(matrix)?;
        if ![source.x, source.y, source.width, source.height]
            .iter()
            .all(|v| v.is_finite())
            || source.width <= 0.0
            || source.height <= 0.0
        {
            return None;
        }
        let visible = affine::bounds(matrix, source).intersection(frame);
        Some(Self {
            source,
            visible,
            frame,
            inverse,
        })
    }

    /// Inverse-map a point within the frame onto the fitted source pixel grid.
    pub fn source_at(&self, at: crate::Point) -> Option<crate::Point> {
        if !self.frame.contains(at) {
            return None;
        }
        let at = affine::point(self.inverse, at);
        ((self.source.x..self.source.right()).contains(&at.x)
            && (self.source.y..self.source.bottom()).contains(&at.y))
        .then_some(at)
    }
}

/// Antialiased frame clipping in the destination pixel grid. Normalized cubic
/// geometry survives resizing; flatten only after the complete outer affine.
/// Preview and print multiply this coverage into the image alpha, preserving
/// the source colours and native CMYK channels.
pub fn clip_coverage(
    shape: &crate::ShapePath,
    frame: Rect,
    transform: Affine,
    output: schist_core::IntRect,
) -> Vec<u8> {
    let mut shape = shape.clone();
    shape.subpaths.retain(|sub| sub.closed);
    shape.map_points(|point| {
        affine::point(
            transform,
            crate::Point::new(
                frame.x + point.x * frame.width,
                frame.y + point.y * frame.height,
            ),
        )
    });
    let rule = if shape.even_odd {
        schist_vector::FillRule::EvenOdd
    } else {
        schist_vector::FillRule::NonZero
    };
    schist_vector::rasterize(&shape.flatten(0.25), output, rule)
}

/// The rectangle occupied by the complete source image. The frame clips it;
/// a normalized crop selects the source portion fitted into that frame.
pub fn image_rect(
    frame: Rect,
    pixels: (u32, u32),
    dpi: f32,
    crop: Option<Rect>,
    fit: GraphicFit,
    scale: f32,
) -> Option<Rect> {
    let crop = crop.unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0));
    if ![
        frame.x,
        frame.y,
        frame.width,
        frame.height,
        crop.x,
        crop.y,
        crop.width,
        crop.height,
        dpi,
        scale,
    ]
    .iter()
    .all(|v| v.is_finite())
        || frame.width <= 0.0
        || frame.height <= 0.0
        || pixels.0 == 0
        || pixels.1 == 0
        || dpi <= 0.0
        || scale <= 0.0
        || crop.width <= 0.0
        || crop.height <= 0.0
    {
        return None;
    }
    let width = pixels.0 as f32 * 72.0 / dpi;
    let height = pixels.1 as f32 * 72.0 / dpi;
    let sx = frame.width / (width * crop.width);
    let sy = frame.height / (height * crop.height);
    let (sx, sy) = match fit {
        GraphicFit::Fill => (sx.max(sy), sx.max(sy)),
        GraphicFit::Contain => (sx.min(sy), sx.min(sy)),
        GraphicFit::Original => (1.0, 1.0),
        GraphicFit::Stretch => (sx, sy),
    };
    let (width, height) = (width * sx * scale, height * sy * scale);
    Some(Rect::new(
        frame.x + (frame.width - width * crop.width) / 2.0 - crop.x * width,
        frame.y + (frame.height - height * crop.height) / 2.0 - crop.y * height,
        width,
        height,
    ))
}

/// Relinking changes the source and preserves the frame and crop exactly.
pub fn relink(doc: &mut LayoutDocument, history: &mut History, id: ObjectId, source: Link) -> bool {
    if doc.object_locked(id) {
        return false;
    }
    let Some(before) = doc.object(id) else {
        return false;
    };
    let mut after = before.clone();
    let LayoutObject::GraphicFrame { link, embedded, .. } = &mut after.object else {
        return false;
    };
    *link = source;
    *embedded = false;
    if &after == before {
        return false;
    }
    history.apply(
        doc,
        LayoutEdit::ObjectChanged {
            id: id.0,
            before: snapshot_object(before),
            after: snapshot_object(&after),
        },
    )
}

pub struct GraphicPage {
    pub source: Link,
    pub name: String,
    pub width: f32,
    pub height: f32,
}

/// Append one page per source in a single transaction. All pages are prepared
/// on a copy so a failed frame (for example, all layers locked) adds nothing.
pub fn import_pages(
    doc: &mut LayoutDocument,
    history: &mut History,
    sources: &[GraphicPage],
) -> bool {
    if sources.is_empty()
        || sources.iter().any(|s| {
            !s.width.is_finite() || !s.height.is_finite() || s.width <= 0.0 || s.height <= 0.0
        })
    {
        return false;
    }
    let mut working = doc.clone();
    let mut changes = History::with_limit(sources.len().saturating_mul(4));
    for source in sources {
        let index = working.pages.len();
        let page = Page::new(&source.name, source.width, source.height);
        if !structure::add_page(&mut working, &mut changes, index, page) {
            return false;
        }
        let Some(id) = authoring::graphic_frame_with_link(
            &mut working,
            &mut changes,
            index,
            Rect::new(0.0, 0.0, source.width, source.height),
            source.source.clone(),
            false,
        ) else {
            return false;
        };
        let _ = id;
    }
    history.apply(
        doc,
        LayoutEdit::Batch {
            edits: changes.pending().to_vec(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fitting_preserves_aspect_ratio_and_original_uses_resolution() {
        for (w, h) in [(100, 200), (200, 100), (150, 150)] {
            let frame = Rect::new(20.0, 30.0, 80.0, 60.0);
            for fit in [GraphicFit::Fill, GraphicFit::Contain, GraphicFit::Original] {
                let rect = image_rect(frame, (w, h), 144.0, None, fit, 1.0).unwrap();
                assert!((rect.width / rect.height - w as f32 / h as f32).abs() < 0.0001);
                assert!((rect.x + rect.width / 2.0 - 60.0).abs() < 0.0001);
                assert!((rect.y + rect.height / 2.0 - 60.0).abs() < 0.0001);
                match fit {
                    GraphicFit::Fill => assert!(
                        rect.width + 0.0001 >= frame.width && rect.height + 0.0001 >= frame.height
                    ),
                    GraphicFit::Contain => assert!(
                        rect.width <= frame.width + 0.0001 && rect.height <= frame.height + 0.0001
                    ),
                    GraphicFit::Original => {
                        assert_eq!((rect.width, rect.height), (w as f32 / 2.0, h as f32 / 2.0))
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    #[test]
    fn every_page_import_is_one_step_and_restores_exactly() {
        for count in 1..12 {
            let mut doc = crate::blank_a4();
            let before = doc.clone();
            let mut history = History::default();
            let sources: Vec<_> = (0..count)
                .map(|i| GraphicPage {
                    source: Link::new(format!("{i}.psd")),
                    name: i.to_string(),
                    width: 200.0 + i as f32,
                    height: 300.0,
                })
                .collect();
            assert!(import_pages(&mut doc, &mut history, &sources));
            assert_eq!(doc.pages.len(), count + 1);
            assert_eq!(history.undo_depth(), 1);
            let after = doc.clone();
            for _ in 0..3 {
                assert!(history.undo(&mut doc));
                assert_eq!(doc, before);
                assert!(history.redo(&mut doc));
                assert_eq!(doc, after);
            }
        }
    }
}
