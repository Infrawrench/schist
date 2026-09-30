use schist_layout::{
    properties::{self, PageProperty},
    History, Insets, LayoutDocument, Page, PageBinding, PasteboardView, Spread,
};

#[test]
fn each_edge_expands_independently_and_media_encloses_bleed_and_slug() {
    for bleed in [
        Insets::ZERO,
        Insets::new(3.0, 7.0, 11.0, 15.0),
        Insets::new(9.0, 0.0, 1.0, 4.0),
    ] {
        for slug in [
            Insets::ZERO,
            Insets::new(0.0, 12.0, 3.0, 20.0),
            Insets::new(20.0, 0.0, 8.0, 2.0),
        ] {
            let mut page = Page::new("page", 180.0, 240.0);
            page.bleed = bleed;
            page.slug = slug;
            let b = page.bleed_rect();
            assert_eq!(
                (b.x, b.y, b.width, b.height),
                (
                    -bleed.left,
                    -bleed.top,
                    180.0 + bleed.left + bleed.right,
                    240.0 + bleed.top + bleed.bottom
                )
            );
            let m = page.media_rect();
            assert_eq!(
                (m.x, m.y),
                (-bleed.left.max(slug.left), -bleed.top.max(slug.top))
            );
            assert_eq!(m.width + m.x, 180.0 + bleed.right.max(slug.right));
            assert_eq!(m.height + m.y, 240.0 + bleed.bottom.max(slug.bottom));
            let view = PasteboardView::fit_page(&page, 500.0, 20.0);
            let fitted = view.rect(b);
            assert!(fitted.x >= 20.0 && fitted.y >= 20.0);
            assert!(fitted.x + fitted.width <= 480.001 && fitted.y + fitted.height <= 480.001);
        }
    }
}

#[test]
fn document_offset_edits_follow_spine_sides_in_one_undo_step() {
    for binding in [PageBinding::LeftToRight, PageBinding::RightToLeft] {
        for spine in 0..=4 {
            let mut doc = LayoutDocument::new(vec![Page::a4(); 4]);
            doc.facing_pages = true;
            doc.page_binding = binding;
            doc.spreads = vec![Spread {
                pages: if binding == PageBinding::LeftToRight {
                    vec![0, 1, 2, 3]
                } else {
                    vec![3, 2, 1, 0]
                },
                binding_location: Some(spine),
                ..Spread::single(0)
            }];
            let before = doc.clone();
            for property in [
                PageProperty::BleedTop,
                PageProperty::BleedBottom,
                PageProperty::BleedInside,
                PageProperty::BleedOutside,
                PageProperty::SlugTop,
                PageProperty::SlugBottom,
                PageProperty::SlugInside,
                PageProperty::SlugOutside,
            ] {
                let mut history = History::default();
                assert!(properties::set_page_property(
                    &mut doc,
                    &mut history,
                    &[0, 1, 2, 3, 0],
                    property,
                    12.0
                ));
                for i in 0..4 {
                    assert_eq!(property.value_for(&doc, i), 12.0);
                    let offsets = match property {
                        PageProperty::BleedTop
                        | PageProperty::BleedBottom
                        | PageProperty::BleedInside
                        | PageProperty::BleedOutside => doc.pages[i].bleed,
                        _ => doc.pages[i].slug,
                    };
                    assert_eq!(
                        offsets.top + offsets.right + offsets.bottom + offsets.left,
                        12.0
                    );
                    match property {
                        PageProperty::BleedInside | PageProperty::SlugInside => assert_eq!(
                            if doc.page_is_left(i) {
                                offsets.right
                            } else {
                                offsets.left
                            },
                            12.0
                        ),
                        PageProperty::BleedOutside | PageProperty::SlugOutside => assert_eq!(
                            if doc.page_is_left(i) {
                                offsets.left
                            } else {
                                offsets.right
                            },
                            12.0
                        ),
                        _ => {}
                    }
                }
                assert_eq!(history.undo_depth(), 1);
                assert!(history.undo(&mut doc));
                assert_eq!(doc, before);
            }
        }
    }
}
