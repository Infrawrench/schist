//! Public IDML image structure: a frame contains an Image, whose Properties
//! hold GraphicBounds and optional base64 Contents, followed by its Link.
//! Verified against fixtures/idml/images.idml and the published IDML cookbook.
use crate::{
    export::{escape, number, path_geometry, rect_geometry},
    import::Report,
    xml::{self, Element},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use schist_layout::{
    affine::{self, Affine},
    graphics::{image_affine, image_rect},
    GraphicFit, GraphicInfo, LayoutDocument, LayoutObject, Link, PlacedObject, Point, Rect,
    ShapePath,
};
use std::{collections::BTreeMap, sync::Arc};

const LABEL: &str = "Schist.Graphic.v1";

pub(crate) fn read(
    element: &Element,
    frame: Rect,
    assets: &mut BTreeMap<String, Arc<Vec<u8>>>,
    report: &mut Report,
) -> LayoutObject {
    let image = element.child("Image").unwrap_or(element);
    let mut link = image
        .find("Link")
        .map(|link| {
            Link::new(crate::import::strip_file_uri(
                link.attr("LinkResourceURI").unwrap_or_default(),
            ))
        })
        .unwrap_or_else(|| Link::new(""));
    let embedded = image.find("Link").and_then(|link| link.attr("StoredState")) == Some("Embedded");
    if embedded {
        if let Some(content) = image.child("Properties").and_then(|p| p.child("Contents")) {
            let encoded: Vec<_> = content
                .text
                .bytes()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            let decoded = (encoded.len() <= 64 * 1024 * 1024)
                .then(|| STANDARD.decode(&encoded))
                .and_then(Result::ok);
            if let Some(bytes) = decoded {
                if assets
                    .get(&link.path)
                    .is_some_and(|previous| previous.as_slice() != bytes)
                {
                    link.path = format!("{}#{}", link.path, image.attr("Self").unwrap_or("image"));
                }
                assets.insert(link.path.clone(), Arc::new(bytes));
            } else {
                report.skip(schist_i18n::t("design.idml_embedded_invalid"));
            }
        } else {
            report.skip(schist_i18n::t("design.idml_embedded_unavailable"));
        }
    }
    let bounds = image
        .child("Properties")
        .and_then(|p| p.child("GraphicBounds"))
        .and_then(|bounds| {
            Some(Rect::new(
                bounds.number("Left")?,
                bounds.number("Top")?,
                bounds.number("Right")? - bounds.number("Left")?,
                bounds.number("Bottom")? - bounds.number("Top")?,
            ))
        })
        .filter(|b| {
            b.width > 0.0
                && b.height > 0.0
                && [b.x, b.y, b.width, b.height].iter().all(|v| v.is_finite())
        });
    let ppi = image
        .attr("ActualPpi")
        .map(xml::numbers)
        .and_then(|v| v.first().copied())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(72.0);
    if let Some(bounds) = bounds.filter(|b| b.width > 0.0 && b.height > 0.0) {
        link.info = Some(GraphicInfo {
            width: (bounds.width * ppi / 72.0).round() as u32,
            height: (bounds.height * ppi / 72.0).round() as u32,
            dpi: ppi,
        });
    }
    let matrix = image
        .attr("ItemTransform")
        .map(xml::numbers)
        .unwrap_or_else(|| vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let native = (matrix.len() == 6 && matrix.iter().all(|n| n.is_finite())).then(|| Affine {
        a: matrix[0],
        b: matrix[1],
        c: matrix[2],
        d: matrix[3],
        tx: matrix[4],
        ty: matrix[5],
    });
    // Unit source square -> frame-local points, independent of source DPI.
    let source_map = native.zip(bounds).and_then(|(matrix, b)| {
        let map = matrix
            .then(&Affine::translate(b.x, b.y))
            .then(&Affine::scale(b.width, b.height));
        affine::finite(map).then_some(map)
    });
    let displayed = bounds.and_then(|b| {
        (matrix.len() == 6
            && matrix.iter().all(|n| n.is_finite())
            && matrix[0] > 0.0
            && matrix[3] > 0.0
            && matrix[1] == 0.0
            && matrix[2] == 0.0)
            .then(|| {
                Rect::new(
                    matrix[4] + matrix[0] * b.x,
                    matrix[5] + matrix[3] * b.y,
                    matrix[0] * b.width,
                    matrix[3] * b.height,
                )
            })
    });
    let crop = displayed.map(|image| {
        Rect::new(
            (frame.x - image.x) / image.width,
            (frame.y - image.y) / image.height,
            frame.width / image.width,
            frame.height / image.height,
        )
    });
    let image_transform = if displayed.is_some() {
        Affine::IDENTITY
    } else {
        source_map
            .filter(|_| frame.width > 0.0 && frame.height > 0.0)
            .map(|map| {
                Affine::scale(1.0 / frame.width, 1.0 / frame.height)
                    .then(&Affine::translate(-frame.x, -frame.y))
                    .then(&map)
            })
            .unwrap_or_else(|| Affine::scale(0.0, 0.0))
    };
    let clip_path = crate::import::path_of(element).and_then(|mut path| {
        if frame.width <= 0.0 || frame.height <= 0.0 {
            return Some(path);
        }
        path.map_points(|p| {
            Point::new(
                (p.x - frame.x) / frame.width,
                (p.y - frame.y) / frame.height,
            )
        });
        (!is_rectangle(&path)).then_some(path)
    });
    let mut object = LayoutObject::GraphicFrame {
        link,
        embedded,
        fit: GraphicFit::Stretch,
        crop,
        scale: 1.0,
        image_transform,
        clip_path: clip_path.clone(),
    };
    // Standard geometry remains authoritative. Our label only recovers fitting
    // intent if its predicted image still agrees, so edits in other programs
    // cannot be overridden by stale private metadata.
    if let Some(mut metadata) = element
        .child("Properties")
        .and_then(|p| p.child("Label"))
        .and_then(|label| {
            label
                .children_named("KeyValuePair")
                .find(|pair| pair.attr("Key") == Some(LABEL))
        })
        .and_then(|pair| pair.attr("Value"))
        .and_then(|value| serde_json::from_str::<LayoutObject>(value).ok())
    {
        if let LayoutObject::GraphicFrame {
            link: saved,
            fit,
            crop,
            scale,
            embedded: saved_embedded,
            image_transform,
            ..
        } = &metadata
        {
            let LayoutObject::GraphicFrame { link, .. } = &object else {
                unreachable!()
            };
            let info = source_info(saved, frame);
            let expected = image_rect(
                frame,
                (info.width, info.height),
                info.dpi,
                *crop,
                *fit,
                *scale,
            )
            .and_then(|r| {
                image_affine(frame, *image_transform).map(|m| {
                    m.then(&Affine::translate(r.x, r.y))
                        .then(&Affine::scale(r.width, r.height))
                })
            });
            if saved.path == link.path
                && *saved_embedded == embedded
                && expected
                    .zip(source_map)
                    .is_some_and(|(a, b)| same_corners(a, b))
            {
                if let LayoutObject::GraphicFrame {
                    clip_path: saved_clip,
                    ..
                } = &mut metadata
                {
                    // Native edits to the outline win independently of image
                    // fitting. Preserve exact handles only when they agree.
                    if !same_outline(saved_clip.as_ref(), clip_path.as_ref()) {
                        *saved_clip = clip_path;
                    }
                }
                object = metadata;
            }
        }
    }
    if source_map.and_then(affine::inverse).is_none() {
        report.skip(schist_i18n::t("design.idml_image_transform"));
    }
    object
}

fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 0.00001 && (a.y - b.y).abs() < 0.00001
}

/// Canonicalize only a real four-corner rectangle, not a bow tie or a
/// degenerate contour that happens to share the same bounds.
fn is_rectangle(path: &ShapePath) -> bool {
    let [sub] = path.subpaths.as_slice() else {
        return false;
    };
    if !sub.closed || sub.points.len() != 4 {
        return false;
    }
    let corners = affine::corners(Rect::new(0.0, 0.0, 1.0, 1.0));
    let mut seen = [false; 4];
    for (i, point) in sub.points.iter().copied().enumerate() {
        let Some(corner) = corners.iter().position(|p| near(*p, point)) else {
            return false;
        };
        if seen[corner] {
            return false;
        }
        seen[corner] = true;
        let handles = sub.handles_at(i);
        if !near(handles.incoming.unwrap_or(point), point)
            || !near(handles.outgoing.unwrap_or(point), point)
        {
            return false;
        }
        let next = sub.points[(i + 1) % 4];
        if (point.x - next.x).abs() > 0.00001 && (point.y - next.y).abs() > 0.00001 {
            return false;
        }
    }
    true
}

fn same_outline(a: Option<&ShapePath>, b: Option<&ShapePath>) -> bool {
    let a = a.filter(|p| !is_rectangle(p));
    let b = b.filter(|p| !is_rectangle(p));
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            // The native path has no supported even-odd declaration. The
            // writer warns and retains that intent only in our own metadata.
            a.subpaths.len() == b.subpaths.len()
                && a.subpaths.iter().zip(&b.subpaths).all(|(a, b)| {
                    a.closed == b.closed
                        && a.points.len() == b.points.len()
                        && a.points
                            .iter()
                            .zip(&b.points)
                            .enumerate()
                            .all(|(i, (p, q))| {
                                let ah = a.handles_at(i);
                                let bh = b.handles_at(i);
                                near(*p, *q)
                                    && near(ah.incoming.unwrap_or(*p), bh.incoming.unwrap_or(*q))
                                    && near(ah.outgoing.unwrap_or(*p), bh.outgoing.unwrap_or(*q))
                            })
                })
        }
        _ => false,
    }
}

