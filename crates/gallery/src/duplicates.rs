//! The local duplicate finder: byte-identical files (size, then streamed
//! SHA-256) and near-duplicates (the similar-photo signatures), a suggested
//! keeper per group, and guarded moves to the platform trash.
//!
//! Nothing here deletes a file. A duplicate leaves only through
//! [`trash_checked`], which re-verifies the group immediately beforehand and
//! hands the file to the operating system's trash, never `remove_file`.

use crate::similar::{self, Photo, Stamp};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    io::Read as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub type Sha256Digest = [u8; 32];

pub use crate::index::FileDigest;

#[derive(Debug)]
pub enum HashError {
    Cancelled,
    /// The file grew, shrank or was rewritten while it was being read.
    Changed,
    Io(std::io::Error),
}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashError::Cancelled => f.write_str("cancelled"),
            HashError::Changed => f.write_str("file changed while hashing"),
            HashError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for HashError {}

/// Stream a file through SHA-256 in 1 MiB reads (as cloud uploads do), so
/// size never decides memory use. A concurrent rewrite is detected by
/// comparing the length and modification stamp before and after.
pub fn sha256_file(path: &Path, cancel: &AtomicBool) -> Result<FileDigest, HashError> {
    let before = Stamp::read(path)
        .ok_or_else(|| HashError::Io(std::io::Error::from(std::io::ErrorKind::NotFound)))?;
    let mut file = std::fs::File::open(path).map_err(HashError::Io)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    let mut read = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(HashError::Cancelled);
        }
        let count = match file.read(&mut buffer) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(HashError::Io(e)),
        };
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > before.bytes {
            return Err(HashError::Changed);
        }
        hash.update(&buffer[..count]);
    }
    if read != before.bytes || Stamp::read(path).as_ref() != Some(&before) {
        return Err(HashError::Changed);
    }
    Ok(FileDigest {
        bytes: before.bytes,
        seconds: before.seconds,
        nanos: before.nanos,
        sha256: hash.finalize().into(),
    })
}

pub fn hex(digest: &Sha256Digest) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The outcome of the exact pass.
#[derive(Default, Debug)]
pub struct ExactScan {
    /// Groups of indices into the input, every member byte-identical.
    pub groups: Vec<Vec<usize>>,
    /// Digests computed this pass (cache misses), for the index.
    pub fresh: Vec<(PathBuf, FileDigest)>,
    /// The digest of every file that was hashed or found in the cache.
    pub digests: HashMap<usize, Sha256Digest>,
    pub failed: usize,
    pub hashed: usize,
}

/// How many files the exact pass will need to read or look up: only those
/// sharing their size with another file.
pub fn size_collisions(files: &[(PathBuf, Stamp)]) -> Vec<Vec<usize>> {
    let mut by_size: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, (_, stamp)) in files.iter().enumerate() {
        // Empty files are all "identical" and say nothing about photos.
        if stamp.bytes > 0 {
            by_size.entry(stamp.bytes).or_default().push(i);
        }
    }
    by_size.into_values().filter(|v| v.len() > 1).collect()
}

