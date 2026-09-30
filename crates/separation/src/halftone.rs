//! Halftoning and trapping: turning continuous coverage into ink on paper.
//!
//! A plate is continuous coverage. A press puts ink on a sheet through a
//! **screen**: a fine grid of cells, each either inked or not. Two things
//! follow from that, and both are implemented here.
//!
//! **Halftoning** decides which cells. The cell's threshold is compared
//! against the plate's coverage, with the comparison rotated to the
//! plate's screen angle. The angle is not decoration: four plates at the
//! same angle print their dots on top of each other and moire, which is
//! why [`Halftone::process`] spaces them 15 degrees apart.
//!
//! **Trapping** fills the gap between two abutting objects. If two inks
//! meet exactly at an edge and the sheet shifts a fraction in the
//! register, bare paper shows through as a white hairline. Laying a
//! little of one ink under the other closes the gap. Which ink traps
//! under which is a printing convention -- the lighter traps under the
//! darker -- and [`trap_order`] returns that order.

use schist_core::IntRect;

use crate::coverage::PlateCoverage;
use crate::plan::{Halftone, Plate, PlateKind, PlatePlan};

/// A screened plate: 1 bits, one per cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub rect: IntRect,
    /// Rows of packed bits, MSB first, `stride` bytes per row.
    pub bits: Vec<u8>,
    pub stride: usize,
}

impl Screen {
    pub fn new(rect: IntRect) -> Screen {
        let width = rect.width().max(0) as usize;
        let height = rect.height().max(0) as usize;
        let stride = width.div_ceil(8);
        Screen {
            rect,
            bits: vec![0u8; stride * height],
            stride,
        }
    }

    pub fn get(&self, x: i32, y: i32) -> bool {
        if !self.rect.contains(x, y) {
            return false;
        }
        let dx = (x - self.rect.left) as usize;
        let dy = (y - self.rect.top) as usize;
        self.bits
            .get(dy * self.stride + dx / 8)
            .map(|b| b & (0b1000_0000 >> (dx % 8)) != 0)
            .unwrap_or(false)
    }

    pub fn set(&mut self, x: i32, y: i32, value: bool) {
        if !self.rect.contains(x, y) {
            return;
        }
        let dx = (x - self.rect.left) as usize;
        let dy = (y - self.rect.top) as usize;
        let slot = dy * self.stride + dx / 8;
        let mask = 0b1000_0000u8 >> (dx % 8);
        if let Some(byte) = self.bits.get_mut(slot) {
            if value {
                *byte |= mask;
            } else {
                *byte &= !mask;
            }
        }
    }

    /// The fraction of cells inked, which should track the plate's mean
    /// coverage. A screen that drifts is one the RIP will correct.
    pub fn coverage(&self) -> f32 {
        let area = self.rect.width().max(0) as usize * self.rect.height().max(0) as usize;
        if area == 0 {
            return 0.0;
        }
        self.bits
            .iter()
            .map(|b| b.count_ones() as usize)
            .sum::<usize>() as f32
            / area as f32
    }
}

/// The sub-cell resolution of the threshold pattern.
const CELL_RESOLUTION: usize = 16;

/// The threshold pattern for one screen cell.
///
/// A screen's job is to put the *right area* of ink on the sheet: a 40%
/// tint has to ink 40% of the plate's cells, or the print is the wrong
/// density and a press operator cannot compensate for it.
///
/// A single radial threshold cannot do that. Thresholding on distance
/// from the cell's centre gives round dots, but a disc of area `c` needs
/// a radius of `sqrt(c / pi)` where the naive rule uses `c`, so a mid
/// tint prints far too light. Real screens solve it with a ranked
/// table: every sub-cell gets the threshold `rank / area` in order of
/// its distance from the centre. Round dots, and the inked fraction
/// matches the coverage to within one sub-cell.
fn cell_thresholds() -> &'static [f32; CELL_RESOLUTION * CELL_RESOLUTION] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[f32; CELL_RESOLUTION * CELL_RESOLUTION]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let n = CELL_RESOLUTION as f32;
        let mut order: Vec<(f32, usize)> = (0..CELL_RESOLUTION)
            .flat_map(|y| {
                (0..CELL_RESOLUTION).map(move |x| {
                    let dx = (x as f32 + 0.5) / n - 0.5;
                    let dy = (y as f32 + 0.5) / n - 0.5;
                    ((dx * dx + dy * dy).sqrt(), y * CELL_RESOLUTION + x)
                })
            })
            .collect();
        order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut table = [0.0f32; CELL_RESOLUTION * CELL_RESOLUTION];
        let area = (CELL_RESOLUTION * CELL_RESOLUTION) as f32;
        for (rank, (_, index)) in order.into_iter().enumerate() {
            // Centred in its bin, so a coverage of c inks exactly
            // round(c * area) sub-cells and no more.
            table[index] = (rank as f32 + 0.5) / area;
        }
        table
    })
}

