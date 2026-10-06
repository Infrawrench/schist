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
//! The root part lists parts by path. Native object IDs often use a `u` and
//! hexadecimal number; the specification requires package-wide uniqueness,
//! not that spelling. Parts and page items use separate generated domains.
//!
//! ## What is not written
//!
//! Styles' definitions include only the properties this document model carries.
//! Unsupported tables, footnotes and anchored objects survive in guarded standard
//! Story Labels. Supported text-only footnotes and text paths are written
//! natively. Unsupported output and Schist-only retention are disclosed
//! in [`Written::warnings`]; retaining data does not establish native rendering.

use schist_layout::{
    FrameOverflow, Insets, LayoutDocument, LayoutObject, Page, Rect, Story, StoryId, StoryPoint,
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
    /// Notices for unsupported native output and Schist-only retention.
    pub warnings: Vec<String>,
}

/// Write a document as an IDML package.
pub fn write(document: &LayoutDocument) -> Written {
    write_inner(document, true)
}

fn write_inner(document: &LayoutDocument, check_identities: bool) -> Written {
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

    for stroke in &document.styles.strokes {
        if !stroke.valid() {
            out.warnings.push(schist_i18n::tf!(
                "design.idml_decoration_invalid",
                property = crate::stroke_style_codec::array_name(&stroke.pattern),
                value = &stroke.name
            ));
        }
    }

    out.warnings
        .extend(crate::list_codec::diagnostics(document));
    let variables =
        crate::custom_text_codec::Exported::new(&document.text_variables, &mut out.warnings);
    let languages =
        crate::language_codec::ExportLanguages::new(&document.styles, &mut out.warnings);

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
    parts.push((
        "META-INF/metadata.xml".into(),
        metadata_xml(&document.dates).into_bytes(),
    ));

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
            story_xml(
                id,
                story,
                &document.styles,
                &variables.bindings,
                &mut out.warnings,
            )
            .into_bytes(),
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
        styles_xml(document, &languages, &mut out.warnings).into_bytes(),
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
    root.push_str(&crate::text_variable_codec::retain(
        &document.retained_text_variables,
        &variables.label(),
        &mut out.warnings,
    ));
    root.push_str(&variables.resources());
    root.push_str(&crate::language_codec::resources(&languages.resources));
    root.push_str(&crate::footnote_codec::write_frame(
        &document.frame_footnote_defaults,
        &mut out.warnings,
    ));
    root.push_str(&crate::list_codec::resources(
        &document.styles.numbering_lists,
    ));
    for layer in &document.layers {
        let properties = document.layer_properties.iter().find(|p| p.id == *layer);
        root.push_str(&format!(
            r#"<Layer Self="SchistLayer{}" Name="{}" Visible="{}" Locked="{}" IgnoreWrap="{}" Printable="true" />"#,
            layer.0,
            escape(properties.map(|p| p.name.as_str()).unwrap_or_default()),
            document.layer_visible(*layer),
            document.layer_locked(*layer),
            document.layer_ignores_wrap(*layer)
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

    if check_identities {
        if let Some(remapped) = crate::resource_identity::prepare(document, &parts) {
            return write_inner(&remapped, false);
        }
    }
    out.bytes = container::write(&parts);
    out
}

/// Part identities occupy a separate domain from page-item identities.
/// Imported resource collisions are resolved after collecting all emitted IDs.
#[derive(Default)]
struct Ids {
    next: u64,
}

impl Ids {
    fn next(&mut self) -> String {
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

/// XMP with the document's creation and modification dates when known; date
/// text variables read them back.
fn metadata_xml(dates: &schist_layout::DocumentDates) -> String {
    let mut known = String::new();
    for (name, date) in [
        ("CreateDate", dates.created),
        ("ModifyDate", dates.modified),
    ] {
        if let Some(date) = date {
            known.push_str(&format!("\n      <xmp:{name}>{}</xmp:{name}>", date.iso()));
        }
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xmpMetadata xmlns:xmp="http://ns.adobe.com/xap/1.0/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/">
      <xmpMM:DocumentID>schist</xmpMM:DocumentID>{known}
    </rdf:Description>
  </rdf:RDF>
</xmpMetadata>"#
    )
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
    let creation = document.creation_ranks();
    for object in &document.objects {
        if let Some((_, page_x)) = positions.iter().find(|(index, _)| *index == object.page) {
            let mut positioned = object.clone();
            positioned.bounds.x += *page_x;
            let mut xml = object_native_xml(
                &positioned,
                document,
                document.object_layer(object.id),
                stories,
                warnings,
            );
            crate::text_wrap_codec::attach(&mut xml, object);
            crate::creation_codec::label(&mut xml, creation.get(&object.id).copied());
            out.push_str(&xml);
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
fn object_native_xml(
    object: &schist_layout::PlacedObject,
    document: &LayoutDocument,
    layer: schist_layout::LayerId,
    stories: &[(StoryId, String, String)],
    warnings: &mut Vec<String>,
) -> String {
    let id = object_id(object.id);
    let rect = object.bounds;
    let transform = format!(
        r#"ItemLayer="SchistLayer{}" ItemTransform="{}" Visible="{}""#,
        layer.0,
        item_transform(object),
        !object.hidden
    );
    let geometry = object
        .appearance
        .outline
        .as_ref()
        .map(|path| {
            let mut path = path.clone();
            path.map_points(|p| schist_layout::Point::new(p.x * rect.width, p.y * rect.height));
            path_geometry(&path)
        })
        .unwrap_or_else(|| rect_geometry(&rect));
    let paint = crate::object_style_codec::object_attributes(object);
    let locked = object.locked;
    let name = escape(&object.name);

    match &object.object {
        LayoutObject::TextFrame {
            story,
            footnotes,
            balance_columns,
            text_path,
            columns,
            gutter,
            insets,
            overflow,
        } => {
            // IDML frame geometry has no fill rule; a shaped frame's holes then
            // follow the nonzero rule.
            if object
                .appearance
                .outline
                .as_ref()
                .is_some_and(|path| path.even_odd)
            {
                warnings.push(schist_i18n::tf!("design.idml_even_odd", name = object.name));
            }
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
            let frame_id = crate::text_path_codec::frame_id;
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
            if let Some(path) = text_path {
                if path.path.even_odd {
                    warnings.push(schist_i18n::tf!("design.idml_even_odd", name = object.name));
                }
                let geometry = path_geometry(&path.path);
                let bounds_label = crate::text_path_codec::bounds_label(path, rect);
                let child =
                    crate::text_path_codec::write(path, &id, &story_id, &previous, &next, warnings);
                let opacity = crate::color_codec::transparency(object.transparency);
                return format!(
                    r#"<Polygon Self="{id}" Name="{name}" {transform} Locked="{locked}"{paint} ContentType="Unassigned"><Properties>{geometry}{bounds_label}</Properties>{child}{opacity}</Polygon>"#
                );
            }
            let mut out = format!(
                r#"<TextFrame Self="{id}" ParentStory="{story_id}" PreviousTextFrame="{previous}" NextTextFrame="{next}" {overflow_note} Name="{name}" {transform} Locked="{locked}"{paint} ApplyToMasterPageOnly="false">"#
            );
            out.push_str(&format!("<Properties>{geometry}</Properties>"));
            out.push_str(&format!(
                r#"<TextFramePreference TextColumnCount="{columns}" TextColumnGutter="{}" TextColumnMaxWidth="0"{}{}>"#,
                number(*gutter),
                crate::preferences_codec::balance_attribute(*balance_columns),
                if object.appearance.ignore_wrap {
                    " IgnoreWrap=\"true\""
                } else {
                    ""
                }
            ));
            out.push_str(&format!(
                "<Properties><InsetSpacing type=\"list\">{}</InsetSpacing></Properties>",
                inset_list(insets)
            ));
            out.push_str("</TextFramePreference>");
            out.push_str(&crate::footnote_codec::write_frame(footnotes, warnings));
            out.push_str(&crate::color_codec::transparency(object.transparency));
            out.push_str("</TextFrame>");
            out
        }
        LayoutObject::Shape { path, .. } => {
            let geometry = path_geometry(path);
            if path.even_odd {
                warnings.push(schist_i18n::tf!("design.idml_even_odd", name = object.name));
            }
            let opacity = crate::color_codec::transparency(object.transparency);
            format!(
                r#"<Polygon Self="{id}" Name="{name}" {transform} Locked="{locked}"{paint} ContentType="Unassigned"><Properties>{geometry}</Properties>{opacity}</Polygon>"#
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

pub(crate) fn object_id(id: schist_layout::ObjectId) -> String {
    format!("SchistObject{}", id.0)
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
        let mut xml = object_native_xml(
            &object,
            document,
            document.object_layer(object.id),
            stories,
            warnings,
        );
        crate::text_wrap_codec::attach(&mut xml, &object);
        out.push_str(&xml);
    }
    out.push_str("</MasterSpread></idPkg:MasterSpread>");
    out
}

/// A story, as its points and ranges.
fn story_xml(
    id: &str,
    story: &Story,
    styles: &schist_layout::StyleSet,
    variables: &std::collections::BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> String {
    let native = story_native_xml(id, story, styles, variables, warnings);
    crate::structured_story::retain(native, id, story, variables, warnings)
}

pub(crate) fn story_native_xml(
    id: &str,
    story: &Story,
    styles: &schist_layout::StyleSet,
    variables: &std::collections::BTreeMap<String, String>,
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

    out.push_str(&story_native_body(
        story,
        &[],
        styles,
        variables,
        id,
        warnings,
    ));
    out.push_str("</Story></idPkg:Story>");
    out
}

pub(crate) fn story_native_body(
    story: &Story,
    markers: &[schist_layout::footnotes::FootnoteMarker],
    styles: &schist_layout::StyleSet,
    variables: &std::collections::BTreeMap<String, String>,
    owner: &str,
    warnings: &mut Vec<String>,
) -> String {
    let mut out = String::new();
    let automatic = crate::auto_direction::lower(story, styles);
    let mut inline =
        crate::footnote_writer::events(story, markers, styles, variables, owner, warnings);
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
                crate::footnote_writer::paragraph_runs(
                    &mut out,
                    &runs,
                    text,
                    offset,
                    &mut inline,
                    paragraph_end,
                );
                out.push_str("</ParagraphStyleRange>");
            }
            StoryPoint::ColumnBreak
            | StoryPoint::PageBreak
            | StoryPoint::FrameBreak
            | StoryPoint::OddPageBreak
            | StoryPoint::EvenPageBreak => {
                let kind = match point {
                    StoryPoint::ColumnBreak => "NextColumn",
                    StoryPoint::FrameBreak => "NextFrame",
                    StoryPoint::OddPageBreak => "NextOddPage",
                    StoryPoint::EvenPageBreak => "NextEvenPage",
                    _ => "NextPage",
                };
                out.push_str(&format!(r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" ParagraphBreakType="{kind}"><Br/></CharacterStyleRange></ParagraphStyleRange>"#));
            }
            StoryPoint::LineBreak => {
                warnings.push(schist_i18n::t("design.idml_structural_line_break").to_string());
                out.push_str(r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Br/></CharacterStyleRange></ParagraphStyleRange>"#);
            }
            StoryPoint::Other { .. } => {} // Retained by the wrapper's guarded Label.
        }
        if !matches!(point, StoryPoint::Other { .. }) {
            range_index += 1;
        }
    }
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
    let mut strokes = document.all_decoration_strokes();
    strokes.retain(|s| !matches!(s.pattern, schist_text_engine::TextDecorationPattern::Solid));
    for stroke in
        std::iter::once(schist_layout::decorations::DecorationStroke::solid()).chain(strokes)
    {
        out.push_str(&crate::stroke_style_codec::resource(&stroke));
    }
    out.push_str("</idPkg:Graphic>");
    out
}

/// The document's paragraph and character styles.
fn styles_xml(
    document: &LayoutDocument,
    languages: &crate::language_codec::ExportLanguages,
    warnings: &mut Vec<String>,
) -> String {
    let mut out = String::new();
    out.push_str(DECLARATION);
    out.push_str(&format!(
        r#"<idPkg:Styles xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">"#
    ));
    out.push_str(r#"<RootParagraphStyleGroup Self="SchistParagraphStyles">"#);
    // Every reference to a root style resolves inside the package.
    if !document
        .styles
        .paragraphs
        .iter()
        .any(|s| s.name == "[No paragraph style]")
    {
        out.push_str(&format!(
            r#"<ParagraphStyle Self="{}" Name="$ID/[No paragraph style]" />"#,
            crate::style_codec::NO_PARAGRAPH_STYLE
        ));
    }
    for style in &document.styles.paragraphs {
        if style.writing_mode.is_some() {
            let message = schist_i18n::t("design.idml_paragraph_orientation").to_string();
            if !warnings.contains(&message) {
                warnings.push(message);
            }
        }
        crate::capitalization_codec::warn(style.all_caps, style.small_caps, warnings);
        crate::opentype_codec::warn(&style.features, warnings);
        crate::style_codec::warn_stroke(style.stroke_weight, style.stroke_miter_limit, warnings);
        crate::decoration_codec::warn([&style.underline_style, &style.strike_style], warnings);
        crate::style_codec::warn_leading(style.leading, style.auto_leading, warnings);
        crate::keep_codec::warn(style, warnings);
        crate::hyphenation_codec::warn(&style.hyphenation, warnings);
        crate::drop_cap_codec::warn(style, warnings);
        let mut native = style.clone();
        native.language = languages.native(&style.language);
        let resolved = document.styles.resolve_paragraph(&style.name);
        crate::drop_cap_codec::warn_composition(&resolved, warnings);
        crate::nested_style_codec::warn(&resolved, warnings);
        if crate::drop_cap_codec::needs_native_default(&document.styles, style, &resolved) {
            native.drop_caps_characters = Some(1);
        }
        let mut xml = crate::style_codec::paragraph_resolved(
            &native,
            (
                resolved.bold.unwrap_or(false),
                resolved.italic.unwrap_or(false),
            ),
        );
        languages.label(&mut xml, &style.language);
        out.push_str(&xml);
    }
    out.push_str(
        r#"</RootParagraphStyleGroup><RootCharacterStyleGroup Self="SchistCharacterStyles">"#,
    );
    if !document
        .styles
        .characters
        .iter()
        .any(|s| s.name == "[No character style]")
    {
        out.push_str(&format!(
            r#"<CharacterStyle Self="{}" Name="$ID/[No character style]" />"#,
            crate::style_codec::NO_CHARACTER_STYLE
        ));
    }
    for style in &document.styles.characters {
        crate::capitalization_codec::warn(style.all_caps, style.small_caps, warnings);
        crate::opentype_codec::warn(&style.features, warnings);
        crate::style_codec::warn_stroke(style.stroke_weight, style.stroke_miter_limit, warnings);
        crate::decoration_codec::warn([&style.underline_style, &style.strike_style], warnings);
        crate::style_codec::warn_leading(style.leading, None, warnings);
        let resolved = document.styles.resolve_character(&style.name);
        let mut native = style.clone();
        native.language = languages.native(&style.language);
        let mut xml = crate::style_codec::character_resolved(
            &native,
            (
                resolved.bold.unwrap_or(false),
                resolved.italic.unwrap_or(false),
            ),
        );
        languages.label(&mut xml, &style.language);
        out.push_str(&xml);
    }
    if !document.styles.paragraphs.is_empty() || !document.styles.characters.is_empty() {
        warnings.push(schist_i18n::t("design.idml_style_limits").to_string());
    }
    out.push_str("</RootCharacterStyleGroup>");
    // Table and cell styles return exactly as they were read.
    for group in &document.retained_table_styles {
        out.push_str(group);
    }
    out.push_str(&crate::object_style_codec::styles_xml(document, warnings));
    out.push_str("</idPkg:Styles>");
    out
}

/// Named and actually composed faces, including a character variant inheriting
/// its family from the paragraph. Kept shared with the delivery manifest.
pub(crate) fn font_inventory(
    document: &LayoutDocument,
) -> std::collections::BTreeSet<(String, String)> {
    let mut faces = std::collections::BTreeSet::new();
    let mut add =
        |family: Option<String>, name: Option<String>, bold: Option<bool>, italic: Option<bool>| {
            if let Some(family) = family.filter(|f| !f.is_empty()) {
                let name = name.unwrap_or_else(|| {
                    match (bold.unwrap_or(false), italic.unwrap_or(false)) {
                        (true, true) => "Bold Italic",
                        (true, false) => "Bold",
                        (false, true) => "Italic",
                        _ => "Regular",
                    }
                    .to_owned()
                });
                faces.insert((family, name));
            }
        };
    for style in &document.styles.paragraphs {
        let r = document.styles.resolve_paragraph(&style.name);
        if let Some(font) = r
            .list
            .bullet_font
            .filter(|v| !matches!(v.as_str(), "" | "$ID/" | "Auto"))
        {
            add(
                Some(font),
                r.list
                    .bullet_font_style
                    .filter(|v| !matches!(v.as_str(), "" | "Auto" | "Nothing")),
                None,
                None,
            );
        }
        add(r.family, r.font_style, r.bold, r.italic);
    }
    for style in &document.styles.characters {
        let r = document.styles.resolve_character(&style.name);
        add(r.family, r.font_style, r.bold, r.italic);
    }
    for story in &document.stories {
        for structure in &story.structures {
            // Every displayed inline value: variables, page numbers and markers.
            if let Some(
                control @ (schist_layout::story::InlineControl::TextVariable { .. }
                | schist_layout::story::InlineControl::PageNumber { .. }
                | schist_layout::story::InlineControl::SectionMarker { .. }),
            ) = &structure.control
            {
                let character_style = control.character_style();
                if let Some(r) = structure.at.and_then(|at| {
                    schist_layout::text_variables::instance_character(
                        document,
                        story,
                        at,
                        character_style,
                    )
                }) {
                    add(r.family, r.font_style, r.bold, r.italic);
                }
            }
            let Some(note) = &structure.footnote else {
                continue;
            };
            if let Some(r) = structure.at.and_then(|at| {
                schist_layout::footnote_composition::reference_character(
                    document,
                    story,
                    at,
                    &note.reference_character_style,
                )
            }) {
                add(r.family, r.font_style, r.bold, r.italic);
            }
            for marker in &note.markers {
                let Some(style) = note
                    .story
                    .points
                    .iter()
                    .zip(note.story.point_offsets())
                    .find_map(|(point, start)| match point {
                        schist_layout::StoryPoint::Paragraph { text, style }
                            if start <= marker.at && marker.at <= start + text.len() =>
                        {
                            Some(style)
                        }
                        _ => None,
                    })
                else {
                    continue;
                };
                let base = document.styles.resolve_paragraph(style).character(
                    document
                        .styles
                        .resolve_character(&document.default_character_style),
                );
                let r = document
                    .styles
                    .resolve_character(&marker.character_style)
                    .over(&base);
                add(r.family, r.font_style, r.bold, r.italic);
            }
        }
    }
    for story in document.stories.iter().flat_map(|story| {
        std::iter::once(story).chain(
            story
                .structures
                .iter()
                .filter_map(|s| s.footnote.as_ref().map(|note| &note.story)),
        )
    }) {
        let markers = schist_layout::list_composition::MarkerPlans::new(document, story);
        for (point, start) in story.points.iter().zip(story.point_offsets()) {
            let schist_layout::StoryPoint::Paragraph { text, style } = point else {
                continue;
            };
            let spec = schist_layout::compose::spec_for(
                story,
                start,
                start + text.len(),
                &document.styles,
                style,
                &document.default_character_style,
                0.0,
            );
            if let Some(marker) = markers.spec(start) {
                let r = marker.style_at(0);
                add(Some(r.family), r.font_style, Some(r.bold), Some(r.italic));
            }
            for at in std::iter::once(0)
                .chain(spec.runs.iter().flat_map(|r| [r.start, r.end]))
                .filter(|at| *at < spec.text.len())
            {
                let r = spec.style_at(at);
                add(Some(r.family), r.font_style, Some(r.bold), Some(r.italic));
            }
        }
    }
    faces
}

/// Native font resources name the exact requested faces, even when missing.
/// Fonts are referenced, never embedded or silently renamed to substitutes.
fn fonts_xml(document: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    let mut families = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (family, style) in font_inventory(document) {
        families.entry(family).or_default().push(style);
    }
    if !families.is_empty() {
        warnings.push(schist_i18n::tf!(
            "design.idml_fonts_unembedded",
            count = families.len()
        ));
    }
    let mut out = format!(
        r#"{DECLARATION}<idPkg:Fonts xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">"#
    );
    for (index, (family, styles)) in families.iter().enumerate() {
        out.push_str(&format!(
            r#"<FontFamily Self="SchistFontFamily{index}" Name="{}">"#,
            escape(family)
        ));
        for (face, style) in styles.iter().enumerate() {
            out.push_str(&format!(r#"<Font Self="SchistFont{index}_{face}" FontFamily="{}" Name="{} {}" FontStyleName="{}"/>"#, escape(family), escape(family), escape(style), escape(style)));
        }
        out.push_str("</FontFamily>");
    }
    out.push_str("</idPkg:Fonts>");
    out
}

/// The document's settings a package records outside the pages.
fn preferences_xml(document: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    let preferences = crate::preferences_codec::preferences(document, warnings);
    let footnotes = crate::footnote_codec::write(document, warnings);
    format!(
        r#"{DECLARATION}<idPkg:Preferences xmlns:idPkg="{NS_PACKAGING}" DOMVersion="{DOM_VERSION}">{preferences}{footnotes}</idPkg:Preferences>"#
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
    escape(&paragraph_reference_raw(name))
}

/// A paragraph style's native reference, unescaped.
pub(crate) fn paragraph_reference_raw(name: &str) -> String {
    if name.is_empty() {
        return crate::style_codec::NO_PARAGRAPH_STYLE.into();
    }
    format!("ParagraphStyle/$ID/{name}")
}

/// A character style's native reference, unescaped.
pub(crate) fn character_reference_raw(name: &str) -> String {
    if name.is_empty() {
        return crate::style_codec::NO_CHARACTER_STYLE.into();
    }
    format!("CharacterStyle/$ID/{name}")
}

/// Unstyled text names the root no-style, which Styles.xml always defines.
pub(crate) fn character_reference(name: &str) -> String {
    if name.is_empty() {
        return crate::style_codec::NO_CHARACTER_STYLE.into();
    }
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
    use schist_layout::ParagraphStyle;

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
