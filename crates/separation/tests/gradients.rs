//! Gradient fills reach the plates as InDesign's PDF of the public
//! paged-media `gradients` sample draws them: a 360 × 200 pt rectangle with
//! no stated start runs black to paper along its width, cyan through magenta
//! to yellow with the middle stop at the centre, and a radial gradient from
//! paper at its bottom-left corner to black 360 pt out.
use schist_layout::gradients::{Gradient, GradientFill, GradientStop};
use schist_layout::{authoring, History, Ink, LayoutDocument, Page, Paint, Rect};
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
