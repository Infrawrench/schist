//! The IDML package container: a ZIP read and written to OPC rules.
//!
//! An IDML file is an OPC/UCF package, which is a ZIP with two extra
//! rules that a plain ZIP writer gets wrong:
//!
//! 1. `mimetype` must be the **first** entry, stored **uncompressed**.
//!    That is not a convention: the spec requires a reader to be able to
//!    find the media type by reading the first bytes of the file, before
//!    it has a central directory, so a deflated or relocated `mimetype`
//!    makes the package unreadable to conforming readers.
//! 2. Everything else may be deflated, as normal.
//!
//! So this writer emits `mimetype` stored-first and the rest deflated, and
//! the reader takes its sizes from the central directory rather than the
//! local headers, which is what makes it tolerant of entries written with
//! a trailing data descriptor.
//!
//! Scope is deliberately small: stored and deflated, no encryption, no
//! multi-disk. The sizes are read as 64-bit so a zip64 archive opens, but
//! zip64 is never *written* — a layout document is kilobytes of XML, and
//! an embedded placed image is a link, not a payload.

use std::io::{Read, Write};

use thiserror::Error;

use crate::error::Error as IdmlError;

pub const LOCAL_SIG: u32 = 0x0403_4b50;
pub const CENTRAL_SIG: u32 = 0x0201_4b50;
pub const EOCD_SIG: u32 = 0x0605_4b50;
pub const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
pub const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;

/// Stored, no compression.
pub const METHOD_STORE: u16 = 0;
/// Deflate.
pub const METHOD_DEFLATE: u16 = 8;

/// Bit 0: the entry's name is UTF-8 rather than CP437.
const FLAG_UTF8: u16 = 0x0800;

#[derive(Debug, Error)]
pub enum ContainerError {
    #[error("not a ZIP package: no end-of-central-directory record")]
    NoEndOfCentralDirectory,
    #[error("package is truncated or corrupt: {0}")]
    Truncated(&'static str),
    #[error("entry {name:?} is compressed with unsupported method {method}")]
    UnsupportedMethod { name: String, method: u16 },
    #[error("entry {name:?} fails its CRC-32 check")]
    BadChecksum { name: String },
    #[error("package is encrypted, which an IDML package must not be")]
    Encrypted,
}

/// One entry, as the central directory describes it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub method: u16,
    /// Where the local header starts.
    pub local_offset: u64,
    /// Compressed and uncompressed lengths, from the central directory.
    pub compressed_size: u64,
    pub size: u64,
    pub crc32: u32,
}

/// A read package: every part decompressed into memory.
///
/// IDML parts are XML and the linked images stay outside the file, so the
/// whole package is small. Decompressing eagerly means a caller reads
/// parts by name without threading decompressors through, and a corrupt
/// entry is reported by name rather than surfacing later as a parse error.
#[derive(Debug, Clone, Default)]
pub struct Package {
    parts: Vec<(String, Vec<u8>)>,
}

impl Package {
    /// An empty package.
    pub fn new() -> Package {
        Package { parts: Vec::new() }
    }

    /// Build a package from `(name, bytes)` pairs.
    pub fn from_parts(parts: Vec<(String, Vec<u8>)>) -> Package {
        Package { parts }
    }

    /// A part's bytes, if it is there.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.parts
            .iter()
            .find(|(part, _)| part == name)
            .map(|(_, bytes)| bytes.as_slice())
    }

    /// Every part name, in package order.
    pub fn names(&self) -> Vec<&str> {
        self.parts.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// A part as UTF-8, if it is there and is text.
    pub fn text(&self, name: &str) -> Option<&str> {
        std::str::from_utf8(self.get(name)?).ok()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Add or replace a part.
    pub fn insert(&mut self, name: impl Into<String>, bytes: Vec<u8>) {
        let name = name.into();
        match self.parts.iter_mut().find(|(part, _)| *part == name) {
            Some(slot) => slot.1 = bytes,
            None => self.parts.push((name, bytes)),
        }
    }

    /// Remove a part, returning whether it was there.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.parts.len();
        self.parts.retain(|(part, _)| part != name);
        self.parts.len() != before
    }

    /// The parts, for writing.
    pub fn into_parts(self) -> Vec<(String, Vec<u8>)> {
        self.parts
    }
}

