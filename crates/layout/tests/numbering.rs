use schist_layout::{numbering, structure, History, LayoutDocument, NumberStyle, Page, Section};

#[test]
fn section_parity_only_changes_inferred_spines_and_leaves_explicit_binding_fixed() {
    for direction in [
        schist_layout::PageBinding::LeftToRight,
        schist_layout::PageBinding::RightToLeft,
    ] {
        for start in 1..12 {
            let mut doc = LayoutDocument::new(vec![Page::a4(); 4]);
            doc.facing_pages = true;
            doc.page_binding = direction;
            doc.pages[2].section = Some(Section {
                start,
                continue_numbering: false,
                ..Default::default()
            });
            for page in 2..4 {
                let even = (start + page as u32 - 2).is_multiple_of(2);
                assert_eq!(
                    doc.page_is_left(page),
                    even != (direction == schist_layout::PageBinding::RightToLeft)
                );
                for spine in [0, 1] {
                    doc.spreads[page].binding_location = Some(spine);
                    assert_eq!(doc.page_is_left(page), spine == 1);
                }
            }
        }
    }
}

fn document(mask: u8) -> LayoutDocument {
    let mut doc = LayoutDocument::new(
        (0..5)
            .map(|i| Page::new(i.to_string(), 200.0, 300.0))
            .collect(),
    );
    for (i, page) in doc.pages.iter_mut().enumerate() {
        if mask & (1 << i) != 0 {
            page.section = Some(Section {
                start: 10 + i as u32,
                continue_numbering: i % 2 != 0,
                prefix: format!("{i}-"),
                include_prefix: i % 2 == 0,
                name: format!("section {i}"),
                marker: format!("marker {i}"),
                style: NumberStyle::RomanLower,
            });
        }
    }
    doc
}

fn check_sequence(doc: &LayoutDocument) {
    let mut number = 0;
    let mut active = Section::default();
    let mut boundary = 0;
    for (i, page) in doc.pages.iter().enumerate() {
        number += 1;
        if let Some(section) = &page.section {
            active = section.clone();
            boundary = i;
            if !section.continue_numbering {
                number = section.start;
            }
        }
        assert_eq!(doc.page_number_value(i), number);
        assert_eq!(doc.section_at(i), (boundary, active.clone()));
        assert_eq!(
            doc.page_number(i),
            format!(
                "{}{}",
                if active.include_prefix {
                    &active.prefix
                } else {
                    ""
                },
                active.style.format(number)
            )
        );
    }
}

#[test]
fn every_boundary_combination_follows_reading_order_and_includes_hidden_pages() {
    for mask in 0..32 {
        let mut doc = document(mask);
        for hidden in 0..32 {
            for (i, page) in doc.pages.iter_mut().enumerate() {
                page.hidden = hidden & (1 << i) != 0;
            }
            check_sequence(&doc);
        }
    }
}

#[test]
fn boundaries_follow_their_pages_through_every_move_and_one_step_undo() {
    for mask in 0..32 {
        let original = document(mask);
        for from in 0..5 {
            for to in 0..5 {
                if from == to {
                    continue;
                }
                let mut doc = original.clone();
                let mut history = History::default();
                assert!(structure::move_page(&mut doc, &mut history, from, to));
                let mut expected = original.pages.clone();
                let page = expected.remove(from);
                expected.insert(to, page);
                assert_eq!(doc.pages, expected);
                check_sequence(&doc);
                assert_eq!(history.undo_depth(), 1);
                let changed = doc.clone();
                assert!(history.undo(&mut doc));
                assert_eq!(doc, original);
                assert!(history.redo(&mut doc));
                assert_eq!(doc, changed);
            }
        }
    }
}

#[test]
fn insert_delete_and_section_changes_are_single_steps_without_duplicate_boundaries() {
    for mask in 0..32 {
        let original = document(mask);
        for index in 0..5 {
            let mut doc = original.clone();
            let mut history = History::default();
            assert!(structure::add_page(
                &mut doc,
                &mut history,
                index,
                original.pages[index].clone()
            ));
            assert!(doc.pages[index + 1].section.is_none());
            check_sequence(&doc);
            assert_eq!(history.undo_depth(), 1);
            assert!(history.undo(&mut doc));
            assert_eq!(doc, original);
            assert!(structure::remove_page(&mut doc, &mut history, index));
            let mut expected = original.pages.clone();
            expected.remove(index);
            assert_eq!(doc.pages, expected);
            check_sequence(&doc);
            assert_eq!(history.undo_depth(), 1);
            assert!(history.undo(&mut doc));
            assert_eq!(doc, original);
            assert!(numbering::edit_section(
                &mut doc,
                &mut history,
                index,
                |s| {
                    s.start = 42;
                    s.continue_numbering = false;
                }
            ));
            check_sequence(&doc);
            assert_eq!(history.undo_depth(), 1);
            assert!(history.undo(&mut doc));
            assert_eq!(doc, original);
        }
    }
}

#[test]
fn invalid_sections_and_redundant_first_default_do_not_create_edits() {
    let mut doc = LayoutDocument::default();
    let original = doc.clone();
    let mut history = History::default();
    for start in [0, 1_000_000, u32::MAX] {
        assert!(!numbering::edit_section(&mut doc, &mut history, 0, |s| s
            .start =
            start));
    }
    assert!(!numbering::set_section(
        &mut doc,
        &mut history,
        0,
        Some(Section::default())
    ));
    assert!(!numbering::set_section(
        &mut doc,
        &mut history,
        1,
        Some(Section::default())
    ));
    assert_eq!(doc, original);
    assert_eq!(history.undo_depth(), 0);
}
