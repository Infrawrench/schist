//! The package's parts: what is in it, and which part is what.
//!
//! An IDML package is an OPC archive, so it is navigated the OPC way
//! rather than by convention: `META-INF/container.xml` names the root
//! part, and the root part (`designmap.xml`) lists every other object part
//! as `<idPkg:Kind src="path"/>`.
//!
//! ## Object ids are not file names
//!
//! The obvious assumption — that a part's file name is its object's id, so
//! `Self="Story_u39c"` names `Stories/Story_u39c.xml` — is **wrong**, and the
//! real exports are what show it. Their ids look like `u39c`, `ueb`, `ub8`
//! and `d`: a prefix and a hexadecimal number, with no type in them.
//! A conventional file name hints at the type; XML supplies the actual id:
//!
//! ```text
//! <idPkg:Story src="Stories/Story_u39c.xml" />   file name  ─┐
//! <Story Self="u39c">                                id      ─┘ together, the part
//! ```
//!
//! So resolution is: read the root part's `src` list, and build the map
//! from each file name to the id inside it. Nothing can be resolved from
//! the id alone. Neither a file name nor a namespace prefix is an object id.
//!
//! ## Three kinds of reference
//!
//! A document contains three sorts of pointer, and confusing them is the
//! easiest way to write a reader that looks correct and resolves nothing:
//!
//! 1. **Part paths** — `src="Spreads/Spread_ueb.xml"`, from the root part.
//! 2. **Object ids** — `Self="u39c"`, resolving through [`Parts::file_for`].
//! 3. **Resource names** — `ParagraphStyle/$ID/NormalParagraphStyle`,
//!    `Color/Black`, `Ink/$ID/Process Cyan`. These name a thing *inside* a
//!    resource part rather than a part of its own, and are resolved
//!    against the resource parts, not the object index.
//!
//! This module deals in the first two. The third belongs to the object
//! layer, because a name like `Color/Black` means nothing without the
//! colour it names.

use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::{NsReader, Reader, XmlVersion};

use crate::container::Package;
use crate::error::Error;

/// The OPC media type of an IDML package.
pub const MIMETYPE: &str = crate::container::MIMETYPE;

pub const MIMETYPE_PART: &str = "mimetype";
pub const CONTAINER_PART: &str = "META-INF/container.xml";
/// The root part every conforming package has.
pub const DESIGNMAP: &str = "designmap.xml";
const PACKAGING_NS: &[u8] = b"http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging";

/// The directory a kind of part lives in, and the prefix its file name
/// carries. Both are conventions of the format rather than requirements,
/// so they are used to *recognise* a part, never to resolve one.
pub mod dirs {
    pub const SPREADS: &str = "Spreads/";
    pub const STORIES: &str = "Stories/";
    pub const MASTER_SPREADS: &str = "MasterSpreads/";
    pub const RESOURCES: &str = "Resources/";
    pub const GRAPHICS: &str = "Graphics/";
    pub const LINKS: &str = "Links/";
    pub const STYLES: &str = "Styles/";
    pub const NOTES: &str = "Notes/";
}

/// Which kind of object a part holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PartKind {
    Spread,
    MasterSpread,
    Story,
    /// A resource collection: `Resources/Graphic.xml` holds inks, colours
    /// and swatches; `Resources/Fonts.xml` holds fonts; and so on. These
    /// are the parts that resource *names* resolve inside.
    Resource,
    Graphic,
    Link,
    Style,
    Note,
    /// Something the root part lists that is not one of the above. Kept
    /// rather than dropped, so a file written by a newer InDesign opens
    /// with a warning instead of a truncated document.
    Unknown,
}

impl PartKind {
    /// The kind a part name suggests, from its directory and file-name
    /// prefix.
    ///
    /// This is a hint for grouping and for naming a part the root did not
    /// list. It is *not* how an id resolves — see the module docs.
    pub fn of_name(name: &str) -> PartKind {
        // A resource part is one file holding many resources, so it is
        // matched on its directory before the prefixed kinds.
        if name.starts_with(dirs::RESOURCES) {
            return PartKind::Resource;
        }
        for kind in [
            PartKind::MasterSpread,
            PartKind::Spread,
            PartKind::Story,
            PartKind::Graphic,
            PartKind::Style,
            PartKind::Note,
        ] {
            if name.starts_with(kind.dir()) {
                return kind;
            }
        }
        if name.starts_with(dirs::LINKS) {
            return PartKind::Link;
        }
        PartKind::Unknown
    }

