//! Clipboard sources for image import. Read only in response to a user action.

/// Keep image and text representations together. GPUI's macOS reader returns
/// text as soon as it finds any, hiding images copied with a URL or caption.
pub fn read_clipboard_entries(cx: &gpui::App) -> Vec<gpui::ClipboardEntry> {
    #[cfg(target_os = "macos")]
    {
        let _ = cx;
        mac::read(&objc2_app_kit::NSPasteboard::generalPasteboard())
    }
    #[cfg(not(target_os = "macos"))]
    cx.read_from_clipboard()
        .into_iter()
        .flat_map(|item| item.into_entries())
        .collect()
}

#[cfg(target_os = "macos")]
mod mac {
    use gpui::{ClipboardEntry, Image, ImageFormat};
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::NSString;

    pub(super) fn read(pasteboard: &NSPasteboard) -> Vec<ClipboardEntry> {
        let mut entries = Vec::new();
        // Prefer lossless bitmap data, including TIFF used by macOS screen
        // captures and Preview. An empty representation must not hide a
        // usable alternative farther down the list.
        for (kind, format) in [
            ("public.png", ImageFormat::Png),
            ("public.tiff", ImageFormat::Tiff),
            ("public.jpeg", ImageFormat::Jpeg),
            ("org.webmproject.webp", ImageFormat::Webp),
            ("com.compuserve.gif", ImageFormat::Gif),
            ("com.microsoft.bmp", ImageFormat::Bmp),
            ("public.svg-image", ImageFormat::Svg),
        ] {
            if let Some(data) = pasteboard.dataForType(&NSString::from_str(kind)) {
                let bytes = data.to_vec();
                if !bytes.is_empty() {
                    entries.push(Image::from_bytes(format, bytes).into());
                    break;
                }
            }
        }
        // Some applications publish a URL without a plain-text representation.
        // Keep both so a caption cannot hide that URL either.
        for kind in ["public.utf8-plain-text", "public.url"] {
            if let Some(text) = pasteboard.stringForType(&NSString::from_str(kind)) {
                entries.push(text.to_string().into());
            }
        }
        entries
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::mac::read;
    use gpui::ClipboardEntry;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSData, NSString};
    use std::io::Cursor;

    fn bitmap(format: image::ImageFormat) -> Vec<u8> {
        let image = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 0, 0]).unwrap();
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn image_survives_text_and_url_on_the_same_pasteboard() {
        // A private pasteboard exercises AppKit without touching the user's
        // clipboard, and lets these tests run concurrently.
        let pasteboard = NSPasteboard::pasteboardWithUniqueName();
        let bytes = bitmap(image::ImageFormat::Png);
        assert!(pasteboard.setString_forType(
            &NSString::from_str("A caption"),
            &NSString::from_str("public.utf8-plain-text"),
        ));
        assert!(pasteboard.setString_forType(
            &NSString::from_str("https://example.com/image"),
            &NSString::from_str("public.url"),
        ));
        assert!(pasteboard.setData_forType(
            Some(&NSData::with_bytes(&bytes)),
            &NSString::from_str("public.png"),
        ));
        let entries = read(&pasteboard);
        assert!(entries.iter().any(|entry| matches!(entry,
            ClipboardEntry::Image(image) if image.bytes == bytes)));
        for expected in ["A caption", "https://example.com/image"] {
            assert!(entries.iter().any(|entry| matches!(entry,
                ClipboardEntry::String(text) if text.text() == expected)));
        }
    }

    #[test]
    fn tiff_capture_survives_empty_text_and_png_representations() {
        let pasteboard = NSPasteboard::pasteboardWithUniqueName();
        assert!(pasteboard.setString_forType(
            &NSString::from_str(""),
            &NSString::from_str("public.utf8-plain-text"),
        ));
        assert!(pasteboard.setData_forType(
            Some(&NSData::with_bytes(&[])),
            &NSString::from_str("public.png"),
        ));
        let bytes = bitmap(image::ImageFormat::Tiff);
        assert!(pasteboard.setData_forType(
            Some(&NSData::with_bytes(&bytes)),
            &NSString::from_str("public.tiff"),
        ));
        let entries = read(&pasteboard);
        let image = entries
            .iter()
            .find_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(image),
                _ => None,
            })
            .expect("TIFF capture must remain available");
        assert_eq!(image.bytes, bytes);
        let decoded = image::load_from_memory(&image.bytes).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.into_raw(), [255, 0, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn url_only_and_empty_pasteboards_do_not_offer_an_image() {
        let pasteboard = NSPasteboard::pasteboardWithUniqueName();
        assert!(read(&pasteboard).is_empty());
        assert!(pasteboard.setString_forType(
            &NSString::from_str("https://example.com/image"),
            &NSString::from_str("public.url"),
        ));
        let entries = read(&pasteboard);
        assert!(entries
            .iter()
            .all(|entry| matches!(entry, ClipboardEntry::String(_))));
        assert!(entries.iter().any(|entry| matches!(entry,
            ClipboardEntry::String(text) if text.text() == "https://example.com/image")));
    }
}
