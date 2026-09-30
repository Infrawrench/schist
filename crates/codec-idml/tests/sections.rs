use schist_codec_idml::{container, export, import, xml};
use schist_layout::{History, LayoutDocument, NumberStyle, Page, PageBinding, Section};

fn document() -> LayoutDocument {
    LayoutDocument::new(
        (0..6)
            .map(|i| Page::new(format!("page {i}"), 200.0, 300.0))
            .collect(),
    )
}

fn with_sections(sections: &str) -> Vec<u8> {
    let mut package = container::read(&export::write(&document()).bytes).unwrap();
    let mut root = package.text("designmap.xml").unwrap().to_owned();
    let from = root.find("<Section ").unwrap();
    let to = root[from..].find("</Section>").unwrap() + from + "</Section>".len();
    root.replace_range(from..to, sections);
    // Native Section references are opaque IDs, not numeric page positions.
    root = root.replace("SchistPage", "opaque-page-");
    package.insert("designmap.xml", root.into_bytes());
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
            .replace("SchistPage", "opaque-page-");
        package.insert(path, text.into_bytes());
    }
    container::write(&package.into_parts())
}

#[test]
fn native_sections_resolve_references_and_continue_across_restarts_and_styles() {
    // Deliberately reverse XML section order. Prefix content must survive even
    // when disabled, and a continuing section must ignore its stored restart.
    let bytes = with_sections(
        r#"
      <Section Self="last" PageStart="SchistPage4" Length="2" ContinueNumbering="true" PageNumberStart="99" IncludeSectionPrefix="true" SectionPrefix="App-&amp;" Name="Appendix" Marker="末">
        <Properties><PageNumberStyle type="enumeration">UpperLetters</PageNumberStyle></Properties>
      </Section>
      <Section Self="body" PageStart="SchistPage2" Length="2" ContinueNumbering="false" PageNumberStart="7" IncludeSectionPrefix="false" SectionPrefix="Hidden-" Name="Body">
        <Properties><PageNumberStyle type="enumeration">Arabic</PageNumberStyle></Properties>
      </Section>
      <Section Self="front" PageStart="SchistPage0" Length="2" ContinueNumbering="false" PageNumberStart="3" IncludeSectionPrefix="true" SectionPrefix="前-">
        <Properties><PageNumberStyle type="enumeration">LowerRoman</PageNumberStyle></Properties>
      </Section>"#,
    );
    let imported = import::read(&bytes).unwrap();
    assert!(!imported
        .report
        .skipped
        .iter()
        .any(|s| s == schist_i18n::t("design.idml_invalid_section")));
    let mut doc = imported.document;
    let expected = ["前-iii", "前-iv", "7", "8", "App-&I", "App-&J"];
    let pages = doc.pages.clone();
    for _ in 0..4 {
        assert_eq!(
            (0..6).map(|i| doc.page_number(i)).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(doc.pages[2].section.as_ref().unwrap().prefix, "Hidden-");
        assert_eq!(doc.pages[4].section.as_ref().unwrap().marker, "末");
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.pages, pages);
    }
}

#[test]
fn every_section_boundary_combination_survives_native_saves_with_correct_lengths() {
    for mask in 0..64 {
        for direction in [PageBinding::LeftToRight, PageBinding::RightToLeft] {
            let mut doc = document();
            doc.page_binding = direction;
            for (i, page) in doc.pages.iter_mut().enumerate() {
                if mask & (1 << i) != 0 {
                    page.section = Some(Section {
                        start: 20 + i as u32,
                        continue_numbering: i % 2 == 1,
                        style: [
                            NumberStyle::Arabic,
                            NumberStyle::RomanLower,
                            NumberStyle::RomanUpper,
                            NumberStyle::AlphaLower,
                            NumberStyle::AlphaUpper,
                        ][i % 5],
                        prefix: format!("章 & <{i}> "),
                        include_prefix: i % 3 == 0,
                        name: format!("Section {i}"),
                        marker: format!("Marker {i}"),
                    });
                }
            }
            let pages = doc.pages.clone();
            let labels: Vec<_> = (0..6).map(|i| doc.page_number(i)).collect();
            for _ in 0..3 {
                let written = export::write(&doc);
                let package = container::read(&written.bytes).unwrap();
                let root = xml::parse(package.text("designmap.xml").unwrap()).unwrap();
                let sections: Vec<_> = root.children_named("Section").collect();
                let mut offset = 0;
                for section in sections {
                    assert_eq!(
                        section.attr("PageStart"),
                        Some(format!("SchistPage{offset}").as_str())
                    );
                    offset += section.attr("Length").unwrap().parse::<usize>().unwrap();
                }
                assert_eq!(offset, 6);
                let imported = import::read(&written.bytes).unwrap();
                assert!(!imported
                    .report
                    .skipped
                    .iter()
                    .any(|s| s == schist_i18n::t("design.idml_invalid_section")));
                doc = imported.document;
                assert_eq!(doc.pages, pages);
                assert_eq!(
                    (0..6).map(|i| doc.page_number(i)).collect::<Vec<_>>(),
                    labels
                );
            }
        }
    }
}

#[test]
fn page_reordering_and_deletion_rewrite_native_section_references_and_lengths() {
    let mut original = document();
    for i in [0, 2, 4] {
        original.pages[i].section = Some(Section {
            start: i as u32 + 7,
            continue_numbering: false,
            name: format!("S{i}"),
            ..Default::default()
        });
    }
    for from in 0..6 {
        for to in 0..6 {
            if from == to {
                continue;
            }
            let mut doc = original.clone();
            let mut history = History::default();
            assert!(schist_layout::structure::move_page(
                &mut doc,
                &mut history,
                from,
                to
            ));
            for _ in 0..2 {
                let back = import::read(&export::write(&doc).bytes).unwrap().document;
                assert_eq!(back.pages, doc.pages);
                for i in 0..6 {
                    assert_eq!(back.page_number(i), doc.page_number(i));
                }
                doc = back;
            }
            assert!(schist_layout::structure::remove_page(
                &mut doc,
                &mut history,
                to
            ));
            let back = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(back.pages, doc.pages);
            for i in 0..5 {
                assert_eq!(back.page_number(i), doc.page_number(i));
            }
        }
    }
}

#[test]
fn malformed_references_duplicates_and_lengths_are_reported_without_discarding_valid_sections() {
    let bytes = with_sections(
        r#"
      <Section Self="good" PageStart="SchistPage2" Length="99" ContinueNumbering="false" PageNumberStart="12" />
      <Section Self="duplicate" PageStart="SchistPage2" PageNumberStart="42" />
      <Section Self="missing" PageStart="missing-page" />
      <Section Self="absent" />"#,
    );
    let imported = import::read(&bytes).unwrap();
    assert_eq!(
        imported
            .report
            .skipped
            .iter()
            .filter(|s| *s == schist_i18n::t("design.idml_invalid_section"))
            .count(),
        4
    );
    assert_eq!(
        (0..6)
            .map(|i| imported.document.page_number(i))
            .collect::<Vec<_>>(),
        ["1", "2", "12", "13", "14", "15"]
    );
    let written = export::write(&imported.document);
    let back = import::read(&written.bytes).unwrap();
    assert!(!back
        .report
        .skipped
        .iter()
        .any(|s| s == schist_i18n::t("design.idml_invalid_section")));
}
