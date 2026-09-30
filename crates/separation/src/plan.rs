//! Stage 1: turning a document's inks into a set of plates.
//!
//! A plate is somewhere ink goes. Process inks share the four CMYK
//! plates every press already has; a spot ink needs one of its own, and
//! two names for the same ink can share one. Deciding which is which is
//! an **output-time** decision, which is why it lives here and on
//! [`InkManager`] rather than on the document's inks: a prepress
//! provider converting a spot to process must not rewrite the document
//! the designer will reopen tomorrow.
//!
//! The plan is a flat, ordered list because plate order is meaningful to
//! a press operator and to a TIFF it writes, and neither should depend
//! on hash order.

use schist_layout::ink::{Ink, InkManager};
use schist_layout::Pt;

/// The process ink channels, in plate order.
pub const PROCESS: [&str; 4] = ["Cyan", "Magenta", "Yellow", "Black"];

/// What one plate is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlateKind {
    /// One of the four process plates.
    Process,
    /// A plate of its own for a premixed ink.
    Spot,
}

/// How a plate is to be screened.
#[derive(Debug, Clone, PartialEq)]
pub struct Halftone {
    /// Screen angle in degrees. Two inks sharing a plate must differ, and
    /// two plates that moire against each other must differ too.
    pub angle: Pt,
    /// Screen frequency in lines per inch.
    pub frequency: f32,
}

impl Default for Halftone {
    fn default() -> Self {
        Halftone {
            angle: 45.0,
            frequency: 175.0,
        }
    }
}

impl Halftone {
    /// A rosette that keeps the four process plates from moiring.
    ///
    /// The classic separations are 15 degrees apart because at 45 the
    /// plates sit on top of each other's harmonics.
    pub fn process(index: usize) -> Halftone {
        Halftone {
            angle: 15.0 + (index as Pt % 4.0) * 30.0,
            frequency: 175.0,
        }
    }
}

/// One plate in the output.
#[derive(Debug, Clone, PartialEq)]
pub struct Plate {
    pub name: String,
    pub kind: PlateKind,
    /// The process channel index, for a process plate: 0..4 as C, M, Y, K.
    pub process_index: Option<usize>,
    /// Display colour, only ever for a screen preview.
    pub preview_rgb: [f32; 3],
    /// The ink's Lab value, kept so a spot can be re-emitted with its
    /// real definition rather than a screen approximation.
    pub lab: [f32; 3],
    pub halftone: Halftone,
}

/// The resolved set of plates for one page.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatePlan {
    pub plates: Vec<Plate>,
    /// Plate index per process channel, C, M, Y, K. A channel with no
    /// ink anywhere in the document still has a plate, because a press
    /// expects all four and a missing one reads as an error.
    pub process: [usize; 4],
    /// Ink name -> the plate its coverage goes on, with the weight each
    /// coat carries. A spot converted to process maps to the plates its
    /// CMYK build needs, so this is a list of (plate, weight).
    pub ink_plates: Vec<(String, Vec<(usize, f32)>)>,
    /// Full definitions parallel to ink_plates: unrelated process paints
    /// may share a display name without sharing a CMYK build.
    resolved_inks: Vec<Ink>,
}

impl PlatePlan {
    /// Build the plan for a document's inks under a manager's rules,
    /// using the layout model's preview-grade CMYK conversion.
    pub fn build(inks: &[Ink], manager: &InkManager) -> PlatePlan {
        PlatePlan::with_build(inks, manager, &crate::build::NaiveBuild)
    }