    /// The directory a part of this kind lives in.
    pub fn dir(self) -> &'static str {
        match self {
            PartKind::Spread => dirs::SPREADS,
            PartKind::MasterSpread => dirs::MASTER_SPREADS,
            PartKind::Story => dirs::STORIES,
            PartKind::Resource => dirs::RESOURCES,
            PartKind::Graphic => dirs::GRAPHICS,
            PartKind::Link => dirs::LINKS,
            PartKind::Style => dirs::STYLES,
            PartKind::Note => dirs::NOTES,
            PartKind::Unknown => "",
        }
    }

    /// The element the root part uses to list a part of this kind, which
    /// is also the file-name prefix: `idPkg:Story` for
    /// `Stories/Story_u39c.xml`.
    pub fn element(self) -> &'static str {
        match self {
            PartKind::Spread => "Spread",
            PartKind::MasterSpread => "MasterSpread",
            PartKind::Story => "Story",
            PartKind::Resource => "Resource",
            PartKind::Graphic => "Graphic",
            PartKind::Link => "Link",
            PartKind::Style => "Style",
            PartKind::Note => "Note",
            PartKind::Unknown => "",
        }
    }

    /// The `idPkg:` element name a part of this kind is listed under.
    pub fn listed_as(self) -> &'static str {
        match self {
            // The root part lists resources by their own file name's stem
            // rather than under one umbrella element, so there is no
            // single name.
            PartKind::Resource => "",
            PartKind::Unknown => "",
            kind => kind.element(),
        }
    }
}

/// The kind an `idPkg:` element name in the root part denotes.
pub fn kind_of_listed(element: &str) -> PartKind {
    match element {
        "Spread" => PartKind::Spread,
        "MasterSpread" => PartKind::MasterSpread,
        "Story" => PartKind::Story,
        "Graphic" => PartKind::Graphic,
        "Link" => PartKind::Link,
        "Style" => PartKind::Style,
        "Note" => PartKind::Note,
        // Fonts, Styles, Preferences, Tags, NumberingList and whatever a
        // newer version adds: all resource parts.
        _ => PartKind::Resource,
    }
}

/// One part, and the object in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub kind: PartKind,
    /// The local name of the packaging element, independent of the part path.
    /// Empty for an unlisted part.
    pub role: String,
    /// The part's name in the package.
    pub name: String,
    /// The object's XML `Self` id, populated when opening the package.
    ///
    /// Empty for a resource part, which holds many objects and is named
    /// by its file rather than by an id.
    pub id: String,
}

/// What the package contains.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parts {
    parts: Vec<Part>,
}

impl Parts {
    /// Every part, in package order.
    pub fn all(&self) -> &[Part] {
        &self.parts
    }

    /// The parts of one kind, in package order.
    pub fn of(&self, kind: PartKind) -> Vec<&Part> {
        self.parts.iter().filter(|part| part.kind == kind).collect()
    }

    /// The part holding an object id, if the package has it.
    ///
    /// This is the lookup the object layer does per reference, so it is a
    /// scan of a list that is tens of entries long rather than a map: a
    /// document with a few hundred stories does not want a hash built to
    /// be thrown away.
    pub fn file_for(&self, id: &str) -> Option<&str> {
        self.parts
            .iter()
            .find(|part| !part.id.is_empty() && part.id == id)
            .map(|part| part.name.as_str())
    }

