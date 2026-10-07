//! The INDD container and the object database inside it.
//!
//! An INDD file is a sequence of 4096-byte pages. Every page ends in a
//! twelve-byte trailer: its type, a field whose meaning depends on the
//! type, and a checksum. The layout below was recovered from the public
//! specimens in `fixtures/indd/` and is documented, with the evidence for
//! each part, in `docs/indd-format.md`.
//!
//! - Pages 0 and 1 are master pages: a sixteen-byte signature, `DOCUMENT`,
//!   a stream byte order, a 64-bit save sequence (the greater one is
//!   current), the database extent in pages, and the roots below.
//! - Allocation and page-map structures come in A/B pairs whose trailers
//!   name each other. A byte in the master page picks the copy.
//! - The page map turns a logical page number into a physical one. Index
//!   and record pages are addressed logically; their trailers repeat the
//!   logical number, which is how a stale copy is told from a live one.
//! - Two B+trees are keyed by UID: one gives each object's class, the
//!   other where its bytes are. The master page records both roots and
//!   how many entries and objects the trees hold.
//! - An object's bytes are either whole data pages or records in slotted
//!   pages. A record that does not fit its page carries a pointer to the
//!   record that continues it.
//!
//! Nothing here is compressed: every object in the specimens reassembles
//! to exactly the length its index entry gives.

use std::collections::{BTreeMap, HashSet};

use crate::error::Error;

/// The database's page size.
pub const PAGE: usize = 4096;
/// Where a page's trailer starts: type, type-specific word, checksum.
const TRAILER: usize = PAGE - 12;
/// The first sixteen bytes of a master page.
pub const SIGNATURE: [u8; 16] = [
    0x06, 0x06, 0xed, 0xf5, 0xd8, 0x1d, 0x46, 0xe5, 0xbd, 0x31, 0xef, 0xe7, 0xfe, 0x74, 0xb7, 0x1d,
];

/// Page types, from the first word of the trailer.
mod kind {
    /// The page-map index: which pages hold the page map.
    pub const MAP_INDEX: u32 = 4;
    /// The page map: logical page number to physical page.
    pub const MAP: u32 = 5;
    /// A B+tree leaf.
    pub const LEAF: u32 = 6;
    /// A B+tree branch.
    pub const BRANCH: u32 = 7;
    /// Records addressed by slot.
    pub const RECORDS: u32 = 9;
}

/// Master page fields.
mod master {
    /// The stream byte order: 1 is little-endian.
    pub const BYTE_ORDER: usize = 24;
    /// Which copy of each A/B pair is current.
    pub const PAIR: usize = 0x25;
    /// The save sequence; the greater master page is current.
    pub const SEQUENCE: usize = 264;
    /// The database extent, in pages.
    pub const EXTENT: usize = 280;
    /// The logical page of the location tree's root.
    pub const LOCATIONS: usize = 0xb7c;
    /// The logical page of the class tree's root.
    pub const CLASSES: usize = 0xb80;
    /// How many entries the location tree holds.
    pub const ENTRIES: usize = 0xb88;
    /// How many objects there are.
    pub const OBJECTS: usize = 0xb90;
}

/// Where page-map entries start, in both the index and the map pages.
const MAP_ENTRIES: usize = 0x80;
/// How many entries one page-map page holds.
const MAP_CAPACITY: usize = (TRAILER - MAP_ENTRIES) / 4;
/// A slotted page's directory header: capacity, then the slot offsets
/// counting down from just below it.
const SLOTS: usize = 0xfdc;
/// The high bit of a record's slot: the record continues elsewhere.
const CONTINUED: u32 = 0x8000;
/// Deeper than any specimen's trees by a wide margin; a guard against a
/// damaged file whose branches point back up.
const MAX_DEPTH: usize = 16;
/// The longest chain of continued records accepted.
const MAX_CONTINUATIONS: usize = 64;

/// One object: its class and its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub class: u32,
    pub bytes: Vec<u8>,
}

/// Every object in the document, by UID.
#[derive(Debug, Default)]
pub struct Database {
    pub objects: BTreeMap<u32, Object>,
}