    /// Build the plan with a caller-supplied CMYK source.
    pub fn with_build(
        inks: &[Ink],
        manager: &InkManager,
        builds: &dyn crate::build::CmykSource,
    ) -> PlatePlan {
        let plates: Vec<Plate> = PROCESS
            .iter()
            .enumerate()
            .map(|(i, name)| Plate {
                name: (*name).to_string(),
                kind: PlateKind::Process,
                process_index: Some(i),
                preview_rgb: PROCESS_RGB[i],
                lab: PROCESS_LAB[i],
                halftone: Halftone::process(i),
            })
            .collect();
        let process = [0usize, 1, 2, 3];
        let mut plan = PlatePlan {
            plates,
            process,
            ink_plates: Vec::new(),
            resolved_inks: Vec::new(),
        };

        // Spot plates are created as the inks are walked, so the order
        // follows the document's ink list rather than a second pass.
        for plated in manager.resolve(inks) {
            let targets = if plated.separate {
                let target = &plated.plate_of;
                match plan
                    .plates
                    .iter()
                    .position(|p| p.kind == PlateKind::Spot && p.name == *target)
                {
                    Some(index) => vec![(index, 1.0)],
                    None => {
                        // A spot nobody carries: give it a plate rather
                        // than losing it.
                        let owner = inks
                            .iter()
                            .find(|ink| ink.name == *target)
                            .unwrap_or(plated.ink);
                        vec![(plan.push_plate(spot_plate(owner)), 1.0)]
                    }
                }
            } else {
                // A process ink lays each channel at its own strength.
                // Weighting them all equally would turn a 28% magenta
                // into a flood of ink.
                let owner = inks
                    .iter()
                    .find(|ink| ink.name == plated.plate_of)
                    .filter(|_| plated.alias_of.is_some())
                    .unwrap_or(plated.ink);
                builds
                    .build(owner)
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| **v > 0.0)
                    .map(|(i, v)| (process[i], v.clamp(0.0, 1.0)))
                    .collect()
            };
            plan.ink_plates.push((plated.ink.name.clone(), targets));
            plan.resolved_inks.push(plated.ink.clone());
        }

        plan
    }

    /// The plate and weight an ink's coverage belongs at.
    ///
    /// An ink with no entry in the plan still has to resolve, because a
    /// document can reference a swatch it does not carry. Process
    /// channels are the safe default: printing on a plate that is not
    /// there loses the ink, whereas printing on process ink is merely
    /// wrong in a way preflight can report.
    pub fn coats_for(&self, ink: &Ink) -> Vec<(usize, f32)> {
        if let Some((_, targets)) = self
            .resolved_inks
            .iter()
            .position(|i| i == ink)
            .and_then(|index| self.ink_plates.get(index))
        {
            return targets.clone();
        }
        if ink.spot {
            // A spot nobody has heard of needs a plate that does not
            // exist yet. This must be read, not written, so the caller
            // adds it with `add_spot`; returning the index just past the
            // end would be a silently wrong plate index.
            return Vec::new();
        }
        self.plates_for_cmyk(ink.to_cmyk())
    }

    /// Whether a plate index is one the plan actually has.
    pub fn has_plate(&self, index: usize) -> bool {
        index < self.plates.len()
    }

    /// The process plates a CMYK build needs, with each channel's share.
    pub fn plates_for_cmyk(&self, cmyk: [f32; 4]) -> Vec<(usize, f32)> {
        cmyk.iter()
            .enumerate()
            .filter(|(_, v)| **v > 0.0)
            .map(|(i, v)| (self.process[i], v.clamp(0.0, 1.0)))
            .collect()
    }

    /// Whether an ink is separated onto its own plate, which decides
    /// whether overprint means anything for it.
    pub fn is_separated(&self, ink: &Ink) -> bool {
        match self
            .resolved_inks
            .iter()
            .position(|i| i == ink)
            .and_then(|index| self.ink_plates.get(index))
        {
            Some((_, targets)) => targets.iter().any(|(index, _)| {
                self.plates
                    .get(*index)
                    .map(|p| p.kind == PlateKind::Spot)
                    .unwrap_or(false)
            }),
            None => ink.spot,
        }
    }

    /// A plate for a spot the plan has not seen, added on demand.
    ///
    /// This registers the mapping as well as the plate, so a later
    /// `plates_for` finds the plate it just created instead of inventing
    /// another index past the end of the list.
    pub fn add_spot(&mut self, ink: &Ink) -> usize {
        // A same-named process paint may have no coats (white). Only a
        // spot plate is a reusable destination for this operation.
        let index = self
            .plates
            .iter()
            .position(|plate| plate.kind == PlateKind::Spot && plate.name == ink.name)
            .unwrap_or_else(|| self.push_plate(spot_plate(ink)));
        if let Some(existing) = self.resolved_inks.iter().position(|entry| entry == ink) {
            self.ink_plates[existing].1 = vec![(index, 1.0)];
        } else {
            self.ink_plates.push((ink.name.clone(), vec![(index, 1.0)]));
            self.resolved_inks.push(ink.clone());
        }
        index
    }

    fn push_plate(&mut self, plate: Plate) -> usize {
        self.plates.push(plate);
        self.plates.len() - 1
    }
}

