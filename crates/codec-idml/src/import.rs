//! Reading a package into a [`schist_layout::LayoutDocument`].
//!
//! Everything here is written from the seven real exports in
//! `fixtures/idml`; `docs/idml-format.md` records the observations, and
//! records just as carefully the places no fixture confirms.
//!
//! ## The encoding, in brief
//!
//! - **Geometry** is points, in two attributes. `GeometricBounds` is
//!   **top left bottom right** — not x/y/width/height — while
//!   `ItemTransform` is an affine `a b c d tx ty` carrying the position.
//!   A page gets its size from the first and its place from the second.
//! - **A page** is a `<Page>` inside a spread part, with its master named
//!   by `AppliedMaster`.
//! - **A story** nests `ParagraphStyleRange > CharacterStyleRange`, whose
//!   `<Content>` chunks and `<Br/>` elements are the text. That flattens
//!   onto [`schist_layout::Story`] with explicit paragraph and structural
//!   breaks. Style ranges may span several paragraphs; `<Br/>` ends one.
//! - **Object ids** do not carry their type, and most objects are
//!   declared inline rather than in a part of their own. Only stories,
//!   spreads and master spreads are promoted to a file each, so those are
//!   the only ids that resolve to a part.
//!
//! ## What is not composed
//!
//! Tables, footnotes and anchored objects retain their outer XML and source
//! anchors as opaque story structures. They are not laid out or painted; the
//! [`Report`] and print preflight disclose that missing output.

use schist_layout::{
    Insets, LayoutDocument, LayoutObject, ObjectId, Orientation, Page, ParentObject, ParentPage,
    PlacedObject, Rect, ShapePath, Spread, Story, StoryId, SubPath,
};
// `Point` names two things in the layout crate: a point on a path, and a
// point in a story. The crate already re-exports the story one as
// `StoryPoint`, and the path one is aliased here.
use schist_layout::geometry::Point as Coordinate;

#[cfg(test)]
use schist_layout::StoryPoint;

use crate::designmap::{DesignPackage, PartKind};
use crate::error::Error;
use crate::xml::{self, Element};

/// What an import could not read.
///
/// Returned beside the document rather than instead of it: a partly read
/// document is useful, and saying what is missing is what makes it
/// usable.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub skipped: Vec<String>,
}

impl Report {
    pub(crate) fn skip(&mut self, what: impl Into<String>) {
        self.skipped.push(what.into());
    }

    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty()
    }
}

/// The outcome of reading a package.
pub struct Imported {
    pub document: LayoutDocument,
    pub report: Report,
}

/// Read a package into a layout document.
pub fn read(bytes: &[u8]) -> Result<Imported, Error> {
    let package = crate::container::open(bytes)?;
    let opened = DesignPackage::open(&package)?;
    read_package(&opened)
}

/// Read an already-opened package.
pub fn read_package(opened: &DesignPackage<'_>) -> Result<Imported, Error> {
    let mut report = Report::default();

    // Inks first: a colour reference elsewhere is a name, and the names
    // only mean something once the inks they point at exist.
    let colors = crate::color_codec::read(opened, &mut report);

    let mut document = LayoutDocument::new(Vec::new());
    // Native IDML's implicit paragraph direction is LTR. Schist-created root
    // styles restore their Auto default through validated standard labels.
    for style in &mut document.styles.paragraphs {
        if style.based_on.is_none() {
            style.direction = Some(schist_layout::ParagraphDirection::LeftToRight);
        }
    }
    document.inks = colors.values().cloned().collect();
    document.styles.languages = crate::language_codec::read(opened, &mut report)?;
    document.styles.numbering_lists = crate::list_codec::read_resources(opened, &mut report)?;
    let layers = read_layers(opened, &mut document)?;
    let mut style_roots = Vec::new();
    for part in opened.listed.iter().filter(|part| part.role == "Styles") {
        let root = xml::parse(opened.text_of(&part.name)?).map_err(|message| Error::Xml {
            part: part.name.clone(),
            message,
        })?;
        style_roots.push(root);
    }
    let mut style_refs = crate::style_codec::References::new(&style_roots);
    style_refs.languages = document.styles.languages.clone();
    style_refs.strokes = crate::stroke_style_codec::read(opened, &mut report);
    document.styles.strokes = style_refs
        .strokes
        .values()
        .filter(|s| !matches!(s.pattern, schist_text_engine::TextDecorationPattern::Solid))
        .cloned()
        .collect();
    for root in &style_roots {
        crate::object_style_codec::read_styles(
            root,
            &mut document,
            &colors,
            &style_refs,
            &mut report,
        );
        crate::style_codec::read(
            root,
            &mut document.styles,
            &colors,
            &mut report,
            &style_refs,
        );
    }
    // Named styles precede local overrides; stories precede the frames that reference them.
    let stories = read_stories(
        opened,
        &mut document.styles,
        &colors,
        &style_refs,
        &mut report,
    );
    for (_, story) in &stories {
        document.stories.push(story.clone());
    }

    // A page names its master by an InDesign id, and a master part
    // carries that same id. The two are joined after both are read, so
    // the claims are collected on the way through the spreads.
    let mut spread_state = SpreadState::default();
    let mut frames = read_spreads(
        opened,
        &stories,
        &mut document,
        &mut spread_state,
        &layers,
        &colors,
        &mut report,
    )?;
    let masters = read_master_spreads(
        opened,
        &mut document,
        &stories,
        &layers,
        &colors,
        &mut report,
    )?;
    frames.extend(
        masters
            .iter()
            .flat_map(|master| master.frames.iter().cloned()),
    );
    crate::thread_codec::resolve(&mut document, &frames, &mut report);
    resolve_master_sources(&mut document, &masters, &mut report);
    apply_masters(
        &mut document,
        &masters,
        &spread_state.master_claims,
        &mut report,
    );
    crate::object_style_codec::resolve_references(&mut document, &style_refs, &mut report);
    crate::preferences_codec::read(opened, &mut document, &mut report)?;
    // Parsing XML visits items in paint order. Only guarded chronology labels
    // can establish their creation order; never certify the incidental walk.
    document.creation_order = spread_state.creation.finish(&mut report);

    report
        .skipped
        .extend(crate::list_codec::diagnostics(&document));
    if !opened.unlisted.is_empty() {
        report.skip(schist_i18n::tf!(
            "design.idml_unlisted_parts",
            count = opened.unlisted.len(),
            parts = opened.unlisted.join(", ")
        ));
    }
    Ok(Imported { document, report })
}