impl Database {
    pub fn get(&self, uid: u32) -> Option<&Object> {
        self.objects.get(&uid)
    }

    /// An object's bytes, when it is of the given class.
    pub fn of_class(&self, uid: u32, class: u32) -> Option<&[u8]> {
        self.objects
            .get(&uid)
            .filter(|object| object.class == class)
            .map(|object| object.bytes.as_slice())
    }
}

pub(crate) fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn is_master(page: &[u8]) -> bool {
    page.len() >= PAGE && page[..16] == SIGNATURE && &page[16..24] == b"DOCUMENT"
}

/// Whether `bytes` starts like an INDD document.
pub fn probe(bytes: &[u8]) -> bool {
    is_master(bytes)
}

/// Read every object in an INDD document.
pub fn read(bytes: &[u8]) -> Result<Database, Error> {
    let masters: Vec<&[u8]> = (0..2)
        .filter_map(|page| bytes.get(page * PAGE..(page + 1) * PAGE))
        .filter(|page| is_master(page))
        .collect();
    let master = masters
        .into_iter()
        .max_by_key(|page| u64_at(page, master::SEQUENCE).unwrap_or(0))
        .ok_or(Error::NotIndd)?;
    if master[master::BYTE_ORDER] != 1 {
        return Err(Error::BigEndian);
    }
    let extent = u32_at(master, master::EXTENT).unwrap_or(0) as usize;
    if extent < 2 || bytes.len() / PAGE < extent {
        return Err(Error::Truncated);
    }
    let field = |at: usize| u32_at(master, at).unwrap_or(0);
    let expected = Expected {
        locations: field(master::LOCATIONS),
        classes: field(master::CLASSES),
        entries: field(master::ENTRIES) as usize,
        objects: field(master::OBJECTS) as usize,
    };

    let pair = map_index_pair(bytes, extent).ok_or(Error::DamagedPage { page: 0 })?;
    // The master page names the current copy. The other is tried only
    // when the named one does not hold together, and either way the trees
    // must account for every entry and object the master page counts.
    let preferred = if master[master::PAIR] == 0 {
        pair.0
    } else {
        pair.1
    };
    let other = if preferred == pair.0 { pair.1 } else { pair.0 };
    let first = Pages::new(bytes, extent, preferred).and_then(|pages| pages.read(&expected));
    match first {
        Ok(database) => Ok(database),
        Err(error) => {
            log::warn!("INDD page map {preferred} unusable ({error}); trying {other}");
            Pages::new(bytes, extent, other)
                .and_then(|pages| pages.read(&expected))
                .map_err(|_| error)
        }
    }
}

/// What the master page says the trees hold.
struct Expected {
    locations: u32,
    classes: u32,
    entries: usize,
    objects: usize,
}

/// The two copies of the page-map index, which name each other.
fn map_index_pair(bytes: &[u8], extent: usize) -> Option<(usize, usize)> {
    // The pairs follow the master pages; the specimens put them within
    // the first dozen pages.
    (1..extent.min(64)).find_map(|page| {
        let (kind, partner) = trailer(bytes, page)?;
        let partner = partner as usize;
        if kind != kind::MAP_INDEX || partner >= extent || partner == page {
            return None;
        }
        let (partner_kind, back) = trailer(bytes, partner)?;
        (partner_kind == kind::MAP_INDEX && back as usize == page)
            .then_some((page.min(partner), page.max(partner)))
    })
}

fn page(bytes: &[u8], page: usize) -> Option<&[u8]> {
    bytes.get(page * PAGE..(page + 1) * PAGE)
}

fn trailer(bytes: &[u8], number: usize) -> Option<(u32, u32)> {
    let page = page(bytes, number)?;
    Some((u32_at(page, TRAILER)?, u32_at(page, TRAILER + 4)?))
}

/// The database's pages, with the page map read from one copy.
struct Pages<'a> {
    bytes: &'a [u8],
    extent: usize,
    /// Logical page number to physical page number.
    map: Vec<u32>,
}

