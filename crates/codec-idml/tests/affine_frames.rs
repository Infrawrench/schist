use schist_codec_idml::{container, export, import};
use schist_layout::{blank_a4, compose_object, Point, Story};

fn apply(m: [f32; 6], p: Point) -> Point {
    Point::new(
        m[0] * p.x + m[2] * p.y + m[4],
        m[1] * p.x + m[3] * p.y + m[5],
    )
}

#[test]
fn native_frame_affines_keep_geometry_and_composition_through_repeated_saves() {
    let matrices = [
        [1.0, 0.0, 0.0, 1.0, 100.0, 120.0],
        [0.0, 1.0, -1.0, 0.0, 180.0, 140.0],
        [-1.0, 0.0, 0.0, 1.0, 250.0, 110.0],
        [1.4, 0.2, 0.4, 0.7, 120.0, 210.0],
    ];
    for matrix in matrices {
        for group in [
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            [0.8, 0.1, -0.2, 1.1, 60.0, 50.0],
        ] {
            let mut doc = blank_a4();
            doc.stories.push(Story::from_text(
                "Rotated text keeps its local wrapping. ".repeat(10),
                "Body",
            ));
            let mut package = container::read(&export::write(&doc).bytes).unwrap();
            let story_id = schist_codec_idml::designmap::DesignPackage::open(&package)
                .unwrap()
                .listed_of(schist_codec_idml::designmap::PartKind::Story)[0]
                .id
                .clone();
            let spread = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("Spreads/"))
                .unwrap()
                .to_string();
            let m = matrix.map(|v| v.to_string()).join(" ");
            let g = group.map(|v| v.to_string()).join(" ");
            package.insert(spread, format!(r#"<idPkg:Spread><Spread Self="s"><Page Self="p" GeometricBounds="0 0 842 595" ItemTransform="1 0 0 1 0 0"/>
            <Group Self="g" ItemTransform="{g}">
              <TextFrame Self="t" ParentStory="{story_id}" GeometricBounds="-20 -30 40 90" ItemTransform="{m}"/>
              <Rectangle Self="r" GeometricBounds="-20 -30 40 90" ItemTransform="{m}"><Image Self="i" ActualPpi="72 72" ItemTransform="1 0 0 1 -30 -20"><Properties><GraphicBounds Left="0" Top="0" Right="120" Bottom="60"/></Properties><Link Self="l" LinkResourceURI="file:art.png" StoredState="Normal"/></Image></Rectangle>
            </Group></Spread></idPkg:Spread>"#).into_bytes());
            let mut doc = import::read(&container::write(&package.into_parts()))
                .unwrap()
                .document;
            let composition = compose_object(&doc, &doc.objects[0]).unwrap();
            for _ in 0..5 {
                for object in &doc.objects {
                    assert!((object.bounds.width - 120.0).abs() < 0.001);
                    assert!((object.bounds.height - 60.0).abs() < 0.001);
                    for local in [
                        Point::ZERO,
                        Point::new(120.0, 0.0),
                        Point::new(0.0, 60.0),
                        Point::new(120.0, 60.0),
                    ] {
                        let expected =
                            apply(group, apply(matrix, local + Point::new(-30.0, -20.0)));
                        let actual = schist_layout::affine::point(
                            object.content_transform(),
                            local + object.bounds.origin(),
                        );
                        assert!(
                            (expected.x - actual.x).abs() < 0.01
                                && (expected.y - actual.y).abs() < 0.01,
                            "{matrix:?} {group:?}: {expected:?} != {actual:?}"
                        );
                    }
                }
                let actual = compose_object(&doc, &doc.objects[0]).unwrap();
                assert_eq!(
                    actual
                        .lines
                        .iter()
                        .map(|l| (l.start, l.end))
                        .collect::<Vec<_>>(),
                    composition
                        .lines
                        .iter()
                        .map(|l| (l.start, l.end))
                        .collect::<Vec<_>>()
                );
                doc = import::read(&export::write(&doc).bytes).unwrap().document;
            }
        }
    }
}