    /// The part with a given package name.
    pub fn part(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|part| part.name == name)
    }

    /// Parse a conventional type-prefixed filename stem. This is only a hint:
    /// producers may choose arbitrary paths. Package opening and object lookup
    /// use the actual XML `Self` id and never use this helper.
    pub fn id_for(name: &str) -> Option<String> {
        let stem = name.rsplit('/').next().unwrap_or(name);
        let stem = stem.strip_suffix(".xml")?;
        let prefix = PartKind::of_name(name).element();
        if prefix.is_empty() {
            return None;
        }
        stem.strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('_'))
            .map(str::to_owned)
    }

    /// Inventory package names. Kinds are hints until the design map is read;
    /// ids require XML content and are deliberately empty here.
    pub fn index(names: impl IntoIterator<Item = String>) -> Parts {
        let parts = names
            .into_iter()
            .filter(|name| !name.ends_with('/'))
            .map(|name| {
                let kind = PartKind::of_name(&name);
                Part {
                    kind,
                    name,
                    role: String::new(),
                    id: String::new(),
                }
            })
            .collect();
        Parts { parts }
    }
}

/// A package, checked to be an IDML one, with its parts indexed.
#[derive(Debug)]
pub struct DesignPackage<'a> {
    package: &'a Package,
    /// The root part, from `container.xml`. Always `designmap.xml` in
    /// practice, but read rather than assumed.
    pub root: String,
    pub parts: Parts,
    /// The parts the root part listed, in the order it listed them.
    ///
    /// Distinct from `parts`, which is every part in the package: this is
    /// what the document says it contains, which is what a writer must
    /// reproduce and what a reader should prefer.
    pub listed: Vec<Part>,
    /// Parts present in the package that the root part did not list.
    pub unlisted: Vec<String>,
}

impl<'a> DesignPackage<'a> {
    /// Index a package, checking that it is an IDML one.
    pub fn open(package: &'a Package) -> Result<DesignPackage<'a>, Error> {
        let mimetype = package.text(MIMETYPE_PART).unwrap_or_default().trim();
        if !mimetype.is_empty() && mimetype != MIMETYPE {
            return Err(Error::Unsupported(format!(
                "package declares {mimetype:?}, which is not an IDML package"
            )));
        }
        let root = read_root(package)?;
        let parts = Parts::index(package.names().into_iter().map(str::to_owned));
        let mut opened = DesignPackage {
            package,
            root,
            parts,
            listed: Vec::new(),
            unlisted: Vec::new(),
        };
        opened.read_listed()?;
        Ok(opened)
    }

    /// Read the root part's `idPkg:* src=…` list.
    fn read_listed(&mut self) -> Result<(), Error> {
        let root = self.root.clone();
        let text = std::str::from_utf8(
            self.package
                .get(&root)
                .ok_or_else(|| Error::MissingPart { part: root.clone() })?,
        )
        .map_err(|error| Error::Xml {
            part: root.clone(),
            message: error.to_string(),
        })?;

        let mut listed = Vec::new();
        let mut reader = NsReader::from_str(text);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        loop {
            match reader.read_resolved_event_into(&mut buf) {
                Ok((namespace, Event::Start(element) | Event::Empty(element))) => {
                    // XML namespace identity is the URI, never a preferred prefix.
                    if !matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == PACKAGING_NS)
                    {
                        buf.clear();
                        continue;
                    }
                    let local = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                    for attribute in element.attributes() {
                        let attribute = attribute.map_err(|error| Error::Xml {
                            part: root.clone(),
                            message: error.to_string(),
                        })?;
                        if attribute.key.as_ref() != b"src" {
                            continue;
                        }
                        let src = attribute
                            .normalized_value(XmlVersion::Implicit1_0)
                            .map_err(|error| Error::Xml {
                                part: root.clone(),
                                message: error.to_string(),
                            })?
                            .into_owned();
                        let kind = kind_of_listed(&local);
                        let id = self.object_id(&src, kind);
                        if let Some(part) =
                            self.parts.parts.iter_mut().find(|part| part.name == src)
                        {
                            part.role = local.clone();
                            part.kind = kind;
                            part.id = id.clone();
                        }
                        listed.push(Part {
                            kind,
                            role: local.clone(),
                            name: src,
                            id,
                        });
                    }
                }
                Ok((_, Event::Eof)) => break,
                Ok(_) => {}
                Err(error) => {
                    return Err(Error::Xml {
                        part: root,
                        message: error.to_string(),
                    })
                }
            }
            buf.clear();
        }
        // Anything in the package the root did not mention. Recorded
        // rather than dropped, so a document can report what it skipped.
        self.unlisted = self
            .parts
            .all()
            .iter()
            .filter(|part| {
                // A linked image is referenced from the frame that places
                // it rather than listed by the root part, so its absence
                // from the list is normal. The same goes for a resource
                // part the root named some other way.
                part.kind != PartKind::Unknown
                    && part.kind != PartKind::Resource
                    && part.kind != PartKind::Link
                    && !listed.iter().any(|entry| entry.name == part.name)
            })
            .map(|part| part.name.clone())
            .collect();
        self.listed = listed;
        Ok(())
    }