// -- stories ----------------------------------------------------------

/// Every story part, with the InDesign id a frame will refer to.
fn read_stories(
    opened: &DesignPackage<'_>,
    styles: &mut schist_layout::StyleSet,
    colors: &crate::color_codec::Colors,
    refs: &crate::style_codec::References,
    report: &mut Report,
) -> Vec<(String, Story)> {
    let mut stories = Vec::new();
    for part in opened.listed_of(PartKind::Story) {
        let Ok(text) = opened.text_of(&part.name) else {
            continue;
        };
        let Ok(root) = xml::parse(text) else {
            report.skip(schist_i18n::tf!(
                "design.idml_unreadable_xml",
                part = part.name
            ));
            continue;
        };
        let Some(story) = root.find("Story") else {
            report.skip(schist_i18n::tf!(
                "design.idml_missing_element",
                part = part.name,
                element = "Story"
            ));
            continue;
        };
        let decoded = flatten_story(&crate::story_codec::normalize(
            story, styles, colors, refs, report,
        ));
        stories.push((
            story.attr("Self").unwrap_or_default().to_owned(),
            crate::structured_story::restore(story, decoded, styles, refs, report),
        ));
    }
    stories
}

/// Decode native story breaks and character ranges into the model's byte coordinates.
fn flatten_story(story: &Element) -> Story {
    crate::story_codec::decode(story)
}

// -- spreads and pages ------------------------------------------------

/// Read every spread part into pages, objects and spreads.
fn read_spreads(
    opened: &DesignPackage<'_>,
    stories: &[(String, Story)],
    document: &mut LayoutDocument,
    state: &mut SpreadState,
    layers: &[(String, schist_layout::LayerId)],
    colors: &crate::color_codec::Colors,
    report: &mut Report,
) -> Result<Vec<crate::thread_codec::FrameReference>, Error> {
    let mut references = Vec::new();
    for part in opened.listed_of(PartKind::Spread) {
        let text = opened.text_of(&part.name)?;
        let root = xml::parse(text).map_err(|message| Error::Xml {
            part: part.name.clone(),
            message,
        })?;
        let Some(spread) = root.find("Spread") else {
            report.skip(schist_i18n::tf!(
                "design.idml_missing_element",
                part = part.name,
                element = "Spread"
            ));
            continue;
        };
        references.extend(read_spread(
            spread, stories, document, state, layers, colors, report,
        ));
    }
    Ok(references)
}

/// One `<Spread>`: its pages, its objects, and the spread itself.
///
/// The element order is `<Page>` first and the page's items after it, as
/// siblings of the page rather than children of it. Objects are assigned
/// to the page containing their center (or the nearest page on the pasteboard),
/// then converted from spread coordinates to the model's page coordinates.
fn read_spread(
    spread: &Element,
    stories: &[(String, Story)],
    document: &mut LayoutDocument,
    state: &mut SpreadState,
    layers: &[(String, schist_layout::LayerId)],
    colors: &crate::color_codec::Colors,
    report: &mut Report,
) -> Vec<crate::thread_codec::FrameReference> {
    let mut references = Vec::new();
    let elements: Vec<&Element> = spread.children_named("Page").collect();
    let mut pages = Vec::new();
    let mut boxes = Vec::new();
    for element in &elements {
        let Some(page) = page_of(element) else {
            report.skip(schist_i18n::t("design.idml_page_geometry"));
            continue;
        };
        let index = document.pages.len();
        boxes
            .push(bounds_of(element, transform(element.attr("ItemTransform"))).unwrap_or_default());
        document.pages.push(page);
        pages.push(index);
        if let Some(master) = element
            .attr("AppliedMaster")
            .filter(|m| !m.is_empty() && *m != "n")
        {
            state.master_claims.push(MasterClaim {
                page: index,
                reference: master.to_owned(),
                bounds: *boxes.last().unwrap(),
                transform: transform(element.attr("MasterPageTransform")),
                visible: spread.attr("ShowMasterItems") != Some("false"),
                overrides: element
                    .attr("OverrideList")
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
            });
        }
    }
    if pages.is_empty() {
        return references;
    }

    // Items hang off the spread, after the pages.
    for (child, parent, locked, opacity) in
        page_items(spread, Transform::default(), false, 1.0, report)
    {
        let Some(mut placed) =
            placed_object(child, stories, parent, colors, report, &mut document.assets)
        else {
            continue;
        };
        placed.locked |= locked;
        placed.transparency *= opacity;
        let center = Coordinate::new(
            placed.visual_bounds().x + placed.visual_bounds().width / 2.0,
            placed.visual_bounds().y + placed.visual_bounds().height / 2.0,
        );
        let owner = boxes
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let distance = |r: &Rect| {
                    (center.x - center.x.clamp(r.x, r.right())).powi(2)
                        + (center.y - center.y.clamp(r.y, r.bottom())).powi(2)
                };
                distance(a).total_cmp(&distance(b))
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        placed.page = pages[owner];
        placed.bounds.x -= boxes[owner].x;
        placed.bounds.y -= boxes[owner].y;
        let id = document.add_object(placed);
        state.creation.collect(child, id);
        if let Some(child) = crate::text_path_codec::reference(child) {
            references.push(crate::thread_codec::FrameReference {
                external: child.attr("Self").unwrap_or_default().to_string(),
                next: child
                    .attr("NextTextFrame")
                    .filter(|s| !s.is_empty() && *s != "n")
                    .map(str::to_string),
                object: id,
            });
        }
        on_layer(document, id, child.attr("ItemLayer"), layers);
    }

    // Native page elements follow reading order. The pasteboard stores
    // physical slots, including right-to-left documents with descending x.
    let mut physical: Vec<_> = pages.into_iter().zip(boxes).collect();
    physical.sort_by(|(_, a), (_, b)| a.x.total_cmp(&b.x));
    let gutter = physical
        .get(1)
        .map(|(_, right)| right.x - physical[0].1.right())
        .unwrap_or(0.0);
    let binding_location = spread
        .attr("BindingLocation")
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.min(physical.len()))
        .unwrap_or_else(|| physical.iter().filter(|(_, b)| b.center().x < 0.0).count());
    document.spreads.push(Spread {
        pages: physical.into_iter().map(|(page, _)| page).collect(),
        binding_location: Some(binding_location),
        gutter,
        origin: schist_layout::geometry::Point::ZERO,
    });
    references
}

