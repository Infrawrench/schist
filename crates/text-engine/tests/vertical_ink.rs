use schist_text_engine::{
    add_font_data, measure, rasterize, ParagraphDirection, StyleRun, TextSpec, WritingMode,
};

#[test]
fn vertical_ink_bounds_enclose_the_same_upright_and_rotated_glyphs_as_the_raster() {
    add_font_data(include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec());
    for mode in [WritingMode::VerticalLr, WritingMode::VerticalRl] {
        for direction in [
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ] {
            for text in ["A", "Éc", "E\u{301}", "日本", "日Ab本", "日A\nB本"] {
                for size in [11.0, 24.0, 45.3] {
                    for wrap in [None, Some(80.0)] {
                        for shift in [-2.75, 0.0, 3.25] {
                            let end = schist_text_engine::grapheme_boundaries(text)
                                .nth(1)
                                .unwrap();
                            let spec = TextSpec {
                                family: "Noto Sans CJK JP".into(),
                                text: text.into(),
                                size,
                                writing_mode: mode,
                                direction,
                                wrap_width: wrap,
                                runs: vec![StyleRun {
                                    start: 0,
                                    end,
                                    size: Some(size * 1.3),
                                    baseline_shift: Some(shift),
                                    ..Default::default()
                                }],
                                ..Default::default()
                            };
                            let metrics = measure(&spec).unwrap();
                            let ink = metrics
                                .ink_bounds
                                .expect("vertical outlines must be measurable");
                            let raster = rasterize(&spec).unwrap();
                            assert!(!raster.is_empty());
                            for (actual, bound) in [
                                raster.bounds.left,
                                raster.bounds.top,
                                raster.bounds.right,
                                raster.bounds.bottom,
                            ]
                            .into_iter()
                            .zip(ink)
                            {
                                assert!((actual as f32 - bound).abs() < 2.0, "{mode:?} {direction:?} {text:?} {size} {wrap:?} {shift}: {actual}, {bound}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn vertical_outline_geometry_scales_without_rounding_and_empty_ink_stays_absent() {
    add_font_data(include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec());
    for mode in [WritingMode::VerticalLr, WritingMode::VerticalRl] {
        for text in ["日Ab本", "Éc\n日本", "", "  ", "\n"] {
            let mut spec = TextSpec {
                family: "Noto Sans CJK JP".into(),
                text: text.into(),
                size: 13.25,
                writing_mode: mode,
                ..Default::default()
            };
            let before = measure(&spec).unwrap().ink_bounds;
            assert_eq!(before.is_some(), text.chars().any(|c| !c.is_whitespace()));
            spec.size *= 3.5;
            let after = measure(&spec).unwrap().ink_bounds;
            match (before, after) {
                (Some(before), Some(after)) => {
                    for (a, b) in before.into_iter().zip(after) {
                        assert!((a * 3.5 - b).abs() < 0.001, "{mode:?} {text:?}: {a}, {b}");
                    }
                }
                (None, None) => {}
                _ => panic!("scaling changed outline existence"),
            }
        }
    }
}
