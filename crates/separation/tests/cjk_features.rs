use schist_layout::{
    authoring, directional_features::DirectionalFeatures, CharacterStyle, History, LayoutDocument,
    Page, ParagraphStyle, Rect, Story, WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

#[test]
fn directional_defaults_and_local_resets_match_explicit_features_on_every_plate_and_axis() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec(),
    );
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        let vertical = mode != WritingMode::Horizontal;
        let mut doc = LayoutDocument::new(vec![Page::new("1", 200.0, 200.0)]);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            family: Some("Noto Sans CJK JP".into()),
            point_size: Some(26.0),
            writing_mode: Some(mode),
            directional_features: DirectionalFeatures {
                kana: Some(true),
                proportional_metrics: Some(false),
            },
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "On".into(),
            directional_features: DirectionalFeatures {
                kana: Some(false),
                proportional_metrics: Some(true),
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(18.0, 18.0, 160.0, 160.0),
        )
        .unwrap();
        let mut story = Story::from_text("かなカナ。、かな", "Base");
        let split = story.text().char_indices().nth(4).unwrap().0;
        story.apply_style(0, split, "On");
        doc.stories[frame.story.0 as usize] = story;
        doc.objects.last_mut().unwrap().transform = schist_core::Affine {
            a: 0.9,
            b: 0.1,
            c: 0.08,
            d: 0.95,
            tx: 0.0,
            ty: 0.0,
        };
        let mut reference = doc.clone();
        for style in &mut reference.styles.paragraphs {
            style.features = style
                .directional_features
                .selected(&style.features, vertical);
            style.directional_features = DirectionalFeatures::default();
        }
        for style in &mut reference.styles.characters {
            style.features = style
                .directional_features
                .selected(&style.features, vertical);
            style.directional_features = DirectionalFeatures::default();
        }
        for dpi in [72.0, 144.0] {
            let actual = separate_page_without_graphics(&doc, 0, OutputSettings::at(dpi)).unwrap();
            let expected =
                separate_page_without_graphics(&reference, 0, OutputSettings::at(dpi)).unwrap();
            for (a, b) in actual
                .separation
                .plates()
                .iter()
                .zip(expected.separation.plates())
            {
                assert_eq!(a.data, b.data, "{mode:?}, dpi={dpi}");
            }
            assert!(actual
                .separation
                .plates()
                .iter()
                .any(|p| p.data.iter().any(|v| *v > 0.5)));
        }
    }
}
