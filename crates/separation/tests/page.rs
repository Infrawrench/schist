//! End-to-end separation of a page, checking the invariants that matter
//! on press rather than the ones that are easy to assert.

use schist_layout::compose::{compose_thread, InsetsLike};
use schist_layout::geometry::{mm, Insets, Point, Rect};
use schist_layout::ink::{Ink, InkAlias};
use schist_layout::model::{
    blank_a4, FrameOverflow, GraphicFit, LayoutDocument, LayoutObject, Link, ObjectId, PlacedObject,
};
use schist_layout::story::Story;
use schist_layout::Page;
use schist_separation::{
    separate_page, separate_page_built, separate_page_without_graphics, GraphicSource, NamedBuilds,
    NoGraphics, OutputSettings, PlacedGraphic, PlateKind, Severity,
};

#[test]
fn authored_fills_and_pen_strokes_are_opaque_and_have_distinct_masks() {
    use schist_layout::authoring::{self, Paint};
    let mut doc = LayoutDocument::new(vec![Page::new("1", 100.0, 100.0)]);
    let ink = doc
        .inks
        .iter()
        .find(|ink| ink.to_cmyk()[3] > 0.9)
        .unwrap()
        .name
        .clone();
    authoring::rectangle(
        &mut doc,
        &mut schist_layout::History::default(),
        0,
        Rect::new(10.0, 10.0, 30.0, 30.0),
        Paint::filled(&ink),
    )
    .unwrap();
    let mut path = schist_layout::ShapePath::ellipse(30.0, 30.0);
    path.map_points(|p| p + Point::new(60.0, 10.0));
    authoring::path_shape(
        &mut doc,
        &mut schist_layout::History::default(),
        0,
        path,
        Paint::stroked(&ink, 2.0),
    )
    .unwrap();
    for dpi in [72.0, 144.0, 300.0] {
        let settings = OutputSettings::at(dpi);
        let separated = separate_page_without_graphics(&doc, 0, settings).unwrap();
        let black = separated
            .separation
            .plate(separated.plan.process[3])
            .unwrap();
        let at = |x, y| black.at(settings.to_pixels(x), settings.to_pixels(y));
        assert!(at(25.0, 25.0) > 0.99, "new fill disappeared");
        assert_eq!(
            at(75.0, 25.0),
            0.0,
            "stroke incorrectly filled its interior"
        );
        assert!(at(60.0, 25.0) > 0.7, "curve stroke disappeared");
        assert_eq!(at(55.0, 25.0), 0.0);
    }
}

#[test]
fn pdf_plate_matrix_preserves_width_and_places_bleed_at_the_sheet_origin() {
    for dpi in [72.0, 144.0, 300.0] {
        for bleed in [0.0, 6.0, 12.0] {
            let mut doc = blank_a4();
            doc.pages[0] = Page::new("1", 180.0, 240.0);
            doc.pages[0].bleed = (bleed).into();
            doc.add_object(shape_at(
                0,
                Rect::new(0.0, 0.0, 20.0, 20.0),
                Some(Ink::black()),
                false,
            ));
            let settings = OutputSettings::at(dpi);
            let separated = separate_page_without_graphics(&doc, 0, settings).unwrap();
            let output = PageOutput {
                separated: &separated,
                trim: (180.0, 240.0),
                bleed: bleed.into(),
                slug: schist_layout::Insets::ZERO,
                settings,
                imposition: Imposition::default(),
                marks: Marks::default(),
                overprint: true,
            };
            let content = schist_separation::page_content(&output, &drawn_plates(&separated));
            let line = content
                .lines()
                .find(|line| line.contains(" cm /InkImage"))
                .unwrap();
            let parts: Vec<_> = line.split_whitespace().collect();
            let matrix: Vec<f32> = parts[3..9]
                .iter()
                .map(|value| value.parse().unwrap())
                .collect();
            assert!(
                (matrix[0] - (180.0 + bleed * 2.0)).abs() < 0.01,
                "width must not depend on x: {line}"
            );
            assert!((matrix[3] - (240.0 + bleed * 2.0)).abs() < 0.01);
            assert!(
                matrix[4].abs() < 0.01 && matrix[5].abs() < 0.01,
                "complete bleed plate starts at the sheet origin: {line}"
            );
        }
    }
}

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

fn shape_at(page: usize, bounds: Rect, fill: Option<Ink>, overprint: bool) -> PlacedObject {
    PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: ObjectId::next(),
        page,
        bounds,
        object: LayoutObject::Shape {
            path: square(bounds.width.min(bounds.height)),
            fill,
            stroke: None,
            stroke_width: 0.0,
            fill_overprint: overprint,
            stroke_overprint: false,
            tints: Default::default(),
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Shape".into(),
        locked: false,
        overprint,
        transparency: 1.0,
    }
}

fn doc_with(inks: Vec<Ink>, page: Page) -> LayoutDocument {
    let mut doc = blank_a4();
    doc.pages = vec![page];
    doc.spreads = vec![schist_layout::Spread::single(0)];
    doc.inks = inks;
    doc
}

