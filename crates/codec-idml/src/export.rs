//! Writing a [`schist_layout::LayoutDocument`] as an IDML package.
//!
//! The mirror of [`crate::import`], and written against the same seven
//! real exports: where this and the reader disagree, one of them is
//! wrong, and `tests/round_trip.rs` is what catches it.
//!
//! ## The encoding, as written
//!
//! The parts and their order are fixed by the OPC rules — `mimetype`
//! first and stored, then `META-INF/container.xml`, then the root part,
//! then the object parts it lists.
//!
//! The root part lists parts by path, and an object's id is a `u` and a
//! hexadecimal number, with the *file name* carrying the type. This writes
//! exactly that, because a reader that can open its own output is not
//! evidence that it can open anyone else's.
//!
//! ## What is not written
//!
//! Styles' *definitions* are written, but only the properties this
//! document model carries. Tables, footnotes, anchored objects, text on a
//! path and ink trapping are not, and a document that
//! uses them loses them. What was dropped is reported in
//! [`Written::warnings`], because a writer that loses a document's
//! contents without saying so is the worst kind.

use schist_layout::{
    CharacterStyle, FrameOverflow, Insets, LayoutDocument, LayoutObject, Page, ParagraphStyle,
    Rect, Story, StoryId, StoryPoint,
};

use crate::container::{self, MIMETYPE, MIMETYPE_PART};

/// The namespaces a part declares.
const NS_PACKAGING: &str = "http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging";
const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
/// Compatibility processing instruction for the emitted IDML/DOM version.
/// This declaration does not establish native-application validation.
const AID: &str = r#"<?aid style="50" type="document" readerVersion="6.0" featureSet="257" product="15.0(209)" ?>"#;
const DOM_VERSION: &str = "15.0";

/// What a write produced, and what it could not carry.
pub struct Written {
    pub bytes: Vec<u8>,
    /// One note per thing the document uses that this writer drops.
    pub warnings: Vec<String>,
}

