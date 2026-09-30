//! Output settings and the pixel geometry derived from them.
//!
//! Layout lives in points; separation happens in pixels. Everything in
//! this crate works in a **page pixel grid** whose size comes from the
//! output resolution, and converts through exactly one place:
//! [`OutputSettings::to_pixels`]. Getting that conversion subtly wrong is
//! how a plate ends up half a pixel out of register with its neighbour,
//! which a press operator notices immediately.

use schist_layout::geometry::Pt;
use schist_layout::Pt as LayoutPt;

/// Pixels per inch used to resolve a page's bleed box when a document
/// has no bleed of its own.
pub const DEFAULT_RESOLUTION_DPI: f32 = 300.0;

/// How a page is turned into plates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputSettings {
    /// Plate resolution. 300 is normal for coated stock; 240 for newsprint.
    pub resolution_dpi: f32,
    /// Include the bleed area in the output. A page destined for a press
    /// must; a page destined for a screen must not.
    pub include_bleed: bool,
    /// Composite the result to a flat preview as well as separating.
    pub include_composite: bool,
}

impl Default for OutputSettings {
    fn default() -> Self {
        OutputSettings {
            resolution_dpi: DEFAULT_RESOLUTION_DPI,
            include_bleed: true,
            include_composite: true,
        }
    }
}

impl OutputSettings {
    pub fn at(dpi: f32) -> OutputSettings {
        OutputSettings {
            resolution_dpi: dpi.clamp(36.0, 1200.0),
            ..OutputSettings::default()
        }
    }

    /// Pixels per point.
    pub fn scale(self) -> f32 {
        self.resolution_dpi / 72.0
    }

    /// A point length in pixels, rounded to the nearest whole pixel.
    ///
    /// Boundaries are what get measured, so they round; interior
    /// coordinates are derived from the rounded boundary to avoid a
    /// rectangle that is one pixel wider than it should be.
    pub fn to_pixels(self, points: Pt) -> i32 {
        (points * self.scale()).round() as i32
    }

    /// A page-space rectangle in pixels, with right and bottom derived
    /// from the left/top so the width is exactly what the settings imply.
    pub fn to_pixels_rect(self, rect: schist_layout::Rect) -> schist_core::IntRect {
        let left = self.to_pixels(rect.x);
        let top = self.to_pixels(rect.y);
        schist_core::IntRect::new(
            left,
            top,
            left + self.to_pixels(rect.width).max(0),
            top + self.to_pixels(rect.height).max(0),
        )
    }

    /// The output box for a page: trim, or trim plus bleed.
    pub fn output_box(self, page: &schist_layout::Page) -> schist_layout::Rect {
        if self.include_bleed {
            page.bleed_rect()
        } else {
            schist_layout::Rect::new(0.0, 0.0, page.width, page.height)
        }
    }
}

/// A point position relative to a page's top-left, in pixels.
///
/// Zero is the trim origin. Including bleed expands the output rectangle
/// into negative coordinates; it never translates the artwork. PDF output
/// offsets the complete plate by the bleed when placing it on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagePixel {
    pub x: i32,
    pub y: i32,
}

impl PagePixel {
    /// The page's own origin in the output grid.
    ///
    /// Pages are laid out from the trim edge, so with bleed the paper
    /// starts above and left of the trim and the origin is negative.
    pub fn origin(settings: OutputSettings, page: &schist_layout::Page) -> PagePixel {
        let bleed = if settings.include_bleed {
            page.bleed
        } else {
            0.0
        };
        PagePixel {
            x: -settings.to_pixels(bleed),
            y: -settings.to_pixels(bleed),
        }
    }

    /// A page-space point in output pixels.
    pub fn of(
        settings: OutputSettings,
        _page: &schist_layout::Page,
        point: schist_layout::Point,
    ) -> PagePixel {
        PagePixel {
            x: settings.to_pixels(point.x),
            y: settings.to_pixels(point.y),
        }
    }

    /// A page-space rectangle in output pixels.
    pub fn rect(
        settings: OutputSettings,
        _page: &schist_layout::Page,
        rect: schist_layout::Rect,
    ) -> schist_core::IntRect {
        let left = settings.to_pixels(rect.x);
        let top = settings.to_pixels(rect.y);
        schist_core::IntRect::new(
            left,
            top,
            left + settings.to_pixels(rect.width).max(0),
            top + settings.to_pixels(rect.height).max(0),
        )
    }
}

/// A rectangle in points, used when a caller needs to hand geometry back
/// to the layout rather than to the rasteriser.
pub type PtRect = schist_layout::Rect;

/// Keep the type alias honest for callers that pass a bare scalar.
pub const fn points(value: LayoutPt) -> LayoutPt {
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::geometry::{mm, Rect};
    use schist_layout::Page;

    #[test]
    fn three_hundred_dpi_is_300_over_72() {
        let s = OutputSettings::at(300.0);
        assert!((s.scale() - 300.0 / 72.0).abs() < 1e-6);
        // A4 is 210mm wide, which is 595.276pt, which at 300dpi is 2480.
        let a4 = Page::a4();
        let px = s.to_pixels(a4.width);
        assert!((px - 2480).abs() <= 1, "got {px}");
    }

    #[test]
    fn a_rect_derived_from_its_origin_keeps_the_implied_width() {
        let s = OutputSettings::at(300.0);
        let r = s.to_pixels_rect(Rect::new(mm(10.0), mm(20.0), mm(100.0), mm(50.0)));
        // The right edge must agree with the origin and the width, or a
        // plate is registered against the wrong neighbour. It can differ
        // from rounding the far edge on its own by a pixel, which is why
        // the two are not compared directly.
        assert_eq!(r.left, s.to_pixels(mm(10.0)));
        assert_eq!(r.width(), s.to_pixels(mm(100.0)));
        assert_eq!(r.height(), s.to_pixels(mm(50.0)));
        // And it lands within a pixel of the naive far edge.
        let naive = s.to_pixels(mm(110.0));
        assert!((r.right - naive).abs() <= 1, "{} vs {naive}", r.right);
    }

    #[test]
    fn the_bleed_origin_is_negative() {
        let mut page = Page::a4();
        page.bleed = mm(3.0);
        let s = OutputSettings::at(300.0);
        let origin = PagePixel::origin(s, &page);
        // Paper starts above and left of the trim.
        assert!(origin.x < 0 && origin.y < 0);
        let s2 = OutputSettings {
            include_bleed: false,
            ..s
        };
        let origin2 = PagePixel::origin(s2, &page);
        assert_eq!(origin2, PagePixel { x: 0, y: 0 });
    }

    #[test]
    fn the_bleed_output_box_is_larger_than_the_trim_box() {
        let mut page = Page::a4();
        page.bleed = mm(3.0);
        let with = OutputSettings::at(300.0).output_box(&page);
        let without = OutputSettings {
            include_bleed: false,
            ..OutputSettings::at(300.0)
        }
        .output_box(&page);
        assert!(with.width > without.width);
        assert!(with.height > without.height);
    }

    #[test]
    fn a_resolution_outside_the_printable_range_is_clamped() {
        // 10 dpi would produce plates too small to hold anything and is
        // always a mistake rather than an intent.
        assert_eq!(OutputSettings::at(1.0).resolution_dpi, 36.0);
        assert_eq!(OutputSettings::at(100_000.0).resolution_dpi, 1200.0);
    }
}