impl<'a> Pages<'a> {
    fn new(bytes: &'a [u8], extent: usize, index: usize) -> Result<Pages<'a>, Error> {
        let damaged = Error::DamagedPage { page: index };
        let index_page = page(bytes, index).ok_or(Error::DamagedPage { page: index })?;
        let mut map = Vec::new();
        for slot in 0..MAP_CAPACITY {
            let physical = u32_at(index_page, MAP_ENTRIES + 4 * slot).unwrap_or(0) as usize;
            if physical == 0 {
                break;
            }
            if physical >= extent || trailer(bytes, physical).map(|t| t.0) != Some(kind::MAP) {
                return Err(Error::DamagedPage { page: physical });
            }
            let map_page = page(bytes, physical).ok_or(Error::DamagedPage { page: physical })?;
            for entry in 0..MAP_CAPACITY {
                map.push(u32_at(map_page, MAP_ENTRIES + 4 * entry).unwrap_or(0));
            }
        }
        if map.is_empty() {
            return Err(damaged);
        }
        Ok(Pages { bytes, extent, map })
    }

    fn physical(&self, number: usize) -> Result<&'a [u8], Error> {
        if number >= self.extent {
            return Err(Error::DamagedPage { page: number });
        }
        page(self.bytes, number).ok_or(Error::DamagedPage { page: number })
    }

    /// A logically addressed page, checked against its trailer.
    fn logical(&self, ordinal: u32, kinds: &[u32]) -> Result<(u32, &'a [u8]), Error> {
        let number = *self.map.get(ordinal as usize).ok_or(Error::DamagedPage {
            page: ordinal as usize,
        })? as usize;
        let page = self.physical(number)?;
        let (kind, repeated) =
            trailer(self.bytes, number).ok_or(Error::DamagedPage { page: number })?;
        if repeated != ordinal || !kinds.contains(&kind) || number == 0 {
            return Err(Error::DamagedPage { page: number });
        }
        Ok((kind, page))
    }