/// Read a package.
pub fn read(bytes: &[u8]) -> Result<Package, ContainerError> {
    let entries = read_directory(bytes)?;
    let mut parts = Vec::with_capacity(entries.len());
    for entry in entries {
        parts.push((entry.name.clone(), read_part(bytes, &entry)?));
    }
    Ok(Package { parts })
}

/// Read a package, reporting failures as codec errors.
pub fn open(bytes: &[u8]) -> Result<Package, IdmlError> {
    read(bytes).map_err(IdmlError::Container)
}

/// The name and compression method of the first entry, without decoding
/// the package.
///
/// An OPC reader is meant to determine a package's media type by reading
/// the first bytes of the file, before it has a central directory, so this
/// is the check a conforming package should pass. It is public because
/// "is this a well-formed OPC package" is a question a caller has before
/// it has decided whether to read the whole thing.
pub fn method_of_first(bytes: &[u8]) -> Option<(String, u16)> {
    let eocd = find_eocd(bytes)?;
    let offset = u32le(bytes, eocd + 16).ok()? as usize;
    let name_len = u16le(bytes, offset + 28).ok()? as usize;
    let method = u16le(bytes, offset + 10).ok()?;
    let name = String::from_utf8_lossy(slice(bytes, offset + 46, name_len).ok()?).into_owned();
    Some((name, method))
}

/// The central directory, in the order it is stored.
fn read_directory(bytes: &[u8]) -> Result<Vec<Entry>, ContainerError> {
    let (offset, count) = end_of_central_directory(bytes)?;
    let mut entries = Vec::with_capacity(count as usize);
    let mut at = offset;
    for _ in 0..count {
        let entry = read_central_entry(bytes, at)?;
        at = entry.next;
        entries.push(entry.entry);
    }
    Ok(entries)
}

/// Where the central directory starts, and how many entries it holds.
///
/// A zip64 archive puts the real values in a zip64 record; the classic
/// record then holds `0xFFFF`/`0xFFFFFFFF` as a placeholder, so those are
/// treated as "look at zip64" rather than as enormous values.
fn end_of_central_directory(bytes: &[u8]) -> Result<(u64, u16), ContainerError> {
    let eocd = find_eocd(bytes).ok_or(ContainerError::NoEndOfCentralDirectory)?;
    let count = u16le(bytes, eocd + 10)?;
    let offset = u32le(bytes, eocd + 16)? as u64;
    if count == u16::MAX || offset == u32::MAX as u64 {
        if let Some((real_offset, real_count)) = zip64_directory(bytes, eocd) {
            return Ok((real_offset, real_count));
        }
    }
    Ok((offset, count))
}

/// The zip64 end-of-central-directory, via its locator just before EOCD.
fn zip64_directory(bytes: &[u8], eocd: usize) -> Option<(u64, u16)> {
    let locator = eocd.checked_sub(20)?;
    if u32le(bytes, locator).ok()? != ZIP64_LOCATOR_SIG {
        return None;
    }
    let record = u64le(bytes, locator + 8).ok()? as usize;
    if u32le(bytes, record).ok()? != ZIP64_EOCD_SIG {
        return None;
    }
    let count = u64le(bytes, record + 32).ok()?;
    let offset = u64le(bytes, record + 48).ok()?;
    Some((offset, count.min(u16::MAX as u64) as u16))
}

/// The last EOCD record in the file.
///
/// The record is at the end, but a trailing comment can follow it, so the
/// scan starts from the end and walks back over the longest comment a ZIP
/// permits. Scanning for the signature rather than trusting a fixed offset
/// is what makes a package with a comment open.
fn find_eocd(bytes: &[u8]) -> Option<usize> {
    /// The largest comment a ZIP allows: 65535 bytes, plus the 22-byte
    /// record, plus the smallest possible leading bytes.
    const MAX_SCAN: usize = 22 + u16::MAX as usize;
    // Saturating, not checked: a small package is shorter than the largest
    // possible comment, and the whole file is then the window.
    let start = bytes.len().saturating_sub(MAX_SCAN);
    let window = &bytes[start..];
    window
        .windows(4)
        .rposition(|sig| u32::from_le_bytes([sig[0], sig[1], sig[2], sig[3]]) == EOCD_SIG)
        .map(|at| start + at)
}

