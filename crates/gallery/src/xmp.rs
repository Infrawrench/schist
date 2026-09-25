//! Portable XMP sidecars. Original image bytes are never modified.
//!
//! Only explicitly edited properties are patched; unrelated XML (including
//! namespaces, processing instructions, comments and application settings)
//! survives byte-for-byte. The public XMP Dublin Core and EXIF schemas are the
//! format reference; no Adobe SDK or headers are used.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, NaiveDateTime};
use roxmltree::{Document, Node};
use std::{
    fs,
    io::{Read, Write},
    ops::Range,
    path::{Path, PathBuf},
};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const EXIF: &str = "http://ns.adobe.com/exif/1.0/";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const MAX_PACKET: u64 = 8 * 1024 * 1024;
const EMPTY: &str = "<?xpacket begin='\u{feff}' id='W5M0MpCehiHzreSzNTczkc9d'?>\n<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'></rdf:RDF></x:xmpmeta>\n<?xpacket end='w'?>\n";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub keywords: Vec<String>,
    pub caption: String,
    pub copyright: String,
    /// Outer option distinguishes absence from an explicit cleared property.
    pub taken: Option<Option<String>>,
    pub gps: Option<Option<(f64, f64)>>,
}

impl Metadata {
    pub fn search_text(&self) -> String {
        format!(
            "{}\n{}\n{}",
            self.keywords.join("\n"),
            self.caption,
            self.copyright
        )
        .to_lowercase()
    }
}

/// None leaves a property unchanged; Some(empty) clears it. A time offset is
/// applied to each photo's own effective capture time, preserving its timezone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Patch {
    pub keywords: Option<Vec<String>>,
    pub caption: Option<String>,
    pub copyright: Option<String>,
    pub taken: Option<String>,
    pub offset_seconds: Option<i64>,
    pub gps: Option<Option<(f64, f64)>>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.keywords.is_none()
            && self.caption.is_none()
            && self.copyright.is_none()
            && self.taken.is_none()
            && self.offset_seconds.is_none()
            && self.gps.is_none()
    }

    pub fn validate(&self) -> Result<()> {
        if self.taken.is_some() && self.offset_seconds.is_some() {
            bail!("capture time and offset are mutually exclusive");
        }
        if let Some(value) = &self.taken {
            if !value.is_empty() {
                normalize_time(value)?;
            }
        }
        if let Some(Some((lat, lon))) = self.gps {
            if !lat.is_finite()
                || !lon.is_finite()
                || !(-90.0..=90.0).contains(&lat)
                || !(-180.0..=180.0).contains(&lon)
            {
                bail!("invalid GPS coordinates");
            }
        }
        for s in self
            .keywords
            .iter()
            .flatten()
            .chain(self.caption.iter())
            .chain(self.copyright.iter())
        {
            if s.chars().any(|c| !matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')) {
                bail!("invalid XML character");
            }
        }
        Ok(())
    }
}

/// Exact-name sidecars are unambiguous (photo.jpg.xmp). Read the conventional
/// stem.xmp as well, but refuse it if multiple image originals share the stem.
/// New writes use stem.xmp when unique for common RAW-tool interoperability.
pub fn sidecar_path(photo: &Path) -> Result<PathBuf> {
    let exact = exact_sidecar(photo)?;
    if exists(&exact)? {
        return Ok(exact);
    }
    let stem = photo.with_extension("xmp");
    let shared = shared_stem(photo)?;
    if exists(&stem)? && shared {
        bail!("ambiguous shared XMP sidecar: {}", stem.display());
    }
    Ok(if shared { exact } else { stem })
}

fn exact_sidecar(photo: &Path) -> Result<PathBuf> {
    let mut name = photo
        .file_name()
        .ok_or_else(|| anyhow!("missing photo filename"))?
        .to_os_string();
    name.push(".xmp");
    Ok(photo.with_file_name(name))
}