/// The threshold at a point inside a cell, given the cell's rotation.
fn threshold_at(fx: f32, fy: f32) -> f32 {
    let table = cell_thresholds();
    let n = CELL_RESOLUTION as f32;
    // Wrap into the cell, so a point past its edge is compared against
    // the same pattern rather than reading off the end.
    let gx = (fx - fx.floor()) * n;
    let gy = (fy - fy.floor()) * n;
    let x = (gx as usize).min(CELL_RESOLUTION - 1);
    let y = (gy as usize).min(CELL_RESOLUTION - 1);
    table[y * CELL_RESOLUTION + x]
}

/// Screen a plate.
///
/// `frequency` is lines per inch and `dpi` the plate's resolution, so
/// the cell size in pixels is `dpi / frequency`. A cell smaller than a
/// pixel is treated as one pixel, because sampling below the plate's
/// resolution can only produce aliasing.
pub fn halftone(plate: &PlateCoverage, screen: &Halftone, dpi: f32) -> Screen {
    let mut out = Screen::new(plate.rect);
    let cell = if screen.frequency > 0.0 && dpi > 0.0 {
        (dpi / screen.frequency).max(1.0)
    } else {
        // A frequency of zero would divide by nothing; one pixel per
        // cell degrades to the plate's own coverage.
        1.0
    };
    let (sin, cos) = screen.angle.to_radians().sin_cos();
    for y in plate.rect.top..plate.rect.bottom {
        for x in plate.rect.left..plate.rect.right {
            let coverage = plate.at(x, y).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            // Where this pixel falls inside the rotated cell grid.
            let px = (x - plate.rect.left) as f32 / cell;
            let py = (y - plate.rect.top) as f32 / cell;
            let rx = px * cos - py * sin;
            let ry = px * sin + py * cos;
            let threshold = threshold_at(rx, ry);
            if coverage >= threshold {
                out.set(x, y, true);
            }
        }
    }
    out
}

/// Screen every plate in a plan.
pub fn halftone_all(
    separation: &crate::coverage::Separation,
    plan: &PlatePlan,
    dpi: f32,
) -> Vec<(String, Screen)> {
    plan.plates
        .iter()
        .enumerate()
        .filter_map(|(index, plate)| {
            let coverage = separation.plate(index)?;
            Some((plate.name.clone(), halftone(coverage, &plate.halftone, dpi)))
        })
        .collect()
}

/// Which ink traps under which.
///
/// The lighter ink traps under the darker, because the darker is the one
/// whose edge a printer is watching: a gap next to a solid black shows,
/// and a gap next to a 5% tint does not. Two inks of equal darkness
/// neither yields to the other, so they are ordered by name to keep the
/// result stable rather than arbitrary.
pub fn trap_order(a: &Plate, b: &Plate) -> (usize, usize) {
    // A larger RGB sum is a lighter ink. Trapping puts the lighter under
    // the darker, so the lighter is the one that yields.
    let a_light = a.preview_rgb.iter().sum::<f32>();
    let b_light = b.preview_rgb.iter().sum::<f32>();
    if a_light > b_light {
        (0, 1)
    } else if b_light > a_light {
        (1, 0)
    } else {
        match a.name.cmp(&b.name) {
            std::cmp::Ordering::Less => (0, 1),
            _ => (1, 0),
        }
    }
}

/// Widen a plate into its neighbours by `width` pixels.
///
/// Trapping a pair means widening both by half the total, so together
/// they close the gap. A plate is never widened past the output box,
/// because ink outside the paper is ink wasted.
pub fn trap(plate: &PlateCoverage, width: i32, bounds: IntRect) -> PlateCoverage {
    if width <= 0 {
        return plate.clone();
    }
    let grown = plate.rect.inflated(width).intersect(&bounds);
    if grown.is_empty() {
        return plate.clone();
    }
    let mut out = PlateCoverage::new(grown);
    for y in grown.top..grown.bottom {
        for x in grown.left..grown.right {
            // The dilation: inked if any pixel within the radius is
            // inked. Cheaper than a distance transform and, for a trap
            // one or two pixels wide, indistinguishable from one.
            let mut inked = false;
            'outer: for dy in -width..=width {
                for dx in -width..=width {
                    if dx * dx + dy * dy > width * width {
                        continue;
                    }
                    if plate.at(x + dx, y + dy) > 0.0 {
                        inked = true;
                        break 'outer;
                    }
                }
            }
            if inked {
                out.set(x, y, 1.0);
            }
        }
    }
    out
}

