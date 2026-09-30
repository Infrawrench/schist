use schist_layout::{
    authoring::{self, Paint},
    CharacterStyle, History, Ink, InkAlias, LayoutDocument, LayoutObject, Page, Rect, Story,
};
use schist_separation::{
    separate_page_built, separate_page_without_graphics, NamedBuilds, NoGraphics, OutputSettings,
};

fn rectangle(doc: &mut LayoutDocument, ink: &str, x: f32) {
    authoring::rectangle(
        doc,
        &mut History::default(),
        0,
        Rect::new(x, 10.0, 20.0, 20.0),
        Paint::filled(ink),
    )
    .unwrap();
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
}

#[test]
fn tint_scales_ink_not_knockout_for_fills_and_strokes_at_every_opacity() {
    for tint in [0.0, 0.125, 0.5, 1.0] {
        for opacity in [0.0, 0.25, 1.0] {
            for overprint in [false, true] {
                for stroke_only in [false, true] {
                    let mut doc = LayoutDocument::new(vec![Page::new("1", 40.0, 40.0)]);
                    let build = [0.6, 0.4, 0.2, 0.0];
                    doc.inks.push(Ink::cmyk("Mix", build));
                    rectangle(&mut doc, "Black", 10.0);
                    rectangle(&mut doc, "Mix", 10.0);
                    let object = doc.objects.last_mut().unwrap();
                    object.transparency = opacity;
                    object.overprint = overprint;
                    let LayoutObject::Shape {
                        tints,
                        fill,
                        stroke,
                        stroke_width,
                        ..
                    } = &mut object.object
                    else {
                        panic!()
                    };
                    tints.fill = tint;
                    tints.stroke = tint;
                    if stroke_only {
                        *stroke = fill.take();
                        *stroke_width = 8.0;
                    }
                    let result =
                        separate_page_without_graphics(&doc, 0, OutputSettings::at(72.0)).unwrap();
                    let x = if stroke_only { 12 } else { 20 };
                    let expected_black = if overprint { 1.0 } else { 1.0 - opacity };
                    for (channel, expected) in [
                        build[0] * tint * opacity,
                        build[1] * tint * opacity,
                        build[2] * tint * opacity,
                        expected_black,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        close(
                            result
                                .separation
                                .plate(result.plan.process[channel])
                                .unwrap()
                                .at(x, 20),
                            expected,
                        );
                        close(result.composite().at(x, 20)[channel], expected);
                    }
                }
            }
        }
    }
}

#[test]
fn all_spot_tints_share_one_plate_and_scale_the_resolved_alias_or_process_build() {
    for convert in [false, true] {
        let mut doc = LayoutDocument::new(vec![Page::new("1", 140.0, 40.0)]);
        doc.inks.push(Ink::spot("Source", [60.0, -40.0, 30.0]));
        doc.inks.push(Ink::spot("Target", [50.0, 20.0, -30.0]));
        doc.ink_manager
            .set_rule("Source", InkAlias::Alias("Target".into()));
        if convert {
            doc.ink_manager
                .set_rule("Target", InkAlias::ConvertToProcess);
        }
        let tints = [0.0, 0.125, 0.25, 0.5, 1.0];
        for (i, tint) in tints.into_iter().enumerate() {
            rectangle(&mut doc, "Source", 5.0 + i as f32 * 25.0);
            let LayoutObject::Shape { tints, .. } = &mut doc.objects.last_mut().unwrap().object
            else {
                panic!()
            };
            tints.fill = tint;
        }
        let builds = NamedBuilds::new().with("Target", [0.8, 0.6, 0.4, 0.2]);
        let result =
            separate_page_built(&doc, 0, OutputSettings::at(72.0), &NoGraphics, &builds).unwrap();
        assert_eq!(result.plan.plates.len(), if convert { 4 } else { 5 });
        for (i, tint) in tints.into_iter().enumerate() {
            let x = 15 + i as i32 * 25;
            if convert {
                for (channel, build) in [0.8, 0.6, 0.4, 0.2].into_iter().enumerate() {
                    close(
                        result
                            .separation
                            .plate(result.plan.process[channel])
                            .unwrap()
                            .at(x, 20),
                        build * tint,
                    );
                }
            } else {
                assert_eq!(result.plan.plates[4].name, "Target");
                close(result.separation.plate(4).unwrap().at(x, 20), tint);
            }
        }
    }
}

#[test]
fn text_tint_inherits_per_run_without_weakening_its_knockout_mask() {
    for tint in [0.0, 0.25, 0.5, 1.0] {
        let mut doc = LayoutDocument::new(vec![Page::new("1", 100.0, 60.0)]);
        let cyan = Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]);
        doc.inks.push(cyan.clone());
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(0.0, 0.0, 100.0, 60.0),
            Paint::filled("Black"),
        )
        .unwrap();
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(5.0, 5.0, 90.0, 45.0),
        )
        .unwrap();
        let paragraph = doc
            .styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Body")
            .unwrap();
        paragraph.point_size = Some(24.0);
        paragraph.fill = Some(cyan);
        paragraph.fill_tint = Some(tint);
        doc.styles.add_character(CharacterStyle {
            name: "Override".into(),
            fill_tint: Some(tint / 2.0),
            ..Default::default()
        });
        doc.stories[frame.story.0 as usize] = Story::from_text("HHHH", "Body");
        doc.stories[frame.story.0 as usize].apply_style(2, 4, "Override");
        let out = separate_page_without_graphics(&doc, 0, OutputSettings::at(144.0)).unwrap();
        let cyan = out.separation.plate(out.plan.process[0]).unwrap();
        let black = out.separation.plate(out.plan.process[3]).unwrap();
        let mut hits = [0; 2];
        for y in 0..120 {
            for x in 0..200 {
                let removed = 1.0 - black.at(x, y);
                if removed > 0.99 {
                    // Classify by the actual amount; the glyph shape itself is irrelevant.
                    let amount = cyan.at(x, y);
                    if (amount - tint).abs() < 1e-5 {
                        hits[0] += 1;
                    } else {
                        close(amount, tint / 2.0);
                        hits[1] += 1;
                    }
                }
            }
        }
        assert!(hits[0] > 100);
        if tint > 0.0 {
            assert!(hits[1] > 100);
        }
    }
}
