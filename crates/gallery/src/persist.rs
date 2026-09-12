//! `library.json`.

use crate::geo::GeoBounds;
use crate::paths::library_path;
use crate::people::{DeniedFace, PersonFile, TaggedFace};
use std::path::PathBuf;

/// A bucket as `library.json` holds it. Untagged so the shape saved
/// before buckets had rules — a bare `[name, [photos]]` pair — still
/// reads; writes always use the named form.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum BucketFile {
    Rich {
        name: String,
        #[serde(default)]
        photos: Vec<PathBuf>,
        #[serde(default)]
        query: Option<String>,
        #[serde(default)]
        area: Option<(GeoBounds, String)>,
    },
    Plain(String, Vec<PathBuf>),
}

impl BucketFile {
    pub fn name(&self) -> &str {
        match self {
            BucketFile::Rich { name, .. } | BucketFile::Plain(name, _) => name,
        }
    }
    pub fn photos(&self) -> &[PathBuf] {
        match self {
            BucketFile::Rich { photos, .. } | BucketFile::Plain(_, photos) => photos,
        }
    }
    pub fn query(&self) -> Option<&str> {
        match self {
            BucketFile::Rich { query, .. } => query.as_deref(),
            BucketFile::Plain(..) => None,
        }
    }
    pub fn area(&self) -> Option<&(GeoBounds, String)> {
        match self {
            BucketFile::Rich { area, .. } => area.as_ref(),
            BucketFile::Plain(..) => None,
        }
    }
}

/// What `library.json` persists: the watched folders, the recents, the
/// grid's preferences, the buckets, and the people — every name given
/// to a face, and every detected face waved away as not one. Everything
/// else — sections, thumbnails, the index, the detections themselves —
/// is derived from the disk.
#[derive(Default, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct LibraryFile {
    pub folders: Vec<PathBuf>,
    #[serde(default)]
    pub recents: Vec<PathBuf>,
    #[serde(default)]
    pub thumb_px: Option<f32>,
    #[serde(default)]
    pub group_by: Option<String>,
    #[serde(default)]
    pub buckets: Vec<BucketFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<PersonFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignored_faces: Vec<TaggedFace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_faces: Vec<DeniedFace>,
}

/// Stands in for the app container's path in the saved file on iOS,
/// where the container moves on every install of the app: a watched
/// folder saved as an absolute path would be lost at the next update.
const SANDBOX_TOKEN: &str = "$SANDBOX";

/// The sandbox path every in-container path is saved relative to; only
/// iOS relocates it.
fn sandbox_home() -> Option<String> {
    if cfg!(target_os = "ios") {
        std::env::var("HOME").ok().filter(|h| !h.is_empty())
    } else {
        None
    }
}

/// Paths saved under an earlier container of this app (before they were
/// saved relative, or by an older build) point at the container's old
/// name, `.../Application/<uuid>`; the contents moved with it, so point
/// them at the current one.
fn relocate_sandbox(text: String, home: &str) -> String {
    let Some((parent, current)) = home.rsplit_once('/') else {
        return text;
    };
    if current.is_empty() {
        return text;
    }
    let prefix = format!("{parent}/");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find(&prefix) {
        let after = at + prefix.len();
        out.push_str(&rest[..after]);
        rest = &rest[after..];
        // A container name is a UUID: 36 ASCII characters up to the next
        // path separator.
        let id_len = rest.find(['/', '"']).unwrap_or(rest.len());
        if id_len == current.len() {
            out.push_str(current);
            rest = &rest[id_len..];
        }
    }
    out.push_str(rest);
    out
}

impl LibraryFile {
    /// Read the file, or the defaults when it is missing or unreadable.
    pub fn load() -> LibraryFile {
        library_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|text| match sandbox_home() {
                Some(home) => relocate_sandbox(text.replace(SANDBOX_TOKEN, &home), &home),
                None => text,
            })
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let Some(path) = library_path() else {
            anyhow::bail!("no config directory to save the library in");
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut text = serde_json::to_string_pretty(self)?;
        if let Some(home) = sandbox_home() {
            // Paths never need JSON escaping on iOS (no quotes or
            // backslashes in a container path), so the substitution is
            // safe on the serialised text.
            text = text.replace(&home, SANDBOX_TOKEN);
        }
        std::fs::write(path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_paths_follow_the_container() {
        let text = r#"{"folders": ["/c/Application/AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA/Documents/Photos", "/elsewhere/Photos"]}"#.to_string();
        let moved = relocate_sandbox(text, "/c/Application/BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB");
        assert!(
            moved.contains("/c/Application/BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB/Documents/Photos")
        );
        assert!(moved.contains("/elsewhere/Photos"));
        assert!(!moved.contains("AAAAAAAA"));
    }

    #[test]
    fn buckets_saved_before_they_had_rules_still_read() {
        let legacy: Vec<BucketFile> =
            serde_json::from_str(r#"[["Trip", ["/a.jpg"]]]"#).expect("legacy shape");
        assert!(matches!(&legacy[0], BucketFile::Plain(name, photos)
            if name == "Trip" && photos == &[PathBuf::from("/a.jpg")]));
        let rich: Vec<BucketFile> = serde_json::from_str(
            r#"[{"name": "NYC dogs", "query": "dog",
                 "area": [{"south": 40.0, "west": -75.0, "north": 41.0, "east": -73.0}, "New York City"]}]"#,
        )
        .expect("rich shape");
        assert_eq!(rich[0].name(), "NYC dogs");
        assert_eq!(rich[0].query(), Some("dog"));
        assert!(rich[0]
            .area()
            .is_some_and(|(b, place)| place == "New York City" && b.contains(40.7, -74.0)));
    }
}