    /// Only these package parts hold one top-level object. Resource collections
    /// have many `Self` attributes; none identifies the collection itself.
    fn object_id(&self, name: &str, kind: PartKind) -> String {
        if !matches!(
            kind,
            PartKind::Story | PartKind::Spread | PartKind::MasterSpread
        ) {
            return String::new();
        }
        let Some(root) = self
            .package
            .text(name)
            .and_then(|text| crate::xml::parse(text).ok())
        else {
            return String::new();
        };
        let object = if root.name == kind.element() {
            Some(&root)
        } else {
            root.child(kind.element())
        };
        object
            .and_then(|object| object.attr("Self"))
            .unwrap_or_default()
            .to_owned()
    }

    /// The root part's bytes.
    pub fn root_bytes(&self) -> Result<&[u8], Error> {
        self.package
            .get(&self.root)
            .ok_or_else(|| Error::MissingPart {
                part: self.root.clone(),
            })
    }

    /// A part's bytes, by object id.
    pub fn part_for_id(&self, id: &str) -> Result<&[u8], Error> {
        let name = self.parts.file_for(id).ok_or_else(|| Error::MissingPart {
            part: format!("{id}.xml"),
        })?;
        self.package.get(name).ok_or_else(|| Error::MissingPart {
            part: name.to_owned(),
        })
    }

    /// A part's text, by object id.
    pub fn text_for_id(&self, id: &str) -> Result<&str, Error> {
        std::str::from_utf8(self.part_for_id(id)?).map_err(|error| Error::Xml {
            part: id.to_owned(),
            message: error.to_string(),
        })
    }

    /// A part's text, by package name.
    pub fn text_of(&self, name: &str) -> Result<&str, Error> {
        let bytes = self.package.get(name).ok_or_else(|| Error::MissingPart {
            part: name.to_owned(),
        })?;
        std::str::from_utf8(bytes).map_err(|error| Error::Xml {
            part: name.to_owned(),
            message: error.to_string(),
        })
    }

    /// Every part of one kind the root part listed.
    pub fn listed_of(&self, kind: PartKind) -> Vec<&Part> {
        self.listed
            .iter()
            .filter(|part| part.kind == kind)
            .collect()
    }
}