/// A source that returns a solid block, standing in for a decoded image.
struct Solid(f32);

impl GraphicSource for Solid {
    fn sample(
        &self,
        _link: &Link,
        placement: &schist_separation::GraphicPlacement,
    ) -> Option<PlacedGraphic> {
        Some(PlacedGraphic::solid(
            placement.dest,
            [self.0, 0.0, 0.0, 0.0],
        ))
    }
}

#[test]
fn an_empty_page_produces_empty_plates() {
    let doc = doc_with(vec![Ink::black()], Page::a4());
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    assert_eq!(page.plan.plates.len(), 4);
    for plate in page.separation.plates() {
        assert!(plate.peak() == 0.0, "a plate has ink on an empty page");
    }
    assert!(page.report.is_printable());
}

#[test]
fn a_black_shape_lands_on_the_black_plate_only() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    doc.add_object(shape_at(
        0,
        Rect::new(mm(50.0), mm(50.0), mm(40.0), mm(40.0)),
        Some(Ink::black()),
        false,
    ));
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    let black = page.plan.process[3];
    let k = page.separation.plate(black).unwrap();
    assert!(k.peak() > 0.9, "no black ink at all");
    // And nothing on the colour plates.
    for (i, index) in page.plan.process.iter().enumerate() {
        if i == 3 {
            continue;
        }
        let p = page.separation.plate(*index).unwrap();
        assert_eq!(p.peak(), 0.0, "black ink leaked onto plate {i}");
    }
}

#[test]
fn a_spot_shape_lands_on_its_own_plate_and_no_others() {
    let spot = Ink::spot("PANTONE 032 C", [50.0, 60.0, 55.0]);
    let mut doc = doc_with(vec![Ink::black(), spot.clone()], Page::a4());
    doc.add_object(shape_at(
        0,
        Rect::new(mm(50.0), mm(50.0), mm(40.0), mm(40.0)),
        Some(spot),
        false,
    ));
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    let spot_plate = page
        .plan
        .plates
        .iter()
        .position(|p| p.kind == PlateKind::Spot && p.name == "PANTONE 032 C")
        .expect("no spot plate");
    assert!(page.separation.plate(spot_plate).unwrap().peak() > 0.9);
    // A separated spot contributes nothing to the process plates.
    for index in page.plan.process {
        assert_eq!(page.separation.plate(index).unwrap().peak(), 0.0);
    }
}

#[test]
fn a_spot_converted_to_process_goes_onto_the_process_plates() {
    let spot = Ink::spot("PANTONE 032 C", [50.0, 60.0, 55.0]);
    let mut doc = doc_with(vec![Ink::black(), spot.clone()], Page::a4());
    doc.ink_manager
        .set_rule("PANTONE 032 C", InkAlias::ConvertToProcess);
    doc.add_object(shape_at(
        0,
        Rect::new(mm(50.0), mm(50.0), mm(40.0), mm(40.0)),
        Some(spot),
        false,
    ));
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    // No spot plate was made...
    assert!(page.plan.plates.iter().all(|p| p.kind != PlateKind::Spot));
    // ...and the process plates carry the ink.
    let peak: f32 = page
        .plan
        .process
        .iter()
        .map(|i| page.separation.plate(*i).unwrap().peak())
        .fold(0.0, f32::max);
    assert!(peak > 0.5, "converted spot left no process ink");
}

#[test]
fn knockout_leaves_a_hole_and_overprint_does_not() {
    // A big black box, then a small *spot* mark inside it, read on the
    // black plate. Knockout punches the black out; overprint leaves it
    // standing. The spot has to differ from the black: a black mark on a
    // black box looks identical either way, which is exactly the bug
    // that makes this worth testing.
    let spot = Ink::spot("REGISTER", [80.0, 40.0, 0.0]);
    let measure = |overprint: bool| {
        let mut doc = doc_with(vec![Ink::black(), spot.clone()], Page::a4());
        doc.add_object(shape_at(
            0,
            Rect::new(mm(40.0), mm(40.0), mm(60.0), mm(60.0)),
            Some(Ink::black()),
            false,
        ));
        doc.add_object(shape_at(
            0,
            Rect::new(mm(65.0), mm(65.0), mm(10.0), mm(10.0)),
            Some(spot.clone()),
            overprint,
        ));
        let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
        let k = page.separation.plate(page.plan.process[3]).unwrap();
        let settings = OutputSettings::at(150.0);
        let x = settings.to_pixels(mm(70.0));
        let y = settings.to_pixels(mm(70.0));
        k.at(x, y)
    };
    let knocked = measure(false);
    let overprinted = measure(true);
    assert!(
        overprinted > knocked + 0.5,
        "overprint did not keep the black under it"
    );
    assert!(overprinted > 0.9, "the box is solid overprinted");
}