/// The plates that were trapped, and the pairs of inks that met.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trapping {
    /// Widened plate coverage, by plate name.
    pub plates: Vec<(String, PlateCoverage)>,
    /// The ink pairs that were trapped, so preflight can say which
    /// colours meet on the page.
    pub pairs: Vec<(String, String)>,
}

/// Trap every pair of plates that touch, by widening the lighter under
/// the darker.
pub fn trap_all(
    separation: &crate::coverage::Separation,
    plan: &PlatePlan,
    width: i32,
) -> Trapping {
    let bounds = separation.rect();
    let mut trapping = Trapping::default();
    let plates: Vec<(usize, &Plate, &PlateCoverage)> = plan
        .plates
        .iter()
        .enumerate()
        .filter_map(|(index, plate)| Some((index, plate, separation.plate(index)?)))
        // A plate with no ink cannot trap, and an ink alone on the page
        // has no neighbour to trap against.
        .filter(|(_, _, coverage)| coverage.data.iter().any(|v| *v > 0.0))
        .collect();

    for i in 0..plates.len() {
        for k in (i + 1)..plates.len() {
            let (index_a, plate_a, coverage_a) = plates[i];
            let (index_b, plate_b, coverage_b) = plates[k];
            if !touch(coverage_a, coverage_b) {
                continue;
            }
            let (under, _) = trap_order(plate_a, plate_b);
            let under_index = if under == 0 { index_a } else { index_b };
            // The union of the pair, widened by half on each side, so
            // the two close the gap by the requested amount rather than
            // twice it. Trapping the union rather than one plate alone
            // is what fills the seam instead of only the edges.
            let half = (width / 2).max(1);
            let union = union_of(coverage_a, coverage_b);
            let widened = trap(&union, half, bounds);
            trapping
                .plates
                .push((plan.plates[under_index].name.clone(), widened));
            trapping.pairs.push((
                plan.plates[index_a].name.clone(),
                plan.plates[index_b].name.clone(),
            ));
        }
    }
    trapping
}

/// The union of two plates, over a rect covering both.
fn union_of(a: &PlateCoverage, b: &PlateCoverage) -> PlateCoverage {
    let rect = a.rect.union(&b.rect);
    let mut out = PlateCoverage::new(rect);
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            let v = a.at(x, y).max(b.at(x, y));
            if v > 0.0 {
                out.set(x, y, v);
            }
        }
    }
    out
}

