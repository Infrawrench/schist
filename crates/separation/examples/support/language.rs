//! Language-sensitive text paired with independently authored Unicode glyphs.
use schist_layout::{
    authoring, CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story,
    WritingMode,
};

pub fn document() -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 24]);
    doc.styles
        .languages
        .push(schist_layout::language::LanguageResource {
            id: "opaque-ro".into(),
            name: "$ID/Romanian".into(),
            ..Default::default()
        });
    doc.styles.add_character(CharacterStyle {
        name: "Reset".into(),
        language: Some("und".into()),
        ..Default::default()
    });
    for page in 0..24 {
        let case = (page / 2) % 4;
        let reference = page % 2 == 1;
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalRightToLeft,
            WritingMode::VerticalLeftToRight,
        ][page / 8];
        let (source, expected, language, caps) = match case {
            0 => ("diyarbakır\ni I ı İ", "DİYARBAKIR\nİ I I İ", "TR_tr", true),
            1 => ("ŞŢşţ\nşţŞŢ", "ȘȚșț\nșțȘȚ", "opaque-ro", false),
            2 => (
                "i\u{307}\u{301} j\u{328}\u{307}\nį\u{307}",
                "I\u{301} J\u{328}\nĮ",
                "lt-LT",
                true,
            ),
            _ => ("şşş\nţţţ", "șşș\nțţț", "opaque-ro", false),
        };
        let base = format!("Language {page}");
        let name = format!("Inherited {page}");
        doc.styles.add_paragraph(ParagraphStyle {
            name: base.clone(),
            language: Some(if reference && caps { "und" } else { language }.into()),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(20.0),
            leading: Some(schist_layout::styles::Leading::Points(34.0)),
            writing_mode: Some(mode),
            all_caps: Some(!reference && caps),
            small_caps: Some(false),
            fill: Some(Ink::spot("Language spot", [45.0, 60.0, 30.0])),
            fill_tint: Some(0.8),
            stroke: Some(Ink::cmyk("Language cyan", [1.0, 0.0, 0.0, 0.0])),
            stroke_tint: Some(0.65),
            stroke_weight: Some(0.5),
            overprint_fill: Some(case % 2 == 0),
            overprint_stroke: Some(case % 2 == 1),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            based_on: Some(base),
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            page,
            Rect::new(25.0, 25.0, 150.0, 150.0),
        )
        .unwrap();
        let mut story = Story::from_text(if reference { expected } else { source }, &name);
        if case == 3 {
            story.apply_style(2, 4, "Reset");
            story.apply_style(9, 11, "Reset");
        }
        doc.stories[frame.story.0 as usize] = story;
        doc.objects.last_mut().unwrap().transform = schist_core::Affine {
            a: 0.9,
            b: 0.12,
            c: 0.15,
            d: 0.9,
            tx: 0.0,
            ty: 0.0,
        };
    }
    doc
}