/// Group byte-identical files. Sizes are compared first; only colliding
/// sizes are hashed, and a cached digest is reused when its stamp matches.
pub fn exact_groups(
    files: &[(PathBuf, Stamp)],
    cached: impl Fn(&Path) -> Option<FileDigest>,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> ExactScan {
    let mut scan = ExactScan::default();
    for bucket in size_collisions(files) {
        let mut by_digest: BTreeMap<Sha256Digest, Vec<usize>> = BTreeMap::new();
        for index in bucket {
            if cancel.load(Ordering::Relaxed) {
                return ExactScan::default();
            }
            let (path, stamp) = &files[index];
            let digest = match cached(path).filter(|d| d.matches(stamp)) {
                Some(d) => Some(d),
                None => match sha256_file(path, cancel) {
                    Ok(d) if d.matches(stamp) => {
                        scan.hashed += 1;
                        scan.fresh.push((path.clone(), d));
                        Some(d)
                    }
                    Ok(_) | Err(HashError::Changed) | Err(HashError::Io(_)) => {
                        scan.failed += 1;
                        None
                    }
                    Err(HashError::Cancelled) => return ExactScan::default(),
                },
            };
            if let Some(d) = digest {
                scan.digests.insert(index, d.sha256);
                by_digest.entry(d.sha256).or_default().push(index);
            }
            progress.fetch_add(1, Ordering::Relaxed);
        }
        scan.groups
            .extend(by_digest.into_values().filter(|g| g.len() > 1));
    }
    scan
}

/// Near-duplicate groups over signed photos. Each byte-identical group takes
/// part through one representative, so a pair of exact copies is reported as
/// exact and does not also appear as a visual match of itself. Indices are
/// into `photos`; a group needs two different contents.
pub fn near_groups(
    photos: &[Photo],
    exact: &[Vec<usize>],
    threshold: u32,
    cancel: &AtomicBool,
) -> Vec<Vec<usize>> {
    let mut hidden = vec![false; photos.len()];
    let mut copies: HashMap<usize, &[usize]> = HashMap::new();
    for group in exact {
        for &i in &group[1..] {
            hidden[i] = true;
        }
        copies.insert(group[0], group);
    }
    let visible: Vec<usize> = (0..photos.len()).filter(|i| !hidden[*i]).collect();
    let subset: Vec<Photo> = visible.iter().map(|&i| photos[i].clone()).collect();
    similar::groups(&subset, similar::Mode::Visual, threshold, cancel)
        .into_iter()
        .map(|g| {
            let mut members = Vec::new();
            for local in g.photos {
                let index = visible[local];
                match copies.get(&index) {
                    Some(group) => members.extend_from_slice(group),
                    None => members.push(index),
                }
            }
            members
        })
        .collect()
}

pub const RAW_EXTENSIONS: &[&str] = &[
    "3fr", "ari", "arw", "bay", "cr2", "cr3", "crw", "dcr", "dng", "erf", "fff", "iiq", "k25",
    "kdc", "mef", "mos", "mrw", "nef", "nrw", "orf", "pef", "raf", "raw", "rw2", "rwl", "sr2",
    "srf", "srw", "x3f",
];

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn is_raw(path: &Path) -> bool {
    RAW_EXTENSIONS.contains(&extension(path).as_str())
}

/// How likely a format is to be the capture rather than an export: camera
/// raws, then the formats cameras and phones write, then lossless rasters,
/// then web re-encodes.
pub fn format_rank(path: &Path) -> u8 {
    match extension(path).as_str() {
        e if RAW_EXTENSIONS.contains(&e) => 4,
        "jpg" | "jpeg" | "heic" | "heif" | "hif" => 3,
        "tif" | "tiff" | "png" | "psd" | "jxl" | "avif" => 2,
        "webp" | "gif" | "bmp" => 1,
        _ => 0,
    }
}

/// What the keep suggestion weighs, per group member.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeepFacts {
    pub path: PathBuf,
    /// Width × height of the original, 0 when unknown.
    pub pixels: u64,
    /// A Schist edit (PSD sidecar or virtual copy) exists.
    pub edited: bool,
    /// A portable XMP sidecar exists.
    pub sidecar: bool,
    /// Capture time when known, else modification time (seconds).
    pub when: i64,
    pub bytes: u64,
}

/// The member to keep: a camera raw over any rendered copy; then the most
/// pixels; then the capture-like format; then one carrying Schist edits or
/// an XMP sidecar; then the earliest; then the largest file; then the path,
/// so the choice is stable.
pub fn suggest_keep(members: &[KeepFacts]) -> usize {
    use std::cmp::Reverse;
    members
        .iter()
        .enumerate()
        .min_by_key(|(_, f)| {
            (
                Reverse(is_raw(&f.path)),
                Reverse(f.pixels),
                Reverse(format_rank(&f.path)),
                Reverse(u8::from(f.edited) * 2 + u8::from(f.sidecar)),
                f.when,
                Reverse(f.bytes),
                f.path.clone(),
            )
        })
        .map_or(0, |(i, _)| i)
}

/// Edits and metadata that belong to one file: they would be orphaned (or
/// silently lost with it) if the file went to the trash.
pub fn attachments(photo: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(psd) = crate::backing_psd(photo).filter(|p| p != photo && p.exists()) {
        found.push(psd);
    }
    if let Some(dir) = crate::variants::directory(photo).filter(|d| d.exists()) {
        found.push(dir);
    }
    let exact = photo.as_os_str().to_owned();
    let mut exact = PathBuf::from(exact);
    exact.as_mut_os_string().push(".xmp");
    for xmp in [exact, photo.with_extension("xmp")] {
        if xmp != photo && xmp.exists() {
            found.push(xmp);
        }
    }
    found
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The keeper and the duplicate are the same file.
    SameFile,
    /// The photo to keep is gone or changed since the scan.
    KeeperChanged,
    /// The duplicate changed since the scan.
    Changed,
    /// An exact duplicate no longer has the keeper's bytes.
    NotIdentical,
    /// It carries edits or an XMP sidecar; flag it instead.
    HasAttachments,
    Trash(String),
}