#[test]
fn overprint_knocks_out_nothing_where_two_shapes_meet() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    // Two shapes side by side, no gap: a knockout would leave a seam.
    doc.add_object(shape_at(
        0,
        Rect::new(mm(40.0), mm(40.0), mm(30.0), mm(30.0)),
        Some(Ink::black()),
        false,
    ));
    doc.add_object(shape_at(
        0,
        Rect::new(mm(70.0), mm(40.0), mm(30.0), mm(30.0)),
        Some(Ink::black()),
        true,
    ));
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    let k = page.separation.plate(page.plan.process[3]).unwrap();
    let settings = OutputSettings::at(150.0);
    // Across the shared edge, coverage must be continuous.
    let edge = settings.to_pixels(mm(70.0));
    let y = settings.to_pixels(mm(55.0));
    let before = k.at(edge - 1, y);
    let after = k.at(edge, y);
    assert!(
        (before - after).abs() < 0.1,
        "seam at the join: {before} then {after}"
    );
}

#[test]
fn transparency_lays_proportionally_less_ink() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    let mut placed = shape_at(
        0,
        Rect::new(mm(50.0), mm(50.0), mm(40.0), mm(40.0)),
        Some(Ink::black()),
        false,
    );
    placed.transparency = 0.5;
    doc.add_object(placed);
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    let k = page.separation.plate(page.plan.process[3]).unwrap();
    let peak = k.peak();
    assert!(peak > 0.4 && peak < 0.6, "50% transparent gave peak {peak}");
}

#[test]
fn text_puts_ink_where_the_glyphs_are() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    let story = doc.add_story(Story::from_text(
        "The quick brown fox jumps over the lazy dog.",
        "Body",
    ));
    doc.add_object(PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: ObjectId::next(),
        page: 0,
        bounds: Rect::new(mm(20.0), mm(20.0), mm(170.0), mm(100.0)),
        object: LayoutObject::TextFrame {
            text_path: None,
            story,
            columns: 1,
            gutter: 0.0,
            insets: Insets::ZERO,
            overflow: FrameOverflow::Clip,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Body".into(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    });
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    let k = page.separation.plate(page.plan.process[3]).unwrap();
    let peak = k.peak();
    assert!(peak > 0.8, "text produced no solid ink, peak {peak}");
    // Glyphs cover only part of the frame, so the plate is not flooded.
    // Antialiased edges carry very little coverage, so this counts any
    // ink at all rather than solid ink.
    let filled = k.data.iter().filter(|v| **v > 0.01).count() as f32 / k.data.len() as f32;
    assert!(
        filled < 0.25,
        "the frame is {filled} covered, which is not text"
    );
    assert!(filled > 0.0005, "no text at all");
}

#[test]
fn a_fully_knocked_out_frame_leaves_no_ink() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    let story = doc.add_story(Story::from_text("Invisible.", "Body"));
    let mut placed = PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: ObjectId::next(),
        page: 0,
        bounds: Rect::new(mm(20.0), mm(20.0), mm(100.0), mm(50.0)),
        object: LayoutObject::TextFrame {
            text_path: None,
            story,
            columns: 1,
            gutter: 0.0,
            insets: Insets::ZERO,
            overflow: FrameOverflow::Clip,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Hidden".into(),
        locked: false,
        overprint: false,
        transparency: 0.0,
    };
    placed.transparency = 0.0;
    doc.add_object(placed);
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    assert_eq!(
        page.separation.plate(page.plan.process[3]).unwrap().peak(),
        0.0
    );
}