/// A `<Page>` element as a [`Page`].
///
/// `GeometricBounds` is `top left bottom right`, so the size is the
/// difference between the last pair rather than the values themselves.
pub(crate) fn page_of(element: &Element) -> Option<Page> {
    let bounds = xml::numbers(element.attr("GeometricBounds")?);
    if bounds.len() < 4 {
        return None;
    }
    let (top, left, bottom, right) = (bounds[0], bounds[1], bounds[2], bounds[3]);
    let width = right - left;
    let height = bottom - top;
    if bounds.iter().any(|v| !v.is_finite()) || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let margins = element
        .child("MarginPreference")
        .map(|margin| Insets {
            top: margin.number("Top").unwrap_or(0.0),
            right: margin.number("Right").unwrap_or(0.0),
            bottom: margin.number("Bottom").unwrap_or(0.0),
            left: margin.number("Left").unwrap_or(0.0),
        })
        .unwrap_or_default();
    Some(Page {
        name: element.attr("Name").unwrap_or_default().to_owned(),
        section: None,
        width,
        height,
        // Filled from DocumentPreference after the pages and parents load.
        bleed: Insets::ZERO,
        slug: Insets::ZERO,
        margins,
        orientation: if width > height {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        },
        hidden: element
            .child("Properties")
            .and_then(|properties| properties.child("Label"))
            .is_some_and(|label| {
                label.children_named("KeyValuePair").any(|entry| {
                    entry.attr("Key") == Some("Schist.PageVisibility.v1")
                        && entry.attr("Value") == Some("Hidden")
                })
            }),
        master: None,
        guides: element
            .children_named("Guide")
            .filter_map(|guide| {
                let position = guide.number("Location")?;
                if !position.is_finite() {
                    return None;
                }
                let horizontal = match guide.attr("Orientation")? {
                    "Horizontal" => true,
                    "Vertical" => false,
                    _ => return None,
                };
                Some(schist_layout::geometry::RulerGuide {
                    horizontal,
                    position,
                    locked: guide.attr("Locked") == Some("true"),
                })
            })
            .collect(),
    })
}

// -- objects ----------------------------------------------------------

/// A frame as a [`PlacedObject`], if this element is one.
fn placed_object(
    element: &Element,
    stories: &[(String, Story)],
    page_transform: Transform,
    colors: &crate::color_codec::Colors,
    report: &mut Report,
    assets: &mut std::collections::BTreeMap<String, std::sync::Arc<Vec<u8>>>,
) -> Option<PlacedObject> {
    let transform = transform(element.attr("ItemTransform")).then(page_transform);
    if element.child("Group").is_some() {
        report.skip(schist_i18n::t("design.idml_clipped_group"));
    }
    let baseline = crate::text_path_codec::read(element, report);
    let mut object = match element.name.as_str() {
        _ if element.name == "TextFrame" || baseline.is_some() => {
            let native = crate::text_path_codec::reference(element).unwrap();
            let reference = native.attr("ParentStory").unwrap_or_default();
            // `n` is InDesign's null reference, on a frame with no flow.
            let Some(story) = story_index(reference, stories) else {
                report.skip(schist_i18n::tf!(
                    "design.idml_missing_story",
                    story = reference
                ));
                return None;
            };
            LayoutObject::TextFrame {
                text_path: baseline,
                story: StoryId(story as u32),
                columns: element
                    .find("TextFramePreference")
                    .and_then(|p| p.number("TextColumnCount"))
                    .unwrap_or(1.0)
                    .max(1.0) as u16,
                gutter: element
                    .find("TextFramePreference")
                    .and_then(|p| p.number("TextColumnGutter"))
                    .unwrap_or(0.0),
                insets: insets_of(element),
                overflow: if native
                    .attr("NextTextFrame")
                    .is_some_and(|next| !next.is_empty() && next != "n")
                {
                    schist_layout::FrameOverflow::Thread
                } else {
                    schist_layout::FrameOverflow::Clip
                },
            }
        }
        "Graphic" | "Rectangle" | "Polygon" | "Oval"
            if element.name == "Graphic" || element.child("Image").is_some() =>
        {
            crate::graphic_codec::read(
                element,
                bounds_of(element, Transform::default()).unwrap_or_default(),
                assets,
                report,
            )
        }
        "Note" => LayoutObject::Note {
            text: element.attr("Name").unwrap_or_default().to_owned(),
            author: String::new(),
        },
        "Rectangle" | "Polygon" | "Ellipse" | "Oval" | "GraphicLine" => {
            schist_layout::ObjectPaint::default().shape(path_of(element).unwrap_or_default())
        }
        other => {
            if !other.is_empty() {
                report.skip(schist_i18n::tf!(
                    "design.idml_unsupported_frame",
                    kind = other
                ));
            }
            return None;
        }
    };

    let local = if matches!(
        object,
        LayoutObject::TextFrame {
            text_path: Some(_),
            ..
        }
    ) {
        crate::text_path_codec::retained_bounds(element)
            .or_else(|| bounds_of(element, Transform::default()))
    } else {
        bounds_of(element, Transform::default())
    };
    let Some(mut bounds) = local else {
        report.skip(schist_i18n::tf!(
            "design.idml_frame_geometry",
            kind = element.name
        ));
        return None;
    };
    let matrix = schist_layout::affine::Affine {
        a: transform.a,
        b: transform.b,
        c: transform.c,
        d: transform.d,
        tx: 0.0,
        ty: 0.0,
    };
    match &mut object {
        LayoutObject::Shape { path, .. } => path.map_points(|point| point - bounds.origin()),
        LayoutObject::TextFrame {
            text_path: Some(path),
            ..
        } => path.path.map_points(|point| point - bounds.origin()),
        _ => {}
    }
    let origin = transform.apply(bounds.origin());
    bounds.x = origin.x;
    bounds.y = origin.y;
    let mut placed = PlacedObject {
        appearance: Default::default(),
        id: ObjectId::next(),
        page: 0,
        bounds,
        object,
        rotation: 0.0,
        transform: matrix,
        name: element.attr("Name").unwrap_or_default().to_owned(),
        locked: element
            .attr("Locked")
            .is_some_and(|locked| locked == "true"),
        overprint: false,
        transparency: crate::color_codec::opacity(element),
    };
    crate::object_style_codec::read_appearance(
        element,
        &mut placed,
        local.unwrap(),
        colors,
        report,
    );
    Some(placed)
}

