//! Gradient fills reach the plates as InDesign's PDF of the public
//! paged-media `gradients` sample draws them: a 360 × 200 pt rectangle with
//! no stated start runs black to paper along its width, cyan through magenta
//! to yellow with the middle stop at the centre, and a radial gradient from
//! paper at its bottom-left corner to black 360 pt out. Strokes and text,
//! which no public sample paints with a gradient, run the same way: a stroke
//! over its path, text over the frame it is set in.
use schist_layout::gradients::{Gradient, GradientFill, GradientStop};
use schist_layout::{
    authoring, History, Ink, LayoutDocument, Page, Paint, ParagraphStyle, Rect, Story,
    StrokeAlignment,
};
use schist_separation::{separate_page, NoGraphics, OutputSettings};

const AREA: Rect = Rect::new(20.0, 50.0, 360.0, 200.0);

fn document(inks: &[Ink], radial: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    let id = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        AREA,
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let last = (inks.len() - 1) as f32;
    let fill = GradientFill {
        gradient: Gradient {
            name: "Ramp".into(),
            radial,
            stops: inks
                .iter()
                .enumerate()
                .map(|(index, ink)| GradientStop {
                    ink: ink.clone(),
                    location: index as f32 / last,
                    midpoint: 0.5,
                })
                .collect(),
        },
        start: None,
        length: None,
        angle: 0.0,
    };
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    object.appearance.paint.fill = Some(Paint::Gradient(Box::new(fill)));
    doc
}

fn near(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 0.03, "{what}: {a} vs {b}");
}

#[test]
fn a_linear_gradient_runs_black_to_paper_along_the_width() {
    let doc = document(&[Ink::black(), Ink::white()], false);
    let result = separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    let black = result.plan.process[3];
    let at = |x: f32| {
        result
            .separation
            .plate(black)
            .unwrap()
            .at((AREA.x + x) as i32, 150)
    };
    near(at(0.5), 1.0, "left edge");
    near(at(90.5), 0.75, "a quarter along");
    near(at(180.5), 0.5, "half way");
    near(at(359.5), 0.0, "right edge");
    // Nothing outside the shape.
    assert_eq!(result.separation.plate(black).unwrap().at(10, 150), 0.0);
}

#[test]
fn three_stops_hand_cyan_to_magenta_to_yellow() {
    let doc = document(
        &[
            Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]),
            Ink::cmyk("Magenta", [0.0, 1.0, 0.0, 0.0]),
            Ink::cmyk("Yellow", [0.0, 0.0, 1.0, 0.0]),
        ],
        false,
    );
    let result = separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    let [cyan, magenta, yellow, _] = result.plan.process;
    let at = |plate: usize, x: f32| {
        result
            .separation
            .plate(plate)
            .unwrap()
            .at((AREA.x + x) as i32, 150)
    };
    near(at(cyan, 0.5), 1.0, "cyan at the start");
    near(at(cyan, 90.5), 0.5, "cyan a quarter along");
    near(at(magenta, 90.5), 0.5, "magenta a quarter along");
    near(at(magenta, 180.5), 1.0, "magenta at the middle stop");
    near(at(yellow, 180.5), 0.0, "no yellow at the middle stop");
    near(at(yellow, 359.5), 1.0, "yellow at the end");
    near(at(cyan, 270.5), 0.0, "no cyan past the middle");
}

#[test]
fn a_radial_gradient_spreads_from_the_bottom_left_corner() {
    let doc = document(&[Ink::white(), Ink::black()], true);
    let result = separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    let black = result.plan.process[3];
    let at = |x: f32, y: f32| {
        result
            .separation
            .plate(black)
            .unwrap()
            .at((AREA.x + x) as i32, (AREA.y + y) as i32)
    };
    // Bottom-left is the centre: paper.
    near(at(0.5, 199.5), 0.0, "centre");
    near(at(180.5, 199.5), 0.5, "half the radius along the bottom");
    near(at(0.5, 19.5), 0.5, "half the radius up the left edge");
    // The far corner is beyond the 360 pt radius: full black.
    near(at(359.5, 0.5), 1.0, "beyond the radius");
}

/// Black to paper, with no stated start.
fn ramp() -> Box<GradientFill> {
    Box::new(GradientFill {
        gradient: Gradient {
            name: "Ramp".into(),
            radial: false,
            stops: vec![
                GradientStop {
                    ink: Ink::black(),
                    location: 0.0,
                    midpoint: 0.5,
                },
                GradientStop {
                    ink: Ink::white(),
                    location: 1.0,
                    midpoint: 0.5,
                },
            ],
        },
        start: None,
        length: None,
        angle: 0.0,
    })
}

