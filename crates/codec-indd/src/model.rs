//! The objects the recovered subset understands.
//!
//! Each object's class is a number from the class tree; each field is a
//! chunk tag. Both were identified by matching the specimens' objects to
//! the IDML exported beside them, where every `Self="u…"` id is the UID
//! of the same object in the INDD database. A field is decoded here only
//! when every specimen object of its class agrees with its IDML twin.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::chunk::{find, Reader};
use crate::database::{Database, Object};
use crate::error::Error;

/// Object classes.
pub mod class {
    pub const DOCUMENT: u32 = 0xe01;
    pub const LAYER: u32 = 0x302;
    pub const SPREAD: u32 = 0x501;
    pub const MASTER_SPREAD: u32 = 0x1401;
    /// One layer's share of a spread: its items, or its guides.
    pub const SPREAD_LAYER: u32 = 0x301;
    pub const PAGE: u32 = 0x50f;
    /// Rectangles, polygons, lines and frames: anything with a path.
    pub const SPLINE: u32 = 0x6201;
    pub const GROUP: u32 = 0x401;
    pub const GUIDE: u32 = 0x3301;
    pub const SWATCH: u32 = 0x1f05;
    pub const SECTION: u32 = 0x4c01;
    /// Document setup: page size, facing pages, bleed.
    pub const DOCUMENT_SETUP: u32 = 0x2202;
    pub const STORY: u32 = 0x201;
    /// The strand holding a story's characters, by block.
    pub const TEXT_STRAND: u32 = 0x234;
    pub const TEXT_BLOCK: u32 = 0xca18;
    /// The strand saying which object owns each range of a story's
    /// characters: the story itself, a table cell or a footnote.
    pub const OWNER_STRAND: u32 = 0x2a3;
    pub const OWNER_BLOCK: u32 = 0x27e;
    pub const TABLE: u32 = 0xb608;
    pub const FOOTNOTE: u32 = 0x24f;
    /// A story's frames, in thread order.
    pub const FRAME_LIST: u32 = 0x228;
    /// The text side of a frame, between it and its columns.
    pub const MULTI_COLUMN: u32 = 0x263;
    pub const COLUMN: u32 = 0x227;
}

/// Chunk tags.
mod tag {
    pub const SPREADS: u32 = 0x501;
    pub const MASTER_SPREADS: u32 = 0x1401;
    pub const LAYERS: u32 = 0x301;
    pub const LAYER: u32 = 0x304;
    pub const SPREAD_CHILDREN: u32 = 0x503;
    pub const SPREAD_TRANSFORM: u32 = 0x56e;
    pub const MASTER_NAME: u32 = 0x1402;
    pub const SPREAD_LAYER: u32 = 0x302;
    pub const SPREAD_LAYER_ITEMS: u32 = 0x303;
    pub const PAGE_BOUNDS: u32 = 0x5dd;
    pub const PAGE_TRANSFORM: u32 = 0x5cc;
    pub const PAGE_MARGINS: u32 = 0x51a;
    pub const APPLIED_MASTER: u32 = 0x140f;
    /// Spread, parent, then the children.
    pub const HIERARCHY: u32 = 0x15b;
    pub const ITEM_TRANSFORM: u32 = 0x151;
    pub const GROUP_TRANSFORM: u32 = 0x40d;
    /// A word: 1 visible, 0 hidden.
    pub const VISIBLE: u32 = 0x2c32;
    pub const PATH: u32 = 0x162b;
    /// The object holding an item's path when the item does not.
    pub const PATH_OWNER: u32 = 0x104;
    pub const ATTRIBUTES: u32 = 0x6e03;
    /// The attribute naming an item's fill swatch.
    pub const FILL: u32 = 0x6e68;
    /// An attribute value that is a UID.
    pub const UID_VALUE: u32 = 0x117;
    pub const FRAME_LIST: u32 = 0x205;
    pub const STORY_PARTS: u32 = 0x223;
    pub const STRAND_BLOCKS: u32 = 0x261;
    pub const BLOCK: u32 = 0x262;
    pub const SWATCH_NAME: u32 = 0x1f10;
    pub const SWATCH_VALUE: u32 = 0x1f01;
    pub const SWATCH_MODEL: u32 = 0x1f09;
    pub const SECTION: u32 = 0x4c02;
    pub const SETUP: u32 = 0x533;
}