/// One central directory entry, and where the next one starts.
struct CentralEntry {
    entry: Entry,
    next: u64,
}

fn read_central_entry(bytes: &[u8], at: u64) -> Result<CentralEntry, ContainerError> {
    let at = at as usize;
    if u32le(bytes, at)? != CENTRAL_SIG {
        return Err(ContainerError::Truncated("central directory entry"));
    }
    let method = u16le(bytes, at + 10)?;
    let name_len = u16le(bytes, at + 28)? as usize;
    let extra_len = u16le(bytes, at + 30)? as usize;
    let comment_len = u16le(bytes, at + 32)? as usize;
    let mut compressed_size = u32le(bytes, at + 20)? as u64;
    let mut size = u32le(bytes, at + 24)? as u64;
    let mut local_offset = u32le(bytes, at + 42)? as u64;
    let name = String::from_utf8_lossy(slice(bytes, at + 46, name_len)?).into_owned();

    // A zip64 entry keeps its real sizes in the extra field, which starts
    // with a 0x0001 header id.
    let extra = slice(bytes, at + 46 + name_len, extra_len)?;
    if size == u32::MAX as u64
        || compressed_size == u32::MAX as u64
        || local_offset == u32::MAX as u64
    {
        if let Some((real_size, real_compressed, real_offset)) = zip64_extra(extra) {
            size = size.max(real_size);
            compressed_size = compressed_size.max(real_compressed);
            local_offset = local_offset.max(real_offset);
        }
    }

    Ok(CentralEntry {
        entry: Entry {
            name,
            method,
            local_offset,
            compressed_size,
            size,
            crc32: u32le(bytes, at + 16)?,
        },
        next: at as u64 + 46 + name_len as u64 + extra_len as u64 + comment_len as u64,
    })
}

/// A zip64 extended-information extra field, if it is there.
fn zip64_extra(extra: &[u8]) -> Option<(u64, u64, u64)> {
    let mut at = 0;
    while at + 4 <= extra.len() {
        let id = u16::from_le_bytes([extra[at], extra[at + 1]]);
        let len = u16::from_le_bytes([extra[at + 2], extra[at + 3]]) as usize;
        if id == 0x0001 {
            let field = extra.get(at + 4..at + 4 + len)?;
            let mut cursor = 0;
            let mut read = || {
                let value = u64::from_le_bytes(field[cursor..cursor + 8].try_into().ok()?);
                cursor += 8;
                Some(value)
            };
            return Some((read()?, read()?, read()?));
        }
        at += 4 + len;
    }
    None
}

/// One part's bytes, decompressed and checked.
fn read_part(bytes: &[u8], entry: &Entry) -> Result<Vec<u8>, ContainerError> {
    let at = entry.local_offset as usize;
    if u32le(bytes, at)? != LOCAL_SIG {
        return Err(ContainerError::Truncated("local file header"));
    }
    // Flag bit 0 is "encrypted".
    if u16le(bytes, at + 6)? & 0x0001 != 0 {
        return Err(ContainerError::Encrypted);
    }
    let name_len = u16le(bytes, at + 26)? as usize;
    let extra_len = u16le(bytes, at + 28)? as usize;
    // The local header repeats the name and may hold a different extra
    // field from the central directory, so the data starts after the
    // local ones, not the central ones.
    let start = at + 30 + name_len + extra_len;
    let data = slice(bytes, start, entry.compressed_size as usize)?;

    let out = match entry.method {
        METHOD_STORE => data.to_vec(),
        METHOD_DEFLATE => {
            let mut out = Vec::with_capacity(entry.size as usize);
            flate2::read::DeflateDecoder::new(data)
                .read_to_end(&mut out)
                .map_err(|_| ContainerError::Truncated("deflate stream"))?;
            out
        }
        method => {
            return Err(ContainerError::UnsupportedMethod {
                name: entry.name.clone(),
                method,
            })
        }
    };

    // The local sizes are deliberately ignored in favour of the central
    // directory's: a writer that streams an entry sets bit 3 and leaves
    // them zero in the local header, putting the real values in a trailing
    // data descriptor instead.
    let _ = entry.local_offset;
    if crc32fast::hash(&out) != entry.crc32 {
        return Err(ContainerError::BadChecksum {
            name: entry.name.clone(),
        });
    }
    Ok(out)
}

