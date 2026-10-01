//! A separations preview: what a page will look like ink by ink.
//!
//! `schist_core::ink::InkPreview` has a `Separation` mode, and it is
//! deliberately crude: it replaces the pixel with greyscale, where
//! black is full ink. That is the right answer for a *painted* plate,
//! where the composite is already there and one channel is being
//! isolated. It is the wrong answer for a page layout, where there is no
//! composite to isolate and the question is "which inks does this page
//! use, and where".
//!
//! This is that view. It is for the panel a prepress provider opens, and
//! it is diagnostic rather than a proof: it shows which plates carry ink,
//! which carry a tint, and which carry nothing at all, which is how you
//! find the object that was accidentally set in a 60% tint of magenta.

use schist_core::IntRect;

use crate::coverage::{PlateCoverage, Separation};
use crate::plan::{Plate, PlateKind, PlatePlan};

/// How one plate is drawn in the preview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlateView {
    /// A flat field of the plate's own ink.
    Ink,
    /// Ink drawn over the composite, so an overprinted plate can be seen
    /// in the context of the colours under it.
    Composite,
}

/// What the preview shows about one plate.
#[derive(Debug, Clone, PartialEq)]
pub struct PlateSummary {
    pub name: String,
    pub kind: PlateKind,
    /// The plate's ink colour, for a preview.
    pub rgb: [f32; 3],
    /// The greatest coverage anywhere on the plate.
    pub peak: f32,
    /// The mean coverage over the plate's area.
    pub mean: f32,
    /// Whether the plate carries any ink at all.
    pub has_ink: bool,
    /// The proportion of the plate's area that is inked above 50%. A
    /// plate with a low peak and a high solid fraction is fine detail,
    /// and a plate with a high peak and a low fraction is a large flat
    /// tint.
    pub solid_fraction: f32,
}

impl PlateSummary {
    /// How a prepress provider would describe the plate.
    ///
    /// Not translated here: these are the words a print buyer uses in a
    /// specification, and translating them would make the panel harder
    /// to use for exactly the person who needs it. The editor supplies
    /// the localised form.
    ///
    /// This is a coarse summary by area, and it does not capture how the
    /// ink is *arranged*: a page of fine line work and a page of flat 4%
    /// wash can share a mean, and nothing in the numbers here tells them
    /// apart. A panel shows the figures alongside the word, and the
    /// figure is what a prepress provider actually reads.
    pub fn describe(&self) -> &'static str {
        if !self.has_ink {
            return "empty";
        }
        if self.peak < 0.02 {
            // The ink is there but so thin a press would not hold it.
            return "faint";
        }
        if self.solid_fraction > 0.5 {
            return "solid";
        }
        if self.mean > 0.5 {
            return "flood";
        }
        if self.mean > 0.02 {
            return "tint";
        }
        "detail"
    }
}

/// A preview of one page's separations.
pub struct SeparationsPreview<'a> {
    separation: &'a Separation,
    plan: &'a PlatePlan,
    view: PlateView,
    rect: IntRect,
}