/// One file to move to the trash, and the evidence it is a duplicate.
#[derive(Clone, Debug)]
pub struct TrashRequest {
    pub victim: PathBuf,
    pub victim_stamp: Stamp,
    pub keeper: PathBuf,
    pub keeper_stamp: Stamp,
    /// Exact groups re-check the bytes of both files.
    pub sha256: Option<Sha256Digest>,
}

/// Re-verify, then trash. `trash` is the platform call ([`system_trash`]),
/// injected so the checks are testable without touching a real trash.
pub fn trash_checked(
    request: &TrashRequest,
    cancel: &AtomicBool,
    trash: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), Refusal> {
    let same = match (
        std::fs::canonicalize(&request.victim),
        std::fs::canonicalize(&request.keeper),
    ) {
        (Ok(a), Ok(b)) => a == b,
        (Err(_), _) => return Err(Refusal::Changed),
        (_, Err(_)) => return Err(Refusal::KeeperChanged),
    };
    if same || request.victim == request.keeper {
        return Err(Refusal::SameFile);
    }
    if Stamp::read(&request.keeper).as_ref() != Some(&request.keeper_stamp) {
        return Err(Refusal::KeeperChanged);
    }
    if Stamp::read(&request.victim).as_ref() != Some(&request.victim_stamp) {
        return Err(Refusal::Changed);
    }
    if !attachments(&request.victim).is_empty() {
        return Err(Refusal::HasAttachments);
    }
    if let Some(expected) = request.sha256 {
        for (path, refusal) in [
            (&request.keeper, Refusal::KeeperChanged),
            (&request.victim, Refusal::NotIdentical),
        ] {
            match sha256_file(path, cancel) {
                Ok(d) if d.sha256 == expected => {}
                _ => return Err(refusal),
            }
        }
    }
    trash(&request.victim).map_err(|e| Refusal::Trash(e.to_string()))
}

