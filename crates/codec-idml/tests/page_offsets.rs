use schist_codec_idml::{container, export, import, xml};
use schist_layout::{Insets, LayoutDocument, Page, PageBinding, Spread};

#[test]
fn native_offsets_keep_every_edge_including_slugs_inside_bleed_on_both_binding_sides() {
    for direction in [PageBinding::LeftToRight, PageBinding::RightToLeft] {
        for facing in [false, true] {
            for spine in 0..=4 {
                let mut doc = LayoutDocument::new(vec![Page::new("page", 200.0, 300.0); 4]);
                doc.page_binding = direction;
                doc.facing_pages = facing;
                doc.spreads = vec![Spread {
                    pages: if direction == PageBinding::LeftToRight {
                        vec![0, 1, 2, 3]
                    } else {
                        vec![3, 2, 1, 0]
                    },
                    binding_location: Some(spine),
                    ..Spread::single(0)
                }];
                let mut package = container::read(&export::write(&doc).bytes).unwrap();
                let binding = if direction == PageBinding::LeftToRight {
                    "LeftToRight"
                } else {
                    "RightToLeft"
                };
                package.insert("Resources/Preferences.xml", format!(r#"<idPkg:Preferences xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><DocumentPreference FacingPages="{facing}" PageBinding="{binding}" DocumentBleedUniformSize="false" DocumentBleedTopOffset="3" DocumentBleedBottomOffset="9" DocumentBleedInsideOrLeftOffset="5" DocumentBleedOutsideOrRightOffset="7" DocumentSlugUniformSize="false" SlugTopOffset="1" SlugBottomOffset="18" SlugInsideOrLeftOffset="11" SlugRightOrOutsideOffset="2" /></idPkg:Preferences>"#).into_bytes());
                doc = import::read(&container::write(&package.into_parts()))
                    .unwrap()
                    .document;
                for _ in 0..3 {
                    for i in 0..4 {
                        let left = facing && doc.spreads[0].pages[..spine].contains(&i);
                        assert_eq!(
                            doc.pages[i].bleed,
                            if left {
                                Insets::new(3.0, 5.0, 9.0, 7.0)
                            } else {
                                Insets::new(3.0, 7.0, 9.0, 5.0)
                            }
                        );
                        assert_eq!(
                            doc.pages[i].slug,
                            if left {
                                Insets::new(1.0, 11.0, 18.0, 2.0)
                            } else {
                                Insets::new(1.0, 2.0, 18.0, 11.0)
                            }
                        );
                    }
                    let written = export::write(&doc);
                    assert!(!written
                        .warnings
                        .iter()
                        .any(|w| w == schist_i18n::t("design.idml_per_page_offsets")));
                    let package = container::read(&written.bytes).unwrap();
                    let root =
                        xml::parse(package.text("Resources/Preferences.xml").unwrap()).unwrap();
                    let prefs = root.find("DocumentPreference").unwrap();
                    assert_eq!(prefs.attr("DocumentBleedUniformSize"), Some("false"));
                    assert_eq!(prefs.attr("DocumentSlugUniformSize"), Some("false"));
                    for (key, value) in [
                        ("DocumentBleedInsideOrLeftOffset", 5.0),
                        ("DocumentBleedOutsideOrRightOffset", 7.0),
                        ("SlugTopOffset", 1.0),
                        ("SlugBottomOffset", 18.0),
                        ("SlugInsideOrLeftOffset", 11.0),
                        ("SlugRightOrOutsideOffset", 2.0),
                    ] {
                        assert_eq!(prefs.number(key), Some(value));
                    }
                    let back = import::read(&written.bytes).unwrap().document;
                    assert_eq!(back.pages, doc.pages);
                    doc = back;
                }
            }
        }
    }
}

#[test]
fn different_per_page_offsets_expand_only_the_affected_native_edges_with_a_notice() {
    let mut doc = LayoutDocument::new(vec![Page::a4(); 2]);
    doc.pages[0].bleed.left = 7.0;
    doc.pages[1].bleed.top = 3.0;
    doc.pages[1].slug.right = 12.0;
    let written = export::write(&doc);
    assert_eq!(
        written
            .warnings
            .iter()
            .filter(|w| *w == schist_i18n::t("design.idml_per_page_offsets"))
            .count(),
        1
    );
    let back = import::read(&written.bytes).unwrap().document;
    for page in back.pages {
        assert_eq!(page.bleed, Insets::new(3.0, 0.0, 0.0, 7.0));
        assert_eq!(page.slug, Insets::new(0.0, 12.0, 0.0, 0.0));
    }
}
