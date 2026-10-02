//! Destination controls use ordinary unthreaded frames at enumerated positions.
use schist_layout::{
    authoring,
    numbering::Section,
    styles::{Leading, ParagraphStart},
    FrameOverflow, History, Ink, Insets, LayoutDocument, LayoutObject, Page, ParagraphStyle, Rect,
    Story, StoryDirection,
};

pub fn document(reference: bool, columns: u16, reverse: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 3]);
    doc.pages[0].section = Some(Section {
        start: if reverse { 2 } else { 1 },
        continue_numbering: false,
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Main".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(7.0),
        leading: Some(Leading::Points(9.0)),
        fill: Some(Ink::cmyk("Before cyan", [1.0, 0.0, 0.0, 0.0])),
        ..Default::default()
    });
    let after_ink = Ink::spot("After violet", [40.0, 50.0, -50.0]);
    for page in 0..3 {
        let heading = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            page,
            Rect::new(10.0, 7.0, 180.0, 15.0),
        )
        .unwrap();
        doc.stories[heading.story.0 as usize] = Story::from_text(
            format!(
                "Page {}  /  {columns} columns  /  {}",
                page + 1,
                if reverse {
                    "RTL / starts at 2"
                } else {
                    "LTR / starts at 1"
                }
            ),
            "Main",
        );
    }
    for (row, policy) in [
        ParagraphStart::Anywhere,
        ParagraphStart::NextColumn,
        ParagraphStart::NextFrame,
        ParagraphStart::NextPage,
        ParagraphStart::NextOddPage,
        ParagraphStart::NextEvenPage,
    ]
    .into_iter()
    .enumerate()
    {
        let name = policy.native_name();
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.into(),
            based_on: Some("Main".into()),
            start_paragraph: (!reference).then_some(policy),
            fill: Some(after_ink.clone()),
            fill_tint: Some(0.7),
            ..Default::default()
        });
        let y = 28.0 + row as f32 * 27.0;
        let column_width = (80.0 - f32::from(columns - 1) * 4.0) / f32::from(columns);
        let x_for = |frame: usize, col: usize| {
            let physical = if reverse {
                usize::from(columns) - 1 - col
            } else {
                col
            };
            10.0 + (frame % 2) as f32 * 100.0 + physical as f32 * (column_width + 4.0)
        };
        if reference {
            let (frame, col) = match policy {
                ParagraphStart::Anywhere => (0, 0),
                ParagraphStart::NextColumn if columns > 1 => (0, 1),
                ParagraphStart::NextColumn | ParagraphStart::NextFrame => (1, 0),
                ParagraphStart::NextPage => (2, 0),
                ParagraphStart::NextOddPage => (if reverse { 2 } else { 4 }, 0),
                ParagraphStart::NextEvenPage => (if reverse { 4 } else { 2 }, 0),
            };
            for (page, x, top, text, style) in [
                (0, x_for(0, 0), y, "pre", "Main"),
                (
                    frame / 2,
                    x_for(frame, col),
                    y + if policy == ParagraphStart::Anywhere {
                        9.0
                    } else {
                        0.0
                    },
                    "café",
                    name,
                ),
            ] {
                let created = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    page,
                    Rect::new(x, top, column_width, 12.0),
                )
                .unwrap();
                doc.stories[created.story.0 as usize] = Story::from_text(text, style);
                if let LayoutObject::TextFrame { insets, .. } =
                    &mut doc.objects.last_mut().unwrap().object
                {
                    *insets = Insets::ZERO;
                }
            }
        } else {
            let mut ids = Vec::new();
            let mut story_id = None;
            for frame in 0..6 {
                let created = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    frame / 2,
                    Rect::new(10.0 + (frame % 2) as f32 * 100.0, y, 80.0, 22.0),
                )
                .unwrap();
                let id = *story_id.get_or_insert(created.story);
                if let LayoutObject::TextFrame {
                    story,
                    columns: count,
                    gutter,
                    insets,
                    balance_columns,
                    overflow,
                    ..
                } = &mut doc.objects.last_mut().unwrap().object
                {
                    *story = id;
                    *count = columns;
                    *gutter = 4.0;
                    *insets = Insets::ZERO;
                    *balance_columns = Some(false);
                    *overflow = FrameOverflow::Thread;
                }
                ids.push(created.object);
            }
            let id = story_id.unwrap();
            let mut story = Story::from_text("pre", "Main");
            story.push_paragraph("café", name);
            story.prefs.direction = if reverse {
                StoryDirection::RightToLeft
            } else {
                StoryDirection::LeftToRight
            };
            doc.stories[id.0 as usize] = story;
            doc.thread_order.push((id, ids));
        }
    }
    doc
}
