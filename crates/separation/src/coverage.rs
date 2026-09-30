//! Coverage: a mask, and how ink is laid onto plates through one.
//!
//! This is the part of separation that is genuinely about print rather
//! than about colour. Every object on a page becomes a coverage mask --
//! how much of this pixel does it cover -- and its inks say how much of
//! each plate that coverage carries. The combination rule is where
//! knockout and overprint differ, and getting it wrong is invisible on
//! screen and catastrophic on a press.

use schist_core::IntRect;
use schist_layout::Pt;

/// A coverage mask over a rectangle, 0 = bare paper, 255 = full coverage.
#[derive(Debug, Clone, PartialEq)]
pub struct Coverage {
    pub rect: IntRect,
    /// `rect.width() * rect.height()` bytes, row-major.
    pub data: Vec<u8>,
}

impl Coverage {
    pub fn new(rect: IntRect) -> Coverage {
        let area = rect.width().max(0) as usize * rect.height().max(0) as usize;
        Coverage {
            rect,
            data: vec![0; area],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rect.is_empty() || self.data.is_empty()
    }

    /// Coverage at a pixel, 0..=1. Outside the mask is bare paper.
    pub fn at(&self, x: i32, y: i32) -> f32 {
        if !self.rect.contains(x, y) {
            return 0.0;
        }
        let i = ((y - self.rect.top) * self.rect.width() + (x - self.rect.left)) as usize;
        self.data.get(i).copied().unwrap_or(0) as f32 / 255.0
    }

    /// The mask's intersection with another rectangle, as a new mask.
    pub fn clipped(&self, to: IntRect) -> Coverage {
        let clip = self.rect.intersect(&to);
        if clip.is_empty() {
            return Coverage::new(IntRect::EMPTY);
        }
        let mut out = Coverage::new(clip);
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                out.data[((y - clip.top) * clip.width() + (x - clip.left)) as usize] =
                    (self.at(x, y) * 255.0).round() as u8;
            }
        }
        out
    }
}

/// One plate's share of a painted object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coat {
    pub plate: usize,
    /// How much of the plate this coverage carries, 0..=1. A 60% tint is
    /// a coat of weight 0.6, which is not the same as painting at 60%
    /// opacity.
    pub weight: f32,
}

impl Coat {
    pub fn full(plate: usize) -> Coat {
        Coat { plate, weight: 1.0 }
    }
}

/// How a painted object interacts with what is already on the plate.
///
/// This is the whole of knockout versus overprint, and it is a property
/// of the *object*, not of the ink or the plate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InkMode {
    /// Remove what is beneath, then print. Two objects meeting on the
    /// same plate leave a hard edge and the underlying ink is gone.
    #[default]
    Knockout,
    /// Print over whatever is there. Two objects sharing a plate add
    /// their coverage, and a hairline of colour in a black box does not
    /// punch a hole in it.
    Overprint,
}

/// A plate's accumulated coverage over the whole page.
#[derive(Debug, Clone, PartialEq)]
pub struct PlateCoverage {
    pub rect: IntRect,
    /// `rect.width() * rect.height()` floats, 0..=1, row-major.
    pub data: Vec<f32>,
}

impl PlateCoverage {
    pub fn new(rect: IntRect) -> PlateCoverage {
        let area = rect.width().max(0) as usize * rect.height().max(0) as usize;
        PlateCoverage {
            rect,
            data: vec![0.0; area],
        }
    }

    pub fn at(&self, x: i32, y: i32) -> f32 {
        if !self.rect.contains(x, y) {
            return 0.0;
        }
        let i = ((y - self.rect.top) * self.rect.width() + (x - self.rect.left)) as usize;
        self.data.get(i).copied().unwrap_or(0.0)
    }

    /// Set coverage at a pixel, ignoring anything outside the plate.
    pub fn set(&mut self, x: i32, y: i32, value: f32) {
        if !self.rect.contains(x, y) {
            return;
        }
        let i = ((y - self.rect.top) * self.rect.width() + (x - self.rect.left)) as usize;
        self.data[i] = value;
    }

    /// The greatest coverage anywhere on the plate.
    pub fn peak(&self) -> f32 {
        self.data.iter().copied().fold(0.0, f32::max)
    }

    /// The mean coverage, ignoring pixels outside the plate's rectangle.
    pub fn mean(&self) -> f32 {
        if self.data.is_empty() {
            return 0.0;
        }
        self.data.iter().sum::<f32>() / self.data.len() as f32
    }
}