fn same_corners(a: Affine, b: Affine) -> bool {
    affine::corners(Rect::new(0.0, 0.0, 1.0, 1.0))
        .into_iter()
        .all(|p| {
            let a = affine::point(a, p);
            let b = affine::point(b, p);
            (a.x - b.x).abs() < 0.05 && (a.y - b.y).abs() < 0.05
        })
}

fn source_info(link: &Link, frame: Rect) -> GraphicInfo {
    link.info.unwrap_or(GraphicInfo {
        width: frame.width.ceil().max(1.0) as u32,
        height: frame.height.ceil().max(1.0) as u32,
        dpi: 72.0,
    })
}

pub(crate) fn write(
    document: &LayoutDocument,
    object: &PlacedObject,
    layer: schist_layout::LayerId,
    id: &str,
) -> Option<String> {
    let LayoutObject::GraphicFrame {
        link,
        embedded,
        fit,
        crop,
        scale,
        image_transform,
        clip_path,
    } = &object.object
    else {
        return None;
    };
    let frame = Rect::new(0.0, 0.0, object.bounds.width, object.bounds.height);
    let info = source_info(link, frame);
    let mapped = image_rect(
        frame,
        (info.width, info.height),
        info.dpi,
        *crop,
        *fit,
        *scale,
    )?;
    let source_width = info.width as f32 * 72.0 / info.dpi;
    let source_height = info.height as f32 * 72.0 / info.dpi;
    let inner = image_affine(frame, *image_transform)?
        .then(&Affine::translate(mapped.x, mapped.y))
        .then(&Affine::scale(
            mapped.width / source_width,
            mapped.height / source_height,
        ));
    affine::inverse(inner)?;
    let inner = [inner.a, inner.b, inner.c, inner.d, inner.tx, inner.ty]
        .map(number)
        .join(" ");
    let metadata = escape(&serde_json::to_string(&object.object).ok()?);
    let contents = if *embedded {
        format!(
            "<Contents>{}</Contents>",
            STANDARD.encode(document.assets.get(&link.path)?.as_slice())
        )
    } else {
        String::new()
    };
    let name = escape(&object.name);
    let geometry = if let Some(path) = clip_path {
        let mut path = path.clone();
        path.map_points(|p| Point::new(p.x * frame.width, p.y * frame.height));
        path_geometry(&path)
    } else {
        rect_geometry(&frame)
    };
    let uri = file_uri(&link.path);
    let opacity = crate::color_codec::transparency(object.transparency);
    let state = if *embedded { "Embedded" } else { "Normal" };
    Some(format!(
        r#"<Rectangle Self="{id}" Name="{name}" ItemLayer="SchistLayer{}" ContentType="GraphicType" Locked="{}" ItemTransform="{}"><Properties>{geometry}<Label><KeyValuePair Key="{LABEL}" Value="{metadata}" /></Label></Properties><Image Self="{id}image" ActualPpi="{dpi} {dpi}" ItemTransform="{inner}"><Properties><GraphicBounds Left="0" Top="0" Right="{}" Bottom="{}" />{contents}</Properties><Link Self="{id}link" LinkResourceURI="{}" StoredState="{state}" /></Image>{opacity}</Rectangle>"#,
        layer.0,
        object.locked,
        crate::export::item_transform(object),
        number(source_width),
        number(source_height),
        escape(&uri),
        dpi = number(info.dpi)
    ))
}