fn containing_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn shared_stem(photo: &Path) -> Result<bool> {
    let dir = containing_dir(photo);
    for item in fs::read_dir(dir)? {
        let path = item?.path();
        if path.file_name() == photo.file_name() || path.file_stem() != photo.file_stem() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "xmp" | "txt" | "json" | "xml" | "lock") && path.is_file() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn exists(path: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

fn read_packet(path: &Path) -> Result<Option<String>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !meta.file_type().is_file() || meta.len() > MAX_PACKET {
        bail!("unsafe or oversized XMP sidecar");
    }
    let mut text = String::new();
    fs::File::open(path)?
        .take(MAX_PACKET + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_PACKET {
        bail!("oversized XMP sidecar");
    }
    Ok(Some(text))
}

pub fn read(photo: &Path) -> Result<Metadata> {
    let capture = crate::variants::capture(photo);
    let photo = capture.as_path();
    // The common case requires two stats, never a directory scan per photo.
    if !exists(&exact_sidecar(photo)?)? && !exists(&photo.with_extension("xmp"))? {
        return Ok(Metadata::default());
    }
    match read_packet(&sidecar_path(photo)?)? {
        Some(text) => parse(&text),
        None => Ok(Metadata::default()),
    }
}

/// Explicit rights override, including an empty value (which clears original EXIF copyright).
pub fn copyright(photo: &Path) -> Result<Option<String>> {
    let capture = crate::capture_original(photo);
    let photo = capture.as_path();
    if !exists(&exact_sidecar(photo)?)? && !exists(&photo.with_extension("xmp"))? {
        return Ok(None);
    }
    let Some(text) = read_packet(&sidecar_path(photo)?)? else {
        return Ok(None);
    };
    copyright_packet(&text)
}

/// Read rights while distinguishing absent properties from explicit empty rights.
pub fn copyright_packet(text: &str) -> Result<Option<String>> {
    parse(text)?;
    let doc = Document::parse(text)?;
    Ok(scalar(&doc, DC, "rights"))
}

fn descriptions<'a, 'i>(doc: &'a Document<'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    let root = doc.descendants().find(|n| n.has_tag_name((RDF, "RDF")));
    doc.descendants()
        .filter(move |n| n.has_tag_name((RDF, "Description")) && n.parent() == root)
        .filter(|n| n.attribute((RDF, "about")).is_none_or(str::is_empty))
}

fn scalar(doc: &Document<'_>, ns: &str, name: &str) -> Option<String> {
    descriptions(doc).find_map(|d| {
        d.attribute((ns, name)).map(str::to_string).or_else(|| {
            d.children().find(|n| n.has_tag_name((ns, name))).map(|n| {
                let alt = n.children().find(|n| n.has_tag_name((RDF, "Alt")));
                if let Some(alt) = alt {
                    let preferred = alt
                        .children()
                        .find(|li| li.attribute((XML, "lang")) == Some("x-default"))
                        .or_else(|| alt.children().find(|li| li.has_tag_name((RDF, "li"))));
                    preferred.and_then(|li| li.text()).unwrap_or("").to_string()
                } else {
                    n.text().unwrap_or("").to_string()
                }
            })
        })
    })
}

pub fn parse(text: &str) -> Result<Metadata> {
    let doc = Document::parse(text)?; // DTD/entity expansion is disabled by default.
    if !doc.descendants().any(|n| n.has_tag_name((RDF, "RDF"))) {
        bail!("XMP packet has no RDF root");
    }
    for (ns, name) in [
        (DC, "subject"),
        (DC, "description"),
        (DC, "rights"),
        (EXIF, "DateTimeOriginal"),
        (EXIF, "GPSLatitude"),
        (EXIF, "GPSLongitude"),
    ] {
        let count: usize = descriptions(&doc)
            .map(|d| {
                usize::from(d.attribute((ns, name)).is_some())
                    + d.children().filter(|n| n.has_tag_name((ns, name))).count()
            })
            .sum();
        if count > 1 {
            bail!("duplicate XMP property: {name}");
        }
    }
    let keywords = descriptions(&doc)
        .flat_map(|d| d.children())
        .filter(|n| n.has_tag_name((DC, "subject")))
        .flat_map(|n| n.descendants())
        .filter(|n| n.has_tag_name((RDF, "li")))
        .filter_map(|n| n.text().map(str::to_string))
        .collect();
    let taken = scalar(&doc, EXIF, "DateTimeOriginal")
        .map(|s| {
            if s.trim().is_empty() {
                Ok(None)
            } else {
                normalize_time(&s).map(Some)
            }
        })
        .transpose()?;
    let lat = scalar(&doc, EXIF, "GPSLatitude");
    let lon = scalar(&doc, EXIF, "GPSLongitude");
    let gps = match (lat, lon) {
        (None, None) => None,
        (Some(a), Some(b)) if a.is_empty() && b.is_empty() => Some(None),
        (Some(a), Some(b)) => Some(Some((
            parse_coordinate(&a, true)?,
            parse_coordinate(&b, false)?,
        ))),
        _ => bail!("incomplete XMP GPS coordinates"),
    };
    Ok(Metadata {
        keywords,
        caption: scalar(&doc, DC, "description").unwrap_or_default(),
        copyright: scalar(&doc, DC, "rights").unwrap_or_default(),
        taken,
        gps,
    })
}

/// XMP GPS values: degrees,minutesH (fractional minutes) or degrees,minutes,secondsH.
fn parse_coordinate(value: &str, latitude: bool) -> Result<f64> {
    let value = value.trim();
    let direction = value
        .chars()
        .last()
        .ok_or_else(|| anyhow!("empty coordinate"))?;
    if !(if latitude { "NS" } else { "EW" }).contains(direction) {
        bail!("invalid GPS hemisphere");
    }
    let parts = value[..value.len() - 1]
        .split(',')
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if !(2..=3).contains(&parts.len())
        || parts.iter().any(|p| !p.is_finite() || *p < 0.0)
        || parts[1] >= 60.0
        || parts.get(2).is_some_and(|p| *p >= 60.0)
    {
        bail!("invalid GPS angle");
    }
    let degrees = parts[0] + parts[1] / 60.0 + parts.get(2).copied().unwrap_or(0.0) / 3600.0;
    if degrees > if latitude { 90.0 } else { 180.0 } {
        bail!("GPS angle outside range");
    }
    Ok(degrees
        * if matches!(direction, 'S' | 'W') {
            -1.0
        } else {
            1.0
        })
}

fn coordinate(value: f64, latitude: bool) -> String {
    // Avoid rounded minutes becoming 60 at degree boundaries.
    let angle = (value.abs() * 60.0 * 1e8).round() / 1e8;
    let degrees = (angle / 60.0).floor();
    let minutes = angle - degrees * 60.0;
    let hemisphere = match (latitude, value < 0.0) {
        (true, false) => 'N',
        (true, true) => 'S',
        (false, false) => 'E',
        (false, true) => 'W',
    };
    format!("{degrees:.0},{minutes:.8}{hemisphere}")
}

pub fn normalize_time(value: &str) -> Result<String> {
    shift_time(value, 0)
}

pub fn shift_time(value: &str, seconds: i64) -> Result<String> {
    let value = value.trim().replace(' ', "T");
    let duration =
        Duration::try_seconds(seconds).ok_or_else(|| anyhow!("capture offset outside range"))?;
    if let Ok(time) = DateTime::parse_from_rfc3339(&value) {
        let shifted = time
            .checked_add_signed(duration)
            .ok_or_else(|| anyhow!("capture time outside range"))?;
        if !(1..=9999).contains(&chrono::Datelike::year(&shifted)) {
            bail!("capture time outside range");
        }
        return Ok(shifted.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true));
    }
    let time = NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S%.f")?;
    let shifted = time
        .checked_add_signed(duration)
        .ok_or_else(|| anyhow!("capture time outside range"))?;
    if !(1..=9999).contains(&chrono::Datelike::year(&shifted)) {
        bail!("capture time outside range");
    }
    Ok(shifted.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\r', "&#13;")
}

/// Replace one property by namespace URI, not by the arbitrary prefix.
fn patch_property(
    text: &str,
    ns: &str,
    name: &str,
    value: &str,
    language_alt: bool,
) -> Result<String> {
    let doc = Document::parse(text)?;
    let root = doc
        .descendants()
        .find(|n| n.has_tag_name((RDF, "RDF")))
        .ok_or_else(|| anyhow!("missing RDF root"))?;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    // Language alternatives are independent metadata. Patch the default item
    // in place to preserve translated values, qualifiers and namespace scope.
    if language_alt {
        let props: Vec<_> = descriptions(&doc)
            .flat_map(|d| d.children())
            .filter(|n| n.has_tag_name((ns, name)))
            .collect();
        if props.len() == 1 {
            if let Some(alt) = props[0].children().find(|n| n.has_tag_name((RDF, "Alt"))) {
                let defaults: Vec<_> = alt
                    .children()
                    .filter(|li| {
                        li.has_tag_name((RDF, "li"))
                            && li.attribute((XML, "lang")) == Some("x-default")
                    })
                    .collect();
                if defaults.len() > 1 {
                    bail!("duplicate XMP language alternative");
                }
                let (range, replacement) = if let Some(li) = defaults.first() {
                    let raw = &text[li.range()];
                    if raw.ends_with("/>") {
                        let qname = raw[1..]
                            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
                            .next()
                            .unwrap();
                        (
                            li.range().end - 2..li.range().end,
                            format!(">{}</{qname}>", escape(value)),
                        )
                    } else {
                        (
                            li.range().start + raw.find('>').unwrap() + 1
                                ..li.range().start + raw.rfind("</").unwrap(),
                            escape(value),
                        )
                    }
                } else {
                    let raw = &text[alt.range()];
                    let item = format!(
                        "<rdf:li xmlns:rdf=\"{RDF}\" xml:lang=\"x-default\">{}</rdf:li>",
                        escape(value)
                    );
                    if raw.ends_with("/>") {
                        let qname = raw[1..]
                            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
                            .next()
                            .unwrap();
                        (
                            alt.range().end - 2..alt.range().end,
                            format!(">{item}</{qname}>"),
                        )
                    } else {
                        let at = alt.range().start + raw.rfind("</").unwrap();
                        (at..at, item)
                    }
                };
                let mut result = text.to_string();
                result.replace_range(range, &replacement);
                Document::parse(&result)?;
                return Ok(result);
            }
        }
        if props.len() > 1 {
            bail!("duplicate XMP language property");
        }
    }
    for d in descriptions(&doc) {
        for a in d
            .attributes()
            .filter(|a| a.namespace() == Some(ns) && a.name() == name)
        {
            edits.push((a.range(), String::new()));
        }
        for n in d.children().filter(|n| n.has_tag_name((ns, name))) {
            edits.push((n.range(), String::new()));
        }
    }
    let body = if language_alt {
        format!(
            "<rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt>",
            escape(value)
        )
    } else {
        value.to_string()
    };
    let insertion = format!("<rdf:Description xmlns:rdf=\"{RDF}\" rdf:about=\"\" xmlns:property=\"{ns}\"><property:{name}>{body}</property:{name}></rdf:Description>");
    let range = root.range();
    let root_text = &text[range.clone()];
    if root_text.ends_with("/>") {
        let qname = root_text[1..]
            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .next()
            .unwrap();
        edits.push((range.end - 2..range.end, format!(">{insertion}</{qname}>")));
    } else {
        let at = range.start
            + root_text
                .rfind("</")
                .ok_or_else(|| anyhow!("invalid RDF root"))?;
        edits.push((at..at, insertion));
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut result = text.to_string();
    for (range, replacement) in edits {
        result.replace_range(range, &replacement);
    }
    Document::parse(&result)?;
    Ok(result)
}

pub fn patch_packet(text: &str, patch: &Patch, fallback_time: Option<&str>) -> Result<String> {
    patch.validate()?;
    let before = parse(text)?;
    let mut result = text.to_string();
    if let Some(keywords) = &patch.keywords {
        let bag = format!(
            "<rdf:Bag>{}</rdf:Bag>",
            keywords
                .iter()
                .map(|s| format!("<rdf:li>{}</rdf:li>", escape(s)))
                .collect::<String>()
        );
        result = patch_property(&result, DC, "subject", &bag, false)?;
    }
    for (name, value) in [
        ("description", &patch.caption),
        ("rights", &patch.copyright),
    ] {
        if let Some(value) = value {
            result = patch_property(&result, DC, name, value, true)?;
        }
    }
    let time = if let Some(offset) = patch.offset_seconds {
        let old = before
            .taken
            .as_ref()
            .map(|s| s.as_deref())
            .unwrap_or(fallback_time)
            .ok_or_else(|| anyhow!("photo has no capture time to shift"))?;
        Some(shift_time(old, offset)?)
    } else {
        patch
            .taken
            .as_ref()
            .map(|s| {
                if s.is_empty() {
                    Ok(String::new())
                } else {
                    normalize_time(s)
                }
            })
            .transpose()?
    };
    if let Some(time) = time {
        result = patch_property(&result, EXIF, "DateTimeOriginal", &escape(&time), false)?;
    }
    if let Some(gps) = patch.gps {
        for (name, value) in [
            ("GPSLatitude", gps.map(|(lat, _)| coordinate(lat, true))),
            ("GPSLongitude", gps.map(|(_, lon)| coordinate(lon, false))),
        ] {
            result = patch_property(
                &result,
                EXIF,
                name,
                &escape(&value.unwrap_or_default()),
                false,
            )?;
        }
    }
    parse(&result)?;
    if result.len() as u64 > MAX_PACKET {
        bail!("resulting XMP packet is too large");
    }
    Ok(result)
}

/// Advisory lock shared by edits and gallery moves. Foreign applications may
/// not honor this lock; writes also compare the packet immediately before save.
pub struct SidecarLock {
    path: PathBuf,
    lock_path: PathBuf,
    file: Option<fs::File>,
}

impl SidecarLock {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SidecarLock {
    fn drop(&mut self) {
        // Windows cannot unlink an open file.
        drop(self.file.take());
        let _ = fs::remove_file(&self.lock_path);
    }
}

pub fn lock_sidecar(photo: &Path) -> Result<SidecarLock> {
    let path = sidecar_path(photo)?;
    let mut lock_name = path.as_os_str().to_os_string();
    lock_name.push(".schist-lock");
    let lock_path = PathBuf::from(lock_name);
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .context("XMP sidecar is locked")?;
    Ok(SidecarLock {
        path,
        lock_path,
        file: Some(file),
    })
}

/// Atomic writes use a sibling temporary file, an exclusive Schist lock and a
/// last-moment unchanged check. Keep every prior packet in .schist/metadata.
pub fn write(photo: &Path, patch: &Patch) -> Result<PathBuf> {
    patch.validate()?;
    let lock = lock_sidecar(photo)?;
    let path = lock.path.clone();
    // Recheck after locking: a concurrent gallery move must not leave a
    // newly created sidecar beside an original that has already departed.
    if !fs::metadata(photo)?.is_file() {
        bail!("original is not a file");
    }
    if photo
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xmp"))
        || path == photo
    {
        bail!("an XMP packet cannot be its own original photo");
    }
    if patch.is_empty() {
        return Ok(path);
    }
    let original = read_packet(&path)?;
    let fallback = if patch.offset_seconds.is_some() {
        crate::exif_of(photo)
            .as_ref()
            .and_then(crate::datetime_from)
    } else {
        None
    };
    let result = patch_packet(
        original.as_deref().unwrap_or(EMPTY),
        patch,
        fallback.as_deref(),
    )?;
    if original.as_deref() == Some(&result) {
        return Ok(path);
    }
    let parent = containing_dir(&path);
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(result.as_bytes())?;
    if original.is_some() {
        tmp.as_file()
            .set_permissions(fs::metadata(&path)?.permissions())?;
    }
    tmp.as_file().sync_all()?;
    if read_packet(&path)? != original {
        bail!("XMP sidecar changed during editing");
    }
    if let Some(original) = &original {
        let archive = parent
            .join(".schist/metadata")
            .join(photo.file_name().unwrap());
        fs::create_dir_all(&archive)?;
        let mut backup = tempfile::Builder::new()
            .prefix("previous-")
            .suffix(".xmp")
            .tempfile_in(archive)?;
        backup.write_all(original.as_bytes())?;
        backup.as_file().sync_all()?;
        backup.keep()?;
    }
    if original.is_none() {
        tmp.persist_noclobber(&path)?;
    } else {
        tmp.persist(&path)?;
    }
    Ok(path)
}

/// Continue after individual failures; caller reports every failed path.
pub fn write_batch(photos: &[PathBuf], patch: &Patch) -> Vec<(PathBuf, Result<PathBuf>)> {
    photos
        .iter()
        .map(|p| (p.clone(), write(p, patch)))
        .collect()
}

/// Integrate overrides without poisoning the original-EXIF cache. Empty values
/// explicitly suppress EXIF; removing a sidecar restores the camera values.
pub fn overlay(meta: &mut crate::PhotoMeta, xmp: &Metadata) {
    if let Some(taken) = &xmp.taken {
        meta.taken = taken.as_ref().map(|t| t.replace('T', " "));
    }
    if let Some(gps) = xmp.gps {
        meta.gps = gps;
        meta.place = gps.and_then(|(lat, lon)| crate::nearest_city(lat, lon));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(body: &str) -> String {
        format!("<x:xmpmeta xmlns:x='adobe:ns:meta/'><r:RDF xmlns:r='{RDF}'><r:Description r:about='' xmlns:d='{DC}' xmlns:e='{EXIF}' xmlns:custom='urn:vendor'>{body}</r:Description></r:RDF></x:xmpmeta>")
    }
    #[test]
    fn xmp_roundtrip_preserves_unrelated_bytes_and_translated_alternatives() {
        let unknown = "<!--keep--><custom:settings custom:flag='&amp;'>  opaque <![CDATA[<value>]]> </custom:settings>";
        let french = "<r:li xml:lang='fr' custom:flag='oui'>Original français</r:li>";
        let source = packet(&format!("{unknown}<d:description><r:Alt><r:li xml:lang='x-default'>Old</r:li>{french}</r:Alt></d:description><e:GPSAltitude>12/1</e:GPSAltitude>"));
        let patch = Patch {
            keywords: Some(vec!["a & b".into(), "<雪>".into()]),
            caption: Some("A < B & C\rD".into()),
            copyright: Some("© Camera".into()),
            taken: Some("2024-02-29 23:30:00+02:00".into()),
            gps: Some(Some((-33.75, 151.2))),
            ..Default::default()
        };
        let result = patch_packet(&source, &patch, None).unwrap();
        assert!(result.contains(unknown));
        assert!(result.contains(french));
        assert!(result.contains("<e:GPSAltitude>12/1</e:GPSAltitude>"));
        let meta = parse(&result).unwrap();
        assert_eq!(meta.caption, "A < B & C\rD");
        assert_eq!(meta.keywords, ["a & b", "<雪>"]);
        assert_eq!(meta.copyright, "© Camera");
        assert_eq!(meta.taken, Some(Some("2024-02-29T23:30:00+02:00".into())));
        assert_eq!(meta.gps, Some(Some((-33.75, 151.2))));
    }
    #[test]
    fn xmp_attribute_properties_are_replaced_by_namespace_and_named_subjects_survive() {
        let source = format!("<r:RDF xmlns:r='{RDF}' xmlns:e='{EXIF}'><r:Description r:about='' e:DateTimeOriginal='2024-01-01T12:00:00'/><r:Description r:about='other' e:DateTimeOriginal='2020-01-01T00:00:00'/></r:RDF>");
        let result = patch_packet(
            &source,
            &Patch {
                offset_seconds: Some(3600),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(
            parse(&result).unwrap().taken,
            Some(Some("2024-01-01T13:00:00".into()))
        );
        assert!(result.contains("r:about='other' e:DateTimeOriginal='2020-01-01T00:00:00'"));
    }
    #[test]
    fn xmp_empty_rdf_and_alt_are_supported() {
        let source = format!("<r:RDF xmlns:r='{RDF}'/>");
        assert_eq!(
            parse(
                &patch_packet(
                    &source,
                    &Patch {
                        caption: Some("Hello".into()),
                        ..Default::default()
                    },
                    None
                )
                .unwrap()
            )
            .unwrap()
            .caption,
            "Hello"
        );
        let source = packet("<d:description><r:Alt/></d:description>");
        let result = patch_packet(
            &source,
            &Patch {
                caption: Some("Hello".into()),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(parse(&result).unwrap().caption, "Hello");
    }
    #[test]
    fn xmp_time_validation_offsets_and_gps_cover_boundaries() {
        assert_eq!(
            shift_time("2024-03-01T00:30:00-05:00", -3600).unwrap(),
            "2024-02-29T23:30:00-05:00"
        );
        assert_eq!(
            shift_time("1969-12-31 23:59:59", 1).unwrap(),
            "1970-01-01T00:00:00"
        );
        assert!(normalize_time("2023-02-29T12:00:00").is_err());
        assert!(normalize_time("2024-01-01T24:00:00").is_err());
        assert!(shift_time("9999-12-31T23:59:59", 1).is_err());
        assert!(shift_time("2024-01-01T00:00:00", i64::MAX).is_err());
        assert_eq!(parse_coordinate("33,45,0S", true).unwrap(), -33.75);
        assert!(parse_coordinate("33,60S", true).is_err());
        assert!(parse_coordinate("NaN,0N", true).is_err());
        for (v, lat) in [
            (90.0, true),
            (-180.0, false),
            (0.0, true),
            (89.999999999999, true),
        ] {
            assert!(parse_coordinate(&coordinate(v, lat), lat).is_ok());
        }
        assert!(Patch {
            gps: Some(Some((f64::NAN, 1.0))),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(Patch {
            caption: Some("bad\0".into()),
            ..Default::default()
        }
        .validate()
        .is_err());
    }
    #[test]
    fn xmp_writes_are_atomic_original_safe_and_batch_failures_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        fs::write(&photo, b"original bytes").unwrap();
        let bad = dir.path().join("b.jpg");
        fs::write(&bad, b"also original").unwrap();
        let bad_xmp = bad.with_extension("xmp");
        fs::write(&bad_xmp, b"broken XML").unwrap();
        let patch = Patch {
            caption: Some("first".into()),
            ..Default::default()
        };
        let results = write_batch(&[photo.clone(), bad.clone()], &patch);
        assert!(results[0].1.is_ok());
        assert!(results[1].1.is_err());
        assert_eq!(fs::read(&photo).unwrap(), b"original bytes");
        assert_eq!(fs::read(&bad_xmp).unwrap(), b"broken XML");
        let old = fs::read(photo.with_extension("xmp")).unwrap();
        write(
            &photo,
            &Patch {
                keywords: Some(vec!["new".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        let backup = fs::read_dir(dir.path().join(".schist/metadata/a.jpg"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(fs::read(backup).unwrap(), old);
        assert_eq!(read(&photo).unwrap().caption, "first");
        fs::write(dir.path().join("a.xmp.schist-lock"), b"lock").unwrap();
        assert!(write(&photo, &patch).is_err());
        assert_eq!(fs::read(&photo).unwrap(), b"original bytes");
    }
    #[test]
    fn xmp_ambiguous_stems_never_overwrite_another_photos_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jpg");
        let b = dir.path().join("a.raw");
        fs::write(&a, b"jpg").unwrap();
        fs::write(&b, b"raw").unwrap();
        assert_eq!(sidecar_path(&a).unwrap(), dir.path().join("a.jpg.xmp"));
        fs::write(dir.path().join("a.xmp"), EMPTY).unwrap();
        assert!(sidecar_path(&a).is_err());
        assert!(sidecar_path(&b).is_err());
        fs::write(dir.path().join("a.jpg.xmp"), EMPTY).unwrap();
        assert_eq!(sidecar_path(&a).unwrap(), dir.path().join("a.jpg.xmp"));
    }
    #[test]
    fn xmp_explicit_clear_overrides_exif_and_entity_definitions_are_rejected() {
        let result = patch_packet(
            EMPTY,
            &Patch {
                taken: Some(String::new()),
                gps: Some(None),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let mut meta = crate::PhotoMeta {
            gps: Some((1.0, 2.0)),
            taken: Some("old".into()),
            place: Some("old".into()),
        };
        overlay(&mut meta, &parse(&result).unwrap());
        assert!(meta.gps.is_none() && meta.taken.is_none() && meta.place.is_none());
        assert!(parse("<!DOCTYPE r [<!ENTITY evil SYSTEM 'file:///etc/passwd'>]><r/>").is_err());
    }
    #[test]
    fn xmp_external_edits_and_deletions_overlay_warm_exif_cache() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        std::fs::write(&photo, b"original bytes").unwrap();
        let cache = dir.path().join("thumb.png");
        let cached = "1 2\n2020-01-02 03:04:05\nCamera place";
        std::fs::write(cache.with_extension("meta"), cached).unwrap();
        let options = Some(cache.clone());
        let sidecar = photo.with_extension("xmp");
        let first = patch_packet(
            EMPTY,
            &Patch {
                taken: Some("2024-02-29T12:30:00".into()),
                gps: Some(Some((50.0, 14.0))),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        std::fs::write(&sidecar, first).unwrap();
        let edited = crate::photo_meta(&options, &photo);
        assert_eq!(edited.taken.as_deref(), Some("2024-02-29 12:30:00"));
        assert_eq!(edited.gps, Some((50.0, 14.0)));
        let cleared = patch_packet(
            EMPTY,
            &Patch {
                taken: Some(String::new()),
                gps: Some(None),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        std::fs::write(&sidecar, cleared).unwrap();
        let edited = crate::photo_meta(&options, &photo);
        assert!(edited.taken.is_none() && edited.gps.is_none());
        std::fs::remove_file(&sidecar).unwrap();
        let restored = crate::photo_meta(&options, &photo);
        assert_eq!(restored.taken.as_deref(), Some("2020-01-02 03:04:05"));
        assert_eq!(restored.gps, Some((1.0, 2.0)));
        assert_eq!(restored.place.as_deref(), Some("Camera place"));
        assert_eq!(
            std::fs::read_to_string(cache.with_extension("meta")).unwrap(),
            cached
        );
    }

    #[cfg(unix)]
    #[test]
    fn xmp_symlink_sidecars_and_original_packets_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        std::fs::write(&photo, b"photo").unwrap();
        let target = dir.path().join("target.xmp");
        std::fs::write(&target, EMPTY).unwrap();
        std::os::unix::fs::symlink(&target, photo.with_extension("xmp")).unwrap();
        let patch = Patch {
            caption: Some("new".into()),
            ..Default::default()
        };
        assert!(write(&photo, &patch).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), EMPTY);
        assert!(write(&target, &patch).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), EMPTY);
    }
    #[test]
    #[ignore = "requires the independent exiftool executable"]
    fn xmp_interoperates_with_exiftool() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        std::fs::write(&photo, b"unchanged original").unwrap();
        let sidecar = write(
            &photo,
            &Patch {
                keywords: Some(vec!["travel".into(), "sea & snow".into()]),
                caption: Some("A < B".into()),
                copyright: Some("© Photographer".into()),
                taken: Some("2024-02-29T23:30:00+02:00".into()),
                gps: Some(Some((-33.75, 151.2))),
                ..Default::default()
            },
        )
        .unwrap();
        let inspect = || {
            let output = std::process::Command::new("exiftool")
                .args(["-j", "-n", "-XMP:All"])
                .arg(&sidecar)
                .output()
                .expect("install exiftool to run this oracle");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()[0].clone()
        };
        let fields = inspect();
        assert_eq!(
            fields["Subject"],
            serde_json::json!(["travel", "sea & snow"])
        );
        assert_eq!(fields["Description"], "A < B");
        assert_eq!(fields["Rights"], "© Photographer");
        assert_eq!(fields["GPSLatitude"].as_f64(), Some(-33.75));
        assert_eq!(fields["GPSLongitude"].as_f64(), Some(151.2));
        assert_eq!(fields["DateTimeOriginal"], "2024:02:29 23:30:00+02:00");
        let external = std::process::Command::new("exiftool")
            .args([
                "-overwrite_original",
                "-XMP-dc:Description=Externally edited",
                "-XMP-xmp:Rating=4",
            ])
            .arg(&sidecar)
            .output()
            .unwrap();
        assert!(
            external.status.success(),
            "{}",
            String::from_utf8_lossy(&external.stderr)
        );
        assert_eq!(read(&photo).unwrap().caption, "Externally edited");
        write(
            &photo,
            &Patch {
                copyright: Some("New owner".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let fields = inspect();
        assert_eq!(fields["Rating"], 4);
        assert_eq!(fields["Description"], "Externally edited");
        assert_eq!(fields["Rights"], "New owner");
        assert_eq!(std::fs::read(&photo).unwrap(), b"unchanged original");
    }
}