/// The accumulating separation for one page.
#[derive(Debug, Clone)]
pub struct Separation {
    rect: IntRect,
    plates: Vec<PlateCoverage>,
    /// A flat process composite alongside the plates, so a preview and a
    /// soft-proof come from the same pass rather than a second one.
    composite: CompositeCoverage,
}

/// A process composite: CMYK per pixel, kept beside the plates.
#[derive(Debug, Clone, PartialEq)]
pub struct CompositeCoverage {
    pub rect: IntRect,
    /// `rect.width() * rect.height()` CMYK samples, row-major.
    pub data: Vec<[f32; 4]>,
}

impl CompositeCoverage {
    pub fn new(rect: IntRect) -> CompositeCoverage {
        let area = rect.width().max(0) as usize * rect.height().max(0) as usize;
        CompositeCoverage {
            rect,
            data: vec![[0.0; 4]; area],
        }
    }

    pub fn at(&self, x: i32, y: i32) -> [f32; 4] {
        if !self.rect.contains(x, y) {
            return [0.0; 4];
        }
        let i = ((y - self.rect.top) * self.rect.width() + (x - self.rect.left)) as usize;
        self.data.get(i).copied().unwrap_or([0.0; 4])
    }

    /// Set the composite's CMYK at a pixel, ignoring anything outside it.
    pub fn set(&mut self, x: i32, y: i32, value: [f32; 4]) {
        if !self.rect.contains(x, y) {
            return;
        }
        let i = ((y - self.rect.top) * self.rect.width() + (x - self.rect.left)) as usize;
        self.data[i] = value;
    }
}

impl Separation {
    pub fn new(plate_count: usize, rect: IntRect) -> Separation {
        Separation {
            rect,
            plates: (0..plate_count).map(|_| PlateCoverage::new(rect)).collect(),
            composite: CompositeCoverage::new(rect),
        }
    }

    pub fn rect(&self) -> IntRect {
        self.rect
    }

    pub fn plates(&self) -> &[PlateCoverage] {
        &self.plates
    }

    pub fn plates_mut(&mut self) -> &mut [PlateCoverage] {
        &mut self.plates
    }

    pub fn plate(&self, index: usize) -> Option<&PlateCoverage> {
        self.plates.get(index)
    }

    pub fn composite(&self) -> &CompositeCoverage {
        &self.composite
    }

    /// The composite, for a caller building one.
    pub fn composite_mut(&mut self) -> &mut CompositeCoverage {
        &mut self.composite
    }