#[test]
fn a_placed_graphic_separates_onto_its_own_channels() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    doc.add_object(PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: ObjectId::next(),
        page: 0,
        bounds: Rect::new(mm(20.0), mm(20.0), mm(80.0), mm(80.0)),
        object: LayoutObject::GraphicFrame {
            link: Link::new("/tmp/cyan.psd"),
            embedded: false,
            fit: GraphicFit::Fill,
            crop: None,
            image_transform: Default::default(),
            clip_path: None,
            scale: 1.0,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Photo".into(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    });
    let page = separate_page(&doc, 0, OutputSettings::at(150.0), &Solid(1.0)).unwrap();
    let c = page.separation.plate(page.plan.process[0]).unwrap();
    let m = page.separation.plate(page.plan.process[1]).unwrap();
    let y_ = page.separation.plate(page.plan.process[2]).unwrap();
    let k = page.separation.plate(page.plan.process[3]).unwrap();
    assert!(c.peak() > 0.9, "no cyan");
    assert_eq!(m.peak(), 0.0, "magenta on a pure cyan image");
    assert_eq!(y_.peak(), 0.0, "yellow on a pure cyan image");
    assert_eq!(k.peak(), 0.0, "black on a pure cyan image");
}

#[test]
fn a_missing_link_is_reported_rather_than_silently_dropped() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    let mut link = Link::new("/nonexistent/photo.psd");
    link.present = false;
    doc.add_object(PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: ObjectId::next(),
        page: 0,
        bounds: Rect::new(mm(20.0), mm(20.0), mm(80.0), mm(80.0)),
        object: LayoutObject::GraphicFrame {
            link,
            embedded: false,
            fit: GraphicFit::Fill,
            crop: None,
            image_transform: Default::default(),
            clip_path: None,
            scale: 1.0,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Missing".into(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    });
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    assert!(!page.report.is_printable());
    let finding = page
        .report
        .findings
        .iter()
        .find(|f| f.severity == Severity::Error)
        .expect("no error for a missing link");
    assert!(finding.message.contains("photo.psd"), "{}", finding.message);
}

#[test]
fn an_embedded_link_is_not_treated_as_missing() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    let mut link = Link::new("/nonexistent/photo.psd");
    link.present = false;
    doc.add_object(PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: ObjectId::next(),
        page: 0,
        bounds: Rect::new(mm(20.0), mm(20.0), mm(80.0), mm(80.0)),
        object: LayoutObject::GraphicFrame {
            link,
            embedded: true,
            fit: GraphicFit::Fill,
            crop: None,
            image_transform: Default::default(),
            clip_path: None,
            scale: 1.0,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Embedded".into(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    });
    // Embedded means an absent external file is fine, provided the
    // caller actually supplies the embedded pixels.
    let page = separate_page(&doc, 0, OutputSettings::at(150.0), &Solid(0.5)).unwrap();
    assert!(page.report.is_printable());
}

#[test]
fn unresolved_graphics_never_pass_preflight_in_either_separation_path() {
    // Neither the link's cached existence nor its embedded flag proves
    // that the caller can supply the pixels. Exercise every combination.
    for embedded in [false, true] {
        for present in [false, true] {
            let mut doc = blank_a4();
            doc.pages[0].width = 24.0;
            doc.pages[0].height = 24.0;
            let mut link = Link::new("unavailable.psd");
            link.present = present;
            doc.add_object(PlacedObject {
                hidden: false,
                appearance: Default::default(),
                id: ObjectId::next(),
                page: 0,
                bounds: Rect::new(0.0, 0.0, 12.0, 12.0),
                object: LayoutObject::GraphicFrame {
                    link,
                    embedded,
                    fit: GraphicFit::Fill,
                    crop: None,
                    image_transform: Default::default(),
                    clip_path: None,
                    scale: 1.0,
                },
                rotation: 0.0,
                transform: Default::default(),
                name: String::new(),
                locked: false,
                overprint: false,
                transparency: 1.0,
            });
            for built in [false, true] {
                let settings = OutputSettings::at(72.0);
                let page = if built {
                    separate_page_built(
                        &doc,
                        0,
                        settings,
                        &NoGraphics,
                        &schist_separation::NaiveBuild,
                    )
                } else {
                    separate_page_without_graphics(&doc, 0, settings)
                }
                .unwrap();
                assert!(
                    !page.report.is_printable(),
                    "embedded={embedded}, present={present}, built={built}"
                );
                assert_eq!(page.report.errors(), 1, "one unresolved graphic, one error");
                let key = if !embedded && !present {
                    "design.preflight_missing_link"
                } else {
                    "design.preflight_unavailable_graphic"
                };
                assert_eq!(
                    page.report.findings[0].message,
                    schist_i18n::tf!(key, path = "unavailable.psd")
                );
            }
        }
    }
}

#[test]
fn a_parent_page_contributes_its_objects_to_every_page() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    doc.add_page(Page::a4());
    let masthead = shape_at(
        0,
        Rect::new(0.0, 0.0, mm(170.0), mm(20.0)),
        Some(Ink::black()),
        false,
    );
    doc.parents.push(schist_layout::model::ParentPage {
        name: "A-Master".into(),
        sheets: Vec::new(),
        placements: Vec::new(),
        applied_to: vec![0, 1],
        based_on: None,
        objects: vec![schist_layout::model::ParentObject {
            object: masthead,
            overridden_on: Vec::new(),
        }],
        hidden: false,
    });
    for page in 0..2 {
        let separated =
            separate_page_without_graphics(&doc, page, OutputSettings::at(150.0)).unwrap();
        assert!(
            separated
                .separation
                .plate(separated.plan.process[3])
                .unwrap()
                .peak()
                > 0.9,
            "page {page} is missing the masthead"
        );
    }
}

#[test]
fn the_ink_limit_breach_is_reported_with_numbers() {
    let mut doc = vec_page();
    // Four overlapping 100% tints of colour, which is 400% coverage.
    let inks = [
        Ink::process("C", [0.0, 1.0, 1.0]),
        Ink::process("M", [1.0, 0.0, 1.0]),
        Ink::process("Y", [1.0, 1.0, 0.0]),
        Ink::black(),
    ];
    doc.inks = inks.to_vec();
    doc.ink_manager.total_area_limit = Some(3.0);
    for (i, ink) in inks.iter().enumerate() {
        // Overprinting and stacked, so every pixel carries all four.
        // Four knockouts would correctly leave only the topmost.
        let offset = mm(50.0 + i as f32 * 0.5);
        doc.add_object(shape_at(
            0,
            Rect::new(offset, offset, mm(60.0), mm(60.0)),
            Some(ink.clone()),
            true,
        ));
    }
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    assert!(
        page.report.peak_total_area > 3.5,
        "peak {}",
        page.report.peak_total_area
    );
    assert!(page.report.pixels_over_limit > 0);
    assert!(!page.report.is_printable());
    assert!(page
        .report
        .findings
        .iter()
        .any(|f| f.severity == Severity::Error && f.message.contains("300%")));
}

#[test]
fn an_ink_limit_that_is_not_breached_stays_quiet() {
    let mut doc = vec_page();
    doc.inks = vec![Ink::black()];
    doc.ink_manager.total_area_limit = Some(3.0);
    doc.add_object(shape_at(
        0,
        Rect::new(mm(50.0), mm(50.0), mm(40.0), mm(40.0)),
        Some(Ink::black()),
        false,
    ));
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    assert!(page.report.is_printable());
    assert!(page.report.pixels_over_limit == 0);
}

#[test]
fn under_colour_removal_pulls_ink_back_onto_the_black_plate() {
    // UCR withdraws from the smallest channel, so it needs a build that
    // has one. The layout model's preview conversion always zeroes a
    // channel, which is exactly why the CMYK build is injected.
    let build = |ucr: f32| {
        let mut doc = vec_page();
        let ink = Ink::process("Sepia", [0.5, 0.5, 0.5]);
        doc.inks = vec![ink.clone()];
        doc.ink_manager.ucr = ucr;
        doc.ink_manager.total_area_limit = None;
        doc.add_object(shape_at(
            0,
            Rect::new(mm(50.0), mm(50.0), mm(40.0), mm(40.0)),
            Some(ink),
            false,
        ));
        let builds = NamedBuilds::new().with("Sepia", [0.62, 0.48, 0.35, 0.18]);
        let page =
            separate_page_built(&doc, 0, OutputSettings::at(150.0), &NoGraphics, &builds).unwrap();
        let m = page.separation.plate(page.plan.process[1]).unwrap().peak();
        let k = page.separation.plate(page.plan.process[3]).unwrap().peak();
        (m, k)
    };
    let (m_off, k_off) = build(0.0);
    let (m_on, k_on) = build(1.0);
    assert!(k_off > 0.0, "the build's black never reached the plate");
    assert!(
        m_on < m_off,
        "UCR left the colour ink alone: {m_off} then {m_on}"
    );
    assert!(k_on > k_off, "UCR did not add black: {k_off} then {k_on}");
}

#[test]
fn the_injected_build_is_what_reaches_the_plates() {
    let mut doc = vec_page();
    let ink = Ink::process("Sepia", [0.5, 0.5, 0.5]);
    doc.inks = vec![ink.clone()];
    doc.ink_manager.total_area_limit = None;
    doc.add_object(shape_at(
        0,
        Rect::new(mm(50.0), mm(50.0), mm(40.0), mm(40.0)),
        Some(ink),
        false,
    ));
    let builds = NamedBuilds::new().with("Sepia", [0.62, 0.48, 0.35, 0.18]);
    let page =
        separate_page_built(&doc, 0, OutputSettings::at(150.0), &NoGraphics, &builds).unwrap();
    let peaks: Vec<f32> = (0..4)
        .map(|i| page.separation.plate(page.plan.process[i]).unwrap().peak())
        .collect();
    // Each plate carries its own channel's share, not a solid flood.
    for (i, peak) in peaks.iter().enumerate() {
        assert!(*peak > 0.0, "channel {i} got no ink");
        assert!(*peak <= 1.0, "channel {i} is over full");
    }
    assert!(
        peaks[0] > peaks[1],
        "cyan should outweigh magenta: {peaks:?}"
    );
    assert!(
        peaks[2] > peaks[3],
        "yellow should outweigh black: {peaks:?}"
    );
}

#[test]
fn separation_emits_ink_channels_that_carry_the_coverage() {
    let spot = Ink::spot("PANTONE 032 C", [50.0, 60.0, 55.0]);
    let mut doc = doc_with(vec![Ink::black(), spot.clone()], Page::a4());
    doc.add_object(shape_at(
        0,
        Rect::new(mm(50.0), mm(50.0), mm(40.0), mm(40.0)),
        Some(spot),
        false,
    ));
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    let channels = page.to_ink_channels();
    assert_eq!(channels.len(), page.plan.plates.len());
    // The spot channel is marked as a spot so the PSD writer emits a
    // DisplayInfo entry, and a process plate is not.
    let spot_channel = channels
        .iter()
        .find(|c| c.info.name == "PANTONE 032 C")
        .expect("no spot channel");
    assert!(spot_channel.info.spot);
    assert!(
        !spot_channel.pixels.0.is_empty(),
        "no coverage carried over"
    );
    let cyan = channels.iter().find(|c| c.info.name == "Cyan").unwrap();
    assert!(!cyan.info.spot, "a process plate was marked as a spot");
}

#[test]
fn composition_and_separation_agree_on_where_the_text_is() {
    // If the two disagree, ink lands somewhere the canvas does not show
    // glyphs, and the failure is invisible until the press.
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    let story = doc.add_story(Story::from_text(
        "Agreement between the two passes.",
        "Body",
    ));
    let bounds = Rect::new(mm(20.0), mm(20.0), mm(170.0), mm(100.0));
    doc.add_object(PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: ObjectId::next(),
        page: 0,
        bounds,
        object: LayoutObject::TextFrame {
            text_path: None,
            story,
            columns: 1,
            gutter: 0.0,
            insets: Insets::ZERO,
            overflow: FrameOverflow::Clip,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Body".into(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    });
    let settings = OutputSettings::at(150.0);
    let thread = compose_thread(
        &doc,
        story,
        &[(
            ObjectId::next(),
            bounds,
            FrameOverflow::Clip,
            1,
            0.0,
            InsetsLike::default(),
        )],
    );
    let page = separate_page_without_graphics(&doc, 0, settings).unwrap();
    let k = page.separation.plate(page.plan.process[3]).unwrap();
    // Every composed line must have ink near the start of its box. The
    // left edge is the probe rather than the centre because a short line
    // does not fill its column, and the centre of a 170mm measure can sit
    // past the last glyph.
    for line in thread.lines().filter(|l| !l.forced_break) {
        let start_x = settings.to_pixels(line.bounds.x);
        let mid_y = settings.to_pixels(line.bounds.y + line.bounds.height / 2.0);
        let has_ink = (k.rect.left..k.rect.right).any(|x| {
            (k.rect.top..k.rect.bottom)
                .any(|y| (x - start_x).abs() < 60 && (y - mid_y).abs() < 30 && k.at(x, y) > 0.01)
        });
        assert!(has_ink, "no ink on a composed line at {start_x},{mid_y}");
    }
}

#[test]
fn a_higher_resolution_puts_the_same_ink_on_more_pixels() {
    let mut doc = doc_with(vec![Ink::black()], Page::a4());
    doc.add_object(shape_at(
        0,
        Rect::new(mm(50.0), mm(50.0), mm(40.0), mm(40.0)),
        Some(Ink::black()),
        false,
    ));
    let measure = |dpi: f32| {
        let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(dpi)).unwrap();
        let k = page.separation.plate(page.plan.process[3]).unwrap();
        (k.data.len(), k.peak())
    };
    let (coarse, _) = measure(150.0);
    let (fine, peak) = measure(300.0);
    assert!(
        fine > coarse * 3,
        "300dpi gave {fine} samples against {coarse}"
    );
    // And the shape is still solid, so the extra resolution did not
    // erode the edge into a grey fringe.
    assert!(peak > 0.95, "peak fell to {peak} at 300dpi");
}

#[test]
fn bleed_moves_the_page_origin_and_keeps_the_artwork_aligned() {
    let mut page = Page::a4();
    page.bleed = (mm(3.0)).into();
    let mut doc = doc_with(vec![Ink::black()], page);
    // A shape at the top-left of the trim, which with bleed is inset from
    // the paper's corner.
    doc.add_object(shape_at(
        0,
        Rect::new(0.0, 0.0, mm(20.0), mm(20.0)),
        Some(Ink::black()),
        false,
    ));
    let with_bleed = separate_page_without_graphics(
        &doc,
        0,
        OutputSettings {
            include_bleed: true,
            ..OutputSettings::at(150.0)
        },
    )
    .unwrap();
    let without = separate_page_without_graphics(
        &doc,
        0,
        OutputSettings {
            include_bleed: false,
            ..OutputSettings::at(150.0)
        },
    )
    .unwrap();
    // The output box is larger...
    assert!(with_bleed.separation.rect().width() > without.separation.rect().width());
    // The trim artwork stays at zero in page coordinates. Its distance
    // from the expanded sheet edge grows by the bleed.
    let offset_within_plate = |page: &schist_separation::SeparatedPage| {
        let k = page.separation.plate(page.plan.process[3]).unwrap();
        // The plate's own left edge, which with bleed is a negative
        // coordinate -- iterating from zero would miss the ink entirely.
        (k.rect.left..k.rect.right)
            .find(|x| (k.rect.top..k.rect.bottom).any(|y| k.at(*x, y) > 0.5))
            .map(|x| x - k.rect.left)
            .unwrap_or(-1)
    };
    assert_eq!(
        offset_within_plate(&with_bleed) - offset_within_plate(&without),
        OutputSettings::at(150.0).to_pixels(mm(3.0))
    );
}

fn vec_page() -> LayoutDocument {
    doc_with(vec![Ink::black()], Page::a4())
}

// ---- prepress PDF ----

use schist_separation::drawn_plates;
use schist_separation::pdf::{write_document, Imposition, Marks, PageOutput, Pdf};

fn separated_with_spot() -> schist_separation::SeparatedPage {
    let spot = Ink::spot("PANTONE 032 C", [50.0, 60.0, 55.0]);
    let mut doc = doc_with(vec![Ink::black(), spot.clone()], Page::a4());
    doc.add_object(shape_at(
        0,
        Rect::new(mm(20.0), mm(20.0), mm(60.0), mm(60.0)),
        Some(spot),
        false,
    ));
    separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap()
}

#[test]
fn a_separated_page_becomes_one_pdf_page_with_named_plates() {
    let page = separated_with_spot();
    let settings = OutputSettings::at(150.0);
    let bytes = write_document(
        std::slice::from_ref(&page),
        &[(595.0, 842.0)],
        &[0.0],
        settings,
    )
    .expect("the file writes");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.starts_with("%PDF-1.6"));
    assert!(text.contains("/Type /Catalog"));
    assert!(text.contains("/Type /Page"));
    // The spot is named as a Separation colour space, which is how a
    // PDF records an ink by name.
    assert!(
        text.contains("/Separation /PANTONE#20032#20C"),
        "no spot colour space"
    );
    // And the process plates are there too.
    for name in ["Cyan", "Magenta", "Yellow", "Black"] {
        assert!(text.contains(name), "{name} missing from the file");
    }
}

#[test]
fn the_file_records_bleed_trim_and_crop_marks() {
    let page = separated_with_spot();
    let settings = OutputSettings::at(150.0);
    let mut pdf = Pdf::new();
    let object = schist_separation::pdf::write_page(
        &mut pdf,
        &PageOutput {
            separated: &page,
            trim: (595.0, 842.0),
            bleed: (mm(3.0)).into(),
            slug: schist_layout::Insets::ZERO,
            settings,
            imposition: Imposition::single(),
            marks: Marks::default(),
            overprint: true,
        },
    )
    .expect("the page writes");
    assert!(object > 2);
    let bytes = pdf.finish(&[object]);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/TrimBox"), "no trim box");
    assert!(text.contains("/BleedBox"), "no bleed box");
    // A 3mm bleed on a 595 x 842pt page: the trim box is inset by the
    // bleed and the bleed box is the whole sheet.
    let bleed = mm(3.0);
    assert!(text.contains(&schist_separation::pdf::trim_box(
        (595.0, 842.0),
        bleed + 9.0
    )));
    assert!(text.contains(&format!(
        "/BleedBox [9.00 9.00 {:.2} {:.2}]",
        595.0 + 2.0 * bleed + 9.0,
        842.0 + 2.0 * bleed + 9.0
    )));
}

#[test]
fn overprint_is_recorded_in_a_graphics_state() {
    let page = separated_with_spot();
    let settings = OutputSettings::at(150.0);
    let write = |overprint: bool| {
        let mut pdf = Pdf::new();
        let object = schist_separation::pdf::write_page(
            &mut pdf,
            &PageOutput {
                separated: &page,
                trim: (595.0, 842.0),
                bleed: (0.0).into(),
                slug: schist_layout::Insets::ZERO,
                settings,
                imposition: Imposition::single(),
                marks: Marks::default(),
                overprint,
            },
        )
        .expect("the page writes");
        String::from_utf8_lossy(&pdf.finish(&[object])).into_owned()
    };
    // A PDF is the only place overprint survives to the press, so a
    // file without it has thrown the distinction away.
    assert!(write(true).contains("/OP true"));
    assert!(write(false).contains("/OP false"));
}

#[test]
fn a_composite_proof_knocks_every_plate_out() {
    let page = separated_with_spot();
    let images = drawn_plates(&page);
    let content = |overprint: bool| {
        schist_separation::page_content(
            &PageOutput {
                separated: &page,
                trim: (595.0, 842.0),
                bleed: (0.0).into(),
                slug: schist_layout::Insets::ZERO,
                settings: OutputSettings::at(150.0),
                imposition: Imposition::single(),
                marks: Marks::default(),
                overprint,
            },
            &images,
        )
    };
    // A PDF is the only place overprint survives to the press, so the
    // content has to name the state it drew each plate in.
    assert!(content(true).contains("/OP"), "no overprint state");
    assert!(
        !content(true).contains("/KO"),
        "a proof state in a press file"
    );
    // A proof shows the reader what they will see, and overprinting a
    // proof shows nothing.
    assert!(!content(false).contains("/OP"), "a proof still overprints");
    assert!(
        content(false).contains("/KO"),
        "the knockout state is unused"
    );
}

#[test]
fn marks_can_be_left_off_for_a_screen_pdf() {
    let page = separated_with_spot();
    let images = drawn_plates(&page);
    let content = |marks_on: bool| {
        schist_separation::page_content(
            &PageOutput {
                separated: &page,
                trim: (595.0, 842.0),
                bleed: (0.0).into(),
                slug: schist_layout::Insets::ZERO,
                settings: OutputSettings::at(150.0),
                imposition: Imposition {
                    up: 1,
                    marks: marks_on,
                },
                marks: Marks::default(),
                overprint: true,
            },
            &images,
        )
    };
    assert!(content(true).contains(" S\n"), "no crop marks");
    assert!(
        !content(false).contains(" S\n"),
        "crop marks on a screen proof"
    );
}

#[test]
fn a_plate_with_no_ink_still_gets_a_colour_space() {
    // A prepress tool treats a missing plate as an error, so an empty
    // plate must be present and simply blank.
    let doc = doc_with(vec![Ink::black()], Page::a4());
    let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(150.0)).unwrap();
    let mut pdf = Pdf::new();
    let object = schist_separation::pdf::write_page(
        &mut pdf,
        &PageOutput {
            separated: &page,
            trim: (595.0, 842.0),
            bleed: (0.0).into(),
            slug: schist_layout::Insets::ZERO,
            settings: OutputSettings::at(150.0),
            imposition: Imposition::single(),
            marks: Marks::default(),
            overprint: true,
        },
    )
    .expect("the page writes");
    let text = String::from_utf8_lossy(&pdf.finish(&[object])).into_owned();
    for name in ["Cyan", "Magenta", "Yellow", "Black"] {
        assert!(
            text.contains(&format!("/Separation /{name}")),
            "{name} was dropped"
        );
    }
}

#[test]
fn process_composite_preserves_each_channel_and_paper_knocks_out_every_plate() {
    use schist_layout::{
        authoring::{self, Paint},
        History,
    };
    for rgb in [
        [0.2, 0.5, 0.7],
        [0.7, 0.3, 0.2],
        [0.1, 0.1, 0.1],
        [1.0, 1.0, 1.0],
    ] {
        let mut doc = LayoutDocument::new(vec![Page::new("1", 40.0, 40.0)]);
        doc.ink_manager = doc.ink_manager.separating_without_limit();
        let mixed = Ink::process("Mixed", rgb);
        doc.inks.push(mixed.clone());
        doc.inks.push(Ink::white());
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(0.0, 0.0, 40.0, 40.0),
            Paint::filled("Mixed"),
        )
        .unwrap();
        let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(72.0)).unwrap();
        assert_eq!(page.composite().at(10, 10), mixed.to_cmyk());
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Paint::filled("Paper"),
        )
        .unwrap();
        let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(72.0)).unwrap();
        assert_eq!(page.composite().at(20, 20), [0.0; 4]);
        for plate in page.separation.plates() {
            assert_eq!(plate.at(20, 20), 0.0);
        }
    }
}