/// The root part, from `META-INF/container.xml`.
///
/// `<rootfile full-path="designmap.xml" …/>`, taking the first one: a
/// package has one root part, and a second rootfile is a package this
/// cannot make sense of rather than one with two documents.
fn read_root(package: &Package) -> Result<String, Error> {
    let part = CONTAINER_PART;
    let text = package
        .text(part)
        .ok_or_else(|| Error::MissingPart { part: part.into() })?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(element)) | Ok(Event::Start(element)) => {
                if local_name(element.name().as_ref()) == b"rootfile" {
                    for attribute in element.attributes().flatten() {
                        if attribute.key.as_ref() == b"full-path" {
                            let path = attribute
                                .normalized_value(XmlVersion::Implicit1_0)
                                .map_err(|error| Error::Xml {
                                    part: part.into(),
                                    message: error.to_string(),
                                })?
                                .into_owned();
                            if !path.is_empty() {
                                return Ok(path);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(Error::Xml {
                    part: part.into(),
                    message: error.to_string(),
                })
            }
        }
        buf.clear();
    }
    Err(Error::MissingPart {
        part: "designmap.xml (no rootfile in META-INF/container.xml)".into(),
    })
}

/// An element's local name, dropping any namespace prefix.
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|byte| *byte == b':') {
        Some(at) => &name[at + 1..],
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> Package {
        Package::from_parts(vec![
            (MIMETYPE_PART.into(), MIMETYPE.as_bytes().to_vec()),
            (
                CONTAINER_PART.into(),
                br#"<container><rootfiles><rootfile full-path="designmap.xml"/></rootfiles></container>"#
                    .to_vec(),
            ),
            (
                "designmap.xml".into(),
                br#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
  <idPkg:Story src="Stories/Story_u39c.xml" />
  <idPkg:Story src="Stories/Story_u373.xml" />
  <idPkg:Spread src="Spreads/Spread_ueb.xml" />
  <idPkg:MasterSpread src="MasterSpreads/MasterSpread_ub8.xml" />
  <idPkg:Graphic src="Resources/Graphic.xml" />
  <idPkg:Fonts src="Resources/Fonts.xml" />
</Document>"#
                .to_vec(),
            ),
            ("Spreads/Spread_ueb.xml".into(), br#"<Spread Self="ueb"/>"#.to_vec()),
            ("Stories/Story_u39c.xml".into(), br#"<Story Self="u39c"/>"#.to_vec()),
            ("Stories/Story_u373.xml".into(), br#"<Story Self="u373"/>"#.to_vec()),
            ("MasterSpreads/MasterSpread_ub8.xml".into(), b"<Master/>".to_vec()),
            ("Resources/Graphic.xml".into(), b"<Resources/>".to_vec()),
            ("Resources/Fonts.xml".into(), b"<Resources/>".to_vec()),
            ("Links/linked.png".into(), vec![0x89, b'P', b'N', b'G']),
        ])
    }

    #[test]
    fn the_root_part_comes_from_container_xml() {
        let package = package();
        let opened = DesignPackage::open(&package).expect("an IDML package");
        assert_eq!(opened.root, "designmap.xml");
        assert!(!opened.root_bytes().unwrap().is_empty());
    }

    #[test]
    fn an_object_id_is_not_a_file_name() {
        // The real exports use `u39c` where the file is
        // `Story_u39c.xml`, and this is the mistake the whole mapping had
        // before the fixtures existed: assuming the id carried its type.
        assert_eq!(
            Parts::id_for("Stories/Story_u39c.xml").as_deref(),
            Some("u39c")
        );
        assert_eq!(
            Parts::id_for("Spreads/Spread_ueb.xml").as_deref(),
            Some("ueb")
        );
        assert_eq!(
            Parts::id_for("MasterSpreads/MasterSpread_ub8.xml").as_deref(),
            Some("ub8")
        );
    }

    #[test]
    fn a_part_without_its_type_prefix_has_no_object_id() {
        // A resource part is named by its file, and holds many objects, so
        // giving it an id would put a name where a reference could collide.
        assert_eq!(Parts::id_for("Resources/Graphic.xml"), None);
        assert_eq!(Parts::id_for("Notes/Notes.xml"), None);
        assert_eq!(Parts::id_for("Links/linked.png"), None);
        // A non-XML part has no id either.
        assert_eq!(Parts::id_for("Spreads/Spread_ueb"), None);
    }

    #[test]
    fn the_root_parts_list_resolves_object_ids_to_parts() {
        let package = package();
        let opened = DesignPackage::open(&package).expect("a usable package");
        assert_eq!(
            opened.parts.file_for("u39c"),
            Some("Stories/Story_u39c.xml")
        );
        assert_eq!(opened.parts.file_for("ueb"), Some("Spreads/Spread_ueb.xml"));
        assert_eq!(opened.parts.file_for("u404"), None, "absent, not guessed");
    }

    #[test]
    fn only_id_pkg_elements_list_parts() {
        // The root part's own elements are document properties, and
        // treating one as a part reference would invent a file.
        let package = package();
        let opened = DesignPackage::open(&package).expect("a usable package");
        assert!(
            !opened
                .listed
                .iter()
                .any(|part| part.name.contains("Document")),
            "{:?}",
            opened.listed
        );
    }

    #[test]
    fn a_resource_part_is_listed_but_carries_no_object_id() {
        let package = package();
        let opened = DesignPackage::open(&package).expect("a usable package");
        let resource = opened
            .listed
            .iter()
            .find(|part| part.name == "Resources/Graphic.xml")
            .expect("listed");
        // A Graphic resource collection has no single object identity,
        // regardless of its file name.
        assert_eq!(resource.kind, PartKind::Graphic);
        assert!(resource.id.is_empty(), "a resource part holds many objects");
        // And so it cannot collide with a real id.
        assert!(opened.parts.file_for("Graphic").is_none());
    }

    #[test]
    fn the_listed_parts_are_what_the_document_says_it_has() {
        let package = package();
        let opened = DesignPackage::open(&package).expect("a usable package");
        assert_eq!(opened.listed_of(PartKind::Story).len(), 2);
        assert_eq!(opened.listed_of(PartKind::Spread).len(), 1);
        assert_eq!(opened.listed_of(PartKind::MasterSpread).len(), 1);
        // A linked image is in the package but is not an object part, so
        // it is neither listed nor reported as unlisted.
        assert!(opened.unlisted.is_empty(), "{:?}", opened.unlisted);
    }

    #[test]
    fn a_part_present_but_not_listed_is_reported() {
        // A writer must be able to say what it skipped, or a document
        // loses content silently.
        let mut package = package();
        package.insert("Stories/Story_udead.xml", b"<Story/>".to_vec());
        let opened = DesignPackage::open(&package).expect("a usable package");
        assert_eq!(opened.unlisted, vec!["Stories/Story_udead.xml".to_string()]);
    }

    #[test]
    fn a_package_that_is_not_idml_is_refused() {
        let mut package = package();
        package.insert(MIMETYPE_PART, b"application/zip".to_vec());
        let error = DesignPackage::open(&package).expect_err("not IDML");
        assert!(matches!(error, Error::Unsupported(_)), "{error:?}");
    }

    #[test]
    fn a_package_without_a_container_is_reported() {
        let mut package = package();
        package.remove(CONTAINER_PART);
        let error = DesignPackage::open(&package).expect_err("no container");
        assert!(matches!(error, Error::MissingPart { .. }), "{error:?}");
    }

    #[test]
    fn a_malformed_root_is_reported_not_guessed() {
        let mut package = package();
        package.insert("designmap.xml", b"<Document></Root>".to_vec());
        let error = DesignPackage::open(&package).expect_err("mismatched tags");
        assert!(matches!(error, Error::Xml { .. }), "{error:?}");
    }

    #[test]
    fn a_part_is_readable_by_its_id_and_by_its_name() {
        let package = package();
        let opened = DesignPackage::open(&package).expect("a usable package");
        assert_eq!(
            opened.part_for_id("u39c").unwrap(),
            br#"<Story Self="u39c"/>"#
        );
        assert_eq!(
            opened.text_of("Spreads/Spread_ueb.xml").unwrap(),
            r#"<Spread Self="ueb"/>"#
        );
        assert!(matches!(
            opened.part_for_id("u9"),
            Err(Error::MissingPart { .. })
        ));
    }

    #[test]
    fn a_master_spread_is_not_mistaken_for_a_spread() {
        // The directory and prefix overlap, and matching the shorter first
        // would send a master reference to a page part.
        assert_eq!(
            PartKind::of_name("MasterSpreads/MasterSpread_ub8.xml"),
            PartKind::MasterSpread
        );
        assert_eq!(
            PartKind::of_name("Spreads/Spread_ueb.xml"),
            PartKind::Spread
        );
    }

    #[test]
    fn a_linked_image_is_a_part_and_not_an_xml_object() {
        let package = package();
        assert_eq!(package.get("Links/linked.png").unwrap()[0], 0x89);
        let opened = DesignPackage::open(&package).expect("a usable package");
        assert_eq!(opened.parts.of(PartKind::Link).len(), 1);
    }
}