    /// Paint spatial CMYK samples with one knockout per source pixel.
    /// Channel-by-channel knockout would erase the channels laid before
    /// it; a whole-image maximum would turn a photograph into a flat tint.
    pub fn paint_process(
        &mut self,
        rect: IntRect,
        alpha: &[u8],
        pixels: &[[f32; 4]],
        process: [usize; 4],
        mode: InkMode,
        opacity: f32,
    ) -> bool {
        let Some(area) = (rect.width().max(0) as usize).checked_mul(rect.height().max(0) as usize)
        else {
            return false;
        };
        if alpha.len() != area
            || pixels.len() != area
            || pixels.iter().flatten().any(|v| !v.is_finite())
        {
            return false;
        }
        let clip = rect.intersect(&self.rect);
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                let index = ((y - rect.top) * rect.width() + x - rect.left) as usize;
                let coverage = f32::from(alpha[index]) / 255.0 * opacity.clamp(0.0, 1.0);
                let ink = pixels[index].map(|v| v.clamp(0.0, 1.0));
                for (plate_index, plate) in self.plates.iter_mut().enumerate() {
                    let weight = process
                        .iter()
                        .position(|p| *p == plate_index)
                        .map_or(0.0, |c| ink[c]);
                    let existing = plate.at(x, y);
                    let next = match mode {
                        InkMode::Knockout => existing * (1.0 - coverage) + weight * coverage,
                        InkMode::Overprint => (existing + weight * coverage).min(1.0),
                    };
                    plate.set(x, y, next);
                }
                let old = self.composite.at(x, y);
                self.composite.set(
                    x,
                    y,
                    std::array::from_fn(|c| match mode {
                        InkMode::Knockout => old[c] * (1.0 - coverage) + ink[c] * coverage,
                        InkMode::Overprint => (old[c] + ink[c] * coverage).min(1.0),
                    }),
                );
            }
        }
        true
    }

    /// Lay `coats` onto their plates through `mask`.
    ///
    /// This is the only place the knockout/overprint distinction exists,
    /// so it is the only place that has to get it right.
    ///
    /// A **knockout** object removes the ink beneath it from *every*
    /// plate it covers, not merely the ones it puts ink on. A spot mark
    /// knocked out over a black box leaves a hole in the black: that is
    /// what a knockout is for, and a rule that only touched the object's
    /// own plates would turn every knockout into a silent overprint.
    ///
    /// `opacity` is the object's own transparency, which is not the same
    /// as an ink's weight: a 50% transparent black object lays down half
    /// the ink, and the other half is paper.
    pub fn paint(&mut self, mask: &Coverage, coats: &[Coat], mode: InkMode, opacity: Pt) {
        if mask.is_empty() {
            return;
        }
        let opacity = opacity.clamp(0.0, 1.0);
        if opacity == 0.0 {
            return;
        }
        // Knockout reaches every plate; overprint only the ones inked.
        let reach: Vec<usize> = match mode {
            InkMode::Knockout => (0..self.plates.len()).collect(),
            InkMode::Overprint => coats
                .iter()
                .map(|c| c.plate)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
        };
        for y in mask.rect.top..mask.rect.bottom {
            for x in mask.rect.left..mask.rect.right {
                let coverage = mask.at(x, y) * opacity;
                if coverage <= 0.0 {
                    continue;
                }
                for index in &reach {
                    let weight = coats
                        .iter()
                        .find(|c| c.plate == *index)
                        .map(|c| c.weight)
                        .unwrap_or(0.0)
                        .clamp(0.0, 1.0);
                    let Some(plate) = self.plates.get_mut(*index) else {
                        continue;
                    };
                    let laid = weight * coverage;
                    let existing = plate.at(x, y);
                    let next = match mode {
                        // Remove what is beneath, proportionally to how
                        // much of this pixel the object covers. A plate
                        // the object does not ink has weight zero, so it
                        // is simply removed.
                        InkMode::Knockout => existing * (1.0 - coverage) + laid,
                        // Add on top. Clamped, because two solid coats of
                        // ink are still solid ink and cannot exceed full.
                        InkMode::Overprint => (existing + laid).min(1.0),
                    };
                    plate.set(x, y, next.clamp(0.0, 1.0));
                }
            }
        }
    }

    /// Record a flat appearance for the composite preview.
    ///
    /// `build` gives each coat's CMYK contribution, which is what a spot
    /// converted for preview looks like. It follows the same knockout or
    /// overprint rule as the plates.
    pub fn paint_composite(
        &mut self,
        mask: &Coverage,
        coats: &[Coat],
        build: &[[f32; 4]],
        mode: InkMode,
        opacity: Pt,
    ) {
        if mask.is_empty() {
            return;
        }
        let opacity = opacity.clamp(0.0, 1.0);
        if opacity == 0.0 {
            return;
        }
        for y in mask.rect.top..mask.rect.bottom {
            for x in mask.rect.left..mask.rect.right {
                let coverage = mask.at(x, y) * opacity;
                if coverage <= 0.0 {
                    continue;
                }
                let mut ink = [0.0f32; 4];
                for (coat, build) in coats.iter().zip(build) {
                    let amount = coverage * coat.weight.clamp(0.0, 1.0);
                    for c in 0..4 {
                        ink[c] += build[c] * amount;
                    }
                }
                let existing = self.composite.at(x, y);
                let next = [0.0f32; 4];
                let mut out = next;
                for c in 0..4 {
                    out[c] = match mode {
                        InkMode::Knockout => existing[c] * (1.0 - coverage) + ink[c],
                        InkMode::Overprint => existing[c] + ink[c],
                    }
                    .clamp(0.0, 1.0);
                }
                self.composite.set(x, y, out);
            }
        }
    }

    /// Total area coverage of a pixel, across the four process plates.
    pub fn total_area_at(&self, process: [usize; 4], x: i32, y: i32) -> f32 {
        process
            .iter()
            .map(|i| self.plates.get(*i).map(|p| p.at(x, y)).unwrap_or(0.0))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_samples_preserve_colour_alpha_and_knock_out_all_channels_once() {
        let rect = IntRect::new(0, 0, 4, 1);
        for mode in [InkMode::Knockout, InkMode::Overprint] {
            for opacity in [0.0, 0.25, 0.5, 1.0] {
                let mut separation = Separation::new(5, rect);
                for plate in separation.plates_mut() {
                    plate.data.fill(0.4);
                }
                let pixels = [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.5, 0.25],
                    [0.0; 4],
                    [0.2, 0.3, 0.4, 0.5],
                ];
                let alpha = [255, 128, 255, 0];
                assert!(separation.paint_process(
                    rect,
                    &alpha,
                    &pixels,
                    [0, 1, 2, 3],
                    mode,
                    opacity
                ));
                for x in 0..4 {
                    let coverage = f32::from(alpha[x]) / 255.0 * opacity;
                    for channel in 0..5 {
                        let ink = pixels[x].get(channel).copied().unwrap_or(0.0);
                        let expected = match mode {
                            InkMode::Knockout => 0.4 * (1.0 - coverage) + ink * coverage,
                            InkMode::Overprint => (0.4 + ink * coverage).min(1.0),
                        };
                        assert!(
                            (separation.plate(channel).unwrap().at(x as i32, 0) - expected).abs()
                                < 0.00001
                        );
                    }
                    for (channel, ink) in pixels[x].iter().enumerate() {
                        assert!(
                            (separation.composite().at(x as i32, 0)[channel] - ink * coverage)
                                .abs()
                                < 0.00001
                        );
                    }
                }
            }
        }
    }

    fn mask(values: &[u8], w: i32) -> Coverage {
        Coverage {
            rect: IntRect::new(0, 0, w, values.len() as i32 / w),
            data: values.to_vec(),
        }
    }

    #[test]
    fn coverage_outside_the_mask_is_bare_paper() {
        let m = mask(&[0, 128, 255, 64], 2);
        assert_eq!(m.at(0, 0), 0.0);
        assert!((m.at(1, 0) - 128.0 / 255.0).abs() < 1e-5);
        assert_eq!(m.at(0, 1), 1.0);
        assert_eq!(m.at(-1, 0), 0.0);
        assert_eq!(m.at(99, 99), 0.0);
    }

    #[test]
    fn knockout_replaces_what_is_beneath() {
        let mut s = Separation::new(1, IntRect::new(0, 0, 2, 1));
        s.paint(&mask(&[255], 1), &[Coat::full(0)], InkMode::Knockout, 1.0);
        assert!((s.plate(0).unwrap().at(0, 0) - 1.0).abs() < 1e-5);

        // A second object covering the same pixel knocks the first out.
        s.paint(
            &mask(&[255], 1),
            &[Coat {
                plate: 0,
                weight: 0.5,
            }],
            InkMode::Knockout,
            1.0,
        );
        assert!((s.plate(0).unwrap().at(0, 0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn overprint_adds_to_what_is_beneath() {
        let mut s = Separation::new(1, IntRect::new(0, 0, 1, 1));
        s.paint(&mask(&[255], 1), &[Coat::full(0)], InkMode::Overprint, 1.0);
        s.paint(
            &mask(&[255], 1),
            &[Coat {
                plate: 0,
                weight: 0.5,
            }],
            InkMode::Overprint,
            1.0,
        );
        // The colour in the black box does not punch a hole in it.
        assert!((s.plate(0).unwrap().at(0, 0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn overprint_saturates_rather_than_exceeding_full() {
        let mut s = Separation::new(1, IntRect::new(0, 0, 1, 1));
        for _ in 0..5 {
            s.paint(&mask(&[255], 1), &[Coat::full(0)], InkMode::Overprint, 1.0);
        }
        assert!((s.plate(0).unwrap().at(0, 0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn knockout_leaves_uncovered_pixels_alone() {
        let mut s = Separation::new(1, IntRect::new(0, 0, 2, 1));
        s.paint(&mask(&[255], 1), &[Coat::full(0)], InkMode::Knockout, 1.0);
        // The second mask covers only the right pixel.
        s.paint(
            &mask(&[0, 255], 2),
            &[Coat::full(0)],
            InkMode::Knockout,
            1.0,
        );
        let p = s.plate(0).unwrap();
        assert!(
            (p.at(0, 0) - 1.0).abs() < 1e-5,
            "left pixel was knocked out"
        );
        assert!((p.at(1, 0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn transparency_lays_down_proportionally_less_ink() {
        let mut s = Separation::new(1, IntRect::new(0, 0, 1, 1));
        s.paint(&mask(&[255], 1), &[Coat::full(0)], InkMode::Knockout, 0.5);
        // Half transparent black is half ink and half paper.
        assert!((s.plate(0).unwrap().at(0, 0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_fully_transparent_object_lays_no_ink() {
        let mut s = Separation::new(1, IntRect::new(0, 0, 1, 1));
        s.paint(&mask(&[255], 1), &[Coat::full(0)], InkMode::Knockout, 0.0);
        assert_eq!(s.plate(0).unwrap().at(0, 0), 0.0);
    }

    #[test]
    fn a_knockout_reaches_plates_the_object_does_not_ink() {
        // A spot mark knocked out over a black box leaves a hole in the
        // black. A rule that only touched the object's own plates would
        // make every knockout a silent overprint.
        let mut s = Separation::new(2, IntRect::new(0, 0, 2, 1));
        // Fill the whole row with black on plate 0.
        s.paint(
            &mask(&[255, 255], 2),
            &[Coat::full(0)],
            InkMode::Knockout,
            1.0,
        );
        // A spot on plate 1 covering only the right pixel, knocked out.
        s.paint(
            &mask(&[0, 255], 2),
            &[Coat {
                plate: 1,
                weight: 1.0,
            }],
            InkMode::Knockout,
            1.0,
        );
        assert!(
            (s.plate(0).unwrap().at(1, 0) - 0.0).abs() < 1e-5,
            "black survived under a knockout"
        );
        assert!(
            (s.plate(0).unwrap().at(0, 0) - 1.0).abs() < 1e-5,
            "knockout spread to the wrong pixel"
        );
        assert!(
            (s.plate(1).unwrap().at(1, 0) - 1.0).abs() < 1e-5,
            "the spot did not print"
        );
    }

    #[test]
    fn an_overprint_leaves_plates_it_does_not_ink_alone() {
        let mut s = Separation::new(2, IntRect::new(0, 0, 1, 1));
        s.paint(&mask(&[255], 1), &[Coat::full(0)], InkMode::Knockout, 1.0);
        s.paint(
            &mask(&[255], 1),
            &[Coat {
                plate: 1,
                weight: 1.0,
            }],
            InkMode::Overprint,
            1.0,
        );
        // The black underneath is untouched: that is what overprinting a
        // spot over black is for.
        assert!((s.plate(0).unwrap().at(0, 0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_coat_on_a_plate_that_does_not_exist_is_ignored() {
        let mut s = Separation::new(1, IntRect::new(0, 0, 1, 1));
        s.paint(&mask(&[255], 1), &[Coat::full(9)], InkMode::Knockout, 1.0);
        assert_eq!(s.plate(0).unwrap().at(0, 0), 0.0);
    }

    #[test]
    fn coverage_never_leaves_the_zero_to_one_range() {
        let mut s = Separation::new(1, IntRect::new(0, 0, 1, 1));
        for mode in [InkMode::Knockout, InkMode::Overprint] {
            for _ in 0..4 {
                s.paint(&mask(&[255], 1), &[Coat::full(0)], mode, 1.0);
            }
        }
        let v = s.plate(0).unwrap().at(0, 0);
        assert!((0.0..=1.0).contains(&v), "{v} out of range");
    }

    #[test]
    fn clipping_a_mask_keeps_the_intersection() {
        let m = mask(&[255, 255, 255, 255], 4);
        let c = m.clipped(IntRect::new(2, 0, 4, 1));
        assert_eq!(c.rect, IntRect::new(2, 0, 4, 1));
        assert_eq!(c.at(2, 0), 1.0);
        assert_eq!(c.at(1, 0), 0.0, "outside the clip");
    }

    #[test]
    fn clipping_to_something_disjoint_is_empty() {
        let m = mask(&[255, 255], 2);
        assert!(m.clipped(IntRect::new(10, 10, 20, 20)).is_empty());
    }

    #[test]
    fn total_area_sums_the_four_process_plates() {
        let mut s = Separation::new(4, IntRect::new(0, 0, 1, 1));
        // Overprint, because stacking four knockouts correctly leaves
        // only the topmost ink: each removes what is beneath it.
        for i in 0..4 {
            s.paint(&mask(&[255], 1), &[Coat::full(i)], InkMode::Overprint, 1.0);
        }
        // 400% coverage.
        assert!((s.total_area_at([0, 1, 2, 3], 0, 0) - 4.0).abs() < 1e-5);
    }

    #[test]
    fn a_knockout_above_four_overprinted_plates_leaves_only_its_own() {
        // This is what a knockout group is for: covering an area that
        // already carries every ink and starting again from nothing.
        let mut s = Separation::new(5, IntRect::new(0, 0, 2, 1));
        for i in 0..4 {
            s.paint(
                &mask(&[255, 255], 2),
                &[Coat::full(i)],
                InkMode::Overprint,
                1.0,
            );
        }
        assert!((s.total_area_at([0, 1, 2, 3], 0, 0) - 4.0).abs() < 1e-5);
        // A spot, knocked out, over both pixels.
        s.paint(
            &mask(&[255, 255], 2),
            &[Coat {
                plate: 4,
                weight: 1.0,
            }],
            InkMode::Knockout,
            1.0,
        );
        assert!(
            (s.total_area_at([0, 1, 2, 3], 0, 0) - 0.0).abs() < 1e-5,
            "process ink survived"
        );
        assert!(
            (s.plate(4).unwrap().at(0, 0) - 1.0).abs() < 1e-5,
            "the spot did not print"
        );
    }
}