impl<'a> SeparationsPreview<'a> {
    pub fn new(separation: &'a Separation, plan: &'a PlatePlan) -> SeparationsPreview<'a> {
        SeparationsPreview {
            rect: separation.rect(),
            separation,
            plan,
            view: PlateView::Ink,
        }
    }

    pub fn over_composite(mut self) -> Self {
        self.view = PlateView::Composite;
        self
    }

    pub fn rect(&self) -> IntRect {
        self.rect
    }

    /// Draw one plate into an RGBA8 buffer over `rect`.
    ///
    /// The buffer is the intersection of the preview's rect and the
    /// caller's, so a panel showing part of a page does not have to ask
    /// for the whole thing.
    pub fn draw(&self, plate: usize, target: &IntRect, rgba: &mut [u8]) {
        let Some(coverage) = self.separation.plate(plate) else {
            return;
        };
        let Some(entry) = self.plan.plates.get(plate) else {
            return;
        };
        let clip = target.intersect(&self.rect);
        if clip.is_empty() {
            return;
        }
        let composite = self.separation.composite();
        let ink = entry.preview_rgb;
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                let slot = &mut rgba
                    [((y - target.top) * target.width() + (x - target.left)) as usize * 4..][..4];
                let value = coverage.at(x, y).clamp(0.0, 1.0);
                let (r, g, b) = match self.view {
                    // The plate's own ink over white paper. A tinted ink
                    // is lighter than a solid one, which is what makes a
                    // tint readable as a tint.
                    PlateView::Ink => {
                        let base = 1.0 - value;
                        (
                            base * (1.0 - ink[0]) + ink[0] * value,
                            base * (1.0 - ink[1]) + ink[1] * value,
                            base * (1.0 - ink[2]) + ink[2] * value,
                        )
                    }
                    // The plate's ink laid over the composite, so an
                    // overprinted plate reads in context.
                    PlateView::Composite => {
                        let under = composite.at(x, y);
                        let base = cmyk_to_rgb(under);
                        let v = value.clamp(0.0, 1.0);
                        (
                            base[0] * (1.0 - v) + ink[0] * v,
                            base[1] * (1.0 - v) + ink[1] * v,
                            base[2] * (1.0 - v) + ink[2] * v,
                        )
                    }
                };
                slot[0] = (r.clamp(0.0, 1.0) * 255.0).round() as u8;
                slot[1] = (g.clamp(0.0, 1.0) * 255.0).round() as u8;
                slot[2] = (b.clamp(0.0, 1.0) * 255.0).round() as u8;
                slot[3] = 255;
            }
        }
    }

    /// Draw the whole composite.
    pub fn draw_composite(&self, target: &IntRect, rgba: &mut [u8]) {
        let clip = target.intersect(&self.rect);
        if clip.is_empty() {
            return;
        }
        let composite = self.separation.composite();
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                let slot = &mut rgba
                    [((y - target.top) * target.width() + (x - target.left)) as usize * 4..][..4];
                let rgb = cmyk_to_rgb(composite.at(x, y));
                slot[0] = (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8;
                slot[1] = (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8;
                slot[2] = (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8;
                slot[3] = 255;
            }
        }
    }

    /// What each plate is doing, for a panel listing them.
    pub fn summaries(&self) -> Vec<PlateSummary> {
        self.plan
            .plates
            .iter()
            .enumerate()
            .map(|(index, plate)| summarise(plate, self.separation.plate(index)))
            .collect()
    }

    /// The plates that carry ink, in plate order. A page with a spot on
    /// it has one more plate than it had before the spot was added, and
    /// that difference is the whole reason to run preflight.
    pub fn used_plates(&self) -> Vec<String> {
        self.summaries()
            .into_iter()
            .filter(|s| s.has_ink)
            .map(|s| s.name)
            .collect()
    }
}

/// What one plate is doing.
pub fn summarise(plate: &Plate, coverage: Option<&PlateCoverage>) -> PlateSummary {
    let Some(coverage) = coverage else {
        return PlateSummary {
            name: plate.name.clone(),
            kind: plate.kind,
            rgb: plate.preview_rgb,
            peak: 0.0,
            mean: 0.0,
            has_ink: false,
            solid_fraction: 0.0,
        };
    };
    let area = coverage.data.len().max(1);
    let mut solid = 0usize;
    let mut sum = 0.0f32;
    for value in &coverage.data {
        sum += *value;
        if *value > 0.5 {
            solid += 1;
        }
    }
    PlateSummary {
        name: plate.name.clone(),
        kind: plate.kind,
        rgb: plate.preview_rgb,
        peak: coverage.peak(),
        mean: sum / area as f32,
        has_ink: coverage.data.iter().any(|v| *v > 0.0),
        solid_fraction: solid as f32 / area as f32,
    }
}