fn file_uri(path: &str) -> String {
    let mut out = String::from("file:");
    for byte in path.replace('\\', "/").bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_outline_edits_override_stale_metadata_without_losing_fitting_intent() {
        let mut doc = schist_layout::blank_a4();
        schist_layout::authoring::graphic_frame(
            &mut doc,
            &mut Default::default(),
            0,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            "art.png",
            false,
        )
        .unwrap();
        let LayoutObject::GraphicFrame { fit, clip_path, .. } = &mut doc.objects[0].object else {
            panic!()
        };
        *fit = GraphicFit::Contain;
        *clip_path = Some(ShapePath::ellipse(1.0, 1.0));
        let original = write(&doc, &doc.objects[0], doc.layers[0], "frame").unwrap();
        for mut path in [
            ShapePath::ellipse(1.0, 1.0),
            schist_layout::authoring::path_for(
                schist_layout::authoring::ShapeKind::Rectangle,
                1.0,
                1.0,
            ),
        ] {
            // Changed curve handles with unchanged anchor bounds must count.
            if !is_rectangle(&path) {
                path.subpaths[0].handles[0].incoming.as_mut().unwrap().y += 0.1;
            }
            let expected = (!is_rectangle(&path)).then_some(path.clone());
            path.map_points(|p| p.scale(100.0));
            let mut element = xml::parse(&original).unwrap();
            let properties = element
                .children
                .iter_mut()
                .find(|e| e.name == "Properties")
                .unwrap();
            *properties
                .children
                .iter_mut()
                .find(|e| e.name == "PathGeometry")
                .unwrap() = xml::parse(&path_geometry(&path)).unwrap();
            let object = read(
                &element,
                Rect::new(0.0, 0.0, 100.0, 100.0),
                &mut Default::default(),
                &mut Report::default(),
            );
            let LayoutObject::GraphicFrame { fit, clip_path, .. } = object else {
                panic!()
            };
            assert_eq!(fit, GraphicFit::Contain);
            assert!(same_outline(clip_path.as_ref(), expected.as_ref()));
            assert_ne!(clip_path, Some(ShapePath::ellipse(1.0, 1.0)));
        }
    }

    #[test]
    fn rectangle_canonicalization_rejects_bow_ties_repeated_corners_and_curved_edges() {
        let base = schist_layout::authoring::path_for(
            schist_layout::authoring::ShapeKind::Rectangle,
            1.0,
            1.0,
        );
        for reverse in [false, true] {
            for start in 0..4 {
                let mut path = base.clone();
                if reverse {
                    path.subpaths[0].points.reverse();
                }
                path.subpaths[0].points.rotate_left(start);
                assert!(is_rectangle(&path));
                let points = &mut path.subpaths[0].points;
                points.swap(0, 1);
                assert!(!is_rectangle(&path));
            }
        }
        let mut path = base.clone();
        path.subpaths[0].points[2] = path.subpaths[0].points[0];
        assert!(!is_rectangle(&path));
        let mut path = base;
        path.subpaths[0].set_handles(
            0,
            schist_layout::BezierHandles {
                incoming: Some(Point::new(0.1, 0.1)),
                outgoing: None,
            },
        );
        assert!(!is_rectangle(&path));
    }

    #[test]
    fn unusable_image_geometry_remains_unpaintable_instead_of_guessing_stretch() {
        for matrix in ["0 0 0 1 0 0", "1 2 3", "NaN 0 0 1 0 0"] {
            let xml = format!(
                r#"<Rectangle><Image ItemTransform="{matrix}"><Properties><GraphicBounds Left="0" Top="0" Right="100" Bottom="100"/></Properties><Link LinkResourceURI="art.png"/></Image></Rectangle>"#
            );
            let mut report = Report::default();
            let object = read(
                &xml::parse(&xml).unwrap(),
                Rect::new(0.0, 0.0, 100.0, 100.0),
                &mut Default::default(),
                &mut report,
            );
            let LayoutObject::GraphicFrame {
                image_transform, ..
            } = object
            else {
                panic!()
            };
            assert!(affine::inverse(image_transform).is_none());
            assert!(report
                .skipped
                .iter()
                .any(|w| w == schist_i18n::t("design.idml_image_transform")));
        }
    }

    #[test]
    fn image_rotation_and_reflection_override_stale_fitting_metadata() {
        let mut doc = schist_layout::blank_a4();
        let id = schist_layout::authoring::graphic_frame(
            &mut doc,
            &mut Default::default(),
            0,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            "art.png",
            false,
        )
        .unwrap();
        let original = write(&doc, doc.object(id).unwrap(), doc.layers[0], "frame").unwrap();
        for matrix in [
            Affine::rotate(0.7).around(50.0, 50.0),
            Affine::scale(-1.0, 1.0).around(50.0, 50.0),
            Affine::skew(0.4, -0.3),
        ] {
            let mut element = xml::parse(&original).unwrap();
            let image = element
                .children
                .iter_mut()
                .find(|e| e.name == "Image")
                .unwrap();
            image
                .attributes
                .iter_mut()
                .find(|(key, _)| key == "ItemTransform")
                .unwrap()
                .1 = [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
                .map(number)
                .join(" ");
            let object = read(
                &element,
                Rect::new(0.0, 0.0, 100.0, 100.0),
                &mut Default::default(),
                &mut Report::default(),
            );
            let LayoutObject::GraphicFrame {
                image_transform, ..
            } = object
            else {
                panic!()
            };
            assert_ne!(image_transform, Affine::IDENTITY);
            assert!(same_corners(
                image_affine(Rect::new(0.0, 0.0, 100.0, 100.0), image_transform)
                    .unwrap()
                    .then(&Affine::scale(100.0, 100.0)),
                matrix.then(&Affine::scale(100.0, 100.0)),
            ));
        }
    }

    #[test]
    fn external_geometry_changes_override_old_schist_labels() {
        let mut doc = schist_layout::blank_a4();
        let mut link = Link::new("/image.png");
        link.info = Some(GraphicInfo {
            width: 100,
            height: 100,
            dpi: 72.0,
        });
        let id = schist_layout::authoring::graphic_frame_with_link(
            &mut doc,
            &mut Default::default(),
            0,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            link,
            false,
        )
        .unwrap();
        let xml = write(&doc, doc.object(id).unwrap(), doc.layers[0], "frame").unwrap();
        let altered = xml.replace(
            "ItemTransform=\"1 0 0 1 0 0\"><Properties><GraphicBounds",
            "ItemTransform=\"2 0 0 2 -50 -50\"><Properties><GraphicBounds",
        );
        assert_ne!(xml, altered);
        let object = read(
            &xml::parse(&altered).unwrap(),
            Rect::new(0.0, 0.0, 100.0, 100.0),
            &mut Default::default(),
            &mut Report::default(),
        );
        let LayoutObject::GraphicFrame {
            fit, crop, scale, ..
        } = object
        else {
            unreachable!()
        };
        assert_eq!(fit, GraphicFit::Stretch);
        assert_eq!(crop, Some(Rect::new(0.25, 0.25, 0.5, 0.5)));
        assert_eq!(scale, 1.0);
    }
}