/// A frame's inset spacing.
///
/// `InsetSpacing` is a scalar or four values in top, left, bottom, right
/// order, expressed as an attribute or nested ListItems.
fn insets_of(element: &Element) -> Insets {
    let Some(preference) = element.find("TextFramePreference") else {
        return Insets::ZERO;
    };
    let values = if let Some(value) = preference.attr("InsetSpacing") {
        xml::numbers(value)
    } else if let Some(spacing) = preference.find("InsetSpacing") {
        let items: Vec<_> = spacing
            .children_named("ListItem")
            .filter_map(|item| xml::parse_number(item.trimmed()))
            .collect();
        if items.is_empty() {
            xml::numbers(spacing.trimmed())
        } else {
            items
        }
    } else {
        Vec::new()
    };
    match values.as_slice() {
        [value] => Insets::uniform(*value),
        [top, left, bottom, right] => Insets::new(*top, *right, *bottom, *left),
        _ => Insets::ZERO,
    }
}

/// A frame's or shape's outline.
///
/// The nesting is `PathGeometry > GeometryPathType > PathPointArray >
/// PathPointType`, and **one `GeometryPathType` is one subpath**: its
/// `PathPointType` children are that subpath's points in order. Reading
/// each point as its own subpath gives a document with one shape per
/// point, which is wrong in a way that looks like a very small drawing
/// rather than like a parse error.
///
/// Points are frame-relative, the same convention a group's children use,
/// so they are kept as they are and the frame's transform places them.
pub(crate) fn path_of(element: &Element) -> Option<ShapePath> {
    let geometry = element.child("Properties")?.child("PathGeometry")?;
    let mut subpaths = Vec::new();
    // A `GeometryPathType` per subpath, so a compound shape is more than
    // one of them.
    for path in geometry.children_named("GeometryPathType") {
        let Some(array) = path.child("PathPointArray") else {
            continue;
        };
        let mut points = Vec::new();
        let mut handles = Vec::new();
        for point in array.children_named("PathPointType") {
            let coordinate = |name| -> Option<Coordinate> {
                let values = xml::numbers(point.attr(name)?);
                (values.len() == 2 && values.iter().all(|v| v.is_finite()))
                    .then(|| Coordinate::new(values[0], values[1]))
            };
            let Some(anchor) = coordinate("Anchor") else {
                continue;
            };
            points.push(anchor);
            handles.push(schist_layout::BezierHandles {
                incoming: coordinate("LeftDirection").filter(|p| *p != anchor),
                outgoing: coordinate("RightDirection").filter(|p| *p != anchor),
            });
        }
        if points.is_empty() {
            continue;
        }
        if handles.iter().all(|h| *h == Default::default()) {
            handles.clear();
        }
        subpaths.push(SubPath {
            handles,
            points,
            // `PathOpen="false"` is a closed path, which is a rectangle
            // for the four-point outline a frame carries.
            closed: !path.attr("PathOpen").is_some_and(|open| open == "true"),
        });
    }
    (!subpaths.is_empty()).then_some(ShapePath {
        subpaths,
        even_odd: geometry
            .attr("EvenOdd")
            .is_some_and(|rule| rule.eq_ignore_ascii_case("true")),
    })
}

/// Tight bounds after the complete item/parent affine.
fn bounds_of(element: &Element, transform: Transform) -> Option<Rect> {
    if let Some(mut path) = path_of(element) {
        path.map_points(|point| transform.apply(point));
        return Some(path.bounds());
    }
    let coordinates = xml::numbers(element.attr("GeometricBounds")?);
    let [top, left, bottom, right] = coordinates.as_slice() else {
        return None;
    };
    let mut corners = [
        Coordinate::new(*left, *top),
        Coordinate::new(*right, *top),
        Coordinate::new(*right, *bottom),
        Coordinate::new(*left, *bottom),
    ]
    .into_iter()
    .map(|p| transform.apply(p));
    let first = corners.next()?;
    Some(corners.fold(Rect::new(first.x, first.y, 0.0, 0.0), |r, p| {
        r.union(Rect::new(p.x, p.y, 0.0, 0.0))
    }))
}

/// Groups are flattened explicitly; child geometry, locking and opacity
/// survive. Group editing semantics are reported as unsupported.
fn page_items<'a>(
    root: &'a Element,
    parent: Transform,
    locked: bool,
    opacity: f32,
    report: &mut Report,
) -> Vec<(&'a Element, Transform, bool, f32)> {
    let mut out = Vec::new();
    for child in &root.children {
        if child.name == "Group" {
            report.skip(schist_i18n::t("design.idml_group_flattened"));
            out.extend(page_items(
                child,
                transform(child.attr("ItemTransform")).then(parent),
                locked || child.attr("Locked") == Some("true"),
                opacity * crate::color_codec::opacity(child),
                report,
            ));
        } else if !matches!(
            child.name.as_str(),
            "Page" | "Properties" | "TransparencySetting" | "FlattenerPreference"
        ) {
            out.push((child, parent, locked, opacity));
        }
    }
    out
}