#[test]
fn each_text_run_uses_its_own_ink_and_opacity_in_both_output_paths() {
    use schist_layout::{authoring, CharacterStyle, History, StyleRange};
    for dpi in [72.0, 144.0] {
        for alpha in [0.25, 0.75, 1.0] {
            let mut doc = LayoutDocument::new(vec![Page::new("1", 240.0, 100.0)]);
            let id = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(10.0, 10.0, 220.0, 80.0),
            )
            .unwrap();
            authoring::set_text(
                &mut doc,
                &mut History::default(),
                id.story,
                "MMMM MMMM MMMM",
            );
            let LayoutObject::TextFrame { story, .. } = doc.object(id.object).unwrap().object
            else {
                panic!()
            };
            // Two distinct spot inks deliberately share a preview: RGB
            // sampling would merge them. Neither is a document swatch.
            for name in ["Spot A", "Spot B"] {
                doc.styles.add_character(CharacterStyle {
                    name: name.into(),
                    point_size: Some(20.0),
                    fill: Some(Ink::spot(name, [50.0, 20.0, 30.0])),
                    opacity: Some(alpha),
                    ..Default::default()
                });
            }
            doc.story_mut(story).ranges = vec![
                StyleRange {
                    start: 0,
                    end: 4,
                    style: "Spot A".into(),
                },
                StyleRange {
                    start: 5,
                    end: 9,
                    style: "Spot B".into(),
                },
            ];
            for built in [false, true] {
                let output = if built {
                    separate_page_built(
                        &doc,
                        0,
                        OutputSettings::at(dpi),
                        &NoGraphics,
                        &schist_separation::NaiveBuild,
                    )
                } else {
                    separate_page_without_graphics(&doc, 0, OutputSettings::at(dpi))
                }
                .unwrap();
                let index = |name: &str| {
                    output
                        .plan
                        .plates
                        .iter()
                        .position(|p| p.name == name)
                        .unwrap()
                };
                let a = output.separation.plate(index("Spot A")).unwrap();
                let b = output.separation.plate(index("Spot B")).unwrap();
                let k = output.separation.plate(output.plan.process[3]).unwrap();
                assert!((a.peak() - alpha).abs() < 0.03);
                assert!((b.peak() - alpha).abs() < 0.03);
                assert!(k.peak() > 0.9, "unformatted text must keep default black");
                assert!(
                    a.data
                        .iter()
                        .zip(&b.data)
                        .all(|(a, b)| *a == 0.0 || *b == 0.0),
                    "independent runs must not flood the line"
                );
                let centroid = |plate: &schist_separation::PlateCoverage| {
                    let mut x = 0.0;
                    let mut n = 0.0;
                    for (i, v) in plate.data.iter().enumerate() {
                        x += (i as i32 % plate.rect.width()) as f32 * v;
                        n += v;
                    }
                    x / n
                };
                assert!(centroid(a) < centroid(b) && centroid(b) < centroid(k));
            }
        }
    }
}