/// Write a package as an OPC container.
///
/// `mimetype` is emitted first and stored, whatever order it was inserted
/// in, because a conforming reader depends on it being readable from the
/// first bytes of the file.
pub fn write(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();

    // `mimetype` first, whatever order the parts arrived in, and stored
    // rather than deflated: see the module docs for why.
    if let Some((name, bytes)) = parts.iter().find(|(name, _)| name == MIMETYPE_PART) {
        write_part(&mut out, &mut directory, name, bytes, METHOD_STORE);
    }
    for (name, bytes) in parts.iter().filter(|(name, _)| name != MIMETYPE_PART) {
        write_part(&mut out, &mut directory, name, bytes, METHOD_DEFLATE);
    }

    let offset = out.len() as u64;
    out.extend_from_slice(&directory);
    write_end_of_central_directory(&mut out, directory, offset, parts.len() as u16);
    out
}

/// The OPC media type for an IDML package.
pub const MIMETYPE_PART: &str = "mimetype";
pub const MIMETYPE: &str = "application/vnd.adobe.indesign-idml-package";

/// Append one part's local header, data, and central directory record.
fn write_part(out: &mut Vec<u8>, directory: &mut Vec<u8>, name: &str, bytes: &[u8], method: u16) {
    let offset = out.len() as u32;
    let crc = crc32fast::hash(bytes);
    let name_bytes = name.as_bytes();

    if method == METHOD_DEFLATE {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(bytes)
            .expect("writing to a Vec cannot fail");
        let compressed = encoder.finish().expect("writing to a Vec cannot fail");
        out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        out.extend_from_slice(&METHOD_DEFLATE.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&compressed);
        write_directory_entry(
            directory,
            name,
            method,
            crc,
            compressed.len() as u64,
            bytes.len() as u64,
            offset as u64,
        );
        return;
    }

    out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
    out.extend_from_slice(&10u16.to_le_bytes());
    out.extend_from_slice(&FLAG_UTF8.to_le_bytes());
    out.extend_from_slice(&METHOD_STORE.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name_bytes);
    out.extend_from_slice(bytes);
    write_directory_entry(
        directory,
        name,
        method,
        crc,
        bytes.len() as u64,
        bytes.len() as u64,
        offset as u64,
    );
}

fn write_directory_entry(
    directory: &mut Vec<u8>,
    name: &str,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    offset: u64,
) {
    let name_bytes = name.as_bytes();
    directory.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
    directory.extend_from_slice(&20u16.to_le_bytes()); // version made by
    directory.extend_from_slice(&20u16.to_le_bytes()); // version needed
    directory.extend_from_slice(&FLAG_UTF8.to_le_bytes());
    directory.extend_from_slice(&method.to_le_bytes());
    directory.extend_from_slice(&0u16.to_le_bytes());
    directory.extend_from_slice(&0u16.to_le_bytes());
    directory.extend_from_slice(&crc.to_le_bytes());
    directory.extend_from_slice(&(compressed as u32).to_le_bytes());
    directory.extend_from_slice(&(size as u32).to_le_bytes());
    directory.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    directory.extend_from_slice(&0u16.to_le_bytes()); // extra
    directory.extend_from_slice(&0u16.to_le_bytes()); // comment
    directory.extend_from_slice(&0u16.to_le_bytes()); // disk
    directory.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
    directory.extend_from_slice(&0u32.to_le_bytes()); // external attrs
    directory.extend_from_slice(&(offset as u32).to_le_bytes());
    directory.extend_from_slice(name_bytes);
}

