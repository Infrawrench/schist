//! Perceptual photo review. Originals are fingerprinted and never modified.
//!
//! Groups are anchored to a representative, not transitive chains. A BK tree
//! indexes 64-bit difference hashes; an 8×8 RGB comparison and aspect ratio
//! reject hash collisions (notably flat fields and unrelated gradients).
use image::{imageops::FilterType, RgbImage};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub bytes: u64,
    pub seconds: u64,
    pub nanos: u32,
}
impl Stamp {
    pub fn read(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let time = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some(Self {
            bytes: meta.len(),
            seconds: time.as_secs(),
            nanos: time.subsec_nanos(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signature {
    pub hash: u64,
    rgb: Vec<u8>,
    aspect: f32,
}
impl Signature {
    pub fn of(image: &RgbImage) -> Option<Self> {
        if image.width() == 0 || image.height() == 0 {
            return None;
        }
        let small = image::imageops::resize(image, 9, 8, FilterType::Triangle);
        let luma = |p: &image::Rgb<u8>| 299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32;
        let mut hash = 0;
        for y in 0..8 {
            for x in 0..8 {
                if luma(small.get_pixel(x, y)) > luma(small.get_pixel(x + 1, y)) {
                    hash |= 1 << (y * 8 + x);
                }
            }
        }
        Some(Self {
            hash,
            rgb: image::imageops::resize(image, 8, 8, FilterType::Triangle).into_raw(),
            aspect: image.width() as f32 / image.height() as f32,
        })
    }
    pub fn distance(&self, other: &Self) -> u32 {
        (self.hash ^ other.hash).count_ones()
    }
    fn matches(&self, other: &Self, threshold: u32) -> bool {
        self.rgb.len() == 192
            && other.rgb.len() == 192
            && (self.aspect / other.aspect - 1.0).abs() <= 0.03
            && self.distance(other) <= threshold.min(10)
            && self
                .rgb
                .iter()
                .zip(&other.rgb)
                .map(|(a, b)| a.abs_diff(*b) as u32)
                .sum::<u32>()
                <= 192 * (12 + threshold.min(10))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Photo {
    pub path: PathBuf,
    pub stamp: Stamp,
    pub signature: Option<Signature>,
    pub captured: Option<i64>,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Cache {
    version: u32,
    photos: BTreeMap<PathBuf, Photo>,
}
impl Cache {
    pub fn load(path: &Path) -> Self {
        read_bounded(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Self>(&b).ok())
            .filter(|c| c.version == 1 && c.photos.len() <= 10_000)
            .unwrap_or_default()
    }
    pub fn save(&mut self, path: &Path) -> std::io::Result<()> {
        self.version = 1;
        save_json(path, self)
    }
    /// Decode at most once per source stamp; failures are retried next scan.
    pub fn scan(
        &mut self,
        paths: &[PathBuf],
        cancel: &AtomicBool,
        progress: &AtomicUsize,
        mut decode: impl FnMut(&Path) -> Option<RgbImage>,
        mut capture: impl FnMut(&Path, &Stamp) -> Option<i64>,
    ) -> Vec<Photo> {
        // Missing originals are discardable cache entries, never user decisions.
        self.photos.retain(|path, _| path.is_file());
        let scope: BTreeSet<_> = paths.iter().take(10_000).collect();
        if self.photos.len() > 10_000 {
            self.photos.retain(|path, _| scope.contains(path));
        }
        let mut result = Vec::new();
        for path in paths.iter().take(10_000) {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if let Some(stamp) = Stamp::read(path) {
                let cached = self
                    .photos
                    .get(path)
                    .filter(|p| p.stamp == stamp && p.signature.is_some());
                let mut photo = cached.cloned().unwrap_or_else(|| {
                    let signature = decode(path).as_ref().and_then(Signature::of);
                    Photo {
                        path: path.clone(),
                        stamp: stamp.clone(),
                        signature,
                        captured: None,
                    }
                });
                // Metadata can change through an XMP sidecar without modifying
                // the original. Reuse pixels, but refresh capture evidence.
                photo.captured = capture(path, &stamp);
                // A camera may still be writing. Never accept a mixed revision.
                if Stamp::read(path).as_ref() == Some(&stamp) {
                    self.photos.insert(path.clone(), photo.clone());
                    result.push(photo);
                }
            }
            progress.fetch_add(1, Ordering::Relaxed);
        }
        if self.photos.len() > 10_000 {
            self.photos.retain(|path, _| scope.contains(path));
        }
        result
    }
}

/// Parse EXIF local civil time without treating filesystem time as capture time.
/// A validated Gregorian calendar supports bursts across month/year boundaries.
pub fn capture_seconds(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() != 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b' '
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let part = |a: usize, z: usize| {
        if !b[a..z].iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(&b[a..z]).ok()?.parse::<i64>().ok()
    };
    let (mut y, m, d, h, min, s) = (
        part(0, 4)?,
        part(5, 7)?,
        part(8, 10)?,
        part(11, 13)?,
        part(14, 16)?,
        part(17, 19)?,
    );
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if y < 1
        || !(1..=days).contains(&d)
        || !(0..24).contains(&h)
        || !(0..60).contains(&min)
        || !(0..60).contains(&s)
    {
        return None;
    }
    y -= i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    Some(
        (era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468) * 86400
            + h * 3600
            + min * 60
            + s,
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Visual,
    Burst,
}
#[derive(Clone, Debug)]
pub struct Group {
    pub photos: Vec<usize>,
}
struct Node {
    photo: usize,
    group: usize,
    children: BTreeMap<u32, usize>,
}

pub fn groups(photos: &[Photo], mode: Mode, threshold: u32, cancel: &AtomicBool) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    match mode {
        Mode::Visual => {
            let mut tree: Vec<Node> = Vec::new();
            for (index, photo) in photos.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Vec::new();
                }
                let Some(sig) = &photo.signature else {
                    continue;
                };
                let mut candidates = if tree.is_empty() { vec![] } else { vec![0] };
                let mut found = None;
                while let Some(at) = candidates.pop() {
                    let node = &tree[at];
                    let other = photos[node.photo].signature.as_ref().unwrap();
                    let distance = sig.distance(other);
                    if sig.matches(other, threshold) {
                        found = Some(node.group);
                        break;
                    }
                    candidates.extend(
                        node.children
                            .range(
                                distance.saturating_sub(threshold.min(10))
                                    ..=distance + threshold.min(10),
                            )
                            .map(|(_, i)| *i),
                    );
                }
                if let Some(group) = found {
                    groups[group].photos.push(index);
                    continue;
                }
                let group = groups.len();
                groups.push(Group {
                    photos: vec![index],
                });
                let fresh = tree.len();
                if !tree.is_empty() {
                    let mut at = 0;
                    loop {
                        let distance =
                            sig.distance(photos[tree[at].photo].signature.as_ref().unwrap());
                        if let Some(next) = tree[at].children.get(&distance) {
                            at = *next;
                        } else {
                            tree[at].children.insert(distance, fresh);
                            break;
                        }
                    }
                }
                tree.push(Node {
                    photo: index,
                    group,
                    children: BTreeMap::new(),
                });
            }
        }
        Mode::Burst => {
            let mut order: Vec<_> = photos
                .iter()
                .enumerate()
                .filter_map(|(i, p)| p.captured.map(|time| (p.path.parent(), time, i)))
                .collect();
            order.sort();
            let mut previous: Option<(Option<&Path>, i64)> = None;
            let mut start = 0;
            for (parent, time, index) in order {
                if cancel.load(Ordering::Relaxed) {
                    return Vec::new();
                }
                // Same folder, adjacent captures <= 2s, total span <= 10s.
                if previous.is_some_and(|(p, t)| p == parent && time - t <= 2 && time - start <= 10)
                {
                    groups.last_mut().unwrap().photos.push(index);
                } else {
                    groups.push(Group {
                        photos: vec![index],
                    });
                    start = time;
                }
                previous = Some((parent, time));
            }
        }
    }
    groups.retain(|g| g.photos.len() > 1);
    groups
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Choice {
    Keep,
    Reject,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Decisions {
    records: BTreeMap<PathBuf, (Stamp, Choice)>,
}
impl Decisions {
    pub fn load(path: &Path) -> std::io::Result<Self> {
        match read_bounded(path) {
            Ok(b) => serde_json::from_slice(&b)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        save_json(path, self)
    }
    pub fn get(&self, photo: &Photo) -> Option<Choice> {
        self.records
            .get(&photo.path)
            .filter(|(stamp, _)| *stamp == photo.stamp)
            .map(|(_, choice)| *choice)
    }
    /// The caller supplies the displayed group. Reject cannot remove its last
    /// undecided/kept candidate; changes outside that group and stale files fail.
    pub fn set(
        &mut self,
        photos: &[Photo],
        group: &Group,
        index: usize,
        choice: Option<Choice>,
    ) -> bool {
        if !group.photos.contains(&index) {
            return false;
        }
        let Some(photo) = photos.get(index) else {
            return false;
        };
        if Stamp::read(&photo.path).as_ref() != Some(&photo.stamp) {
            return false;
        }
        if choice == Some(Choice::Reject)
            && !group.photos.iter().any(|i| {
                *i != index
                    && photos.get(*i).is_some_and(|p| {
                        self.get(p) != Some(Choice::Reject)
                            && Stamp::read(&p.path).as_ref() == Some(&p.stamp)
                    })
            })
        {
            return false;
        }
        if let Some(choice) = choice {
            self.records
                .insert(photo.path.clone(), (photo.stamp.clone(), choice));
        } else {
            self.records.remove(&photo.path);
        }
        true
    }
}
const MAX_JSON_BYTES: u64 = 32 * 1024 * 1024;
fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "review data exceeds size limit",
        ));
    }
    Ok(bytes)
}
fn save_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing parent"))?;
    std::fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "review data exceeds size limit",
        ));
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "schist-similar-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn file(&self, name: &str) -> PathBuf {
            let p = self.0.join(name);
            std::fs::write(&p, b"photo").unwrap();
            p
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn pattern() -> RgbImage {
        RgbImage::from_fn(180, 120, |x, y| {
            image::Rgb([
                ((x / 15 % 3) * 70 + y / 5) as u8,
                ((x / 7 + y / 9) % 2 * 140) as u8,
                (x + y) as u8,
            ])
        })
    }
    fn photo(path: PathBuf, signature: Option<Signature>, captured: Option<i64>) -> Photo {
        let stamp = Stamp::read(&path).unwrap_or(Stamp {
            bytes: 0,
            seconds: 0,
            nanos: 0,
        });
        Photo {
            path,
            stamp,
            signature,
            captured,
        }
    }
    #[test]
    fn duplicate_resized_and_light_reencoding_match_but_distinct_images_do_not() {
        let image = pattern();
        let original = Signature::of(&image).unwrap();
        let resized = Signature::of(&image::imageops::resize(
            &image,
            90,
            60,
            FilterType::Triangle,
        ))
        .unwrap();
        assert!(original.matches(&original, 0));
        assert!(original.matches(&resized, 6));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 88)
            .encode_image(&image::DynamicImage::ImageRgb8(image.clone()))
            .unwrap();
        let recompressed = image::load_from_memory(&jpeg).unwrap().to_rgb8();
        assert!(original.matches(&Signature::of(&recompressed).unwrap(), 6));
        let mut adjusted = image.clone();
        for pixel in adjusted.pixels_mut() {
            for c in &mut pixel.0 {
                *c = c.saturating_add(2);
            }
        }
        assert!(original.matches(&Signature::of(&adjusted).unwrap(), 6));
        let different = RgbImage::from_pixel(180, 120, image::Rgb([250, 250, 250]));
        assert!(!original.matches(&Signature::of(&different).unwrap(), 10));
        let black = Signature::of(&RgbImage::from_pixel(30, 30, image::Rgb([0, 0, 0]))).unwrap();
        let red = Signature::of(&RgbImage::from_pixel(30, 30, image::Rgb([255, 0, 0]))).unwrap();
        assert_eq!(black.hash, red.hash);
        assert!(
            !black.matches(&red, 10),
            "RGB verification must reject flat-field hash collisions"
        );
        let wide = Signature::of(&image::imageops::resize(
            &image,
            180,
            60,
            FilterType::Triangle,
        ))
        .unwrap();
        assert!(
            !original.matches(&wide, 10),
            "different aspect ratios should not group"
        );
    }
    #[test]
    fn groups_are_anchored_instead_of_chaining_dissimilar_endpoints() {
        let sig = |hash| {
            Some(Signature {
                hash,
                rgb: vec![100; 192],
                aspect: 1.0,
            })
        };
        let photos = [
            photo("a".into(), sig(0), None),
            photo("b".into(), sig(3), None),
            photo("c".into(), sig(15), None),
        ];
        let result = groups(&photos, Mode::Visual, 2, &AtomicBool::new(false));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].photos, vec![0, 1]);
        let result = groups(&photos, Mode::Visual, 6, &AtomicBool::new(false));
        assert_eq!(result[0].photos, vec![0, 1, 2]);
    }
    #[test]
    fn bursts_respect_gap_span_folder_missing_time_and_calendar_boundaries() {
        let time = capture_seconds("2024-12-31 23:59:59").unwrap();
        assert_eq!(capture_seconds("2025-01-01 00:00:00"), Some(time + 1));
        assert!(capture_seconds("2023-02-29 12:00:00").is_none());
        assert!(capture_seconds("2024-02-29 12:00:00").is_some());
        assert!(capture_seconds("2024-01-01 24:00:00").is_none());
        assert!(capture_seconds("2024-01-01").is_none());
        let mut photos: Vec<_> = (0..8)
            .map(|i| photo(format!("folder/{i}").into(), None, Some(time + i * 2)))
            .collect();
        photos.push(photo("folder/missing".into(), None, None));
        photos.push(photo("elsewhere/a".into(), None, Some(time)));
        photos.push(photo("folder/after-gap".into(), None, Some(time + 18)));
        let result = groups(&photos, Mode::Burst, 6, &AtomicBool::new(false));
        assert_eq!(
            result.iter().map(|g| g.photos.len()).collect::<Vec<_>>(),
            [6, 2]
        );
    }
    #[test]
    fn cache_roundtrips_skips_unchanged_and_reloads_rewritten_sources() {
        let temp = Temp::new();
        let path = temp.file("photo.png");
        let mut cache = Cache::default();
        let progress = AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        let first = cache.scan(
            std::slice::from_ref(&path),
            &cancel,
            &progress,
            |_| Some(pattern()),
            |_, _| None,
        );
        assert_eq!(first.len(), 1);
        assert_eq!(progress.load(Ordering::Relaxed), 1);
        let disk = temp.0.join("cache.json");
        cache.save(&disk).unwrap();
        let mut loaded = Cache::load(&disk);
        let refreshed = loaded.scan(
            std::slice::from_ref(&path),
            &cancel,
            &progress,
            |_| panic!("cache hit decoded"),
            |_, _| Some(123),
        );
        assert_eq!(
            refreshed[0].captured,
            Some(123),
            "metadata refresh must run even on a pixel cache hit"
        );
        std::fs::write(&path, b"a changed image").unwrap();
        let mut decodes = 0;
        loaded.scan(
            std::slice::from_ref(&path),
            &cancel,
            &progress,
            |_| {
                decodes += 1;
                Some(pattern())
            },
            |_, _| None,
        );
        assert_eq!(decodes, 1);
        std::fs::remove_file(&path).unwrap();
        loaded.scan(
            &[],
            &cancel,
            &progress,
            |_| panic!("empty scan decoded"),
            |_, _| None,
        );
        assert!(loaded.photos.is_empty());
        std::fs::write(&disk, b"truncated").unwrap();
        assert!(Cache::load(&disk).photos.is_empty());
    }
    #[test]
    fn failed_decodes_retry_and_a_changed_source_is_not_cached() {
        let temp = Temp::new();
        let path = temp.file("photo.png");
        let paths = [path.clone()];
        let mut cache = Cache::default();
        let progress = AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        let first = cache.scan(&paths, &cancel, &progress, |_| None, |_, _| None);
        assert!(first[0].signature.is_none());
        let second = cache.scan(&paths, &cancel, &progress, |_| Some(pattern()), |_, _| None);
        assert!(second[0].signature.is_some());
        std::fs::write(&path, b"new source").unwrap();
        let changed = cache.scan(
            &paths,
            &cancel,
            &progress,
            |_| {
                std::fs::write(&path, b"changed during decode").unwrap();
                Some(pattern())
            },
            |_, _| None,
        );
        assert!(changed.is_empty());
    }
    #[test]
    fn oversized_persistence_is_rejected_without_overwriting_it() {
        let temp = Temp::new();
        let path = temp.0.join("oversized.json");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_JSON_BYTES + 1)
            .unwrap();
        assert!(Cache::load(&path).photos.is_empty());
        assert_eq!(
            Decisions::load(&path).err().unwrap().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::metadata(path).unwrap().len(), MAX_JSON_BYTES + 1);
    }
    #[test]
    fn cancellation_stops_decoding_and_discards_groups() {
        let temp = Temp::new();
        let paths = [temp.file("a"), temp.file("b")];
        let mut cache = Cache::default();
        let progress = AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        let result = cache.scan(
            &paths,
            &cancel,
            &progress,
            |_| {
                cancel.store(true, Ordering::Relaxed);
                Some(pattern())
            },
            |_, _| None,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(progress.load(Ordering::Relaxed), 1);
        assert!(groups(&result, Mode::Visual, 6, &cancel).is_empty());
    }
    #[test]
    fn review_persists_without_touching_files_and_preserves_a_candidate() {
        let temp = Temp::new();
        let photos: Vec<_> = ["a", "b", "outside"]
            .into_iter()
            .map(|p| photo(temp.file(p), None, None))
            .collect();
        let group = Group { photos: vec![0, 1] };
        let mut decisions = Decisions::default();
        assert!(!decisions.set(&photos, &group, 2, Some(Choice::Reject)));
        assert!(decisions.set(&photos, &group, 0, Some(Choice::Keep)));
        assert!(decisions.set(&photos, &group, 1, Some(Choice::Reject)));
        assert!(!decisions.set(&photos, &group, 0, Some(Choice::Reject)));
        let file = temp.0.join("decisions.json");
        decisions.save(&file).unwrap();
        let restored = Decisions::load(&file).unwrap();
        assert_eq!(restored.get(&photos[0]), Some(Choice::Keep));
        assert_eq!(restored.get(&photos[1]), Some(Choice::Reject));
        assert!(decisions.set(&photos, &group, 1, None));
        for p in &photos {
            assert_eq!(std::fs::read(&p.path).unwrap(), b"photo");
        }
        std::fs::write(&photos[0].path, b"changed").unwrap();
        assert!(!decisions.set(&photos, &group, 0, Some(Choice::Reject)));
        let current = photo(photos[0].path.clone(), None, None);
        assert_eq!(
            restored.get(&current),
            None,
            "marks must not transfer to replacement files"
        );
        // A vanished alternate does not make it safe to reject the survivor.
        std::fs::remove_file(&photos[0].path).unwrap();
        assert!(!decisions.set(&photos, &group, 1, Some(Choice::Reject)));
        std::fs::write(&file, b"bad JSON").unwrap();
        assert!(
            Decisions::load(&file).is_err(),
            "bad decisions must not be silently overwritten"
        );
    }
}
