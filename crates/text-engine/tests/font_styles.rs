use schist_text_engine::{
    add_font_data, has_font_style, rasterize, StyleRun, TextSpec, WritingMode,
};

#[test]
fn named_faces_change_real_glyphs_and_survive_range_edits_in_every_writing_mode() {
    add_font_data(include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec());
    add_font_data(include_bytes!("fixtures/IBMPlexSans-Light.ttf").to_vec());
    for name in ["Regular", "Light", "light"] {
        assert!(has_font_style("IBM Plex Sans", name));
    }
    assert!(!has_font_style("IBM Plex Sans", "Not a real face 97351"));
    let mut ligatures = TextSpec {
        text: "office AV".into(),
        family: "IBM Plex Sans".into(),
        font_style: Some("Light".into()),
        size: 48.0,
        ..Default::default()
    };
    ligatures.set_feature("liga", true);
    let whole = ligatures.clone();
    ligatures.runs = vec![StyleRun {
        start: 2,
        end: 4,
        font_style: Some(" light ".into()),
        ..Default::default()
    }];
    same_pixels(&ligatures, &whole);
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for size in [24.0, 48.0, 72.0] {
            let regular = TextSpec {
                text: "HéH".into(),
                family: "IBM Plex Sans".into(),
                font_style: Some("Regular".into()),
                size,
                writing_mode: mode,
                ..Default::default()
            };
            let mut light = regular.clone();
            light.font_style = Some("Light".into());
            let mass = |s: &TextSpec| {
                rasterize(s)
                    .unwrap()
                    .coverage
                    .iter()
                    .map(|v| *v as u64)
                    .sum::<u64>()
            };
            assert!(
                mass(&light) < mass(&regular) * 4 / 5,
                "must rasterize the actual lighter face"
            );
            let mut ranged = regular.clone();
            ranged.runs = vec![StyleRun {
                start: 0,
                end: ranged.text.len(),
                font_style: Some("Light".into()),
                ..Default::default()
            }];
            same_pixels(&ranged, &light);
            ranged.apply_style(
                1..3,
                &StyleRun {
                    size: Some(size * 1.2),
                    ..Default::default()
                },
            );
            for byte in [0, 1, 3] {
                assert_eq!(ranged.style_at(byte).font_style.as_deref(), Some("Light"));
            }
            ranged.apply_style(
                1..3,
                &StyleRun {
                    bold: Some(false),
                    italic: Some(false),
                    ..Default::default()
                },
            );
            assert_eq!(ranged.style_at(1).font_style, None);
            assert_eq!(ranged.style_at(0).font_style.as_deref(), Some("Light"));
            ranged.apply_style(
                0..ranged.text.len(),
                &StyleRun {
                    font_style: Some("Light".into()),
                    size: Some(size),
                    ..Default::default()
                },
            );
            for byte in [0, 1, 3] {
                assert_eq!(ranged.style_at(byte).font_style.as_deref(), Some("Light"));
            }
            same_pixels(&ranged, &light);
            ranged.apply_style(
                0..ranged.text.len(),
                &StyleRun {
                    font_style: Some("Regular".into()),
                    size: Some(size),
                    ..Default::default()
                },
            );
            same_pixels(&ranged, &regular);
            let encoded = serde_json::to_value(&light).unwrap();
            assert_eq!(
                serde_json::from_value::<TextSpec>(encoded.clone()).unwrap(),
                light
            );
            let mut legacy = encoded;
            legacy.as_object_mut().unwrap().remove("font_style");
            assert_eq!(
                serde_json::from_value::<TextSpec>(legacy)
                    .unwrap()
                    .font_style,
                None
            );
        }
    }
}

fn same_pixels(a: &TextSpec, b: &TextSpec) {
    let (a, b) = (rasterize(a).unwrap(), rasterize(b).unwrap());
    assert_eq!(a.bounds, b.bounds);
    assert_eq!(a.coverage, b.coverage);
    assert_eq!(a.first_baseline, b.first_baseline);
}
