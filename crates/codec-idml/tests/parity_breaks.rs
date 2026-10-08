use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    authoring, compose, numbering::Section, FrameOverflow, History, Insets, LayoutDocument,
    LayoutObject, Page, Rect, Story, StoryId, StoryPoint,
};

fn native(kind: &str, attribute: &str, count: usize, before: bool, first: u32) -> LayoutDocument {
    let mut doc = schist_layout::blank_a4();
    for _ in 1..7 {
        doc.add_page(Page::a4());
    }
    doc.pages[0].section = Some(Section {
        start: first,
        continue_numbering: false,
        ..Default::default()
    });
    let mut ids = Vec::new();
    for frame in 0..14 {
        let created = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            frame / 2,
            Rect::new(0.0, 0.0, 500.0, 700.0),
        )
        .unwrap();
        if let LayoutObject::TextFrame {
            story,
            insets,
            columns,
            balance_columns,
            overflow,
            ..
        } = &mut doc.objects.last_mut().unwrap().object
        {
            *story = StoryId(0);
            *insets = Insets::ZERO;
            *columns = 2;
            *balance_columns = Some(false);
            *overflow = FrameOverflow::Thread;
        }
        ids.push(created.object);
    }
    let mut story = Story::default();
    if before {
        story.push_paragraph("Before", "Body");
    }
    story.points.extend(vec![StoryPoint::PageBreak; count]);
    story.push_paragraph("é中😀", "Body");
    doc.stories[0] = story;
    doc.thread_order = vec![(StoryId(0), ids)];
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let path = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Stories/"))
        .unwrap()
        .to_owned();
    let source = package.text(&path).unwrap().replace(
        "ParagraphBreakType=\"NextPage\"",
        &format!("{attribute}=\"{kind}\""),
    );
    package.insert(path, source.into_bytes());
    import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document
}

#[test]
fn native_odd_even_breaks_reach_each_successive_numbered_destination_and_survive_saves() {
    for attribute in ["ParagraphBreakType", "GoToNextX"] {
        for (kind, parity) in [("NextOddPage", 1), ("NextEvenPage", 0)] {
            for first in [1, 2, 7, 8] {
                for count in [1, 2, 3] {
                    for before in [false, true] {
                        let mut doc = native(kind, attribute, count, before, first);
                        let source = doc.stories[0].clone();
                        let mut page = 0;
                        for _ in 0..count {
                            page += 1;
                            while doc.page_number_value(page) % 2 != parity {
                                page += 1;
                            }
                        }
                        for _ in 0..3 {
                            let flow = compose::compose_story(&doc, StoryId(0));
                            let line = flow.frames[page * 2].lines.last().unwrap_or_else(|| panic!("{kind}/{attribute}, first={first}, count={count}, before={before}: expected page {page}"));
                            assert_eq!(doc.stories[0].slice(line.start, line.end), "é中😀");
                            assert!(!flow.has_overflow());
                            let written = export::write(&doc);
                            let package = container::read(&written.bytes).unwrap();
                            let path = package
                                .names()
                                .into_iter()
                                .find(|n| n.starts_with("Stories/"))
                                .unwrap();
                            let root = xml::parse(package.text(path).unwrap()).unwrap();
                            let breaks: Vec<_> = root
                                .find_all("CharacterStyleRange")
                                .into_iter()
                                .filter_map(|e| e.attr("ParagraphBreakType"))
                                .collect();
                            assert_eq!(breaks, vec![kind; count]);
                            let imported = import::read(&written.bytes).unwrap();
                            assert!(!imported
                                .report
                                .skipped
                                .iter()
                                .any(|s| s == schist_i18n::t("design.idml_page_parity")));
                            doc = imported.document;
                            assert_eq!(doc.stories[0], source);
                        }
                    }
                }
            }
        }
    }
}