/// The rectangle unfilled and stroked 10 pt with the ramp.
fn stroked(alignment: StrokeAlignment) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    let id = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        AREA,
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    object.appearance.paint.stroke = Some(Paint::Gradient(ramp()));
    object.appearance.paint.stroke_width = Some(10.0);
    object.appearance.paint.stroke_alignment = Some(alignment);
    doc
}

#[test]
fn a_gradient_stroke_runs_over_its_path_as_a_fill_would() {
    for alignment in [StrokeAlignment::Center, StrokeAlignment::Inside] {
        let doc = stroked(alignment);
        let result = separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
        let black = result.plan.process[3];
        let at = |x: f32, y: f32| {
            result
                .separation
                .plate(black)
                .unwrap()
                .at((AREA.x + x).floor() as i32, (AREA.y + y).floor() as i32)
        };
        // Along the top edge the stroke is half black half way along the
        // path's width, whatever side of the path it is on.
        near(at(180.5, 2.5), 0.5, "the top edge's middle");
        near(at(90.5, 2.5), 0.75, "a quarter along the top edge");
        // Up the right edge, by the end of the path's width, paper.
        near(at(356.5, 100.5), 0.0, "the right edge");
        // Nothing within the stroke's band.
        near(at(180.5, 100.5), 0.0, "unfilled");
        if alignment == StrokeAlignment::Center {
            // Beyond the path's left edge, the first stop continues.
            near(at(-3.5, 100.5), 1.0, "outside the left edge");
            near(at(180.5, -2.5), 0.5, "outside the top edge");
        } else {
            near(at(3.5, 100.5), 0.99, "inside the left edge");
            near(at(180.5, 7.5), 0.5, "inside the top edge");
            near(at(180.5, -2.5), 0.0, "nothing outside");
        }
    }
}

/// A frame over the rectangle set in `style`.
fn lettered(style: ParagraphStyle) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    let name = style.name.clone();
    doc.styles.add_paragraph(style);
    let frame = authoring::text_frame(&mut doc, &mut History::default(), 0, AREA).unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("HHHH HHHH", &name);
    doc
}

/// Every pixel the solid black `reference` covers fully is, in `doc`, the
/// ramp's mix where it falls across the frame: black at its left edge,
/// paper at its right.
fn follows_the_ramp(doc: &LayoutDocument, reference: &LayoutDocument) {
    let actual = separate_page(doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    let expected = separate_page(reference, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    let black = actual.plan.process[3];
    let (actual, expected) = (
        actual.separation.plate(black).unwrap(),
        expected.separation.plate(black).unwrap(),
    );
    let mut checked = [0; 2];
    for y in AREA.y as i32..AREA.bottom() as i32 {
        for x in AREA.x as i32..AREA.right() as i32 {
            if expected.at(x, y) < 0.99 {
                continue;
            }
            let t = ((x as f32 + 0.5 - AREA.x) / AREA.width).clamp(0.0, 1.0);
            near(actual.at(x, y), 1.0 - t, &format!("({x}, {y})"));
            checked[usize::from(t > 0.5)] += 1;
        }
    }
    assert!(checked.iter().all(|n| *n > 50), "{checked:?}");
}

#[test]
fn text_takes_its_gradient_across_the_frame_it_is_set_in() {
    let style = ParagraphStyle {
        name: "Ramp".into(),
        point_size: Some(48.0),
        fill: Some(Ink::black()),
        fill_gradient: Some(ramp()),
        // An underline in the text's colour takes the gradient too.
        underline: Some(true),
        ..Default::default()
    };
    let solid = ParagraphStyle {
        fill_gradient: None,
        ..style.clone()
    };
    follows_the_ramp(&lettered(style), &lettered(solid));
}

#[test]
fn text_strokes_take_their_gradient_across_the_frame() {
    let style = ParagraphStyle {
        name: "Outline".into(),
        point_size: Some(48.0),
        fill_disabled: true,
        stroke: Some(Ink::black()),
        stroke_gradient: Some(ramp()),
        stroke_weight: Some(3.0),
        ..Default::default()
    };
    let solid = ParagraphStyle {
        stroke_gradient: None,
        ..style.clone()
    };
    follows_the_ramp(&lettered(style), &lettered(solid));
}
