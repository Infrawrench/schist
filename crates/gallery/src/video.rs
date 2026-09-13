//! Video containers accepted by the desktop gallery. These are media,
//! never image codecs: originals must not acquire PSD edit sidecars.
use std::path::Path;

pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "m4v", "mkv", "webm", "avi", "mpg", "mpeg", "mts", "m2ts", "3gp",
];

pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            VIDEO_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn containers_are_case_insensitive_and_do_not_claim_images_or_playlists() {
        assert!(is_video(Path::new("holiday.MOV")));
        assert!(is_video(Path::new("clip.webm")));
        assert!(!is_video(Path::new("photo.png")));
        assert!(!is_video(Path::new("stream.m3u8")));
    }
}