// -- master spreads ---------------------------------------------------

/// Read master spread parts into parent pages.
fn read_master_spreads(
    opened: &DesignPackage<'_>,
    document: &mut LayoutDocument,
    stories: &[(String, Story)],
    layers: &[(String, schist_layout::LayerId)],
    colors: &crate::color_codec::Colors,
    report: &mut Report,
) -> Result<Vec<MasterReference>, Error> {
    let mut ids = Vec::new();
    for part in opened.listed_of(PartKind::MasterSpread) {
        let text = opened.text_of(&part.name)?;
        let root = xml::parse(text).map_err(|message| Error::Xml {
            part: part.name.clone(),
            message,
        })?;
        let Some(master) = root.find("MasterSpread") else {
            report.skip(schist_i18n::tf!(
                "design.idml_missing_element",
                part = part.name,
                element = "MasterSpread"
            ));
            continue;
        };
        let name = master
            .attr("Name")
            .filter(|name| !name.is_empty())
            .unwrap_or(part.name.as_str())
            .to_owned();
        let mut sheets = Vec::new();
        let mut sources = Vec::new();
        for element in master.children_named("Page") {
            let Some(page) = page_of(element) else {
                continue;
            };
            let Some(bounds) = bounds_of(element, transform(element.attr("ItemTransform"))) else {
                continue;
            };
            if let Some(reference) = element
                .attr("AppliedMaster")
                .filter(|m| !m.is_empty() && *m != "n")
            {
                sources.push(MasterClaim {
                    page: sheets.len(),
                    reference: reference.to_owned(),
                    bounds,
                    transform: transform(element.attr("MasterPageTransform")),
                    visible: master.attr("ShowMasterItems") != Some("false"),
                    overrides: element
                        .attr("OverrideList")
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect(),
                });
            }
            sheets.push(schist_layout::parents::ParentSheet {
                page,
                origin: bounds.origin(),
                source: None,
            });
        }
        let mut object_refs = Vec::new();
        let mut frames = Vec::new();
        let mut objects = Vec::new();
        for (child, parent, locked, opacity) in
            page_items(master, Transform::default(), false, 1.0, report)
        {
            if let Some(mut placed) =
                placed_object(child, stories, parent, colors, report, &mut document.assets)
            {
                placed.locked |= locked;
                placed.transparency *= opacity;
                let id = placed.id;
                let owner = nearest_sheet(&sheets, placed.visual_bounds().center());
                let origin = sheets.get(owner).map(|s| s.origin).unwrap_or_default();
                placed.page = owner;
                placed.bounds.x -= origin.x;
                placed.bounds.y -= origin.y;
                if let Some(reference) = child.attr("Self") {
                    object_refs.push((reference.to_owned(), id));
                }
                if let Some(child) = crate::text_path_codec::reference(child) {
                    frames.push(crate::thread_codec::FrameReference {
                        external: child.attr("Self").unwrap_or_default().to_owned(),
                        next: child
                            .attr("NextTextFrame")
                            .filter(|s| !s.is_empty() && *s != "n")
                            .map(str::to_owned),
                        object: id,
                    });
                }
                // `overridden_on` is empty, which is what makes the item
                // track its parent on every page that uses this master.
                objects.push(ParentObject {
                    object: placed,
                    overridden_on: Vec::new(),
                });
                on_layer(document, id, child.attr("ItemLayer"), layers);
            }
        }
        ids.push(MasterReference {
            external: master.attr("Self").unwrap_or_default().to_owned(),
            index: document.parents.len(),
            objects: object_refs,
            frames,
            sources,
        });
        document.parents.push(ParentPage {
            name,
            sheets,
            placements: Vec::new(),
            applied_to: Vec::new(),
            based_on: None,
            objects,
            hidden: false,
        });
    }
    Ok(ids)
}

#[derive(Default)]
struct SpreadState {
    master_claims: Vec<MasterClaim>,
    creation: crate::creation_codec::Reader,
}

struct MasterClaim {
    page: usize,
    reference: String,
    bounds: Rect,
    transform: Transform,
    visible: bool,
    overrides: Vec<String>,
}

struct MasterReference {
    external: String,
    index: usize,
    objects: Vec<(String, ObjectId)>,
    frames: Vec<crate::thread_codec::FrameReference>,
    sources: Vec<MasterClaim>,
}