/// Screen approximations for the process inks, C, M, Y, K.
const PROCESS_RGB: [[f32; 3]; 4] = [
    [0.0, 0.68, 0.94],
    [0.93, 0.11, 0.55],
    [1.0, 0.94, 0.0],
    [0.0, 0.0, 0.0],
];

/// Lab values for the process inks, C, M, Y, K.
const PROCESS_LAB: [[f32; 3]; 4] = [
    [56.0, -37.0, -50.0],
    [48.0, 74.0, -3.0],
    [97.0, -21.0, 94.0],
    [0.0, 0.0, 0.0],
];

/// A plate for a separated ink, keeping its real definition.
fn spot_plate(ink: &Ink) -> Plate {
    Plate {
        name: ink.name.clone(),
        kind: PlateKind::Spot,
        process_index: None,
        preview_rgb: ink.preview_rgb,
        lab: ink.lab,
        halftone: Halftone::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::ink::{InkAlias, InkManager};

    fn inks() -> Vec<Ink> {
        vec![
            Ink::black(),
            Ink::process("Cyan", [0.0, 0.68, 0.94]),
            Ink::spot("PANTONE 032 C", [50.0, 60.0, 55.0]),
        ]
    }

    #[test]
    fn every_document_gets_the_four_process_plates() {
        // A press expects four whether or not the document uses them, and
        // a missing plate reads as an error rather than an empty one.
        let plan = PlatePlan::build(&inks(), &InkManager::with_defaults());
        assert_eq!(plan.plates.len(), 5);
        assert_eq!(plan.plates[0].name, "Cyan");
        assert_eq!(plan.plates[3].name, "Black");
        for (i, name) in PROCESS.iter().enumerate() {
            assert_eq!(plan.plates[i].kind, PlateKind::Process);
            assert_eq!(plan.plates[i].process_index, Some(i));
            assert!(plan.plates[i].name == *name);
        }
    }

    #[test]
    fn a_spot_gets_a_plate_of_its_own() {
        let plan = PlatePlan::build(&inks(), &InkManager::with_defaults());
        let spot = &plan.ink_plates[2];
        assert_eq!(spot.0, "PANTONE 032 C");
        assert_eq!(spot.1.len(), 1);
        // A separated spot lays its plate at full strength.
        assert_eq!(spot.1[0].1, 1.0);
        let plate = &plan.plates[spot.1[0].0];
        assert_eq!(plate.kind, PlateKind::Spot);
        assert_eq!(plate.name, "PANTONE 032 C");
        // A spot's real definition survives, not a screen approximation.
        assert_eq!(plate.lab, [50.0, 60.0, 55.0]);
    }

    #[test]
    fn a_process_ink_maps_to_the_channels_its_colour_needs() {
        let plan = PlatePlan::build(&inks(), &InkManager::with_defaults());
        let targets = plan.coats_for(&Ink::process("Cyan", [0.0, 0.68, 0.94]));
        assert!(targets.iter().any(|(p, _)| *p == 0), "no cyan plate");
        // A saturated cyan still needs black to reach the colour.
        assert!(targets.iter().any(|(p, _)| *p == 3), "no black plate");
    }

    #[test]
    fn a_process_ink_weighs_each_channel_by_its_own_share() {
        // Weighting every channel equally would turn a 28% magenta into
        // a flood of ink, which is the whole reason a build is per
        // channel rather than a single tint.
        let plan = PlatePlan::with_build(
            &inks(),
            &InkManager::with_defaults(),
            &crate::build::NamedBuilds::new()
                .with("Cyan", [1.0, 0.25, 0.0, 0.0])
                .with("PANTONE 032 C", [0.0, 0.0, 0.0, 0.0])
                .fallback([0.0, 0.0, 0.0, 1.0]),
        );
        let entry = plan
            .ink_plates
            .iter()
            .find(|(n, _)| n == "Cyan")
            .expect("no entry for Cyan");
        let magenta = entry.1.iter().find(|(p, _)| *p == 1);
        assert_eq!(magenta.map(|(_, w)| *w), Some(0.25));
    }

    #[test]
    fn a_spot_converted_to_process_goes_on_the_process_plates() {
        let mut manager = InkManager::with_defaults();
        manager.set_rule("PANTONE 032 C", InkAlias::ConvertToProcess);
        let plan = PlatePlan::build(&inks(), &manager);
        let spot = &plan.ink_plates[2];
        assert_ne!(spot.1[0].0, 4, "still separated after conversion");
        for (target, _) in &spot.1 {
            assert!(*target < 4, "targeted a spot plate");
        }
        // And no spot plate was created for it.
        assert_eq!(plan.plates.len(), 4);
    }

    #[test]
    fn an_alias_lands_on_its_targets_plate() {
        let mut inks = inks();
        inks.push(Ink::spot("PANTONE 032 C U", [50.0, 60.0, 55.0]));
        let mut manager = InkManager::with_defaults();
        manager.set_rule("PANTONE 032 C U", InkAlias::Alias("PANTONE 032 C".into()));
        let plan = PlatePlan::build(&inks, &manager);
        let a = plan
            .ink_plates
            .iter()
            .find(|(n, _)| n == "PANTONE 032 C")
            .unwrap();
        let b = plan
            .ink_plates
            .iter()
            .find(|(n, _)| n == "PANTONE 032 C U")
            .unwrap();
        assert_eq!(a.1, b.1);
    }

    #[test]
    fn alias_chains_keep_the_final_plate_definition_in_every_resource_order() {
        let inks = [
            Ink::spot("first", [20.0, 30.0, 40.0]),
            Ink::spot("middle", [50.0, 60.0, 70.0]),
            Ink::spot("final", [80.0, -30.0, 10.0]),
        ];
        let mut manager = InkManager::with_defaults();
        manager.set_rule("first", InkAlias::Alias("middle".into()));
        manager.set_rule("middle", InkAlias::Alias("final".into()));
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let ordered: Vec<_> = order.iter().map(|i| inks[*i].clone()).collect();
            let plan = PlatePlan::build(&ordered, &manager);
            assert_eq!(plan.plates.len(), 5);
            assert_eq!(plan.plates[4].name, "final");
            assert_eq!(plan.plates[4].lab, inks[2].lab);
            for ink in &inks {
                assert_eq!(plan.coats_for(ink), vec![(4, 1.0)]);
            }
            let mut process = manager.clone();
            process.set_rule("final", InkAlias::ConvertToProcess);
            let plan = PlatePlan::build(&ordered, &process);
            assert_eq!(plan.plates.len(), 4);
            for ink in &inks {
                assert_eq!(plan.coats_for(ink), plan.plates_for_cmyk(inks[2].to_cmyk()));
            }
        }
    }

    #[test]
    fn process_paints_with_shared_names_keep_distinct_channel_builds() {
        let inks = [
            Ink::cmyk("same", [0.3, 0.0, 0.0, 0.0]),
            Ink::cmyk("same", [0.0, 0.6, 0.0, 0.0]),
            Ink::cmyk("same", [0.0, 0.0, 0.0, 0.9]),
        ];
        for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2]] {
            let plan = PlatePlan::build(
                &order.map(|i| inks[i].clone()),
                &InkManager::with_defaults(),
            );
            for ink in &inks {
                assert_eq!(plan.coats_for(ink), plan.plates_for_cmyk(ink.to_cmyk()));
            }
        }
    }

    #[test]
    fn the_process_plates_are_rosetted_15_degrees_apart() {
        let plan = PlatePlan::build(&inks(), &InkManager::with_defaults());
        let angles: Vec<Pt> = plan.plates[..4].iter().map(|p| p.halftone.angle).collect();
        assert_eq!(angles, vec![15.0, 45.0, 75.0, 105.0]);
        // No two process plates share an angle, which is what stops them
        // moiring against each other.
        let mut sorted = angles.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted.dedup();
        assert_eq!(sorted.len(), 4);
    }

    #[test]
    fn an_unknown_process_ink_still_resolves_to_the_process_plates() {
        // A document can reference a swatch it does not carry, and
        // dropping its ink silently is worse than mis-placing it.
        let plan = PlatePlan::build(&inks(), &InkManager::with_defaults());
        let unknown = Ink::process("Missing", [0.2, 0.4, 0.6]);
        let targets = plan.coats_for(&unknown);
        assert!(!targets.is_empty());
        for (index, _) in targets {
            assert!(plan.has_plate(index));
        }
    }

    #[test]
    fn an_unknown_spot_reports_no_plate_rather_than_a_wrong_one() {
        // Returning the index just past the end would be a silently
        // wrong plate, which is far worse than reporting none.
        let plan = PlatePlan::build(&inks(), &InkManager::with_defaults());
        let unknown = Ink::spot("MISSING SPOT", [10.0, 20.0, 30.0]);
        assert!(plan.coats_for(&unknown).is_empty());
        assert!(plan.is_separated(&unknown));
    }

    #[test]
    fn a_spot_the_plan_has_not_seen_can_be_added_on_demand() {
        let mut plan = PlatePlan::build(&inks(), &InkManager::with_defaults());
        let before = plan.plates.len();
        let spot = Ink::spot("LATE ARRIVAL", [10.0, 20.0, 30.0]);
        let index = plan.add_spot(&spot);
        assert_eq!(plan.plates.len(), before + 1);
        assert_eq!(plan.plates[index].name, "LATE ARRIVAL");
        // The mapping is registered, so a later lookup finds this plate
        // rather than inventing another index past the end.
        assert_eq!(plan.coats_for(&spot), vec![(index, 1.0)]);
        // And adding it twice is idempotent.
        assert_eq!(plan.add_spot(&spot), index);
        assert_eq!(plan.plates.len(), before + 1);
    }

    #[test]
    fn adding_a_spot_never_reuses_same_named_process_coats() {
        for build in [[0.0; 4], [0.4, 0.0, 0.0, 0.0], [0.1, 0.2, 0.3, 0.4]] {
            let process = Ink::cmyk("Shared", build);
            let spot = Ink::spot("Shared", [50.0, 20.0, 30.0]);
            let mut plan =
                PlatePlan::build(std::slice::from_ref(&process), &InkManager::with_defaults());
            let original = plan.coats_for(&process);
            let index = plan.add_spot(&spot);
            assert_eq!(plan.plates[index].kind, PlateKind::Spot);
            assert_eq!(plan.coats_for(&spot), vec![(index, 1.0)]);
            assert_eq!(plan.coats_for(&process), original);
            assert_eq!(plan.add_spot(&spot), index);
            assert_eq!(plan.plates.len(), 5);
        }
    }

    #[test]
    fn the_plan_is_ordered_deterministically() {
        // Plate order is meaningful to a press and to the file it writes,
        // so it must not depend on iteration order.
        let a = PlatePlan::build(&inks(), &InkManager::with_defaults());
        let b = PlatePlan::build(&inks(), &InkManager::with_defaults());
        assert_eq!(a, b);
        let names: Vec<&str> = a.plates.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Cyan", "Magenta", "Yellow", "Black", "PANTONE 032 C"]
        );
    }
}