/// Whether two plates share an edge.
///
/// A strict neighbour test: one plate's pixels touch the other's, in
/// the eight directions. Diagonals count, because a misregistration in
/// both axes opens a diagonal gap just as a straight one does.
fn touch(a: &PlateCoverage, b: &PlateCoverage) -> bool {
    if a.rect.is_empty() || b.rect.is_empty() {
        return false;
    }
    // Only worth checking where the rects are close at all.
    if a.rect.intersect(&b.rect).is_empty() && !near(a.rect, b.rect, 2) {
        return false;
    }
    for y in a.rect.top..a.rect.bottom {
        for x in a.rect.left..a.rect.right {
            if a.at(x, y) <= 0.0 {
                continue;
            }
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if b.at(x + dx, y + dy) > 0.0 {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Whether two rects are within `gap` pixels of each other.
fn near(a: IntRect, b: IntRect, gap: i32) -> bool {
    a.left - gap <= b.right
        && b.left - gap <= a.right
        && a.top - gap <= b.bottom
        && b.top - gap <= a.bottom
}

/// The plates a document will actually print, for a report.
pub fn printable_plates(plan: &PlatePlan) -> Vec<&Plate> {
    plan.plates
        .iter()
        .filter(|p| p.kind == PlateKind::Process || p.process_index.is_none())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plate(values: &[f32], w: i32) -> PlateCoverage {
        PlateCoverage {
            rect: IntRect::new(0, 0, w, values.len() as i32 / w),
            data: values.to_vec(),
        }
    }

    #[test]
    fn an_empty_plate_screens_to_nothing() {
        let p = plate(&[0.0, 0.0, 0.0, 0.0], 2);
        let screen = halftone(&p, &Halftone::default(), 300.0);
        assert_eq!(screen.coverage(), 0.0);
    }

    #[test]
    fn a_full_plate_screens_to_solid_ink() {
        let p = plate(&[1.0, 1.0, 1.0, 1.0], 2);
        let screen = halftone(&p, &Halftone::default(), 300.0);
        assert_eq!(screen.coverage(), 1.0);
        assert!(screen.get(0, 0));
        assert!(screen.get(1, 1));
    }

    #[test]
    fn a_half_tone_screens_about_half_inked() {
        // A dot screen's whole point is that a 50% tint is half inked
        // cells, not a checkerboard of 50% grey.
        let values: Vec<f32> = (0..400).map(|_| 0.5).collect();
        let p = PlateCoverage {
            rect: IntRect::new(0, 0, 20, 20),
            data: values,
        };
        let screen = halftone(&p, &Halftone::default(), 300.0);
        let coverage = screen.coverage();
        assert!(
            (coverage - 0.5).abs() < 0.2,
            "a 50% tint screened to {coverage}"
        );
    }

    #[test]
    fn screen_coverage_tracks_the_plates_mean() {
        // A screen that drifts is one the RIP will silently correct.
        for level in [0.1f32, 0.35, 0.6, 0.85] {
            let values: Vec<f32> = (0..1600).map(|_| level).collect();
            let p = PlateCoverage {
                rect: IntRect::new(0, 0, 40, 40),
                data: values,
            };
            let screen = halftone(&p, &Halftone::default(), 300.0);
            assert!(
                (screen.coverage() - level).abs() < 0.12,
                "a {level} tint screened to {}",
                screen.coverage()
            );
        }
    }

    #[test]
    fn a_rotated_screen_differs_from_an_unrotated_one() {
        // Two plates at the same angle moire against each other, which
        // is the reason the rosette exists.
        let p = PlateCoverage {
            rect: IntRect::new(0, 0, 40, 40),
            data: vec![0.5; 1600],
        };
        let straight = halftone(
            &p,
            &Halftone {
                angle: 0.0,
                ..Halftone::default()
            },
            300.0,
        );
        let turned = halftone(
            &p,
            &Halftone {
                angle: 45.0,
                ..Halftone::default()
            },
            300.0,
        );
        assert_ne!(straight.bits, turned.bits, "the angle changed nothing");
    }

    #[test]
    fn the_process_rosette_uses_four_angles() {
        let plan = PlatePlan::build(&[], &schist_layout::ink::InkManager::with_defaults());
        let angles: Vec<f32> = plan.plates.iter().map(|p| p.halftone.angle).collect();
        let mut unique = angles.clone();
        unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
        unique.dedup();
        assert_eq!(unique.len(), 4, "{angles:?}");
    }

    #[test]
    fn a_frequency_below_the_plate_resolution_degrades_to_one_pixel() {
        // Sampling finer than the plate can only alias.
        let p = plate(&[1.0, 1.0, 0.0, 0.0], 2);
        let coarse = halftone(
            &p,
            &Halftone {
                frequency: 1.0,
                ..Halftone::default()
            },
            300.0,
        );
        let fine = halftone(
            &p,
            &Halftone {
                frequency: 150.0,
                ..Halftone::default()
            },
            300.0,
        );
        assert_eq!(coarse.coverage(), fine.coverage());
    }

    #[test]
    fn a_zero_frequency_does_not_divide_by_nothing() {
        let p = plate(&[0.5, 0.5, 0.5, 0.5], 2);
        let screen = halftone(
            &p,
            &Halftone {
                frequency: 0.0,
                ..Halftone::default()
            },
            300.0,
        );
        assert_eq!(screen.coverage(), 0.5);
    }

    #[test]
    fn a_screen_reports_its_own_rect() {
        let p = plate(&[1.0; 4], 2);
        let screen = halftone(&p, &Halftone::default(), 300.0);
        assert_eq!(screen.rect, p.rect);
        // Outside the rect is bare.
        assert!(!screen.get(50, 50));
    }

    #[test]
    fn a_screen_packs_its_bits() {
        let mut screen = Screen::new(IntRect::new(0, 0, 9, 1));
        screen.set(0, 0, true);
        screen.set(7, 0, true);
        screen.set(8, 0, true);
        assert_eq!(screen.stride, 2);
        // Eight pixels to a byte, MSB first: pixels 0 and 7 are in the
        // first byte and pixel 8 starts the second.
        assert_eq!(screen.bits[0], 0b1000_0001);
        assert_eq!(screen.bits[1], 0b1000_0000);
    }

    #[test]
    fn a_lighter_ink_traps_under_a_darker_one() {
        let plan = PlatePlan::build(&[], &schist_layout::ink::InkManager::with_defaults());
        let yellow = plan.plates.iter().find(|p| p.name == "Yellow").unwrap();
        let black = plan.plates.iter().find(|p| p.name == "Black").unwrap();
        let (under, over) = trap_order(yellow, black);
        assert_eq!(
            under, 0,
            "yellow should trap under black, not the other way round"
        );
        assert_eq!(over, 1);
    }

    #[test]
    fn two_inks_of_equal_darkness_trap_in_a_stable_order() {
        // Arbitrary would be acceptable; unstable would mean the same
        // pair traps differently on two runs of the same job.
        let plan = PlatePlan::build(&[], &schist_layout::ink::InkManager::with_defaults());
        let cyan = plan.plates.iter().find(|p| p.name == "Cyan").unwrap();
        let magenta = plan.plates.iter().find(|p| p.name == "Magenta").unwrap();
        assert_eq!(trap_order(cyan, magenta), trap_order(cyan, magenta));
        let (a, b) = trap_order(cyan, magenta);
        assert_ne!(a, b, "every plate trapped under itself");
    }

    #[test]
    fn trapping_widens_a_plate_into_its_neighbour() {
        let left = PlateCoverage {
            rect: IntRect::new(0, 0, 10, 10),
            data: vec![1.0; 100],
        };
        let trapped = trap(&left, 2, IntRect::new(0, 0, 20, 20));
        // The ink now reaches past the original edge.
        assert!(trapped.rect.width() > left.rect.width());
        assert_eq!(trapped.at(10, 5), 1.0);
    }

    #[test]
    fn trapping_never_grows_past_the_paper() {
        let left = PlateCoverage {
            rect: IntRect::new(0, 0, 10, 10),
            data: vec![1.0; 100],
        };
        let bounds = IntRect::new(0, 0, 10, 10);
        let trapped = trap(&left, 3, bounds);
        assert_eq!(trapped.rect, left.rect, "ink grew off the sheet");
    }

    #[test]
    fn a_zero_width_trap_changes_nothing() {
        let left = plate(&[1.0, 1.0, 1.0, 1.0], 2);
        assert_eq!(trap(&left, 0, IntRect::new(0, 0, 10, 10)), left);
    }

    #[test]
    fn two_ink_swatches_that_touch_are_trapped() {
        let plan = PlatePlan::build(&[], &schist_layout::ink::InkManager::with_defaults());
        let mut separation =
            crate::coverage::Separation::new(plan.plates.len(), IntRect::new(0, 0, 40, 40));
        // Cyan on the left, black on the right, sharing an edge.
        let cyan = plan.plates.iter().position(|p| p.name == "Cyan").unwrap();
        let black = plan.plates.iter().position(|p| p.name == "Black").unwrap();
        for y in 0..40 {
            for x in 0..20 {
                separation.plates_mut()[cyan].set(x, y, 1.0);
            }
            for x in 20..40 {
                separation.plates_mut()[black].set(x, y, 1.0);
            }
        }
        let trapping = trap_all(&separation, &plan, 1);
        assert_eq!(trapping.pairs.len(), 1, "{:?}", trapping.pairs);
        let (a, b) = &trapping.pairs[0];
        assert!(a == "Cyan" || b == "Cyan", "{trapping:?}");
        assert!(a == "Black" || b == "Black", "{trapping:?}");
        // The lighter ink is the one that was widened.
        assert_eq!(trapping.plates[0].0, "Cyan", "{:?}", trapping.plates[0].0);
    }

    #[test]
    fn two_ink_swatches_far_apart_are_not_trapped() {
        let plan = PlatePlan::build(&[], &schist_layout::ink::InkManager::with_defaults());
        let mut separation =
            crate::coverage::Separation::new(plan.plates.len(), IntRect::new(0, 0, 100, 20));
        let cyan = plan.plates.iter().position(|p| p.name == "Cyan").unwrap();
        let black = plan.plates.iter().position(|p| p.name == "Black").unwrap();
        for y in 0..20 {
            for x in 0..20 {
                separation.plates_mut()[cyan].set(x, y, 1.0);
            }
            for x in 80..100 {
                separation.plates_mut()[black].set(x, y, 1.0);
            }
        }
        let trapping = trap_all(&separation, &plan, 1);
        assert!(
            trapping.pairs.is_empty(),
            "trapped inks that are nowhere near each other"
        );
    }

    #[test]
    fn an_empty_page_traps_nothing() {
        let plan = PlatePlan::build(&[], &schist_layout::ink::InkManager::with_defaults());
        let separation =
            crate::coverage::Separation::new(plan.plates.len(), IntRect::new(0, 0, 40, 40));
        let trapping = trap_all(&separation, &plan, 1);
        assert!(trapping.plates.is_empty());
        assert!(trapping.pairs.is_empty());
    }
}