fn nearest_sheet(sheets: &[schist_layout::parents::ParentSheet], center: Coordinate) -> usize {
    sheets
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let distance = |s: &schist_layout::parents::ParentSheet| {
                let x = center.x.clamp(s.origin.x, s.origin.x + s.page.width);
                let y = center.y.clamp(s.origin.y, s.origin.y + s.page.height);
                (center.x - x).powi(2) + (center.y - y).powi(2)
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn resolved_overrides(
    references: &[String],
    masters: &[MasterReference],
    report: &mut Report,
) -> Vec<ObjectId> {
    references
        .iter()
        .filter_map(|reference| {
            let found = masters
                .iter()
                .flat_map(|m| &m.objects)
                .find(|(external, _)| external == reference)
                .map(|(_, id)| *id);
            if found.is_none() {
                report.skip(schist_i18n::tf!(
                    "design.idml_parent_override",
                    item = reference
                ));
            }
            found
        })
        .collect()
}

fn parent_overlay(
    claim: &MasterClaim,
    parent: &ParentPage,
) -> (usize, schist_layout::affine::Affine) {
    use schist_layout::affine::Affine;
    let sheet = parent.sheet_for_side(claim.bounds.center().x < 0.0);
    let origin = parent
        .sheets
        .get(sheet)
        .map(|s| s.origin)
        .unwrap_or_default();
    let m = claim.transform;
    (
        sheet,
        Affine::translate(-claim.bounds.x, -claim.bounds.y)
            .then(&Affine {
                a: m.a,
                b: m.b,
                c: m.c,
                d: m.d,
                tx: m.tx,
                ty: m.ty,
            })
            .then(&Affine::translate(origin.x, origin.y)),
    )
}

fn resolve_master_sources(
    document: &mut LayoutDocument,
    masters: &[MasterReference],
    report: &mut Report,
) {
    for master in masters {
        for claim in &master.sources {
            let Some(base) = masters.iter().find(|m| m.external == claim.reference) else {
                report.skip(schist_i18n::tf!(
                    "design.idml_unknown_parent",
                    parent = claim.reference
                ));
                continue;
            };
            let (sheet, transform) = parent_overlay(claim, &document.parents[base.index]);
            let overrides = resolved_overrides(&claim.overrides, masters, report);
            document.parents[master.index].sheets[claim.page].source =
                Some(schist_layout::parents::ParentSource {
                    parent: base.index,
                    sheet,
                    transform,
                    visible: claim.visible,
                    overrides,
                });
        }
    }
    // Iterative graph traversal also bounds stack usage for long hierarchies.
    let mut state = std::collections::BTreeMap::new();
    for root in 0..document.parents.len() {
        for sheet in 0..document.parents[root].sheets.len().max(1) {
            let mut stack = vec![((root, sheet), false)];
            while let Some((key, exit)) = stack.pop() {
                if exit {
                    state.insert(key, 2);
                    continue;
                }
                match state.get(&key) {
                    Some(1) => {
                        report.skip(schist_i18n::t("design.idml_parent_cycle"));
                        return;
                    }
                    Some(2) => continue,
                    _ => {}
                }
                state.insert(key, 1);
                stack.push((key, true));
                if let Some(source) = document.parents[key.0].source(document, key.1) {
                    stack.push(((source.parent, source.sheet), false));
                }
            }
        }
    }
}

/// Resolve native page/master/object Self references. The overlay is converted
/// to page-local coordinates once; it never moves shared template objects.
fn apply_masters(
    document: &mut LayoutDocument,
    masters: &[MasterReference],
    claims: &[MasterClaim],
    report: &mut Report,
) {
    for claim in claims {
        let Some(master) = masters.iter().find(|m| m.external == claim.reference) else {
            report.skip(schist_i18n::tf!(
                "design.idml_unknown_parent",
                parent = claim.reference
            ));
            continue;
        };
        document.pages[claim.page].master = Some(master.index);
        let (sheet, transform) = parent_overlay(claim, &document.parents[master.index]);
        let parent = &mut document.parents[master.index];
        parent.applied_to.push(claim.page);
        parent
            .placements
            .push(schist_layout::parents::ParentPlacement {
                page: claim.page,
                sheet,
                transform,
                visible: claim.visible,
            });
        let overrides = resolved_overrides(&claim.overrides, masters, report);
        for parent in &mut document.parents {
            for item in &mut parent.objects {
                if overrides.contains(&item.object.id) {
                    item.overridden_on.push(claim.page);
                }
            }
        }
    }
}

// -- document assembly ------------------------------------------------

/// Put an object on the document's first layer.
///
/// An object on no layer cannot be selected, so every imported object goes
/// on layer zero rather than nowhere.
fn on_layer(
    document: &mut LayoutDocument,
    id: ObjectId,
    reference: Option<&str>,
    layers: &[(String, schist_layout::LayerId)],
) {
    let layer = reference
        .and_then(|name| {
            layers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, id)| *id)
        })
        .or_else(|| document.layers.first().copied());
    if let Some(layer) = layer {
        if let Some((_, existing)) = document
            .object_layers
            .iter_mut()
            .find(|(object, _)| *object == id)
        {
            *existing = layer;
        } else {
            document.object_layers.push((id, layer));
        }
    }
}

/// Layers are inline in designmap.xml; placed items refer to their Self
/// through ItemLayer. Both are present in the public vendor fixtures.
fn read_layers(
    opened: &DesignPackage<'_>,
    document: &mut LayoutDocument,
) -> Result<Vec<(String, schist_layout::LayerId)>, Error> {
    let root = xml::parse(opened.text_of(&opened.root)?).map_err(|message| Error::Xml {
        part: opened.root.clone(),
        message,
    })?;
    let mut refs = Vec::new();
    let mut properties = Vec::new();
    for element in root.children_named("Layer") {
        let Some(reference) = element.attr("Self") else {
            continue;
        };
        let id = schist_layout::LayerId(
            reference
                .strip_prefix("SchistLayer")
                .and_then(|n| n.parse().ok())
                .unwrap_or(refs.len() as u32),
        );
        if refs.iter().any(|(_, existing)| *existing == id) {
            continue;
        }
        refs.push((reference.to_owned(), id));
        let name = element.attr("Name").unwrap_or_default().to_owned();
        let visible = element.attr("Visible") != Some("false");
        let locked = element.attr("Locked") == Some("true");
        if !name.is_empty() || !visible || locked {
            properties.push(schist_layout::LayoutLayer {
                id,
                name,
                visible,
                locked,
            });
        }
    }
    if !refs.is_empty() {
        document.layers = refs.iter().map(|(_, id)| *id).collect();
        document.layer_properties = properties;
    }
    Ok(refs)
}

/// A story's index, from the id a frame refers to it by.
fn story_index(reference: &str, stories: &[(String, Story)]) -> Option<usize> {
    if reference.is_empty() || reference == "n" {
        return None;
    }
    stories.iter().position(|(id, _)| id == reference)
}

// -- values -----------------------------------------------------------