/// The page number style the specimens' sections all have: Arabic.
const ARABIC: u32 = 0x4c15;

/// The document root's UID in every specimen.
const DOCUMENT: u32 = 1;

/// An affine transform as IDML writes one: a b c d tx ty.
pub type Matrix = [f64; 6];
pub const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[derive(Debug, Default)]
pub struct Document {
    pub layers: Vec<Layer>,
    pub swatches: Vec<Swatch>,
    pub spreads: Vec<Spread>,
    pub masters: Vec<Spread>,
    pub stories: Vec<Story>,
    pub sections: Vec<Section>,
    /// Single pages or facing pages, when the setup says.
    pub facing: Option<bool>,
    /// The bleed, when it is the same on every side.
    pub bleed: Option<f64>,
    /// What the subset could not read, for the import report.
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    pub uid: u32,
    /// The page the section starts on; the document's first when None.
    pub start: Option<u32>,
    pub number: u32,
    pub continues: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub uid: u32,
    pub name: String,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorModel {
    Process,
    Spot,
    Registration,
}

/// A colour's components in IDML's units: percentages for CMYK, 0–255
/// for RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorValue {
    Cmyk([f64; 4]),
    Rgb([f64; 3]),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Swatch {
    pub uid: u32,
    pub name: String,
    pub model: ColorModel,
    pub value: ColorValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MasterName {
    pub prefix: String,
    pub base: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spread {
    pub uid: u32,
    pub transform: Matrix,
    pub master: Option<MasterName>,
    pub pages: Vec<PageObject>,
    /// In paint order, bottom first.
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageObject {
    pub uid: u32,
    pub name: String,
    /// Left, top, right, bottom, in the page's own coordinates.
    pub bounds: [f64; 4],
    pub transform: Matrix,
    /// The master spread applied, with the transform that places it.
    pub master: Option<(u32, Matrix)>,
    /// Left, top, right, bottom.
    pub margins: Option<[f64; 4]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub uid: u32,
    /// The document layer, for an item directly on a spread.
    pub layer: Option<u32>,
    pub transform: Matrix,
    pub visible: bool,
    pub kind: ItemKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Group(Vec<Item>),
    Spline {
        paths: Vec<Path>,
        fill: Option<u32>,
        frame: Option<TextFrame>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// Corner points; the specimens have no curves.
    pub points: Vec<[f64; 2]>,
    pub open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextFrame {
    pub story: u32,
    pub previous: Option<u32>,
    pub next: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Story {
    pub uid: u32,
    /// The story's own characters, with table cells and footnotes left
    /// out. Paragraphs end in `\r`.
    pub text: String,
}

/// Read the subset from a database.
pub fn read(db: &Database) -> Result<Document, Error> {
    let root = db
        .of_class(DOCUMENT, class::DOCUMENT)
        .ok_or(Error::DamagedObject { uid: DOCUMENT })?;
    let mut reader = Reading {
        db,
        out: Document::default(),
        unknown: BTreeMap::new(),
        guides: 0,
        curves: 0,
        contents: 0,
    };
    reader.layers(root);
    reader.swatches();
    let frames = reader.frames();
    reader.out.spreads = reader.spreads(root, tag::SPREADS, class::SPREAD, &frames);
    reader.out.masters = reader.spreads(root, tag::MASTER_SPREADS, class::MASTER_SPREAD, &frames);
    reader.stories(&frames);
    reader.inherit_margins();
    reader.setup();
    reader.sections();
    reader.name_pages();
    reader.summarize();
    Ok(reader.out)
}

struct Reading<'a> {
    db: &'a Database,
    out: Document,
    /// Page item classes the subset does not read, with how many.
    unknown: BTreeMap<u32, usize>,
    guides: usize,
    curves: usize,
    contents: usize,
}

/// A parent page's position across the spine, and its margins.
type Sided = (f64, Option<[f64; 4]>);

/// Each frame's story and neighbours in its thread, by frame UID.
type Frames = HashMap<u32, TextFrame>;

impl<'a> Reading<'a> {
    fn object(&self, uid: u32) -> Option<&'a Object> {
        self.db.get(uid)
    }

    fn chunk(&self, uid: u32, tag: u32) -> Option<&'a [u8]> {
        find(&self.object(uid)?.bytes, tag)
    }

    fn skip(&mut self, message: String) {
        self.out.skipped.push(message);
    }

    fn layers(&mut self, root: &[u8]) {
        let Some(uids) = find(root, tag::LAYERS).and_then(|c| Reader::new(c).uids()) else {
            return;
        };
        for uid in uids {
            let Some(layer) = self
                .db
                .of_class(uid, class::LAYER)
                .and_then(|bytes| find(bytes, tag::LAYER))
            else {
                continue;
            };
            // The first byte marks InDesign's internal layer, which holds
            // the pages and has no IDML counterpart.
            if layer.first() != Some(&0) {
                continue;
            }
            let Some(name) = Reader::at(layer, 18).string() else {
                self.skip(schist_i18n::tf!("design.indd_layer_unread", uid = uid));
                continue;
            };
            self.out.layers.push(Layer {
                uid,
                name,
                visible: layer.get(2) == Some(&1),
            });
        }
    }

    fn swatches(&mut self) {
        let db = self.db;
        let mut swatches = Vec::new();
        let mut unread = Vec::new();
        for (&uid, object) in &db.objects {
            if object.class != class::SWATCH {
                continue;
            }
            let Some(value) = find(&object.bytes, tag::SWATCH_VALUE) else {
                continue;
            };
            let name = find(&object.bytes, tag::SWATCH_NAME)
                .and_then(|c| Reader::new(c).string())
                .unwrap_or_default();
            let model =
                match find(&object.bytes, tag::SWATCH_MODEL).and_then(|c| Reader::new(c).u32()) {
                    Some(0) => ColorModel::Process,
                    Some(1) => ColorModel::Spot,
                    Some(2) => ColorModel::Registration,
                    _ => {
                        if !name.is_empty() {
                            unread.push(name);
                        }
                        continue;
                    }
                };
            match color(value) {
                Some(value) => swatches.push(Swatch {
                    uid,
                    name,
                    model,
                    value,
                }),
                // Unnamed swatches are InDesign's internal colours, which
                // IDML export leaves out; one an item uses is reported
                // with the item.
                None if name.is_empty() => {}
                None => unread.push(name),
            }
        }
        self.out.swatches = swatches;
        for name in unread {
            self.skip(schist_i18n::tf!("design.indd_swatch_unread", name = name));
        }
    }

    /// Every story's frames, from the frame lists: a story, then its
    /// columns in thread order, each column inside one frame.
    fn frames(&mut self) -> Frames {
        let db = self.db;
        let mut threads: Vec<(u32, Vec<u32>)> = Vec::new();
        for (&uid, object) in &db.objects {
            if object.class != class::FRAME_LIST {
                continue;
            }
            let Some(list) = find(&object.bytes, tag::FRAME_LIST) else {
                continue;
            };
            let mut reader = Reader::new(list);
            let (Some(story), Some(columns)) = (reader.u32(), reader.uids()) else {
                log::warn!("INDD frame list {uid} unreadable");
                continue;
            };
            // A frame of several columns appears once per column.
            let mut frames: Vec<u32> = Vec::new();
            for column in columns {
                if let Some(frame) = self.frame_of_column(column) {
                    if frames.last() != Some(&frame) {
                        frames.push(frame);
                    }
                }
            }
            threads.push((story, frames));
        }
        let mut out = Frames::new();
        for (story, frames) in threads {
            for (i, &frame) in frames.iter().enumerate() {
                out.insert(
                    frame,
                    TextFrame {
                        story,
                        previous: i.checked_sub(1).map(|p| frames[p]),
                        next: frames.get(i + 1).copied(),
                    },
                );
            }
        }
        out
    }

    /// The frame a text column belongs to: column, then the frame's text
    /// side, then the frame itself, each the parent of the one before.
    fn frame_of_column(&self, column: u32) -> Option<u32> {
        let parent = |uid: u32, of: u32| {
            (self.object(uid)?.class == of)
                .then(|| hierarchy(self.chunk(uid, tag::HIERARCHY)?))
                .flatten()
                .map(|h| h.parent)
        };
        let side = parent(column, class::COLUMN)?;
        let frame = parent(side, class::MULTI_COLUMN)?;
        (self.object(frame)?.class == class::SPLINE).then_some(frame)
    }

    fn spreads(&mut self, root: &[u8], list: u32, of: u32, frames: &Frames) -> Vec<Spread> {
        let uids = find(root, list)
            .and_then(|c| Reader::new(c).uids())
            .unwrap_or_default();
        let mut out = Vec::new();
        for uid in uids {
            match self.spread(uid, of, frames) {
                Some(spread) => out.push(spread),
                None => self.skip(schist_i18n::tf!("design.indd_spread_unread", uid = uid)),
            }
        }
        out
    }

    fn spread(&mut self, uid: u32, of: u32, frames: &Frames) -> Option<Spread> {
        let object = self.db.of_class(uid, of)?;
        let mut children = Reader::new(find(object, tag::SPREAD_CHILDREN)?);
        children.u32()?;
        children.u32()?;
        let layers = children.uids()?;
        let transform = find(object, tag::SPREAD_TRANSFORM)
            .and_then(|c| Reader::new(c).f64s::<6>())
            .unwrap_or(IDENTITY);
        let master = (of == class::MASTER_SPREAD)
            .then(|| {
                let mut name = Reader::new(find(object, tag::MASTER_NAME)?);
                Some(MasterName {
                    prefix: name.string()?,
                    base: name.string()?,
                })
            })
            .flatten();
        let mut spread = Spread {
            uid,
            transform,
            master,
            pages: Vec::new(),
            items: Vec::new(),
        };
        for spread_layer in layers {
            let Some(bytes) = self.db.of_class(spread_layer, class::SPREAD_LAYER) else {
                continue;
            };
            let layer = find(bytes, tag::SPREAD_LAYER).and_then(|c| Reader::new(c).u32());
            let Some(items) = find(bytes, tag::SPREAD_LAYER_ITEMS).and_then(|c| {
                let mut reader = Reader::new(c);
                reader.u32()?;
                reader.u32()?;
                reader.uids()
            }) else {
                continue;
            };
            for item in items {
                match self.object(item).map(|o| o.class) {
                    Some(class::PAGE) => {
                        if let Some(page) = self.page(item) {
                            spread.pages.push(page);
                        } else {
                            self.skip(schist_i18n::tf!("design.indd_page_unread", uid = item));
                        }
                    }
                    Some(class::GUIDE) => self.guides += 1,
                    _ => {
                        if let Some(mut item) = self.item(item, frames, 0) {
                            item.layer = layer;
                            spread.items.push(item);
                        }
                    }
                }
            }
        }
        Some(spread)
    }

    fn page(&self, uid: u32) -> Option<PageObject> {
        let object = self.db.of_class(uid, class::PAGE)?;
        let bounds = Reader::new(find(object, tag::PAGE_BOUNDS)?).f64s::<4>()?;
        if bounds.iter().any(|v| !v.is_finite()) || bounds[2] <= bounds[0] || bounds[3] <= bounds[1]
        {
            return None;
        }
        let transform = find(object, tag::PAGE_TRANSFORM)
            .and_then(|c| Reader::new(c).f64s::<6>())
            .unwrap_or(IDENTITY);
        let master = find(object, tag::APPLIED_MASTER).and_then(|c| {
            let mut reader = Reader::new(c);
            let master = reader.u32()?;
            reader.u16()?;
            Some((master, reader.f64s::<6>()?))
        });
        let margins = find(object, tag::PAGE_MARGINS).and_then(|c| Reader::new(c).f64s::<4>());
        Some(PageObject {
            uid,
            name: String::new(),
            bounds,
            transform,
            master,
            margins,
        })
    }

    fn item(&mut self, uid: u32, frames: &Frames, depth: usize) -> Option<Item> {
        let object = self.object(uid)?;
        match object.class {
            class::GROUP if depth < 32 => {
                let transform = find(&object.bytes, tag::GROUP_TRANSFORM)
                    .and_then(|c| Reader::new(c).f64s::<6>())
                    .unwrap_or(IDENTITY);
                let children = find(&object.bytes, tag::HIERARCHY)
                    .and_then(hierarchy)
                    .map(|h| h.children)
                    .unwrap_or_default();
                let items: Vec<Item> = children
                    .into_iter()
                    .filter_map(|child| self.item(child, frames, depth + 1))
                    .collect();
                (!items.is_empty()).then_some(Item {
                    uid,
                    layer: None,
                    transform,
                    visible: visible(&object.bytes),
                    kind: ItemKind::Group(items),
                })
            }
            class::SPLINE => self.spline(uid, &object.bytes, frames),
            other => {
                *self.unknown.entry(other).or_default() += 1;
                None
            }
        }
    }

    fn spline(&mut self, uid: u32, bytes: &[u8], frames: &Frames) -> Option<Item> {
        let transform = find(bytes, tag::ITEM_TRANSFORM)
            .and_then(|c| Reader::new(c).f64s::<6>())
            .unwrap_or(IDENTITY);
        let path = find(bytes, tag::PATH).or_else(|| {
            let mut owner = Reader::new(find(bytes, tag::PATH_OWNER)?);
            owner.u16()?;
            let owner = *owner.uids()?.first()?;
            self.chunk(owner, tag::PATH)
        });
        let Some(paths) = path.and_then(paths) else {
            self.curves += 1;
            return None;
        };
        let children = find(bytes, tag::HIERARCHY)
            .and_then(hierarchy)
            .map(|h| h.children)
            .unwrap_or_default();
        if children
            .iter()
            .any(|&child| self.object(child).map(|o| o.class) != Some(class::MULTI_COLUMN))
        {
            self.contents += 1;
        }
        let fill = find(bytes, tag::ATTRIBUTES).and_then(fill);
        Some(Item {
            uid,
            layer: None,
            transform,
            visible: visible(bytes),
            kind: ItemKind::Spline {
                paths,
                fill,
                frame: frames.get(&uid).copied(),
            },
        })
    }

    fn stories(&mut self, frames: &Frames) {
        let mut wanted: Vec<u32> = frames.values().map(|f| f.story).collect();
        wanted.sort_unstable();
        wanted.dedup();
        for uid in wanted {
            match self.story(uid) {
                Some(story) => self.out.stories.push(story),
                None => self.skip(schist_i18n::tf!("design.indd_story_unread", uid = uid)),
            }
        }
    }

    fn story(&mut self, uid: u32) -> Option<Story> {
        let parts = story_parts(self.db.of_class(uid, class::STORY)?)?;
        let strand = |of: u32| {
            parts
                .iter()
                .copied()
                .find(|&p| self.object(p).map(|o| o.class) == Some(of))
        };
        let text = self.text(strand(class::TEXT_STRAND)?)?;
        let owners = self.owners(strand(class::OWNER_STRAND)?)?;
        if owners.iter().map(|(length, _)| length).sum::<usize>() != text.len() {
            return None;
        }
        let mut own = String::new();
        let (mut tables, mut notes, mut other) = (HashSet::new(), HashSet::new(), HashSet::new());
        let mut at = 0;
        for (length, owner) in owners {
            if owner == uid {
                own.extend(text[at..at + length].iter());
            } else {
                match self.object(owner).map(|o| o.class) {
                    Some(class::TABLE) => tables.insert(owner),
                    Some(class::FOOTNOTE) => notes.insert(owner),
                    _ => other.insert(owner),
                };
            }
            at += length;
        }
        let name = format!("u{uid:x}");
        if !tables.is_empty() {
            self.skip(schist_i18n::tf!(
                "design.indd_story_tables",
                name = name,
                count = tables.len()
            ));
        }
        if !notes.is_empty() {
            self.skip(schist_i18n::tf!(
                "design.indd_story_footnotes",
                name = name,
                count = notes.len()
            ));
        }
        if !other.is_empty() {
            self.skip(schist_i18n::tf!(
                "design.indd_story_embedded",
                name = name,
                count = other.len()
            ));
        }
        Some(Story { uid, text: own })
    }

    /// A strand's characters: blocks, each a run of segments.
    fn text(&self, strand: u32) -> Option<Vec<char>> {
        let mut out = Vec::new();
        for (count, block) in strand_blocks(self.chunk(strand, tag::STRAND_BLOCKS)?)? {
            let body = self
                .db
                .of_class(block, class::TEXT_BLOCK)
                .and_then(|b| find(b, tag::BLOCK))?;
            let mut reader = Reader::new(body);
            reader.u32()?;
            reader.u32()?;
            let runs = reader.u16()?;
            let mut got = 0;
            for _ in 0..runs {
                let size = reader.u32()? as usize;
                let end = reader.position().checked_add(size)?;
                let chars = reader.u32()? as usize;
                let mut seen = 0;
                while reader.position() < end {
                    let (text, count) = reader.segment()?;
                    out.extend(text.chars());
                    seen += count;
                }
                if reader.position() != end || seen != chars {
                    return None;
                }
                got += chars;
            }
            if got != count {
                return None;
            }
        }
        Some(out)
    }

    /// Which object owns each range of a story's characters, in order.
    fn owners(&self, strand: u32) -> Option<Vec<(usize, u32)>> {
        let mut out = Vec::new();
        for (count, block) in strand_blocks(self.chunk(strand, tag::STRAND_BLOCKS)?)? {
            let body = self
                .db
                .of_class(block, class::OWNER_BLOCK)
                .and_then(|b| find(b, tag::BLOCK))?;
            let mut reader = Reader::new(body);
            reader.u32()?;
            reader.u32()?;
            let entries = reader.u16()?;
            let mut got = 0;
            for _ in 0..entries {
                let size = reader.u32()? as usize;
                let start = reader.position();
                let length = reader.u32()? as usize;
                let owner = reader.u32()?;
                reader.take((start + size).checked_sub(reader.position())?)?;
                out.push((length, owner));
                got += length;
            }
            if got != count {
                return None;
            }
        }
        Some(out)
    }

    /// A page without margins of its own has its parent's: the parent
    /// page on the same side of the spine, as the specimens' IDML shows
    /// for every such page.
    fn inherit_margins(&mut self) {
        let side = |page: &PageObject| {
            let [left, top, right, bottom] = page.bounds;
            let (x, y) = ((left + right) / 2.0, (top + bottom) / 2.0);
            let [a, _, c, _, tx, _] = page.transform;
            a * x + c * y + tx
        };
        let parents: HashMap<u32, Vec<Sided>> = self
            .out
            .masters
            .iter()
            .map(|m| {
                (
                    m.uid,
                    m.pages.iter().map(|p| (side(p), p.margins)).collect(),
                )
            })
            .collect();
        for page in self.out.spreads.iter_mut().flat_map(|s| s.pages.iter_mut()) {
            if page.margins.is_some() {
                continue;
            }
            let Some(sheet) = page.master.and_then(|(uid, _)| parents.get(&uid)) else {
                continue;
            };
            let left = side(page) < 0.0;
            let chosen = if left {
                sheet.iter().min_by(|a, b| a.0.total_cmp(&b.0))
            } else {
                sheet.iter().max_by(|a, b| a.0.total_cmp(&b.0))
            };
            page.margins = chosen.and_then(|(_, margins)| *margins);
        }
    }

    /// Facing pages and bleed, from the document setup. The setup holds
    /// the bleed as four sides; the specimens' are all equal, so which
    /// side is which is unverified and only an equal bleed is read.
    fn setup(&mut self) {
        let Some(setup) = self
            .db
            .objects
            .values()
            .find(|o| o.class == class::DOCUMENT_SETUP)
            .and_then(|o| find(&o.bytes, tag::SETUP))
        else {
            self.skip(schist_i18n::t("design.indd_document_setup").to_owned());
            return;
        };
        self.out.facing = match Reader::at(setup, 58).u32() {
            Some(1) => Some(false),
            Some(2) => Some(true),
            _ => None,
        };
        match Reader::at(setup, 70).f64s::<4>() {
            Some([a, b, c, d])
                if [b, c, d].iter().all(|v| *v == a) && a.is_finite() && a >= 0.0 =>
            {
                self.out.bleed = Some(a)
            }
            _ => self.skip(schist_i18n::t("design.indd_document_setup").to_owned()),
        }
    }

    /// Sections: two strings, the start page (0 for the document's first),
    /// the first number, the number style and whether numbering continues.
    /// The alternate layout's name and, in all but the oldest specimen,
    /// the start page again follow.
    fn sections(&mut self) {
        let db = self.db;
        let mut unread = 0;
        for (&uid, object) in &db.objects {
            if object.class != class::SECTION {
                continue;
            }
            let section = find(&object.bytes, tag::SECTION).and_then(|chunk| {
                let mut reader = Reader::new(chunk);
                let labels = [reader.string()?, reader.string()?];
                let start = reader.u32()?;
                let number = reader.u32()?;
                let style = reader.u32()?;
                let continues = reader.u32()? != 0;
                (labels.iter().all(String::is_empty) && style == ARABIC).then_some(Section {
                    uid,
                    start: (start != 0).then_some(start),
                    number,
                    continues,
                })
            });
            match section {
                Some(section) => self.out.sections.push(section),
                None => unread += 1,
            }
        }
        if unread > 0 {
            self.skip(schist_i18n::tf!("design.indd_sections", count = unread));
        }
    }

    /// Pages are named by their number in their section, as InDesign
    /// names them; a parent's pages by its prefix.
    fn name_pages(&mut self) {
        let first = self
            .out
            .spreads
            .iter()
            .flat_map(|s| &s.pages)
            .map(|p| p.uid)
            .next();
        let starts: HashMap<u32, Section> = self
            .out
            .sections
            .iter()
            .filter_map(|s| Some((s.start.or(first)?, *s)))
            .collect();
        let mut number = 0;
        for page in self.out.spreads.iter_mut().flat_map(|s| s.pages.iter_mut()) {
            number = match starts.get(&page.uid) {
                Some(section) if !section.continues => section.number,
                _ => number + 1,
            };
            page.name = number.to_string();
        }
        for master in &mut self.out.masters {
            let prefix = master
                .master
                .as_ref()
                .map(|m| m.prefix.clone())
                .unwrap_or_default();
            for page in &mut master.pages {
                page.name = prefix.clone();
            }
        }
    }

    fn summarize(&mut self) {
        if self.guides > 0 {
            let message = schist_i18n::tf!("design.indd_guides", count = self.guides);
            self.skip(message);
        }
        if self.curves > 0 {
            let message = schist_i18n::tf!("design.indd_paths_unread", count = self.curves);
            self.skip(message);
        }
        if self.contents > 0 {
            let message = schist_i18n::tf!("design.indd_contents", count = self.contents);
            self.skip(message);
        }
        for (class, count) in std::mem::take(&mut self.unknown) {
            let message = schist_i18n::tf!(
                "design.indd_item_unread",
                class = format!("{class:#x}"),
                count = count
            );
            self.skip(message);
        }
        // Read nowhere in the subset, whatever the document holds.
        self.skip(schist_i18n::t("design.indd_formatting").to_owned());
    }
}

/// A swatch value: the space, the component count, the components as
/// fractions. HSB is converted as IDML export does, to RGB.
fn color(value: &[u8]) -> Option<ColorValue> {
    let mut reader = Reader::new(value);
    let space = reader.u32()?;
    let count = reader.u16()?;
    match (space, count) {
        (6, 4) => {
            let [c, m, y, k] = reader.f64s::<4>()?;
            Some(ColorValue::Cmyk([
                c * 100.0,
                m * 100.0,
                y * 100.0,
                k * 100.0,
            ]))
        }
        (5, 3) => {
            let [r, g, b] = reader.f64s::<3>()?;
            Some(ColorValue::Rgb([r * 255.0, g * 255.0, b * 255.0]))
        }
        (14, 3) => {
            let [h, s, v] = reader.f64s::<3>()?;
            let [r, g, b] = hsb_to_rgb(h, s, v);
            Some(ColorValue::Rgb([r * 255.0, g * 255.0, b * 255.0]))
        }
        _ => None,
    }
    .filter(|value| match value {
        ColorValue::Cmyk(v) => v.iter().all(|c| c.is_finite()),
        ColorValue::Rgb(v) => v.iter().all(|c| c.is_finite()),
    })
}

/// Hue as a fraction of a turn, saturation and brightness as fractions.
fn hsb_to_rgb(h: f64, s: f64, v: f64) -> [f64; 3] {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match sector as u32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Whether an item is shown; one without the flag is.
fn visible(item: &[u8]) -> bool {
    find(item, tag::VISIBLE).and_then(|c| Reader::new(c).u16()) != Some(0)
}

/// A hierarchy chunk: the spread, the parent and the children.
struct Hierarchy {
    parent: u32,
    children: Vec<u32>,
}

fn hierarchy(chunk: &[u8]) -> Option<Hierarchy> {
    let mut reader = Reader::new(chunk);
    reader.u32()?;
    let parent = reader.u32()?;
    let children = reader.uids()?;
    Some(Hierarchy { parent, children })
}

/// A path chunk: subpaths of points, each closed or open. Only corner
/// points, which have no direction handles, are read; a path with curves
/// is declined rather than flattened.
fn paths(chunk: &[u8]) -> Option<Vec<Path>> {
    let mut reader = Reader::new(chunk);
    let subpaths = reader.u32()?;
    let mut out = Vec::new();
    for _ in 0..subpaths {
        let count = reader.u32()? as usize;
        if count > chunk.len() / 20 {
            return None;
        }
        let mut points = Vec::with_capacity(count);
        for _ in 0..count {
            if reader.u32()? != 2 {
                return None;
            }
            let [x, y] = reader.f64s::<2>()?;
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            points.push([x, y]);
        }
        let open = reader.u16()? != 0;
        out.push(Path { points, open });
    }
    (!out.is_empty() && reader.position() == chunk.len()).then_some(out)
}

/// The fill swatch from an attribute list: entries of an attribute id and
/// a list of typed values.
fn fill(chunk: &[u8]) -> Option<u32> {
    let mut reader = Reader::new(chunk);
    let entries = reader.u32()?;
    for _ in 0..entries {
        let id = reader.u32()?;
        let size = reader.u16()? as usize;
        let body = reader.take(size)?;
        if id != tag::FILL {
            continue;
        }
        let mut values = Reader::new(body);
        for _ in 0..values.u16()? {
            let kind = values.u32()?;
            let length = values.u16()? as usize;
            let value = values.take(length)?;
            if kind == tag::UID_VALUE {
                return Reader::new(value).u32();
            }
        }
    }
    None
}

/// A story's parts: its strands and the objects beside them.
fn story_parts(story: &[u8]) -> Option<Vec<u32>> {
    let mut reader = Reader::new(find(story, tag::STORY_PARTS)?);
    reader.u32()?;
    reader.u16()?;
    let first = reader.u32()?;
    let mut parts = vec![first];
    parts.extend(reader.uids()?);
    Some(parts)
}

/// A strand's blocks: how many characters each holds, and its UID.
fn strand_blocks(chunk: &[u8]) -> Option<Vec<(usize, u32)>> {
    let mut reader = Reader::new(chunk);
    let count = reader.u16()?;
    (0..count)
        .map(|_| Some((reader.u32()? as usize, reader.u32()?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsb_converts_as_idml_export_does() {
        // The specimens' "H=353 S=100 B=85" swatch, which their IDML
        // writes as RGB 216.75 0 25.2875.
        let [r, g, b] = hsb_to_rgb(353.0 / 360.0, 1.0, 0.85);
        assert!((r * 255.0 - 216.75).abs() < 1e-9);
        assert!(g.abs() < 1e-9);
        assert!((b * 255.0 - 25.2875).abs() < 1e-6);
    }

    #[test]
    fn a_curved_path_is_declined() {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&1u32.to_le_bytes());
        chunk.extend_from_slice(&1u32.to_le_bytes());
        chunk.extend_from_slice(&1u32.to_le_bytes());
        chunk.extend_from_slice(&[0u8; 48]);
        chunk.extend_from_slice(&0u16.to_le_bytes());
        assert_eq!(paths(&chunk), None);
    }
}
