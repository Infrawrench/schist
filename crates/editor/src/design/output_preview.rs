//! Output preview: each page as it separates, drawn on the canvas in place of
//! the canvas's own painting, so blend modes, shadows cast by text and
//! images, overprinting and every effect output draws show as they print.
//! The canvas otherwise paints items one by one and cannot blend them with
//! what lies beneath.
use std::sync::Arc;

use schist_layout::{pasteboard::Pasteboard, LayoutDocument, Rect};
use schist_separation::{cmyk_to_rgb, separate_page, GraphicSource, OutputSettings};

/// The previews last drawn, kept while the document, zoom and fonts stay put.
#[derive(Default)]
pub(super) struct PreviewCache {
    document: Option<LayoutDocument>,
    resolution: f32,
    fonts: usize,
    pages: Vec<(usize, Arc<gpui::RenderImage>)>,
}

impl super::DesignState {
    /// Each page of `plan` as it separates, with the bleed rectangle it
    /// covers, rendered a pixel to the canvas unit.
    pub fn output_previews(&self, plan: &Pasteboard) -> Vec<(Rect, Arc<gpui::RenderImage>)> {
        let resolution = (72.0 * self.view.scale).clamp(36.0, 600.0);
        let fonts = schist_text_engine::font_revision();
        let mut cache = self.preview_cache.borrow_mut();
        if cache.document.as_ref() != Some(&self.document)
            || cache.resolution != resolution
            || cache.fonts != fonts
        {
            *cache = PreviewCache {
                document: Some(self.document.clone()),
                resolution,
                fonts,
                pages: Vec::new(),
            };
        }
        plan.pages
            .iter()
            .filter_map(|page| {
                let index = page.page.page;
                let image = match cache.pages.iter().find(|(i, _)| *i == index) {
                    Some((_, image)) => image.clone(),
                    None => {
                        let pixels = render(&self.document, index, resolution, &*self.graphics)?;
                        let image = super::graphics::render_image(pixels);
                        cache.pages.push((index, image.clone()));
                        image
                    }
                };
                Some((page.page.bleed, image))
            })
            .collect()
    }
}

/// Page `page` of `doc` as it separates, bleed included, its composite inks
/// shown as the separations preview shows them.
pub(super) fn render(
    doc: &LayoutDocument,
    page: usize,
    resolution: f32,
    graphics: &dyn GraphicSource,
) -> Option<image::RgbaImage> {
    let mut settings = OutputSettings::at(resolution);
    settings.include_bleed = true;
    settings.include_composite = true;
    let separated = separate_page(doc, page, settings, graphics)?;
    let rect = separated.separation.rect();
    let (width, height) = (rect.width().max(0) as u32, rect.height().max(0) as u32);
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 40_000_000 {
        return None;
    }
    let composite = separated.separation.composite();
    let mut pixels = image::RgbaImage::new(width, height);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let rgb = cmyk_to_rgb(composite.at(rect.left + x as i32, rect.top + y as i32));
        let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        *pixel = image::Rgba([channel(rgb[0]), channel(rgb[1]), channel(rgb[2]), 255]);
    }
    Some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::{
        authoring, effects::BlendMode, History, Ink, LayoutObject, Page, PaintTints,
    };

    fn rectangle(doc: &mut LayoutDocument, area: Rect, ink: Ink) -> schist_layout::ObjectId {
        let id = authoring::shape(
            doc,
            &mut History::default(),
            0,
            area,
            authoring::ShapeKind::Rectangle,
            authoring::Paint::none(),
        )
        .unwrap();
        let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
        let LayoutObject::Shape { path, .. } = &object.object else {
            unreachable!()
        };
        object.object = LayoutObject::Shape {
            path: path.clone(),
            fill: Some(ink),
            stroke: None,
            stroke_width: 0.0,
            fill_overprint: false,
            stroke_overprint: false,
            tints: PaintTints::default(),
        };
        id
    }

    #[test]
    fn the_preview_shows_what_the_canvas_cannot() {
        let mut doc = LayoutDocument::new(vec![Page::new("1", 200.0, 100.0)]);
        rectangle(
            &mut doc,
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]),
        );
        let top = rectangle(
            &mut doc,
            Rect::new(50.0, 25.0, 100.0, 50.0),
            Ink::cmyk("Magenta", [0.0, 1.0, 0.0, 0.0]),
        );
        let pixel = |doc: &LayoutDocument| {
            let image = render(doc, 0, 72.0, &schist_separation::NoGraphics).unwrap();
            assert_eq!((image.width(), image.height()), (200, 100));
            image.get_pixel(100, 50).0
        };
        // Normal: magenta knocks the cyan out.
        assert_eq!(pixel(&doc), [255, 0, 255, 255]);
        // Multiply keeps the cyan beneath: blue, as it prints.
        doc.objects
            .iter_mut()
            .find(|o| o.id == top)
            .unwrap()
            .appearance
            .blend_mode = Some(BlendMode::Multiply);
        assert_eq!(pixel(&doc), [0, 0, 255, 255]);
    }
}