    /// Every leaf entry under `root`, in key order: four words each.
    fn leaves(&self, root: u32) -> Result<Vec<[u32; 4]>, Error> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.walk(root, 0, &mut seen, &mut out)?;
        Ok(out)
    }

    fn walk(
        &self,
        ordinal: u32,
        depth: usize,
        seen: &mut HashSet<u32>,
        out: &mut Vec<[u32; 4]>,
    ) -> Result<(), Error> {
        if depth > MAX_DEPTH || !seen.insert(ordinal) {
            return Err(Error::DamagedPage {
                page: ordinal as usize,
            });
        }
        let (kind, page) = self.logical(ordinal, &[kind::LEAF, kind::BRANCH])?;
        let count = u32_at(page, 0).unwrap_or(0) as usize;
        let width = if kind == kind::BRANCH { 12 } else { 16 };
        if 4 + count * width > TRAILER {
            return Err(Error::DamagedPage {
                page: ordinal as usize,
            });
        }
        for entry in 0..count {
            let at = 4 + entry * width;
            let word = |i: usize| u32_at(page, at + 4 * i).unwrap_or(0);
            if kind == kind::BRANCH {
                // Child, then the greatest key under it: sequence and UID.
                self.walk(word(0), depth + 1, seen, out)?;
            } else {
                out.push([word(0), word(1), word(2), word(3)]);
            }
        }
        Ok(())
    }

    fn read(&self, expected: &Expected) -> Result<Database, Error> {
        let damaged = Error::DamagedPage { page: 0 };
        // Class leaves: zero, UID, class, zero.
        let classes: BTreeMap<u32, u32> = self
            .leaves(expected.classes)?
            .into_iter()
            .map(|[_, uid, class, _]| (uid, class))
            .collect();
        // Location leaves: segment sequence, UID, slot and length, where.
        let locations = self.leaves(expected.locations)?;
        if locations.len() != expected.entries || classes.len() != expected.objects {
            return Err(damaged);
        }
        let mut segments: BTreeMap<u32, Vec<[u32; 4]>> = BTreeMap::new();
        for entry in locations {
            if !classes.contains_key(&entry[1]) {
                return Err(Error::DamagedObject { uid: entry[1] });
            }
            segments.entry(entry[1]).or_default().push(entry);
        }
        let mut database = Database::default();
        // An object can have a class and no bytes: one of the Proof
        // specimen's does.
        for (&uid, &class) in &classes {
            let mut parts = segments.remove(&uid).unwrap_or_default();
            parts.sort_by_key(|part| part[0]);
            let mut bytes = Vec::new();
            for [_, _, size, place] in parts {
                let (slot, length) = (size >> 16, (size & 0xffff) as usize);
                if slot == 0 {
                    // A whole data page, physically addressed.
                    if length > TRAILER {
                        return Err(Error::DamagedObject { uid });
                    }
                    bytes.extend_from_slice(&self.physical(place as usize)?[..length]);
                } else {
                    self.record(place, slot, length, uid, &mut bytes)?;
                }
            }
            database.objects.insert(uid, Object { class, bytes });
        }
        if database.objects.len() != expected.objects {
            return Err(damaged);
        }
        Ok(database)
    }

    /// `length` bytes of record `slot` on logical page `ordinal`,
    /// following continuations.
    fn record(
        &self,
        mut ordinal: u32,
        mut slot: u32,
        mut length: usize,
        uid: u32,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let damaged = Error::DamagedObject { uid };
        for _ in 0..MAX_CONTINUATIONS {
            let (_, page) = self.logical(ordinal, &[kind::RECORDS])?;
            let capacity = u32_at(page, SLOTS).ok_or(Error::DamagedObject { uid })?;
            if slot == 0 || slot > capacity || SLOTS < 4 * slot as usize {
                return Err(damaged);
            }
            let at = u32_at(page, SLOTS - 4 * slot as usize).unwrap_or(u32::MAX) as usize;
            let head = u32_at(page, at).ok_or(Error::DamagedObject { uid })?;
            let (size, owner) = ((head & 0xffff) as usize, head >> 16);
            if owner & !CONTINUED != slot || at + size > TRAILER {
                return Err(damaged);
            }
            if owner & CONTINUED == 0 {
                let data = page
                    .get(at + 4..at + 4 + length)
                    .filter(|_| length + 4 <= size)
                    .ok_or(Error::DamagedObject { uid })?;
                out.extend_from_slice(data);
                return Ok(());
            }
            // A continued record: the next slot and logical page, then as
            // much of the object as fits here.
            let next_slot = u32_at(page, at + 4).ok_or(Error::DamagedObject { uid })? >> 16;
            let next_page = u32_at(page, at + 8).ok_or(Error::DamagedObject { uid })?;
            let here = size.checked_sub(12).ok_or(Error::DamagedObject { uid })?;
            if here >= length {
                return Err(damaged);
            }
            out.extend_from_slice(&page[at + 12..at + 12 + here]);
            length -= here;
            ordinal = next_page;
            slot = next_slot;
        }
        Err(damaged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_without_the_signature_is_not_indd() {
        assert!(!probe(b"not an indesign document"));
        assert!(matches!(read(&[0u8; PAGE * 3]), Err(Error::NotIndd)));
    }

    #[test]
    fn a_master_page_alone_is_truncated_not_a_panic() {
        let mut bytes = vec![0u8; PAGE];
        bytes[..16].copy_from_slice(&SIGNATURE);
        bytes[16..24].copy_from_slice(b"DOCUMENT");
        bytes[master::BYTE_ORDER] = 1;
        bytes[master::EXTENT..master::EXTENT + 4].copy_from_slice(&600u32.to_le_bytes());
        assert!(probe(&bytes));
        assert!(matches!(read(&bytes), Err(Error::Truncated)));
    }

    #[test]
    fn big_endian_streams_are_declined() {
        let mut bytes = vec![0u8; PAGE * 4];
        bytes[..16].copy_from_slice(&SIGNATURE);
        bytes[16..24].copy_from_slice(b"DOCUMENT");
        bytes[master::BYTE_ORDER] = 2;
        assert!(matches!(read(&bytes), Err(Error::BigEndian)));
    }
}