/// Write a document as an IDML package.
pub fn write(document: &LayoutDocument) -> Written {
    let mut ids = Ids::default();
    let mut out = Written {
        bytes: Vec::new(),
        warnings: Vec::new(),
    };
    if document.pages.iter().any(|page| page.hidden) {
        out.warnings
            .push(schist_i18n::t("design.idml_page_visibility").into());
    }

    for shift in document
        .styles
        .paragraphs
        .iter()
        .filter_map(|s| s.baseline_shift)
        .chain(
            document
                .styles
                .characters
                .iter()
                .filter_map(|s| s.baseline_shift),
        )
    {
        if matches!(shift, schist_layout::styles::BaselineShift::Offset(value) if !value.is_finite())
        {
            out.warnings.push(schist_i18n::tf!(
                "design.idml_position_unsupported",
                value = format!("{shift:?}")
            ));
        }
    }

    // Part names and the ids in them, gathered before anything is
    // written, because the root part has to list them all.
    let story_parts: Vec<(StoryId, String, String)> = document
        .stories
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let id = ids.next();
            let path = format!("Stories/Story_{id}.xml");
            (StoryId(index as u32), id, path)
        })
        .collect();
    let spread_parts: Vec<String> = document
        .spreads
        .iter()
        .map(|_| {
            let id = ids.next();
            format!("Spreads/Spread_{id}.xml")
        })
        .collect();
    let master_parts: Vec<String> = document
        .parents
        .iter()
        .map(|_| {
            let id = ids.next();
            format!("MasterSpreads/MasterSpread_{id}.xml")
        })
        .collect();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    parts.push((MIMETYPE_PART.into(), MIMETYPE.as_bytes().to_vec()));
    parts.push((
        "META-INF/container.xml".into(),
        container_xml().into_bytes(),
    ));
    parts.push(("META-INF/metadata.xml".into(), metadata_xml().into_bytes()));

    for (path, element) in spread_parts.iter().zip(document.spreads.iter()) {
        parts.push((
            path.clone(),
            spread_xml(
                element,
                path,
                document,
                &story_parts,
                &master_parts,
                &mut out.warnings,
            )
            .into_bytes(),
        ));
    }
    for (path, parent) in master_parts.iter().zip(document.parents.iter()) {
        parts.push((
            path.clone(),
            master_xml(
                parent,
                path,
                document,
                &master_parts,
                &story_parts,
                &mut out.warnings,
            )
            .into_bytes(),
        ));
    }
    for ((_, id, path), story) in story_parts.iter().zip(document.stories.iter()) {
        parts.push((
            path.clone(),
            story_xml(id, story, &document.styles, &mut out.warnings).into_bytes(),
        ));
    }

    // These keep the names a real export gives them -- `Graphic.xml`,
    // not `Graphic_u123.xml`. The id is a property of the objects inside
    // them, not of the part, and a writer that invents its own part names
    // produces a file only this writer can read.
    parts.push((
        "Resources/Graphic.xml".into(),
        graphic_xml(document).into_bytes(),
    ));
    parts.push((
        "Resources/Fonts.xml".into(),
        fonts_xml(document, &mut out.warnings).into_bytes(),
    ));
    parts.push((
        "Resources/Styles.xml".into(),
        styles_xml(document, &mut out.warnings).into_bytes(),
    ));
    parts.push((
        "Resources/Preferences.xml".into(),
        preferences_xml(document, &mut out.warnings).into_bytes(),
    ));
    parts.push(("XML/Tags.xml".into(), tags_xml().into_bytes()));

    let mut root = String::new();
    root.push_str(DECLARATION);
    root.push_str(AID);
    root.push_str(&format!(
        r#"<Document xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}" Self="d" Name="Schist" ZeroPoint="0 0">"#
    ));
    for layer in &document.layers {
        let properties = document.layer_properties.iter().find(|p| p.id == *layer);
        root.push_str(&format!(
            r#"<Layer Self="SchistLayer{}" Name="{}" Visible="{}" Locked="{}" Printable="true" />"#,
            layer.0,
            escape(properties.map(|p| p.name.as_str()).unwrap_or_default()),
            document.layer_visible(*layer),
            document.layer_locked(*layer)
        ));
    }
    root.push_str(r#"<idPkg:Graphic src="Resources/Graphic.xml" />"#);
    root.push_str(r#"<idPkg:Fonts src="Resources/Fonts.xml" />"#);
    root.push_str(r#"<idPkg:Styles src="Resources/Styles.xml" />"#);
    root.push_str(r#"<idPkg:Preferences src="Resources/Preferences.xml" />"#);
    root.push_str(r#"<idPkg:Tags src="XML/Tags.xml" />"#);
    for path in &master_parts {
        root.push_str(&format!(r#"<idPkg:MasterSpread src="{}" />"#, escape(path)));
    }
    for path in &spread_parts {
        root.push_str(&format!(r#"<idPkg:Spread src="{}" />"#, escape(path)));
    }
    for (_, _, path) in &story_parts {
        root.push_str(&format!(r#"<idPkg:Story src="{}" />"#, escape(path)));
    }
    root.push_str(&crate::preferences_codec::section(
        document,
        &mut out.warnings,
    ));
    root.push_str("</Document>");
    parts.push(("designmap.xml".into(), root.into_bytes()));

    out.bytes = container::write(&parts);
    out
}

/// Hand out object ids in the shape InDesign writes them: a `u` and a
/// hexadecimal number, with the type carried by the file name instead.
#[derive(Default)]
struct Ids {
    next: u32,
}

impl Ids {
    fn next(&mut self) -> String {
        // Starting above 0x100 keeps the ids from looking like the small
        // numbers a hand-written fixture would use, which makes an
        // accidental collision with a real file's namespace less likely.
        self.next += 1;
        format!("u{:x}", 0x100 + self.next)
    }
}

// -- the parts --------------------------------------------------------

fn container_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
  <rootfiles>
    <rootfile full-path="designmap.xml" media-type="{MIMETYPE}" />
  </rootfiles>
</container>"#
    )
}

fn metadata_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<xmpMetadata xmlns:xmp="http://ns.adobe.com/xap/1.0/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/">
      <xmpMM:DocumentID>schist</xmpMM:DocumentID>
    </rdf:Description>
  </rdf:RDF>
</xmpMetadata>"#
        .to_owned()
}

fn tags_xml() -> String {
    format!(
        r#"<idPkg:Tags xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">
  <XMLTag Self="XMLTag/Root" Name="Root" />
</idPkg:Tags>"#
    )
}

/// A spread, its pages, and the items on them.
fn spread_xml(
    spread: &schist_layout::Spread,
    path: &str,
    document: &LayoutDocument,
    stories: &[(StoryId, String, String)],
    masters: &[String],
    warnings: &mut Vec<String>,
) -> String {
    // The writer uses conventional names; readers resolve the XML Self id.
    let id = part_id(path);
    let show_master = spread.pages.iter().any(|index| {
        document
            .pages
            .get(*index)
            .and_then(|p| p.master)
            .and_then(|m| document.parents.get(m))
            .is_none_or(|parent| parent.placement(document, *index).visible)
    });
    let mut out = String::new();
    out.push_str(DECLARATION);
    out.push_str(&format!(
        r#"<idPkg:Spread xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">"#
    ));
    out.push_str(&format!(
        r#"<Spread Self="{id}" ItemTransform="1 0 0 1 0 0" PageCount="{}" BindingLocation="{}" ShowMasterItems="{show_master}" AllowPageShuffle="false">"#,
        spread.pages.len(), document.spread_binding(spread)
    ));

    // Spread coordinates are anchored at the spine, independently of the
    // parent artwork assigned to any page. Reading order is separate from
    // physical placement; keep fixed spread slots when a native app paginates.
    let left = document.spread_binding(spread);
    let start_x = -spread
        .pages
        .iter()
        .take(left)
        .filter_map(|i| document.pages.get(*i))
        .map(|p| p.width)
        .sum::<f32>()
        - spread.gutter * left.saturating_sub(usize::from(left == spread.pages.len())) as f32;
    let mut page_x = start_x;
    let mut positions = Vec::new();
    for index in &spread.pages {
        positions.push((*index, page_x));
        if let Some(page) = document.pages.get(*index) {
            page_x += page.width + spread.gutter;
        }
    }
    if document.page_binding == schist_layout::PageBinding::RightToLeft {
        positions.reverse();
    }
    for (index, page_x) in &positions {
        let Some(page) = document.pages.get(*index) else {
            continue;
        };
        // A page's master is named by the master's own id, which is the
        // stem of its part's file name.
        let master = page
            .master
            .and_then(|master| masters.get(master))
            .map(|path| part_id(path))
            .unwrap_or_else(|| "n".to_string());
        let overlay = page
            .master
            .and_then(|m| document.parents.get(m))
            .map(|parent| {
                let placement = parent.placement(document, *index);
                let origin = master_sheets(parent, document)
                    .get(placement.sheet)
                    .map(|s| s.origin)
                    .unwrap_or_default();
                schist_layout::affine::Affine::translate(*page_x, 0.0)
                    .then(&placement.transform)
                    .then(&schist_layout::affine::Affine::translate(
                        -origin.x, -origin.y,
                    ))
            })
            .unwrap_or_default();
        let overrides = document
            .parents
            .iter()
            .flat_map(|parent| {
                let hide =
                    parent.hidden || (show_master && !parent.placement(document, *index).visible);
                parent
                    .objects
                    .iter()
                    .filter(move |entry| entry.overridden_on.contains(index) || hide)
                    .map(|entry| object_id(entry.object.id))
            })
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&page_xml(
            page,
            &master,
            &format!("SchistPage{index}"),
            schist_layout::Point::new(*page_x, 0.0),
            overlay,
            &overrides,
        ));
    }

    // Items are spread siblings. Grouping them by owning page changes stacking
    // whenever artwork crosses the gutter, even though its geometry survives.
    for object in &document.objects {
        if let Some((_, page_x)) = positions.iter().find(|(index, _)| *index == object.page) {
            let mut positioned = object.clone();
            positioned.bounds.x += *page_x;
            out.push_str(&object_xml(
                &positioned,
                document,
                document.object_layer(object.id),
                stories,
                warnings,
            ));
        }
    }
    out.push_str("</Spread></idPkg:Spread>");
    out
}

/// A page, as a `<Page>`.
///
/// `GeometricBounds` is top, left, bottom, right, and the transform
/// carries the page's place on the pasteboard.
fn page_xml(
    page: &Page,
    master: &str,
    id: &str,
    origin: schist_layout::Point,
    overlay: schist_layout::affine::Affine,
    overrides: &str,
) -> String {
    let mut out = format!(
        r#"<Page Self="{}" Name="{}" GeometricBounds="0 0 {} {}" ItemTransform="1 0 0 1 {} {}" AppliedMaster="{}" MasterPageTransform="{}" OverrideList="{}" LayoutRule="UseMaster">"#,
        escape(id),
        escape(&page.name),
        number(page.height),
        number(page.width),
        number(origin.x),
        number(origin.y),
        escape(master),
        [overlay.a, overlay.b, overlay.c, overlay.d, overlay.tx, overlay.ty]
            .map(number)
            .join(" "),
        escape(overrides),
    );
    if page.hidden {
        // The native Page schema has no equivalent to Schist's output-visibility
        // toggle. Keep our intent in the public extension mechanism and disclose
        // that other consumers can still show/export the page.
        out.push_str(r#"<Properties><Label><KeyValuePair Key="Schist.PageVisibility.v1" Value="Hidden"/></Label></Properties>"#);
    }
    out.push_str(&format!(
        r#"<MarginPreference ColumnCount="1" ColumnGutter="0" Top="{}" Bottom="{}" Left="{}" Right="{}" />"#,
        number(page.margins.top),
        number(page.margins.bottom),
        number(page.margins.left),
        number(page.margins.right)
    ));
    for (index, guide) in page.guides.iter().enumerate() {
        if guide.position.is_finite() {
            out.push_str(&format!(r#"<Guide Self="{}_guide{}" Orientation="{}" Location="{}" FitToPage="true" Locked="{}" />"#,
                escape(id),index,if guide.horizontal {"Horizontal"} else {"Vertical"},number(guide.position),guide.locked));
        }
    }
    out.push_str("</Page>");
    out
}

/// A frame, as one of the elements that can hold one.
fn object_xml(
    object: &schist_layout::PlacedObject,
    document: &LayoutDocument,
    layer: schist_layout::LayerId,
    stories: &[(StoryId, String, String)],
    warnings: &mut Vec<String>,
) -> String {
    let id = object_id(object.id);
    let rect = object.bounds;
    let transform = format!(
        r#"ItemLayer="SchistLayer{}" ItemTransform="{}""#,
        layer.0,
        item_transform(object)
    );
    let geometry = rect_geometry(&rect);
    let locked = object.locked;
    let name = escape(&object.name);

    match &object.object {
        LayoutObject::TextFrame {
            story,
            columns,
            gutter,
            insets,
            overflow,
        } => {
            // The story is named by the id its part's file name carries.
            let story_id = stories
                .get(story.0 as usize)
                .map(|(_, id, _)| id.clone())
                .unwrap_or_else(|| "n".to_string());
            let overflow_note = match overflow {
                FrameOverflow::Clip => "ContentType=\"TextType\"",
                FrameOverflow::Thread => "ContentType=\"TextType\"",
            };
            let frames = document.frame_thread(object);
            let position = frames.iter().position(|frame| frame.id == object.id);
            let frame_id =
                |frame: &schist_layout::PlacedObject| format!("u{:x}", 0x8000 + frame.id.0);
            let previous = position
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| frames.get(i))
                .map(|f| frame_id(f))
                .unwrap_or_else(|| "n".into());
            let next = position
                .and_then(|i| frames.get(i + 1))
                .filter(|_| *overflow == FrameOverflow::Thread)
                .map(|f| frame_id(f))
                .unwrap_or_else(|| "n".into());
            let mut out = format!(
                r#"<TextFrame Self="{id}" ParentStory="{story_id}" PreviousTextFrame="{previous}" NextTextFrame="{next}" {overflow_note} Name="{name}" {transform} Locked="{locked}" ApplyToMasterPageOnly="false">"#
            );
            out.push_str(&format!("<Properties>{geometry}</Properties>"));
            out.push_str(&format!(
                r#"<TextFramePreference TextColumnCount="{columns}" TextColumnGutter="{}" TextColumnMaxWidth="0">"#,
                number(*gutter)
            ));
            out.push_str(&format!(
                "<Properties><InsetSpacing type=\"list\">{}</InsetSpacing></Properties>",
                inset_list(insets)
            ));
            out.push_str("</TextFramePreference>");
            out.push_str(&crate::color_codec::transparency(object.transparency));
            out.push_str("</TextFrame>");
            out
        }
        LayoutObject::Shape {
            path,
            fill,
            stroke,
            stroke_width,
            fill_overprint,
            stroke_overprint,
            tints,
        } => {
            let geometry = path_geometry(path);
            if path.even_odd {
                warnings.push(schist_i18n::tf!("design.idml_even_odd", name = object.name));
            }
            let fill_tint =
                crate::color_codec::paint_tint(fill.as_ref(), Some(tints.fill)).unwrap();
            let stroke_tint =
                crate::color_codec::paint_tint(stroke.as_ref(), Some(tints.stroke)).unwrap();
            let fill = fill
                .as_ref()
                .map(crate::color_codec::reference)
                .unwrap_or_else(|| "Swatch/None".into());
            let stroke = stroke
                .as_ref()
                .map(crate::color_codec::reference)
                .unwrap_or_else(|| "Swatch/None".into());
            let opacity = crate::color_codec::transparency(object.transparency);
            let fill_overprint = object.overprint || *fill_overprint;
            let stroke_overprint = object.overprint || *stroke_overprint;
            format!(
                r#"<Polygon Self="{id}" Name="{name}" {transform} Locked="{locked}" StrokeWeight="{}" FillColor="{}" StrokeColor="{}" FillTint="{}" StrokeTint="{}" OverprintFill="{fill_overprint}" OverprintStroke="{stroke_overprint}" ContentType="Unassigned"><Properties>{geometry}</Properties>{opacity}</Polygon>"#,
                number(*stroke_width),
                escape(&fill),
                escape(&stroke),
                number(fill_tint),
                number(stroke_tint)
            )
        }
        LayoutObject::GraphicFrame { clip_path, .. } => {
            if clip_path.as_ref().is_some_and(|path| path.even_odd) {
                warnings.push(schist_i18n::tf!("design.idml_even_odd", name = object.name));
            }
            crate::graphic_codec::write(document, object, layer, &id).unwrap_or_else(|| {
                warnings.push(schist_i18n::tf!(
                    "design.idml_graphic_unwritten",
                    name = object.name
                ));
                String::new()
            })
        }
        LayoutObject::Note { text, author } => format!(
            r#"<Note Self="{id}" Name="{}" Author="{}" {transform} Locked="false" />"#,
            escape(text),
            escape(author)
        ),
        LayoutObject::Group { children } => {
            warnings.push(schist_i18n::tf!(
                "design.idml_group_unwritten",
                name = object.name,
                count = children.len()
            ));
            String::new()
        }
    }
}

/// IDML maps frame-local geometry directly into spread coordinates.
pub(crate) fn item_transform(object: &schist_layout::PlacedObject) -> String {
    let m = object
        .content_transform()
        .then(&schist_layout::affine::Affine::translate(
            object.bounds.x,
            object.bounds.y,
        ));
    [m.a, m.b, m.c, m.d, m.tx, m.ty].map(number).join(" ")
}

/// Native cubic contours: each outgoing handle controls the following
/// segment and the next anchor's incoming handle controls its arrival.
pub(crate) fn path_geometry(path: &schist_layout::ShapePath) -> String {
    let mut xml = String::from("<PathGeometry>");
    for sub in &path.subpaths {
        xml.push_str(&format!(
            r#"<GeometryPathType PathOpen="{}"><PathPointArray>"#,
            !sub.closed
        ));
        for (i, anchor) in sub.points.iter().enumerate() {
            let handles = sub.handles_at(i);
            let left = handles.incoming.unwrap_or(*anchor);
            let right = handles.outgoing.unwrap_or(*anchor);
            xml.push_str(&format!(
                r#"<PathPointType Anchor="{} {}" LeftDirection="{} {}" RightDirection="{} {}"/>"#,
                number(anchor.x),
                number(anchor.y),
                number(left.x),
                number(left.y),
                number(right.x),
                number(right.y)
            ));
        }
        xml.push_str("</PathPointArray></GeometryPathType>");
    }
    xml.push_str("</PathGeometry>");
    xml
}

/// A rectangle's outline, as the path geometry a frame carries.
///
/// The points are relative to the frame's own origin, and the transform
/// places it — which is what the reader's rule needs to get the same box
/// back.
pub(crate) fn rect_geometry(rect: &Rect) -> String {
    let (left, top, right, bottom) = (0.0, 0.0, rect.width, rect.height);
    format!(
        r#"<PathGeometry><GeometryPathType PathOpen="false"><PathPointArray><PathPointType Anchor="{} {}" /><PathPointType Anchor="{} {}" /><PathPointType Anchor="{} {}" /><PathPointType Anchor="{} {}" /></PathPointArray></GeometryPathType></PathGeometry>"#,
        number(left),
        number(top),
        number(left),
        number(bottom),
        number(right),
        number(bottom),
        number(right),
        number(top)
    )
}

/// An `InsetSpacing` list, top, bottom, left, right.
fn inset_list(insets: &Insets) -> String {
    [insets.top, insets.left, insets.bottom, insets.right]
        .iter()
        .map(|value| format!(r#"<ListItem type="unit">{}</ListItem>"#, number(*value)))
        .collect()
}

fn object_id(id: schist_layout::ObjectId) -> String {
    format!("u{:x}", 0x8000 + id.0)
}

fn master_sheets<'a>(
    parent: &'a schist_layout::ParentPage,
    document: &LayoutDocument,
) -> std::borrow::Cow<'a, [schist_layout::parents::ParentSheet]> {
    if !parent.sheets.is_empty() {
        return std::borrow::Cow::Borrowed(&parent.sheets);
    }
    let mut page = parent
        .applied_to
        .first()
        .and_then(|i| document.pages.get(*i))
        .or_else(|| document.pages.first())
        .cloned()
        .unwrap_or_else(Page::a4);
    page.master = None;
    std::borrow::Cow::Owned(vec![schist_layout::parents::ParentSheet {
        source: None,
        page,
        origin: schist_layout::Point::ZERO,
    }])
}

/// Native master pages retain their geometry. Items are siblings of pages,
/// positioned from their page-local composition boxes into master-spread space.
fn master_xml(
    parent: &schist_layout::ParentPage,
    path: &str,
    document: &LayoutDocument,
    masters: &[String],
    stories: &[(StoryId, String, String)],
    warnings: &mut Vec<String>,
) -> String {
    let id = part_id(path);
    let sheets = master_sheets(parent, document);
    let show_master =
        (0..sheets.len()).any(|index| parent.source(document, index).is_none_or(|s| s.visible));
    let mut out = format!(
        r#"{DECLARATION}<idPkg:MasterSpread xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}"><MasterSpread Self="{id}" Name="{}" ItemTransform="1 0 0 1 0 0" PageCount="{}" ShowMasterItems="{show_master}">"#,
        escape(&parent.name),
        sheets.len()
    );
    let mut sheet_order: Vec<_> = sheets.iter().enumerate().collect();
    sheet_order.sort_by(|(_, a), (_, b)| a.origin.x.total_cmp(&b.origin.x));
    if document.page_binding == schist_layout::PageBinding::RightToLeft {
        sheet_order.reverse();
    }
    for (index, sheet) in sheet_order {
        let source = parent.source(document, index);
        let master = source
            .as_ref()
            .and_then(|s| masters.get(s.parent))
            .map(|p| part_id(p))
            .unwrap_or_else(|| "n".into());
        let overlay = source
            .as_ref()
            .map(|s| {
                let origin = document
                    .parents
                    .get(s.parent)
                    .and_then(|p| master_sheets(p, document).get(s.sheet).map(|p| p.origin))
                    .unwrap_or_default();
                schist_layout::affine::Affine::translate(sheet.origin.x, sheet.origin.y)
                    .then(&s.transform)
                    .then(&schist_layout::affine::Affine::translate(
                        -origin.x, -origin.y,
                    ))
            })
            .unwrap_or_default();
        let mut overrides = source
            .as_ref()
            .map(|s| s.overrides.clone())
            .unwrap_or_default();
        if show_master && source.as_ref().is_some_and(|s| !s.visible) {
            overrides.extend(
                document
                    .parents
                    .iter()
                    .flat_map(|p| p.objects.iter().map(|o| o.object.id)),
            );
        }
        let overrides = overrides
            .into_iter()
            .map(object_id)
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&page_xml(
            &sheet.page,
            &master,
            &format!("{id}page{index}"),
            sheet.origin,
            overlay,
            &overrides,
        ));
    }
    for entry in &parent.objects {
        let mut object = entry.object.clone();
        let origin = sheets
            .get(object.page)
            .or_else(|| sheets.first())
            .map(|s| s.origin)
            .unwrap_or_default();
        object.bounds.x += origin.x;
        object.bounds.y += origin.y;
        out.push_str(&object_xml(
            &object,
            document,
            document.object_layer(object.id),
            stories,
            warnings,
        ));
    }
    out.push_str("</MasterSpread></idPkg:MasterSpread>");
    out
}

/// A story, as its points and ranges.
fn story_xml(
    id: &str,
    story: &Story,
    styles: &schist_layout::StyleSet,
    warnings: &mut Vec<String>,
) -> String {
    let mut out = String::new();
    out.push_str(DECLARATION);
    out.push_str(&format!(
        r#"<idPkg:Story xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">"#
    ));
    out.push_str(&format!(
        r#"<Story Self="{id}" UserText="true" IsEndnoteStory="false" TrackChanges="false" StoryTitle="$ID/">"#
    ));
    let automatic = crate::auto_direction::lower(story, styles);
    out.push_str(&crate::auto_direction::properties(&automatic));
    let orientation = match story.prefs.orientation {
        schist_layout::StoryOrientation::Horizontal => "Horizontal",
        schist_layout::StoryOrientation::Vertical => "Vertical",
    };
    let direction = match story.prefs.direction {
        schist_layout::StoryDirection::LeftToRight => "LeftToRightDirection",
        schist_layout::StoryDirection::RightToLeft => "RightToLeftDirection",
    };
    out.push_str(&format!(
        r#"<StoryPreference OpticalMarginAlignment="false" OpticalMarginSize="0" FrameType="TextFrameType" StoryOrientation="{orientation}" StoryDirection="{direction}" />"#,
    ));

    let offsets = story.point_offsets();
    let mut range_index = 0;
    for (index, point) in story.points.iter().enumerate() {
        let offset = offsets[index];
        match point {
            StoryPoint::Paragraph { text, style } => {
                out.push_str(&format!(
                    r#"<ParagraphStyleRange AppliedParagraphStyle="{}""#,
                    paragraph_reference(style)
                ));
                if let Some(paragraph) = automatic.iter().find(|p| p.index == range_index) {
                    out.push_str(&format!(r#" ParagraphDirection="{}""#, paragraph.direction));
                }
                out.push('>');
                let runs = runs_for(story, offset, text);
                let paragraph_end = matches!(
                    story.points.get(index + 1),
                    Some(StoryPoint::Paragraph { .. } | StoryPoint::LineBreak)
                );
                for (run_index, (start, end, style)) in runs.iter().enumerate() {
                    out.push_str(&format!(
                        r#"<CharacterStyleRange AppliedCharacterStyle="{}">"#,
                        character_reference(style)
                    ));
                    if start < end {
                        // LF inside Content is a soft break; Br outside it
                        // is a paragraph break. Never interchange the two.
                        out.push_str(&format!(
                            "<Content>{}</Content>",
                            escape(&text[*start..*end]).replace('\n', "&#10;")
                        ));
                    }
                    if paragraph_end && run_index + 1 == runs.len() {
                        out.push_str("<Br/>");
                    }
                    out.push_str("</CharacterStyleRange>");
                }
                out.push_str("</ParagraphStyleRange>");
            }
            StoryPoint::ColumnBreak | StoryPoint::PageBreak | StoryPoint::FrameBreak => {
                let kind = match point {
                    StoryPoint::ColumnBreak => "NextColumn",
                    StoryPoint::FrameBreak => "NextFrame",
                    _ => "NextPage",
                };
                out.push_str(&format!(r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" ParagraphBreakType="{kind}"><Br/></CharacterStyleRange></ParagraphStyleRange>"#));
            }
            StoryPoint::LineBreak => {
                warnings.push(schist_i18n::t("design.idml_structural_line_break").to_string());
                out.push_str(r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Br/></CharacterStyleRange></ParagraphStyleRange>"#);
            }
            StoryPoint::Other { .. } => {
                warnings.push(schist_i18n::t("design.idml_story_structure").to_string())
            }
        }
        if !matches!(point, StoryPoint::Other { .. }) {
            range_index += 1;
        }
    }
    out.push_str("</Story></idPkg:Story>");
    out
}

/// Partition a paragraph once, including unstyled bytes. Snap malformed
/// imported endpoints to UTF-8 boundaries and never duplicate text when
/// ranges overlap. The first range wins, matching the engine's priority.
fn runs_for(story: &Story, offset: usize, text: &str) -> Vec<(usize, usize, String)> {
    let mut covered = Vec::new();
    let mut boundaries = vec![0, text.len()];
    for range in &story.ranges {
        if range.end <= offset || range.start >= offset + text.len() {
            continue;
        }
        let mut start = range.start.saturating_sub(offset).min(text.len());
        let mut end = range.end.saturating_sub(offset).min(text.len());
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        while !text.is_char_boundary(end) {
            end += 1;
        }
        if start < end {
            covered.push((start, end, range.style.as_str()));
            boundaries.extend([start, end]);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut runs: Vec<(usize, usize, String)> = Vec::new();
    for pair in boundaries.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let style = covered
            .iter()
            .find(|(from, to, _)| *from <= start && *to >= end)
            .map(|(_, _, s)| *s)
            .unwrap_or_default();
        if let Some(last) = runs.last_mut().filter(|r| r.2 == style) {
            last.1 = end;
        } else {
            runs.push((start, end, style.to_owned()));
        }
    }
    if runs.is_empty() {
        runs.push((0, 0, String::new()));
    }
    runs
}

/// Inks and the colours that name them.
fn graphic_xml(document: &LayoutDocument) -> String {
    let mut out = String::new();
    out.push_str(DECLARATION);
    out.push_str(&format!(
        r#"<idPkg:Graphic xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">"#
    ));
    out.push_str(r#"<Swatch Self="Swatch/None" Name="None"/>"#);
    let mut inks = document.all_inks();
    for ink in inks.clone() {
        let base = ink.base_color().into_owned();
        if !inks.contains(&base) {
            inks.push(base);
        }
    }
    for ink in inks {
        out.push_str(&crate::color_codec::resource(&ink));
    }
    out.push_str("</idPkg:Graphic>");
    out
}

/// The document's paragraph and character styles.
fn styles_xml(document: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    let mut out = String::new();
    out.push_str(DECLARATION);
    out.push_str(&format!(
        r#"<idPkg:Styles xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">"#
    ));
    out.push_str(r#"<RootParagraphStyleGroup Self="SchistParagraphStyles">"#);
    for style in &document.styles.paragraphs {
        crate::opentype_codec::warn(&style.features, warnings);
        crate::style_codec::warn_leading(style.leading, style.auto_leading, warnings);
        let mut native = style.clone();
        let resolved = document.styles.resolve_paragraph(&style.name);
        if resolved.drop_caps_lines.is_some_and(|lines| lines > 1)
            && resolved.drop_caps_characters.is_none()
        {
            // Schist's legacy unset count means one, while a native consumer
            // needs an explicit character count to enable the initial.
            native.drop_caps_characters = Some(1);
        }
        if native.bold.is_some() || native.italic.is_some() {
            native.bold = Some(resolved.bold.unwrap_or(false));
            native.italic = Some(resolved.italic.unwrap_or(false));
        }
        out.push_str(&paragraph_style_xml(&native));
    }
    out.push_str(
        r#"</RootParagraphStyleGroup><RootCharacterStyleGroup Self="SchistCharacterStyles">"#,
    );
    for style in &document.styles.characters {
        crate::opentype_codec::warn(&style.features, warnings);
        crate::style_codec::warn_leading(style.leading, None, warnings);
        let mut native = style.clone();
        if native.bold.is_some() || native.italic.is_some() {
            let resolved = document.styles.resolve_character(&style.name);
            native.bold = Some(resolved.bold.unwrap_or(false));
            native.italic = Some(resolved.italic.unwrap_or(false));
        }
        out.push_str(&character_style_xml(&native));
    }
    if !document.styles.paragraphs.is_empty() || !document.styles.characters.is_empty() {
        warnings.push(schist_i18n::t("design.idml_style_limits").to_string());
    }
    out.push_str("</RootCharacterStyleGroup></idPkg:Styles>");
    out
}

fn paragraph_style_xml(style: &ParagraphStyle) -> String {
    crate::style_codec::paragraph(style)
}

fn character_style_xml(style: &CharacterStyle) -> String {
    crate::style_codec::character(style)
}

/// The fonts a document's styles name.
///
/// A font that is not installed is a warning rather than a failure: a
/// document can name a font the machine opening it does not have, which
/// is the normal case for a document travelling between machines.
fn fonts_xml(document: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    let mut families: Vec<&str> = document
        .styles
        .characters
        .iter()
        .filter_map(|style| style.family.as_deref())
        .chain(
            document
                .styles
                .paragraphs
                .iter()
                .filter_map(|style| style.family.as_deref()),
        )
        .collect();
    families.sort_unstable();
    families.dedup();
    if !families.is_empty() {
        warnings.push(schist_i18n::tf!(
            "design.idml_fonts_unembedded",
            count = families.len()
        ));
    }
    let mut out = String::new();
    out.push_str(DECLARATION);
    out.push_str(&format!(
        r#"<idPkg:Fonts xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">"#
    ));
    for (index, family) in families.iter().enumerate() {
        out.push_str(&format!(
            r#"<FontFamily Self="u{:x}" Name="{}" />"#,
            0x900 + index,
            escape(family)
        ));
    }
    out.push_str("</idPkg:Fonts>");
    out
}

/// The document's settings a package records outside the pages.
fn preferences_xml(document: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    format!(
        r#"{DECLARATION}<idPkg:Preferences xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">{}</idPkg:Preferences>"#,
        crate::preferences_codec::preferences(document, warnings)
    )
}

// -- values -----------------------------------------------------------

/// The id a part's file name carries, which is what a frame names.
fn part_id(path: &str) -> String {
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".xml");
    // Strip the type prefix the file name carries: `Spread_u123` is
    // `u123`.
    stem.split_once('_')
        .map(|(_, id)| id.to_owned())
        .unwrap_or_else(|| stem.to_owned())
}

fn paragraph_reference(name: &str) -> String {
    format!("ParagraphStyle/$ID/{}", escape(name))
}

fn character_reference(name: &str) -> String {
    format!("CharacterStyle/$ID/{}", escape(name))
}

/// A number, formatted so it reads back as the same value.
///
/// A float printed at full precision round-trips exactly, which matters
/// for a coordinate: `0.1` written as `0.1` and read back as `0.1`, but
/// written as `0.100000001` and read back as a different number would
/// make a round trip that never settles.
pub(crate) fn number(value: f32) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    if value == value.trunc() && value.abs() < 1e9 {
        return format!("{}", value as i64);
    }
    // Rust's shortest round-trippable representation retains small
    // Bézier handles as well as large page coordinates.
    value.to_string()
}

/// Escape text for an attribute or a text node.
///
/// The five entities XML defines, and nothing else: an unknown entity
/// would not be valid, and a document that contained one would be
/// rejected by a conforming reader rather than by us.
pub(crate) fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // A control character has no XML representation at all, and
            // writing one raw produces a file no reader can open.
            character if (character as u32) < 0x20 && character != '\n' && character != '\t' => {
                out.push(' ')
            }
            character => out.push(character),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_written_comes_back_the_same() {
        for value in [0.0, -400.0, 841.8898, 0.1, -0.25, 1234.5, 595.27563] {
            let text = number(value);
            let back: f32 = text.parse().expect("parses");
            assert!(
                (back - value).abs() < 0.001,
                "{value} written as {text} read back as {back}"
            );
        }
    }

    #[test]
    fn a_non_finite_number_is_written_as_zero() {
        // `NaN` in a coordinate produces a file no reader can place, and
        // zero is at least a position.
        assert_eq!(number(f32::NAN), "0");
        assert_eq!(number(f32::INFINITY), "0");
    }

    #[test]
    fn text_is_escaped_and_reads_back_unchanged() {
        // A frame named "Line & count <RT_f>" is a real one in the
        // fixtures, so this is not a hypothetical.
        for value in [
            "Line & count",
            "<RT_f>",
            "quoted \" and ' apostrophe",
            "a > b",
            "Grüße, 日本語",
        ] {
            let written = escape(value);
            // The round trip is the assertion: a `<` written raw would
            // make the file unreadable, and a dropped `&` would make the
            // name come back different.
            let root = crate::xml::parse(&format!("<a v=\"{written}\">{written}</a>"))
                .expect("the escaped text parses");
            assert_eq!(root.attr("v"), Some(value), "attribute");
            assert_eq!(root.text, value, "text");
        }
    }

    #[test]
    fn a_control_character_becomes_a_space_rather_than_a_broken_file() {
        let written = escape("a\u{1}b");
        assert_eq!(written, "a b");
        assert!(crate::xml::parse(&format!("<a>{written}</a>")).is_ok());
    }

    #[test]
    fn a_parts_id_is_its_file_names_stem() {
        // The reader's rule, applied in reverse: `Spreads/Spread_u123.xml`
        // is the object `u123`.
        assert_eq!(part_id("Spreads/Spread_u123.xml"), "u123");
        assert_eq!(part_id("Stories/Story_uabc.xml"), "uabc");
        assert_eq!(
            part_id("MasterSpreads/MasterSpread_u9.xml"),
            "u9",
            "the longest prefix is not a type here"
        );
    }

    #[test]
    fn native_alignment_values_and_absence_preserve_style_inheritance() {
        use schist_layout::styles::Align;
        for (align, native) in [
            (Align::Left, "LeftAlign"),
            (Align::Center, "CenterAlign"),
            (Align::Right, "RightAlign"),
            (Align::Justify, "LeftJustified"),
            (Align::JustifyAll, "FullyJustified"),
        ] {
            let style = ParagraphStyle {
                name: "Body".into(),
                align: Some(align),
                ..Default::default()
            };
            let xml = crate::xml::parse(&crate::style_codec::paragraph(&style)).unwrap();
            assert_eq!(xml.attr("Justification"), Some(native));
        }
        let style = ParagraphStyle {
            name: "Inherited".into(),
            based_on: Some("Body".into()),
            ..Default::default()
        };
        let xml = crate::xml::parse(&crate::style_codec::paragraph(&style)).unwrap();
        assert_eq!(xml.attr("Justification"), None);
    }

    #[test]
    fn runs_partition_unicode_text_once_even_with_overlapping_or_mid_character_endpoints() {
        let text = "é中😀abcdef";
        for start in 0..text.len() {
            for end in start + 1..=text.len() {
                let mut story = Story::from_text(text, "Body");
                story.ranges = vec![
                    schist_layout::StyleRange::new(start, end, "A"),
                    schist_layout::StyleRange::new(0, text.len(), "B"),
                ];
                let runs = runs_for(&story, 0, text);
                let recovered: String = runs.iter().map(|(a, b, _)| &text[*a..*b]).collect();
                assert_eq!(recovered, text);
                for adjacent in runs.windows(2) {
                    assert_eq!(adjacent[0].1, adjacent[1].0);
                }
            }
        }
    }
}