fn write_end_of_central_directory(out: &mut Vec<u8>, directory: Vec<u8>, offset: u64, count: u16) {
    out.extend_from_slice(&EOCD_SIG.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // this disk
    out.extend_from_slice(&0u16.to_le_bytes()); // directory start disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length
}

// -- little-endian reads, each of which fails rather than panicking on a
// short buffer, because the input is a file and files are short.

fn u16le(bytes: &[u8], at: usize) -> Result<u16, ContainerError> {
    let raw = slice(bytes, at, 2)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32le(bytes: &[u8], at: usize) -> Result<u32, ContainerError> {
    let raw = slice(bytes, at, 4)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn u64le(bytes: &[u8], at: usize) -> Result<u64, ContainerError> {
    let raw = slice(bytes, at, 8)?;
    Ok(u64::from_le_bytes(raw.try_into().expect("8 bytes")))
}

fn slice(bytes: &[u8], at: usize, len: usize) -> Result<&[u8], ContainerError> {
    let end = at
        .checked_add(len)
        .ok_or(ContainerError::Truncated("offset overflow"))?;
    bytes
        .get(at..end)
        .ok_or(ContainerError::Truncated("entry data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> Vec<(String, Vec<u8>)> {
        vec![
            (MIMETYPE_PART.to_string(), MIMETYPE.as_bytes().to_vec()),
            (
                "META-INF/container.xml".to_string(),
                b"<container/>".to_vec(),
            ),
            (
                "designmap.xml".to_string(),
                b"<DesignDocument><Self>doc_1</Self></DesignDocument>".to_vec(),
            ),
        ]
    }

    #[test]
    fn a_package_round_trips() {
        let written = write(&parts());
        let read_back = read(&written).expect("a written package reads");
        assert_eq!(
            read_back.names(),
            vec!["mimetype", "META-INF/container.xml", "designmap.xml"]
        );
        assert_eq!(
            read_back.text("designmap.xml").unwrap(),
            "<DesignDocument><Self>doc_1</Self></DesignDocument>"
        );
    }

    #[test]
    fn mimetype_is_first_and_stored() {
        // The spec requires a reader to find the media type by reading the
        // first bytes of the file, so this is load-bearing, not tidy.
        let written = write(&parts());
        assert_eq!(&written[..4], &LOCAL_SIG.to_le_bytes());
        // The name follows the 30-byte fixed local header.
        let name_len = u16le(&written, 26).unwrap() as usize;
        assert_eq!(&written[30..30 + name_len], b"mimetype");
        // And stored means the payload is right there in the clear.
        let method = u16le(&written, 8).unwrap();
        assert_eq!(method, METHOD_STORE);
        let size = u32le(&written, 22).unwrap() as usize;
        let start = 30 + name_len;
        assert_eq!(&written[start..start + size], MIMETYPE.as_bytes());
    }

    #[test]
    fn mimetype_is_first_even_when_inserted_last() {
        // The writer orders it, not the caller.
        let mut shuffled = parts();
        shuffled.reverse();
        let written = write(&shuffled);
        let read_back = read(&written).unwrap();
        assert_eq!(read_back.names()[0], "mimetype");
    }

    #[test]
    fn other_parts_are_deflated() {
        let written = write(&parts());
        let entries = read_directory(&written).expect("a written package has a directory");
        let methods: Vec<u16> = entries.iter().map(|entry| entry.method).collect();
        assert_eq!(
            methods,
            vec![METHOD_STORE, METHOD_DEFLATE, METHOD_DEFLATE],
            "only mimetype is stored"
        );
        // The directory's sizes describe the compressed form. A part that
        // actually compresses has a compressed size below its own -- note
        // that a *tiny* part goes the other way, since deflate's own
        // overhead exceeds a dozen bytes of XML, which is why this uses
        // data with something to squeeze.
        let compressible = vec![b"<Self>Story_1</Self>".repeat(200), b"a".repeat(4096)];
        let entries = read_directory(&write(
            &compressible
                .into_iter()
                .enumerate()
                .map(|(i, bytes)| (format!("part{i}.bin"), bytes))
                .collect::<Vec<_>>(),
        ))
        .unwrap();
        for entry in &entries {
            assert!(
                entry.compressed_size < entry.size,
                "{} compressed to {} from {}",
                entry.name,
                entry.compressed_size,
                entry.size
            );
        }
    }

    #[test]
    fn a_package_with_a_trailing_comment_still_opens() {
        // A ZIP comment can follow the end record, so the scan walks back
        // over it rather than assuming the record is the last 22 bytes.
        let written = write(&parts());
        let eocd = written.len() - 22;
        let directory_size = u32le(&written, eocd + 12).unwrap();
        let directory_offset = u32le(&written, eocd + 16).unwrap();
        let comment = b"made by schist";

        let mut with_comment = written[..eocd].to_vec();
        with_comment.extend_from_slice(&EOCD_SIG.to_le_bytes());
        with_comment.extend_from_slice(&0u16.to_le_bytes());
        with_comment.extend_from_slice(&0u16.to_le_bytes());
        with_comment.extend_from_slice(&3u16.to_le_bytes());
        with_comment.extend_from_slice(&3u16.to_le_bytes());
        with_comment.extend_from_slice(&directory_size.to_le_bytes());
        with_comment.extend_from_slice(&directory_offset.to_le_bytes());
        with_comment.extend_from_slice(&(comment.len() as u16).to_le_bytes());
        with_comment.extend_from_slice(comment);

        assert_eq!(
            read(&with_comment)
                .expect("a commented package opens")
                .len(),
            3
        );
    }

    #[test]
    fn a_corrupt_part_is_reported_by_name() {
        // A bad CRC must be caught here, not turned into a confusing XML
        // parse error three layers up.
        let written = write(&parts());
        // The stored mimetype payload starts right after its local header,
        // so a byte in it is guaranteed to be inside a part rather than in
        // the directory.
        let name_len = u16le(&written, 26).unwrap() as usize;
        let payload = 30 + name_len;
        let mut corrupt = written.clone();
        corrupt[payload] ^= 0xFF;
        let error = read(&corrupt).expect_err("a flipped byte is a bad checksum");
        assert!(
            matches!(error, ContainerError::BadChecksum { ref name } if name == MIMETYPE_PART),
            "{error:?}"
        );
    }

    #[test]
    fn a_file_that_is_not_a_package_is_rejected() {
        let error = read(b"this is not a zip file").expect_err("not a package");
        assert!(matches!(error, ContainerError::NoEndOfCentralDirectory));
    }

    #[test]
    fn a_truncated_package_is_rejected_rather_than_panicking() {
        let written = write(&parts());
        for cut in [1, 10, 30, 64, written.len() / 2] {
            let _ = read(&written[..cut.min(written.len())]);
        }
    }

    #[test]
    fn an_unsupported_compression_is_named() {
        // Method 12 is bzip2. A package using it is a real ZIP, and
        // saying so is better than garbage.
        let mut written = write(&parts());
        let dir = find_eocd(&written).unwrap();
        let offset = u32le(&written, dir + 16).unwrap() as usize;
        // The first central entry is the stored mimetype; make the second
        // claim bzip2.
        let first_name = u16le(&written, offset + 28).unwrap() as u64;
        let first_extra = u16le(&written, offset + 30).unwrap() as u64;
        let first_comment = u16le(&written, offset + 32).unwrap() as u64;
        let second = offset as u64 + 46 + first_name + first_extra + first_comment;
        written[second as usize + 10..second as usize + 12].copy_from_slice(&12u16.to_le_bytes());
        let error = read(&written).expect_err("bzip2 is not supported");
        assert!(
            matches!(error, ContainerError::UnsupportedMethod { method: 12, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_package_helpers_manipulate_parts() {
        let mut package = Package::new();
        assert!(package.is_empty());
        package.insert("a", b"1".to_vec());
        package.insert("a", b"2".to_vec());
        assert_eq!(package.get("a").unwrap(), b"2", "insert replaces");
        assert_eq!(package.len(), 1);
        assert!(package.remove("a"));
        assert!(!package.remove("a"));
        assert!(package.get("a").is_none());
    }

    #[test]
    fn binary_parts_survive() {
        // Parts are not all text: a thumbnail or a swatch image can be
        // stored verbatim.
        let blob: Vec<u8> = (0..=255u8).collect();
        let package = Package::from_parts(vec![("blob.bin".into(), blob.clone())]);
        let written = write(&package.clone().into_parts());
        let read_back = read(&written).unwrap();
        assert_eq!(read_back.get("blob.bin").unwrap(), blob.as_slice());
    }
}
