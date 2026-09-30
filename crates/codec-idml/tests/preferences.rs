use schist_codec_idml::{container, export, import, xml};
use schist_layout::{blank_a4, NumberStyle, Page, Spread};

#[test]
fn native_fixture_document_offsets_are_applied_to_every_page() {
    let bytes = include_bytes!("../../../fixtures/idml/multipage.idml");
    let doc = import::read(bytes).unwrap().document;
    assert!(doc.pages.len() > 1);
    assert!(!doc.facing_pages);
    for page in doc.pages {
        assert_eq!(page.bleed, 9.0);
        assert_eq!(page.slug, 9.0); // native slug = 18 from trim
    }
}

#[test]
fn page_settings_survive_repeated_native_saves_for_every_numbering_style() {
    for (style, native) in [
        (NumberStyle::Arabic, "Arabic"),
        (NumberStyle::RomanLower, "LowerRoman"),
        (NumberStyle::RomanUpper, "UpperRoman"),
        (NumberStyle::AlphaLower, "LowerLetters"),
        (NumberStyle::AlphaUpper, "UpperLetters"),
    ] {
        for facing in [true, false] {
            for (bleed, slug) in [(0.0, 0.0), (9.0, 0.0), (3.5, 18.0)] {
                let mut doc = blank_a4();
                doc.pages.push(Page::letter());
                doc.spreads = vec![Spread {
                    pages: vec![0, 1],
                    binding_location: None,
                    gutter: 0.0,
                    origin: Default::default(),
                }];
                for page in &mut doc.pages {
                    page.bleed = bleed;
                    page.slug = slug;
                }
                doc.facing_pages = facing;
                doc.page_number_start = 27;
                doc.page_number_style = style;
                doc.page_number_prefix = "Ch / 空 & ".into();
                for _ in 0..4 {
                    let written = export::write(&doc);
                    let package = container::read(&written.bytes).unwrap();
                    let root = xml::parse(package.text("designmap.xml").unwrap()).unwrap();
                    let section = root.child("Section").unwrap();
                    assert_eq!(section.attr("PageStart"), Some("SchistPage0"));
                    assert_eq!(
                        section
                            .child("Properties")
                            .unwrap()
                            .child("PageNumberStyle")
                            .unwrap()
                            .trimmed(),
                        native
                    );
                    let preferences =
                        xml::parse(package.text("Resources/Preferences.xml").unwrap()).unwrap();
                    assert!(preferences.find("DocumentPreference").is_some());
                    assert!(preferences.find("DocumentPreferences").is_none());
                    let back = import::read(&written.bytes).unwrap().document;
                    assert_eq!(back.pages, doc.pages);
                    assert_eq!(back.facing_pages, doc.facing_pages);
                    for page in 0..doc.pages.len() {
                        assert_eq!(back.page_number(page), doc.page_number(page));
                    }
                    doc = back;
                }
            }
        }
    }
}

#[test]
fn per_page_offsets_cannot_silently_become_document_offsets() {
    let mut doc = blank_a4();
    doc.pages.push(Page::letter());
    doc.spreads.push(Spread {
        pages: vec![1],
        binding_location: None,
        gutter: 0.0,
        origin: Default::default(),
    });
    doc.pages[0].bleed = 5.0;
    doc.pages[1].slug = 12.0;
    let written = export::write(&doc);
    assert!(written
        .warnings
        .iter()
        .any(|w| w == schist_i18n::t("design.idml_per_page_offsets")));
    let back = import::read(&written.bytes).unwrap().document;
    assert!(back.pages.iter().all(|p| p.bleed == 5.0 && p.slug == 12.0));
}

#[test]
fn every_hidden_page_combination_survives_saves_with_an_explicit_native_visibility_notice() {
    for mask in 0u8..16 {
        let mut doc = schist_layout::LayoutDocument::new(
            (0..4)
                .map(|i| Page::new(format!("page {i}"), 200.0, 300.0))
                .collect(),
        );
        for (i, page) in doc.pages.iter_mut().enumerate() {
            page.hidden = mask & (1 << i) != 0;
        }
        let expected = doc.pages.clone();
        for _ in 0..4 {
            let written = export::write(&doc);
            assert_eq!(
                written
                    .warnings
                    .iter()
                    .filter(|w| *w == schist_i18n::t("design.idml_page_visibility"))
                    .count(),
                usize::from(mask != 0)
            );
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(doc.pages, expected);
            // This is private Schist intent. Without that label, native pages
            // remain visible; do not reinterpret OptionalPage or other flags.
            let mut package = container::read(&written.bytes).unwrap();
            let paths: Vec<_> = package
                .names()
                .into_iter()
                .filter(|p| p.starts_with("Spreads/"))
                .map(str::to_owned)
                .collect();
            for path in paths {
                let text = package
                    .text(&path)
                    .unwrap()
                    .replace("Schist.PageVisibility.v1", "UnrelatedLabel");
                package.insert(path, text.into_bytes());
            }
            let native = import::read(&container::write(&package.into_parts())).unwrap();
            assert!(native.document.pages.iter().all(|p| !p.hidden));
        }
    }
}
