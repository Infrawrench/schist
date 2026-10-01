use schist_codec_idml::{container, designmap::DesignPackage, export, import};
use schist_layout::{blank_a4, Ink, ParagraphStyle, Story};

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;")
}

#[test]
fn part_names_and_namespace_prefixes_do_not_change_the_document() {
    let mut doc = blank_a4();
    doc.pages[0].bleed = (7.0).into();
    doc.pages[0].slug = (11.0).into();
    doc.stories.push(Story::from_text("é & 空", "Body"));
    let ink = Ink::cmyk("Brand / 空", [0.1, 0.7, 0.3, 0.2]);
    doc.inks.push(ink.clone());
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Body".into(),
        fill: Some(ink),
        point_size: Some(19.0),
        ..Default::default()
    });
    let bytes = export::write(&doc).bytes;
    let expected = import::read(&bytes).unwrap().document;
    for prefix in ["idPkg", "pkg", "任意"] {
        for decoration in ["plain", "空", "a&b"] {
            let mut package = container::read(&bytes).unwrap();
            let references = DesignPackage::open(&package).unwrap().listed;
            let mut root = package.text("designmap.xml").unwrap().to_owned();
            for (index, part) in references.iter().enumerate() {
                let new_name = format!("arbitrary/{decoration}/{index}.xml");
                root = root.replace(
                    &format!("src=\"{}\"", escape(&part.name)),
                    &format!("src=\"{}\"", escape(&new_name)),
                );
                let contents = package.get(&part.name).unwrap().to_vec();
                package.remove(&part.name);
                package.insert(new_name, contents);
            }
            root = root
                .replace("idPkg:", &format!("{prefix}:"))
                .replace("xmlns:idPkg=", &format!("xmlns:{prefix}="));
            let root_name = format!("root-{decoration}.xml");
            package.remove("designmap.xml");
            package.insert(&root_name, root.into_bytes());
            package.insert(
                "META-INF/container.xml",
                format!(
                    "<container><rootfiles><rootfile full-path=\"{}\"/></rootfiles></container>",
                    escape(&root_name),
                )
                .into_bytes(),
            );
            let opened = DesignPackage::open(&package).unwrap();
            for part in &references {
                if !part.id.is_empty() {
                    assert_eq!(
                        opened.part_for_id(&part.id).unwrap(),
                        container::read(&bytes).unwrap().get(&part.name).unwrap()
                    );
                }
            }
            let actual = import::read_package(&opened).unwrap().document;
            assert_eq!(actual.pages, expected.pages);
            assert_eq!(actual.stories, expected.stories);
            assert_eq!(actual.styles, expected.styles);
            assert_eq!(actual.inks, expected.inks);
        }
    }
}

#[test]
fn a_rebound_packaging_prefix_does_not_invent_parts() {
    let mut package = container::read(&export::write(&blank_a4()).bytes).unwrap();
    let root = package.text("designmap.xml").unwrap().replace(
        "</Document>",
        r#"<idPkg:Story xmlns:idPkg="urn:unrelated" src="missing.xml"/></Document>"#,
    );
    package.insert("designmap.xml", root.into_bytes());
    assert!(!DesignPackage::open(&package)
        .unwrap()
        .listed
        .iter()
        .any(|part| part.name == "missing.xml"));
}