/// A CMYK build as displayable RGB.
///
/// The naive conversion, for a preview only: it is what the ink
/// definitions themselves are built from, and a separations panel is
/// showing the inks as named rather than as a colour-managed proof.
pub fn cmyk_to_rgb(cmyk: [f32; 4]) -> [f32; 3] {
    let k = cmyk[3].clamp(0.0, 1.0);
    [
        ((1.0 - cmyk[0].clamp(0.0, 1.0)) * (1.0 - k)).clamp(0.0, 1.0),
        ((1.0 - cmyk[1].clamp(0.0, 1.0)) * (1.0 - k)).clamp(0.0, 1.0),
        ((1.0 - cmyk[2].clamp(0.0, 1.0)) * (1.0 - k)).clamp(0.0, 1.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::ink::{Ink, InkManager};

    fn plan_with_spot() -> PlatePlan {
        let inks = vec![Ink::black(), Ink::spot("PANTONE 032 C", [50.0, 60.0, 55.0])];
        PlatePlan::with_build(
            &inks,
            &InkManager::with_defaults(),
            &crate::build::NaiveBuild,
        )
    }

    fn separation_with(rect: IntRect) -> (Separation, PlatePlan) {
        let plan = plan_with_spot();
        (Separation::new(plan.plates.len(), rect), plan)
    }

    #[test]
    fn an_empty_page_reports_every_plate_as_empty() {
        let (separation, plan) = separation_with(IntRect::new(0, 0, 10, 10));
        let preview = SeparationsPreview::new(&separation, &plan);
        for summary in preview.summaries() {
            assert!(
                !summary.has_ink,
                "{} has ink on an empty page",
                summary.name
            );
            assert_eq!(summary.describe(), "empty");
        }
        assert!(preview.used_plates().is_empty());
    }

    #[test]
    fn only_the_plates_with_ink_are_listed_as_used() {
        let (mut separation, plan) = separation_with(IntRect::new(0, 0, 10, 10));
        let black = plan.plates.iter().position(|p| p.name == "Black").unwrap();
        separation.plates_mut()[black].set(5, 5, 1.0);
        let preview = SeparationsPreview::new(&separation, &plan);
        let used = preview.used_plates();
        assert!(used.iter().any(|n| n == "Black"), "{used:?}");
        assert!(!used.iter().any(|n| n == "Cyan"), "{used:?}");
    }

    #[test]
    fn plates_are_described_by_how_much_ink_they_carry() {
        let (mut separation, plan) = separation_with(IntRect::new(0, 0, 10, 10));
        let black = plan.plates.iter().position(|p| p.name == "Black").unwrap();
        let magenta = plan
            .plates
            .iter()
            .position(|p| p.name == "Magenta")
            .unwrap();
        let yellow = plan.plates.iter().position(|p| p.name == "Yellow").unwrap();
        // Black floods the whole plate, magenta a flat third of it, and
        // yellow carries a single mark.
        for y in 0..10 {
            for x in 0..10 {
                separation.plates_mut()[black].set(x, y, 1.0);
                if y < 3 {
                    separation.plates_mut()[magenta].set(x, y, 0.3);
                }
                if x == 0 && y == 0 {
                    // A single mark: fine detail, not a flat area.
                    separation.plates_mut()[yellow].set(x, y, 1.0);
                }
            }
        }
        let preview = SeparationsPreview::new(&separation, &plan);
        let summaries = preview.summaries();
        let find = |name: &str| summaries.iter().find(|s| s.name == name).cloned().unwrap();
        assert_eq!(find("Black").describe(), "solid");
        assert_eq!(find("Magenta").describe(), "tint");
        assert_eq!(find("Cyan").describe(), "empty");
        // One mark in a hundred cells is a mean of 1%, which reads as
        // detail rather than a tint.
        assert_eq!(find("Yellow").mean, 0.01);
        assert_eq!(find("Yellow").solid_fraction, 0.01);
        assert_eq!(find("Yellow").describe(), "detail");
        // And the numbers behind the words are right.
        assert_eq!(find("Black").solid_fraction, 1.0);
        // A flat 30% over three rows of ten, measured over the whole
        // plate rather than the inked part.
        assert!(
            (find("Magenta").mean - 0.09).abs() < 1e-4,
            "{}",
            find("Magenta").mean
        );
    }

    #[test]
    fn a_plate_draws_its_own_ink_over_white_paper() {
        let (mut separation, plan) = separation_with(IntRect::new(0, 0, 4, 1));
        let black = plan.plates.iter().position(|p| p.name == "Black").unwrap();
        separation.plates_mut()[black].set(0, 0, 1.0);
        separation.plates_mut()[black].set(1, 0, 0.0);
        let preview = SeparationsPreview::new(&separation, &plan);
        let mut rgba = vec![0u8; 4 * 4];
        preview.draw(black, &IntRect::new(0, 0, 4, 1), &mut rgba);
        // Full ink is black, no ink is white.
        assert_eq!(&rgba[0..3], &[0, 0, 0]);
        assert_eq!(&rgba[4..7], &[255, 255, 255]);
        assert_eq!(rgba[3], 255);
    }

    #[test]
    fn a_tint_draws_lighter_than_a_solid() {
        let (mut separation, plan) = separation_with(IntRect::new(0, 0, 2, 1));
        let cyan = plan.plates.iter().position(|p| p.name == "Cyan").unwrap();
        separation.plates_mut()[cyan].set(0, 0, 1.0);
        separation.plates_mut()[cyan].set(1, 0, 0.4);
        let preview = SeparationsPreview::new(&separation, &plan);
        let mut rgba = vec![0u8; 2 * 4];
        preview.draw(cyan, &IntRect::new(0, 0, 2, 1), &mut rgba);
        // Probed on red: cyan's own blue is high, so a tint of it draws
        // *darker* in blue and lighter in red, and the overall value is
        // what makes a tint read as lighter.
        let solid = rgba[0] as i32;
        let tint = rgba[4] as i32;
        assert!(
            tint > solid,
            "a tint drew darker than a solid: {tint} vs {solid}"
        );
    }

    #[test]
    fn drawing_clips_to_the_requested_rect() {
        let (mut separation, plan) = separation_with(IntRect::new(0, 0, 10, 10));
        let black = plan.plates.iter().position(|p| p.name == "Black").unwrap();
        for y in 0..10 {
            for x in 0..10 {
                separation.plates_mut()[black].set(x, y, 1.0);
            }
        }
        let preview = SeparationsPreview::new(&separation, &plan);
        // A panel asking for a quarter of the page.
        let mut rgba = vec![0u8; 4 * 4 * 4];
        preview.draw(black, &IntRect::new(2, 2, 6, 6), &mut rgba);
        assert_eq!(rgba.len(), 64);
        // Every pixel asked for was inside the plate, so all are inked.
        for chunk in rgba.as_chunks::<4>().0 {
            assert_eq!(chunk[0], 0);
        }
    }

    #[test]
    fn drawing_outside_the_page_leaves_the_buffer_alone() {
        let (separation, plan) = separation_with(IntRect::new(0, 0, 4, 4));
        let preview = SeparationsPreview::new(&separation, &plan);
        let mut rgba = vec![7u8; 4 * 4];
        preview.draw(0, &IntRect::new(100, 100, 104, 104), &mut rgba);
        assert!(rgba.iter().all(|b| *b == 7), "the buffer was written to");
    }

    #[test]
    fn an_unknown_plate_draws_nothing_rather_than_panicking() {
        let (separation, plan) = separation_with(IntRect::new(0, 0, 4, 4));
        let preview = SeparationsPreview::new(&separation, &plan);
        let mut rgba = vec![0u8; 16];
        preview.draw(99, &IntRect::new(0, 0, 4, 4), &mut rgba);
        assert!(rgba.iter().all(|b| *b == 0));
    }

    #[test]
    fn the_composite_view_puts_a_plate_over_the_page_beneath_it() {
        let (mut separation, plan) = separation_with(IntRect::new(0, 0, 2, 1));
        let cyan = plan.plates.iter().position(|p| p.name == "Cyan").unwrap();
        let black = plan.plates.iter().position(|p| p.name == "Black").unwrap();
        // Process black beneath, then a spot over it.
        separation.plates_mut()[black].set(0, 0, 1.0);
        separation.composite_mut().set(0, 0, [0.0, 0.0, 0.0, 1.0]);
        let spot = plan.plates.len() - 1;
        separation.plates_mut()[spot].set(0, 0, 1.0);
        let _ = cyan;
        let preview = SeparationsPreview::new(&separation, &plan).over_composite();
        let mut rgba = vec![0u8; 2 * 4];
        preview.draw(spot, &IntRect::new(0, 0, 2, 1), &mut rgba);
        // The spot's own ink, not black, because the view is composite.
        assert!(
            rgba[0] > 0 || rgba[1] > 0,
            "the spot did not show over the black"
        );
    }

    #[test]
    fn the_composite_draws_the_page_as_it_would_print() {
        let (mut separation, plan) = separation_with(IntRect::new(0, 0, 2, 1));
        separation.composite_mut().set(0, 0, [1.0, 0.0, 0.0, 0.0]);
        separation.composite_mut().set(1, 0, [0.0, 0.0, 0.0, 1.0]);
        let preview = SeparationsPreview::new(&separation, &plan);
        let mut rgba = vec![0u8; 2 * 4];
        preview.draw_composite(&IntRect::new(0, 0, 2, 1), &mut rgba);
        // 100% cyan and 100% black.
        assert_eq!(&rgba[0..3], &[0, 255, 255]);
        assert_eq!(&rgba[4..7], &[0, 0, 0]);
    }

    #[test]
    fn a_cmyk_build_converts_to_displayable_rgb() {
        assert_eq!(cmyk_to_rgb([0.0, 0.0, 0.0, 0.0]), [1.0, 1.0, 1.0]);
        assert_eq!(cmyk_to_rgb([0.0, 0.0, 0.0, 1.0]), [0.0, 0.0, 0.0]);
        assert_eq!(cmyk_to_rgb([1.0, 0.0, 0.0, 0.0]), [0.0, 1.0, 1.0]);
        // Out of range is clamped rather than wrapping round.
        assert_eq!(cmyk_to_rgb([2.0, -1.0, 0.0, 0.0]), [0.0, 1.0, 1.0]);
    }
}
