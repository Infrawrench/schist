//! An enlarged initial is compared with independently positioned ordinary
//! vertical frames. Reference geometry uses measured font outlines and column
//! centers directly, never the layout initial planner or its composed lines.
use schist_layout::{
    affine::Affine, authoring, History, Ink, Insets, LayoutDocument, LayoutObject, Page,
    ParagraphStyle, Rect, Story, WritingMode,
};
use schist_text_engine::{measure, TextSpec, WritingMode as EngineMode};

const SIZE: f32 = 12.0;
const LEADING: f32 = 20.0;
const LINES: usize = 3;

pub fn register_font() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../../web/fonts/NotoSansJP-Schist.otf").to_vec(),
    );
}

fn frame(doc: &mut LayoutDocument, bounds: Rect, text: &str, style: &str, global: Affine) {
    let made = authoring::text_frame(doc, &mut History::default(), 0, bounds).unwrap();
    let object = doc.objects.last_mut().unwrap();
    if let LayoutObject::TextFrame { insets, .. } = &mut object.object {
        *insets = Insets::ZERO;
    }
    object.transform = global.around(-bounds.x, -bounds.y);
    doc.stories[made.story.0 as usize] = Story::from_text(text, style);
}

pub fn document(reference: bool, spot: bool, axis: WritingMode, latin: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 360.0, 260.0)]);
    let ink = if spot {
        Ink::spot("Violet", [40.0, 50.0, -50.0])
    } else {
        Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])
    };
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Body".into(),
        family: Some("Noto Sans CJK JP".into()),
        point_size: Some(SIZE),
        leading: Some(schist_layout::styles::Leading::Points(LEADING)),
        writing_mode: Some(axis),
        fill: Some(ink),
        keep_lines: Some(1),
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Initial".into(),
        based_on: Some("Body".into()),
        drop_caps_lines: Some(LINES),
        drop_caps_characters: Some(1),
        ..Default::default()
    });
    let rl = axis == WritingMode::VerticalRightToLeft;
    let mode = if rl {
        EngineMode::VerticalRl
    } else {
        EngineMode::VerticalLr
    };
    let prefix = if latin { "É" } else { "日" };
    let spec = |text: &str| TextSpec {
        text: text.into(),
        family: "Noto Sans CJK JP".into(),
        size: SIZE,
        leading: Some(LEADING),
        writing_mode: mode,
        ..Default::default()
    };
    let body = measure(&spec("H")).unwrap();
    let cap = measure(&spec(prefix)).unwrap();
    let body_ink = body.ink_bounds.unwrap();
    let cap_ink = cap.ink_bounds.unwrap();
    // The reference edge is H's outer outline, not its cell edge. The final
    // edge meets the third body column center. A vertical initial always
    // reserves space at the inline start, regardless of column progression.
    let start = if rl {
        body.height - body_ink[2]
    } else {
        body_ink[0]
    };
    let end = body.height / 2.0 + (LINES - 1) as f32 * LEADING;
    let scale = (end - start) / (cap_ink[2] - cap_ink[0]);
    let initial_size = SIZE * scale;
    let initial_height = cap.height * scale;
    let inline_ink = (cap_ink[3] - cap_ink[1]) * scale;
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Enlarged".into(),
        based_on: Some("Body".into()),
        point_size: Some(initial_size),
        leading: Some(schist_layout::styles::Leading::Points(initial_height)),
        ..Default::default()
    });
    let words = ["alpha", "bravo", "cello", "delta"];
    for (x, global) in [(25.0, Affine::IDENTITY), (200.0, Affine::skew(0.125, 0.0))] {
        let bounds = Rect::new(x, 40.0, 105.0, 180.0);
        if reference {
            let cross = if rl {
                bounds.right() - start - cap_ink[2] * scale
            } else {
                bounds.x + start - cap_ink[0] * scale
            };
            let inline = bounds.y - cap_ink[1] * scale;
            frame(
                &mut doc,
                // Leave room around the unwrapped reference glyph: scaling
                // point size can round its measured cell slightly above the
                // base cell times scale. Preserve the RL column's outer edge.
                Rect::new(
                    cross - if rl { 1.0 } else { 0.0 },
                    inline,
                    initial_height + 1.0,
                    cap.width * scale + 1.0,
                ),
                prefix,
                "Enlarged",
                global,
            );
            for (index, word) in words.iter().enumerate() {
                let column = if rl {
                    bounds.right() - body.height - index as f32 * LEADING
                } else {
                    bounds.x + index as f32 * LEADING
                };
                let top = if index < LINES {
                    bounds.y + inline_ink + SIZE * 0.15
                } else {
                    bounds.y
                };
                frame(
                    &mut doc,
                    Rect::new(column, top, body.height, 160.0),
                    word,
                    "Body",
                    global,
                );
            }
        } else {
            frame(
                &mut doc,
                bounds,
                &format!("{prefix}{}", words.join("\u{2028}")),
                "Initial",
                global,
            );
        }
    }
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Heading".into(),
        family: Some("Noto Sans CJK JP".into()),
        point_size: Some(7.0),
        ..Default::default()
    });
    frame(
        &mut doc,
        Rect::new(10.0, 8.0, 340.0, 15.0),
        &format!(
            "Vertical initial / {axis:?} / {prefix} / {}",
            if spot { "spot" } else { "process" }
        ),
        "Heading",
        Affine::IDENTITY,
    );
    doc
}