/// The public IDML affine order is a b c d tx ty.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Transform {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    tx: f32,
    ty: f32,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }
}
impl Transform {
    fn apply(self, p: Coordinate) -> Coordinate {
        Coordinate::new(
            self.a * p.x + self.c * p.y + self.tx,
            self.b * p.x + self.d * p.y + self.ty,
        )
    }
    fn then(self, p: Transform) -> Transform {
        let origin = p.apply(Coordinate::new(self.tx, self.ty));
        Transform {
            a: p.a * self.a + p.c * self.b,
            b: p.b * self.a + p.d * self.b,
            c: p.a * self.c + p.c * self.d,
            d: p.b * self.c + p.d * self.d,
            tx: origin.x,
            ty: origin.y,
        }
    }
}
fn transform(value: Option<&str>) -> Transform {
    let values = value.map(xml::numbers).unwrap_or_default();
    match values.as_slice() {
        [a, b, c, d, tx, ty] if values.iter().all(|v| v.is_finite()) => Transform {
            a: *a,
            b: *b,
            c: *c,
            d: *d,
            tx: *tx,
            ty: *ty,
        },
        _ => Transform::default(),
    }
}

/// `file:C:/dir/file%20name.png` is a link's original path, not ours.
pub(crate) fn strip_file_uri(uri: &str) -> String {
    let path = uri.strip_prefix("file:").unwrap_or(uri);
    // InDesign writes Windows separators and percent-escapes spaces.
    percent_decode(path).replace('\\', "/")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap_or(""),
                16,
            ) {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_story_references_never_borrow_an_unrelated_story() {
        let stories = vec![
            ("first".into(), Story::from_text("unrelated", "Body")),
            ("last".into(), Story::default()),
        ];
        for reference in ["", "n", "missing", "First"] {
            assert_eq!(story_index(reference, &stories), None);
        }
        assert_eq!(story_index("first", &stories), Some(0));
        assert_eq!(story_index("last", &stories), Some(1));
    }

    #[test]
    fn every_affine_term_and_parent_affine_reaches_shape_anchors_and_handles() {
        let source = r#"<Polygon ItemTransform="1 0.25 -0.5 2 17 -11"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
          <PathPointType Anchor="3 4" LeftDirection="1 9" RightDirection="13 -4"/>
          <PathPointType Anchor="30 14" LeftDirection="25 3" RightDirection="40 18"/>
          <PathPointType Anchor="10 24" LeftDirection="6 17" RightDirection="5 20"/>
        </PathPointArray></GeometryPathType></PathGeometry></Properties></Polygon>"#;
        let element = xml::parse(source).unwrap();
        for parent in [
            Transform::default(),
            Transform {
                a: 0.0,
                b: 2.0,
                c: -2.0,
                d: 0.0,
                tx: 70.0,
                ty: 90.0,
            },
            Transform {
                a: -1.0,
                b: 0.2,
                c: 0.8,
                d: 1.0,
                tx: -90.0,
                ty: 45.0,
            },
        ] {
            let object = placed_object(
                &element,
                &[],
                parent,
                &Default::default(),
                &mut Report::default(),
                &mut Default::default(),
            )
            .unwrap();
            let LayoutObject::Shape { path, .. } = &object.object else {
                panic!()
            };
            let original = path_of(&element).unwrap();
            for (a, b) in original.subpaths.iter().zip(&path.subpaths) {
                for i in 0..a.points.len() {
                    for (p, q) in [
                        (a.points[i], b.points[i]),
                        (
                            a.handles_at(i).incoming.unwrap(),
                            b.handles_at(i).incoming.unwrap(),
                        ),
                        (
                            a.handles_at(i).outgoing.unwrap(),
                            b.handles_at(i).outgoing.unwrap(),
                        ),
                    ] {
                        let local =
                            Coordinate::new(p.x - 0.5 * p.y + 17.0, 0.25 * p.x + 2.0 * p.y - 11.0);
                        let expected = Coordinate::new(
                            parent.a * local.x + parent.c * local.y + parent.tx,
                            parent.b * local.x + parent.d * local.y + parent.ty,
                        );
                        let actual = schist_layout::affine::point(
                            object.content_transform(),
                            q + object.bounds.origin(),
                        );
                        assert!(
                            (actual.x - expected.x).abs() < 0.0001
                                && (actual.y - expected.y).abs() < 0.0001
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn translating_a_spread_never_changes_page_local_geometry() {
        for count in 1..5 {
            for (dx, dy) in [(-500.0, -300.0), (0.0, 0.0), (721.0, 93.0)] {
                let mut xml = String::from("<Spread>");
                for page in 0..count {
                    let x = dx + page as f32 * 115.0;
                    xml.push_str(&format!(r#"<Page Self="page{page}" Name="same name" GeometricBounds="10 20 90 120" ItemTransform="1 0 0 1 {x} {dy}" />"#));
                }
                for page in 0..count {
                    let x = dx + page as f32 * 115.0 + 27.0;
                    let y = dy + 23.0;
                    xml.push_str(&format!(r#"<Rectangle Self="frame{page}" ItemTransform="1 0 0 1 {x} {y}"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray><PathPointType Anchor="0 0"/><PathPointType Anchor="10 0"/><PathPointType Anchor="10 20"/><PathPointType Anchor="0 20"/></PathPointArray></GeometryPathType></PathGeometry></Properties></Rectangle>"#));
                }
                xml.push_str("</Spread>");
                let spread = xml::parse(&xml).unwrap();
                let mut doc = LayoutDocument::new(Vec::new());
                read_spread(
                    &spread,
                    &[],
                    &mut doc,
                    &mut SpreadState::default(),
                    &[],
                    &Default::default(),
                    &mut Report::default(),
                );
                assert_eq!(doc.pages.len(), count);
                for page in &doc.pages {
                    assert_eq!((page.width, page.height), (100.0, 80.0));
                }
                for (index, object) in doc.objects.iter().enumerate() {
                    assert_eq!(object.page, index);
                    assert_eq!(object.bounds, Rect::new(7.0, 13.0, 10.0, 20.0));
                }
                assert_eq!(doc.spreads[0].gutter, if count > 1 { 15.0 } else { 0.0 });
            }
        }
    }

    /// The shape of a real frame, lifted from `fixtures/idml/text.idml`:
    /// the geometry nests inside `Properties`, and each `GeometryPathType`
    /// is one subpath.
    const FRAME: &str = r#"<TextFrame Self="u370" ParentStory="u373" Name="Body" ItemTransform="1 0 0 1 -260 181.97">
        <Properties>
            <PathGeometry>
                <GeometryPathType PathOpen="false">
                    <PathPointArray>
                        <PathPointType Anchor="-104 -114" />
                        <PathPointType Anchor="-104 486" />
                        <PathPointType Anchor="696 486" />
                        <PathPointType Anchor="696 -114" />
                    </PathPointArray>
                </GeometryPathType>
            </PathGeometry>
            <TextWrapPreference Inverse="false" />
        </Properties>
        <TextFramePreference TextColumnCount="1" TextColumnFixedWidth="720">
            <Properties><InsetSpacing type="list">
                <ListItem type="unit">4</ListItem>
                <ListItem type="unit">4</ListItem>
                <ListItem type="unit">4</ListItem>
                <ListItem type="unit">4</ListItem>
            </InsetSpacing></Properties>
        </TextFramePreference>
    </TextFrame>"#;

    #[test]
    fn a_frame_outline_is_one_subpath_of_four_points() {
        let root = xml::parse(FRAME).expect("parses");
        let path = path_of(&root).expect("the frame has a path");
        // One `GeometryPathType` is one subpath, not one point each.
        assert_eq!(path.subpaths.len(), 1, "{:?}", path.subpaths);
        assert_eq!(path.subpaths[0].points.len(), 4);
        assert!(path.subpaths[0].closed, "PathOpen=false is a closed path");
        assert_eq!(path.subpaths[0].points[0].x, -104.0);
    }

    #[test]
    fn a_frames_size_comes_from_its_outline_and_its_place_from_its_transform() {
        let root = xml::parse(FRAME).expect("parses");
        let bounds = bounds_of(&root, transform(root.attr("ItemTransform"))).expect("a size");
        // The outline spans -104..696 by -114..486, and the transform
        // moves it by (-260, 181.97).
        assert_eq!(bounds.width, 800.0, "{}", bounds.width);
        assert_eq!(bounds.height, 600.0, "{}", bounds.height);
        assert!((bounds.x - (-364.0)).abs() < 0.01, "{}", bounds.x);
        assert!((bounds.y - 67.97).abs() < 0.01, "{}", bounds.y);
    }

    #[test]
    fn a_frame_reads_its_columns_and_insets() {
        let root = xml::parse(FRAME).expect("parses");
        let insets = insets_of(&root);
        assert_eq!(insets.top, 4.0);
        assert_eq!(insets.left, 4.0);
        let columns = root
            .find("TextFramePreference")
            .and_then(|p| p.number("TextColumnCount"));
        assert_eq!(columns, Some(1.0));
    }

    #[test]
    fn a_soft_line_break_stays_inside_one_paragraph() {
        let story = xml::parse(
            r#"<Story Self="u1">
                <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Body">
                    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Emphasis" FontStyle="Bold">
                        <Content>one&#10;two</Content>
                    </CharacterStyleRange>
                </ParagraphStyleRange>
            </Story>"#,
        )
        .expect("parses");
        let story = flatten_story(&story);
        // A literal LF in Content is a soft break, unlike a Br element.
        assert_eq!(story.points.len(), 1, "{:?}", story.points);
        let schist_layout::StoryPoint::Paragraph { text, style } = &story.points[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(text, "one\ntwo");
        assert_eq!(style, "Body", "the style name, not its reference");
        // The run covers the whole paragraph.
        assert_eq!(story.ranges.len(), 1);
        assert_eq!(story.ranges[0].end, text.len());
        assert_eq!(story.ranges[0].style, "Emphasis");
    }

    #[test]
    fn a_run_with_no_character_style_is_not_a_style_range() {
        // Every text frame in a real export has one of these, so
        // recording it would fill the range list with `[No character
        // style]` entries rather than with actual styling.
        let story = xml::parse(
            r#"<Story Self="u1">
                <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/A">
                    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">
                        <Content>plain</Content>
                    </CharacterStyleRange>
                </ParagraphStyleRange>
            </Story>"#,
        )
        .expect("parses");
        let story = flatten_story(&story);
        assert!(story.ranges.is_empty(), "{:?}", story.ranges);
        // The text itself is still there.
        let StoryPoint::Paragraph { text, .. } = &story.points[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(text, "plain");
    }

    #[test]
    fn two_paragraphs_get_separate_ranges_over_the_story_text() {
        let story = xml::parse(
            r#"<Story Self="u1">
                <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/A">
                    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">
                        <Content>first</Content><Br/>
                    </CharacterStyleRange>
                </ParagraphStyleRange>
                <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/B">
                    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Em">
                        <Content>second</Content>
                    </CharacterStyleRange>
                </ParagraphStyleRange>
            </Story>"#,
        )
        .expect("parses");
        let story = flatten_story(&story);
        assert_eq!(story.points.len(), 2);
        // The first paragraph is unstyled, so it contributes no range; the
        // second's does, and its offsets index the *concatenated* text, so
        // it starts after the first paragraph and its separator.
        assert_eq!(story.ranges.len(), 1, "{:?}", story.ranges);
        assert_eq!(story.ranges[0].start, 6);
        assert_eq!(story.ranges[0].end, 12);
        assert_eq!(story.ranges[0].style, "Em");
    }

    #[test]
    fn a_file_uri_becomes_a_relinkable_path() {
        // A link that does not exist on this machine is a relinking
        // problem, not a parse error, so the path has to come back usable.
        assert_eq!(
            strip_file_uri("file:C:/Users/sioch/Desktop/idml%20samples/linked.png"),
            "C:/Users/sioch/Desktop/idml samples/linked.png"
        );
    }

    #[test]
    fn a_transform_reads_only_its_translation_and_says_so() {
        let value = transform(Some("1 0 0 1 -400 -300"));
        assert_eq!(
            value,
            Transform {
                tx: -400.0,
                ty: -300.0,
                ..Transform::default()
            }
        );
        // A malformed or missing transform is the identity rather than a
        // guess, so a frame lands at the origin instead of nowhere.
        assert_eq!(transform(None), Transform::default());
        assert_eq!(transform(Some("nonsense")), Transform::default());
    }
}
