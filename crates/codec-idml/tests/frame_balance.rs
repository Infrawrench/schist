use schist_codec_idml::{container, export, import};
use schist_layout::{authoring, blank_a4, History, LayoutObject, ObjectStyle, Rect};

fn local(doc: &schist_layout::LayoutDocument) -> Vec<Option<bool>> {
    doc.objects
        .iter()
        .filter_map(|o| match o.object {
            LayoutObject::TextFrame {
                balance_columns, ..
            } => Some(balance_columns),
            _ => None,
        })
        .collect()
}

#[test]
fn native_balance_values_inheritance_defaults_and_invalid_input_survive_saves() {
    for raw in [None, Some("false"), Some("true"), Some("invalid")] {
        let mut doc = blank_a4();
        authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(0.0, 0.0, 300.0, 400.0),
        )
        .unwrap();
        doc.balance_columns_default = true;
        doc.styles.objects.extend([
            ObjectStyle {
                name: "Base".into(),
                enable_text_frame_general: Some(true),
                balance_columns: Some(true),
                ..Default::default()
            },
            ObjectStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                ..Default::default()
            },
        ]);
        doc.objects[0].appearance.style = Some("Child".into());
        let bytes = export::write(&doc).bytes;
        let package = container::read(&bytes).unwrap();
        let bytes = container::write(
            &package
                .names()
                .iter()
                .map(|name| {
                    let bytes = package.get(name).unwrap();
                    let bytes = if name.starts_with("Spreads/") {
                        String::from_utf8(bytes.to_vec())
                            .unwrap()
                            .replace(
                                " VerticalBalanceColumns=\"false\"",
                                &raw.map(|v| format!(" VerticalBalanceColumns=\"{v}\""))
                                    .unwrap_or_default(),
                            )
                            .into_bytes()
                    } else {
                        bytes.to_vec()
                    };
                    ((*name).to_owned(), bytes)
                })
                .collect::<Vec<_>>(),
        );
        let result = import::read(&bytes).unwrap();
        let expected = match raw {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        };
        if raw == Some("invalid") {
            assert!(format!("{:?}", result.report).contains("VerticalBalanceColumns"));
        }
        let mut doc = result.document;
        for _ in 0..3 {
            assert_eq!(local(&doc), vec![expected]);
            assert!(doc.balance_columns_default);
            assert_eq!(
                doc.styles.frame_balance(&doc.objects[0]),
                expected.unwrap_or(true)
            );
            assert_eq!(
                doc.styles.resolve_object("Child").balance_columns,
                Some(true)
            );
            let bytes = export::write(&doc).bytes;
            let package = container::read(&bytes).unwrap();
            let spread = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("Spreads/"))
                .unwrap();
            let xml = std::str::from_utf8(package.get(spread).unwrap()).unwrap();
            assert_eq!(xml.contains("VerticalBalanceColumns="), expected.is_some());
            doc = import::read(&bytes).unwrap().document;
        }
    }
}

#[test]
fn public_native_balance_values_remain_distinct_from_style_and_document_defaults() {
    let bytes = include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.idml");
    let mut doc = import::read(bytes).unwrap().document;
    let expected = local(&doc);
    assert!(expected.contains(&Some(true)));
    assert!(!doc.balance_columns_default);
    assert!(doc
        .styles
        .objects
        .iter()
        .any(|s| s.balance_columns == Some(false)));
    for _ in 0..3 {
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(local(&doc), expected);
        assert!(!doc.balance_columns_default);
        assert!(doc
            .styles
            .objects
            .iter()
            .any(|s| s.balance_columns == Some(false)));
    }
}