/// Move a file to the operating system's trash (Freedesktop trash on Linux
/// and BSD, the Finder's Trash on macOS, the Recycle Bin on Windows). Where
/// there is no trash this fails; it never falls back to deleting.
pub fn system_trash(path: &Path) -> std::io::Result<()> {
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    {
        #[cfg(target_os = "macos")]
        let context = {
            use trash::macos::{DeleteMethod, TrashContextExtMacos as _};
            let mut context = trash::TrashContext::default();
            // NSFileManager needs no Finder automation permission.
            context.set_delete_method(DeleteMethod::NsFileManager);
            context
        };
        #[cfg(not(target_os = "macos"))]
        let context = trash::TrashContext::default();
        context.delete(path).map_err(std::io::Error::other)
    }
    #[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
    {
        let _ = path;
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, RgbImage};

    fn pattern(seed: u32) -> RgbImage {
        RgbImage::from_fn(320, 240, |x, y| {
            let v = (x * (3 + seed) + y * (5 + seed * 2) + (x * y) / (7 + seed)) % 256;
            image::Rgb([
                v as u8,
                ((x + seed * 40) % 256) as u8,
                ((y * 2) % 256) as u8,
            ])
        })
    }

    fn jpeg(image: &RgbImage, quality: u8) -> Vec<u8> {
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, quality)
            .encode_image(image)
            .unwrap();
        out
    }

    fn files(dir: &Path, names: &[&str]) -> Vec<(PathBuf, Stamp)> {
        names
            .iter()
            .map(|n| {
                let p = dir.join(n);
                let s = Stamp::read(&p).unwrap();
                (p, s)
            })
            .collect()
    }

    #[test]
    fn streamed_hash_matches_a_one_shot_digest_and_cancels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let bytes: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        let digest = sha256_file(&path, &AtomicBool::new(false)).unwrap();
        assert_eq!(digest.sha256, <[u8; 32]>::from(Sha256::digest(&bytes)));
        assert_eq!(digest.bytes, bytes.len() as u64);
        assert!(matches!(
            sha256_file(&path, &AtomicBool::new(true)),
            Err(HashError::Cancelled)
        ));
        assert!(hex(&digest.sha256).len() == 64);
    }

    #[test]
    fn exact_groups_hash_only_size_collisions_and_reuse_cached_digests() {
        let dir = tempfile::tempdir().unwrap();
        let a = jpeg(&pattern(1), 90);
        std::fs::write(dir.path().join("a.jpg"), &a).unwrap();
        std::fs::write(dir.path().join("copy of a.jpg"), &a).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a.jpg"), &a).unwrap();
        // Same size, different bytes.
        let mut b = a.clone();
        let last = b.len() - 3;
        b[last] ^= 0x55;
        std::fs::write(dir.path().join("b.jpg"), &b).unwrap();
        std::fs::write(dir.path().join("unique.jpg"), jpeg(&pattern(2), 80)).unwrap();
        std::fs::write(dir.path().join("empty1"), b"").unwrap();
        std::fs::write(dir.path().join("empty2"), b"").unwrap();
        let list = files(
            dir.path(),
            &[
                "a.jpg",
                "b.jpg",
                "copy of a.jpg",
                "empty1",
                "empty2",
                "sub/a.jpg",
                "unique.jpg",
            ],
        );
        let progress = AtomicUsize::new(0);
        let scan = exact_groups(&list, |_| None, &AtomicBool::new(false), &progress);
        assert_eq!(scan.groups, vec![vec![0, 2, 5]]);
        // a, b, copy, sub/a share a size; unique and the empty files are never read.
        assert_eq!(scan.hashed, 4);
        assert_eq!(progress.load(Ordering::Relaxed), 4);
        let cache: HashMap<PathBuf, FileDigest> = scan.fresh.into_iter().collect();
        let rescan = exact_groups(
            &list,
            |p| cache.get(p).copied(),
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        );
        assert_eq!(rescan.groups, vec![vec![0, 2, 5]]);
        assert_eq!(rescan.hashed, 0);
        // A rewritten file's stale digest is not trusted.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.path().join("copy of a.jpg"), &b).unwrap();
        let list = files(
            dir.path(),
            &["a.jpg", "b.jpg", "copy of a.jpg", "sub/a.jpg"],
        );
        let changed = exact_groups(
            &list,
            |p| cache.get(p).copied(),
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        );
        assert_eq!(changed.hashed, 1);
        assert_eq!(changed.groups.len(), 2);
        assert!(
            exact_groups(&list, |_| None, &AtomicBool::new(true), &progress)
                .groups
                .is_empty()
        );
    }

    #[test]
    fn near_groups_find_resized_reencoded_and_lightly_cropped_copies() {
        let base = pattern(3);
        let other = pattern(9);
        let resized = image::imageops::resize(&base, 160, 120, FilterType::Lanczos3);
        let decode = |bytes: Vec<u8>| image::load_from_memory(&bytes).unwrap().to_rgb8();
        let reencoded = decode(jpeg(&base, 40));
        // A 2% border trimmed keeps the aspect within the matcher's tolerance.
        let cropped = image::imageops::crop_imm(&base, 3, 2, 314, 236).to_image();
        let images = [&base, &base, &resized, &reencoded, &cropped, &other];
        let photos: Vec<Photo> = images
            .iter()
            .enumerate()
            .map(|(i, img)| Photo {
                path: PathBuf::from(format!("/p/{i}.jpg")),
                stamp: Stamp {
                    bytes: 1,
                    seconds: 0,
                    nanos: 0,
                },
                signature: similar::Signature::of(img),
                captured: None,
            })
            .collect();
        // 0 and 1 are byte-identical: one exact group.
        let exact = vec![vec![0, 1]];
        let groups = near_groups(&photos, &exact, 6, &AtomicBool::new(false));
        assert_eq!(groups.len(), 1, "{groups:?}");
        let mut members = groups[0].clone();
        members.sort();
        assert_eq!(members, vec![0, 1, 2, 3, 4]);
        // Exact copies alone are not a visual group of themselves.
        let pair = near_groups(&photos[..2], &exact, 6, &AtomicBool::new(false));
        assert!(pair.is_empty());
    }

    #[test]
    fn keep_suggestion_prefers_raw_resolution_format_edits_then_earliest() {
        let f = |path: &str, pixels: u64, edited: bool, sidecar: bool, when: i64| KeepFacts {
            path: PathBuf::from(path),
            pixels,
            edited,
            sidecar,
            when,
            bytes: 10,
        };
        assert_eq!(
            suggest_keep(&[
                f("/a.jpg", 24_000_000, false, false, 1),
                f("/a.NEF", 0, false, false, 5)
            ]),
            1
        );
        assert_eq!(
            suggest_keep(&[
                f("/small.jpg", 1_000_000, true, true, 1),
                f("/big.jpg", 12_000_000, false, false, 9)
            ]),
            1
        );
        assert_eq!(
            suggest_keep(&[
                f("/x.webp", 100, false, false, 1),
                f("/x.jpg", 100, false, false, 9)
            ]),
            1
        );
        assert_eq!(
            suggest_keep(&[
                f("/1.jpg", 100, false, false, 1),
                f("/2.jpg", 100, false, true, 9),
                f("/3.jpg", 100, true, false, 9),
            ]),
            2
        );
        assert_eq!(
            suggest_keep(&[
                f("/late.jpg", 100, false, false, 9),
                f("/early.jpg", 100, false, false, 3)
            ]),
            1
        );
        // Full ties fall back to the path, so the suggestion is stable.
        assert_eq!(
            suggest_keep(&[
                f("/b.jpg", 1, false, false, 0),
                f("/a.jpg", 1, false, false, 0)
            ]),
            1
        );
    }

    #[test]
    fn trash_is_refused_unless_the_group_still_holds() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = jpeg(&pattern(4), 90);
        let keeper = dir.path().join("keep.jpg");
        let victim = dir.path().join("dupe.jpg");
        std::fs::write(&keeper, &bytes).unwrap();
        std::fs::write(&victim, &bytes).unwrap();
        let digest = sha256_file(&keeper, &AtomicBool::new(false))
            .unwrap()
            .sha256;
        let request = || TrashRequest {
            victim: victim.clone(),
            victim_stamp: Stamp::read(&victim).unwrap(),
            keeper: keeper.clone(),
            keeper_stamp: Stamp::read(&keeper).unwrap(),
            sha256: Some(digest),
        };
        let never = |_: &Path| -> std::io::Result<()> { panic!("must not trash") };
        let no = AtomicBool::new(false);

        // Keeping and trashing the same file.
        let mut same = request();
        same.keeper = victim.clone();
        same.keeper_stamp = same.victim_stamp.clone();
        assert_eq!(trash_checked(&same, &no, never), Err(Refusal::SameFile));

        // An XMP sidecar or a Schist edit keeps a duplicate out of the trash.
        let xmp = dir.path().join("dupe.xmp");
        std::fs::write(&xmp, "<x/>").unwrap();
        assert_eq!(
            trash_checked(&request(), &no, never),
            Err(Refusal::HasAttachments)
        );
        std::fs::remove_file(&xmp).unwrap();
        std::fs::create_dir_all(dir.path().join(".schist")).unwrap();
        let psd = dir.path().join(".schist/dupe.jpg.psd");
        std::fs::write(&psd, b"8BPS").unwrap();
        assert_eq!(
            trash_checked(&request(), &no, never),
            Err(Refusal::HasAttachments)
        );
        std::fs::remove_file(&psd).unwrap();

        // Stale evidence: the duplicate was rewritten after the scan.
        let stale = request();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut changed = bytes.clone();
        let at = changed.len() - 3;
        changed[at] ^= 1;
        std::fs::write(&victim, &changed).unwrap();
        assert_eq!(trash_checked(&stale, &no, never), Err(Refusal::Changed));
        // Re-stamped but no longer identical.
        assert_eq!(
            trash_checked(&request(), &no, never),
            Err(Refusal::NotIdentical)
        );
        std::fs::write(&victim, &bytes).unwrap();

        // The keeper vanished.
        let gone = request();
        std::fs::rename(&keeper, dir.path().join("moved.jpg")).unwrap();
        assert_eq!(
            trash_checked(&gone, &no, never),
            Err(Refusal::KeeperChanged)
        );
        std::fs::rename(dir.path().join("moved.jpg"), &keeper).unwrap();

        // Everything holds: the injected trash is called, and a failing
        // trash leaves the file exactly where it was.
        let mut trashed = None;
        trash_checked(&request(), &no, |p| {
            trashed = Some(p.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(trashed.as_deref(), Some(victim.as_path()));
        let failed = trash_checked(&request(), &no, |_| {
            Err(std::io::ErrorKind::Unsupported.into())
        });
        assert!(matches!(failed, Err(Refusal::Trash(_))));
        assert!(victim.exists() && keeper.exists());
    }
}
