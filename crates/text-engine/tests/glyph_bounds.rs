use schist_text_engine::{measure, rasterize, ParagraphDirection, StyleRun, TextSpec};

#[test]
fn measured_glyph_outlines_agree_with_rasters_across_sizes_and_style_runs() {
    for text in ["A", "Éc", "E\u{301}", "ffi", "عربي"] {
        for size in [11.0, 24.0, 45.3] {
            for italic in [false, true] {
                let spec = TextSpec {
                    text: text.into(),
                    size,
                    italic,
                    direction: ParagraphDirection::LeftToRight,
                    runs: vec![StyleRun {
                        start: 0,
                        end: text.len(),
                        size: Some(size * 1.3),
                        ..Default::default()
                    }],
                    ..Default::default()
                };
                let metrics = measure(&spec).unwrap();
                let ink = metrics.ink_bounds.unwrap();
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
                    assert!(
                        (actual as f32 - bound).abs() < 2.0,
                        "{text}: {actual}, {bound}"
                    );
                }
                assert!((metrics.first_baseline - raster.first_baseline).abs() < 0.001);
            }
        }
    }
    assert!(measure(&TextSpec {
        text: "   ".into(),
        ..Default::default()
    })
    .unwrap()
    .ink_bounds
    .is_none());
}
