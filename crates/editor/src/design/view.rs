//! The Design viewport, independent of the raster editor's transform.

use super::{DesignState, PasteboardMode};
use schist_layout::{Point, Rect};

impl DesignState {
    /// Fit the active page, or its spread, without changing view toggles.
    pub fn fit_view(&mut self, width: f32, height: f32) {
        let page = self.current_page();
        let Some(origin) = self.document.page_origin(page) else {
            return;
        };
        let definition = &self.document.pages[page];
        let area = match self.mode {
            PasteboardMode::SinglePage => definition.bleed_rect().translated(origin),
            PasteboardMode::Spread => {
                let Some(spread) = self.document.spread_containing(page) else {
                    return;
                };
                let pages: Vec<_> = spread
                    .pages
                    .iter()
                    .filter_map(|page| {
                        Some(
                            self.document
                                .pages
                                .get(*page)?
                                .bleed_rect()
                                .translated(self.document.page_origin(*page)?),
                        )
                    })
                    .collect();
                let Some(first) = pages.first() else {
                    return;
                };
                pages
                    .iter()
                    .skip(1)
                    .fold(*first, |area, page| area.union(*page))
            }
        };
        self.fit_area(area, width, height);
        self.needs_refit = false;
    }

    fn fit_area(&mut self, area: Rect, width: f32, height: f32) {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return;
        }
        let scale = ((width - 80.0).max(1.0) / area.width.max(1.0))
            .min((height - 80.0).max(1.0) / area.height.max(1.0))
            .clamp(0.005, 32.0);
        self.view.scale = scale;
        self.view.origin = Point::new(
            (width - area.width * scale) / 2.0 - area.x * scale,
            (height - area.height * scale) / 2.0 - area.y * scale,
        );
    }

    /// Keep the page point under the canvas-local pivot fixed.
    pub fn zoom_view(&mut self, factor: f32, pivot: Point) {
        if !factor.is_finite()
            || factor <= 0.0
            || !self.view.scale.is_finite()
            || self.view.scale <= 0.0
        {
            return;
        }
        let new = (self.view.scale * factor).clamp(0.005, 32.0);
        let ratio = new / self.view.scale;
        self.view.origin = Point::new(
            pivot.x - (pivot.x - self.view.origin.x) * ratio,
            pivot.y - (pivot.y - self.view.origin.y) * ratio,
        );
        self.view.scale = new;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::{Page, Spread};

    #[test]
    fn every_page_fits_and_its_ruler_zero_matches_its_painted_trim() {
        let mut state = DesignState::new();
        state.document.add_page(Page::a4());
        state.document.add_page(Page::a4());
        state.document.spreads = vec![
            Spread {
                pages: vec![0, 1],
                ..Spread::single(0)
            },
            Spread::single(2),
        ];
        // Nonzero stored hints must not move objects independently of
        // their paper; canonical spread placement is computed.
        state.document.spreads[1].origin = Point::new(9000.0, 3000.0);
        let ids: Vec<_> = (0..3)
            .map(|page| {
                schist_layout::authoring::rectangle(
                    &mut state.document,
                    &mut state.history,
                    page,
                    Rect::new(10.0, 20.0, 60.0, 80.0),
                    Default::default(),
                )
                .unwrap()
            })
            .collect();
        for mode in [PasteboardMode::SinglePage, PasteboardMode::Spread] {
            state.mode = mode;
            for (page, id) in ids.iter().enumerate() {
                state.page = Some(page);
                state.fit_view(900.0, 600.0);
                let plan = state.plan().unwrap();
                let trim = plan
                    .pages
                    .iter()
                    .find(|p| p.page.page == page)
                    .unwrap()
                    .page
                    .trim;
                let zero = state.to_pasteboard(Point::ZERO);
                assert!((zero.x - trim.x).abs() < 0.001);
                assert!((zero.y - trim.y).abs() < 0.001);
                assert!(trim.x >= 39.9 && trim.y >= 39.9);
                assert!(trim.x + trim.width <= 860.1);
                assert!(trim.y + trim.height <= 560.1);
                if mode == PasteboardMode::SinglePage {
                    assert_eq!(plan.pages.len(), 1);
                }
                let point = Point::new(15.0, 35.0);
                assert_eq!(
                    crate::design::select::hit_test(&plan, state.to_pasteboard(point)).object(),
                    Some(*id)
                );
                let object = state.view.rect(state.document.object_rect(*id).unwrap());
                assert!((object.x - (trim.x + 10.0 * state.view.scale)).abs() < 0.001);
                assert!((object.y - (trim.y + 20.0 * state.view.scale)).abs() < 0.001);
                let page_plan = plan.pages.iter().find(|p| p.page.page == page).unwrap();
                assert!(page_plan.guides.iter().any(|guide| matches!(guide,
                    schist_layout::Guide::Vertical(x, schist_layout::pasteboard::GuideKind::Trim) if (*x - trim.x).abs() < 0.001)));
                let back = state.to_page(state.to_pasteboard(point));
                assert!((point.x - back.x).abs() < 0.001 && (point.y - back.y).abs() < 0.001);
            }
        }
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer_fixed() {
        for factor in [0.1, 0.8, 1.25, 10.0, 1000.0] {
            let mut state = DesignState::new();
            state.view.origin = Point::new(-132.0, 71.5);
            state.view.scale = 0.75;
            let pivot = Point::new(317.0, 193.0);
            let before = state.to_page(pivot);
            state.zoom_view(factor, pivot);
            let after = state.to_page(pivot);
            assert!((before.x - after.x).abs() < 0.01 && (before.y - after.y).abs() < 0.01);
        }
    }
}
