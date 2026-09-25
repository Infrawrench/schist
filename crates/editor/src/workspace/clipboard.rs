//! Bridging the internal clipboard to the system one.

use super::*;
use schist_i18n::{t, tf};

/// The clipboard snapshot taken when New opens. Keeping it in the modal
/// avoids reading the system clipboard on every render or importing a
/// different image if another app changes it before the button is clicked.
#[derive(Debug, Default, PartialEq)]
pub struct NewFileClipboard {
    pub image: Option<gpui::Image>,
    pub url: Option<String>,
}

impl NewFileClipboard {
    pub(super) fn from_entries(entries: impl IntoIterator<Item = gpui::ClipboardEntry>) -> Self {
        let mut result = Self::default();
        for entry in entries {
            match entry {
                gpui::ClipboardEntry::Image(image)
                    if result.image.is_none() && !image.bytes.is_empty() =>
                {
                    result.image = Some(image);
                }
                gpui::ClipboardEntry::String(text) if result.url.is_none() => {
                    result.url = clipboard_image_url(text.text());
                }
                _ => {}
            }
        }
        result
    }
}

fn clipboard_image_url(text: &str) -> Option<String> {
    let text = text.trim();
    // URLs need not have an image extension (CDNs often use query strings).
    // Check the downloaded bytes with the codecs only after the user clicks.
    if text.chars().any(char::is_whitespace) {
        return None;
    }
    let url = url::Url::parse(text).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some()).then(|| url.into())
}

fn decode_clipboard_document(
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
    bytes: &[u8],
) -> anyhow::Result<Document> {
    let codec = codecs
        .iter()
        .find(|codec| codec.probe(bytes))
        .ok_or_else(|| anyhow::anyhow!(t("common.unsupported_format")))?;
    let mut doc = codec.import(bytes)?;
    // This is a new, unsaved document, even if the codec marks imports saved.
    doc.path = None;
    doc.dirty = true;
    Ok(doc)
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_clipboard_image(url: &str) -> anyhow::Result<Vec<u8>> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build();
    let mut response = ureq::Agent::new_with_config(config).get(url).call()?;
    Ok(response
        .body_mut()
        .with_config()
        .limit(256 << 20)
        .read_to_vec()?)
}

impl Workspace {
    pub(crate) fn new_document_from_clipboard(&mut self, from_url: bool, cx: &mut Context<Self>) {
        let Some(Modal::NewFilePicker {
            clipboard,
            importing,
            error,
        }) = &mut self.modal
        else {
            return;
        };
        if *importing
            || (from_url && clipboard.url.is_none())
            || (!from_url && clipboard.image.is_none())
        {
            return;
        }
        *importing = true;
        *error = None;
        let clipboard = clipboard.clone();
        let codecs = self.registry.shared_codecs();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let source = clipboard.clone();
            #[cfg(not(target_arch = "wasm32"))]
            let result = cx
                .background_executor()
                .spawn(async move {
                    if from_url {
                        let bytes = fetch_clipboard_image(source.url.as_deref().unwrap())?;
                        decode_clipboard_document(&codecs, &bytes)
                    } else {
                        decode_clipboard_document(&codecs, &source.image.as_ref().unwrap().bytes)
                    }
                })
                .await;
            #[cfg(target_arch = "wasm32")]
            let result = if from_url {
                crate::web::fetch_bytes(source.url.clone().unwrap(), Arc::new(AtomicU64::new(0)))
                    .await
                    .map_err(anyhow::Error::msg)
                    .and_then(|bytes| decode_clipboard_document(&codecs, &bytes))
            } else {
                decode_clipboard_document(&codecs, &source.image.as_ref().unwrap().bytes)
            };
            this.update(cx, |ws, cx| {
                let Some(Modal::NewFilePicker {
                    clipboard: current,
                    importing,
                    error,
                }) = &mut ws.modal
                else {
                    return;
                };
                // Cancel, Custom, a preset, or a newly opened picker wins
                // over a late network/decode completion.
                if !Arc::ptr_eq(current, &clipboard) {
                    return;
                }
                *importing = false;
                match result {
                    Ok(mut doc) => {
                        doc.title = ws.next_untitled_name();
                        ws.close_modal(cx);
                        ws.status = tf!("workspace.docs.opened", name = doc.title).into();
                        ws.install_document(doc);
                    }
                    Err(err) => {
                        *error = Some(tf!("workspace.docs.open_failed", error = err));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Push the internal clipboard out to the system clipboard as a PNG.
    ///
    /// Schist's own copy/paste has always worked between its documents;
    /// this is what makes it work with everything else.
    pub fn sync_clipboard_out(&mut self, cx: &mut Context<Self>) {
        let Some(clip) = self.editor.clipboard.clone() else {
            return;
        };
        let (w, h) = (clip.rect.width() as u32, clip.rect.height() as u32);
        if w == 0 || h == 0 {
            return;
        }
        let Some(codec) = self.png_codec() else {
            return;
        };
        // Codecs export documents, so the clipboard becomes a one-layer one.
        let mut doc = Document::new("clipboard", w, h, Depth::Eight);
        let mut layer = Layer::new_raster("clipboard");
        schist_core::blit_rgba8(
            &mut layer.as_raster_mut().unwrap().tiles,
            Depth::Eight,
            IntRect::from_size(w, h),
            &clip.rgba,
        );
        doc.push_layer(layer);
        match codec.export(&doc) {
            Ok(bytes) => {
                let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes);
                cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image));
            }
            Err(e) => log::error!("clipboard export: {e}"),
        }
    }

    /// Pull an image off the system clipboard into the internal one.
    ///
    /// Returns false when the clipboard holds nothing we can use, so the
    /// caller can fall back to whatever was copied inside the app.
    pub fn sync_clipboard_in(&mut self, cx: &mut Context<Self>) -> bool {
        for entry in schist_app_platform::clipboard::read_clipboard_entries(cx) {
            let gpui::ClipboardEntry::Image(image) = entry else {
                continue;
            };
            if image.bytes.is_empty() {
                continue;
            }
            // Route it through the codecs, so anything Schist can open
            // it can also paste.
            let Some(codec) = self.registry.codecs().find(|c| c.probe(&image.bytes)) else {
                continue;
            };
            match codec.import(&image.bytes) {
                Ok(doc) => {
                    let rect = doc.canvas_rect();
                    let rgba = schist_compositor::composite_region_rgba8(&doc, rect);
                    self.editor.clipboard =
                        Some(Arc::new(schist_plugin_api::ClipboardImage { rect, rgba }));
                    return true;
                }
                Err(e) => log::error!("clipboard import: {e}"),
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn png() -> Vec<u8> {
        bitmap(image::ImageFormat::Png)
    }

    fn bitmap(format: image::ImageFormat) -> Vec<u8> {
        let image = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 0, 0]).unwrap();
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    fn codecs() -> Vec<Arc<dyn schist_plugin_api::CodecPlugin>> {
        vec![
            Arc::new(schist_codecs_common::PngCodec),
            Arc::new(schist_codecs_common::TiffCodec),
        ]
    }

    #[test]
    fn clipboard_urls_accept_http_without_requiring_an_extension() {
        for url in [
            "https://example.com/image.png",
            "http://localhost:8080/image?id=42&size=original",
            "https://example.com/a%20b",
        ] {
            assert_eq!(
                clipboard_image_url(&format!(" \n{url}\r\n")),
                Some(url.into())
            );
        }
        for text in [
            "",
            "hello",
            "example.com/image.png",
            "file:///tmp/image.png",
            "data:image/png;base64,AAAA",
            "javascript:alert(1)",
            "https://",
            "Here is https://example.com/image.png",
            "https://example.com/one\nhttps://example.com/two",
            "https://example.com/a b",
        ] {
            assert_eq!(clipboard_image_url(text), None, "{text}");
        }
    }

    #[test]
    fn clipboard_snapshot_keeps_image_and_url_and_ignores_empty_images() {
        let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, png());
        let snapshot = NewFileClipboard::from_entries([
            gpui::Image::empty().into(),
            "ordinary text".to_string().into(),
            image.clone().into(),
            "https://example.com/image".to_string().into(),
        ]);
        assert_eq!(snapshot.image, Some(image));
        assert_eq!(snapshot.url.as_deref(), Some("https://example.com/image"));
        assert_eq!(
            NewFileClipboard::from_entries([]),
            NewFileClipboard::default()
        );
    }

    #[test]
    fn imported_clipboard_document_keeps_size_and_alpha_and_needs_saving() {
        for format in [image::ImageFormat::Png, image::ImageFormat::Tiff] {
            let doc = decode_clipboard_document(&codecs(), &bitmap(format)).unwrap();
            assert_eq!((doc.width, doc.height), (2, 1));
            assert!(doc.path.is_none());
            assert!(doc.dirty);
            assert_eq!(
                schist_compositor::composite_region_rgba8(&doc, doc.canvas_rect()),
                [255, 0, 0, 255, 0, 0, 0, 0],
            );
        }
    }

    #[test]
    fn import_rejects_web_pages_and_corrupt_images() {
        for bytes in [b"<html>not an image</html>".as_slice(), &[], &png()[..12]] {
            assert!(decode_clipboard_document(&codecs(), bytes).is_err());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn image_url_follows_redirects_and_checks_http_errors() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let image_url = format!("{base}/image");
        let server = std::thread::spawn(move || {
            for (status, headers, body) in [
                (
                    "302 Found",
                    format!("Location: {image_url}\r\n"),
                    Vec::new(),
                ),
                ("200 OK", String::new(), png()),
                ("404 Not Found", String::new(), Vec::new()),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                write!(
                    socket,
                    "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                socket.write_all(&body).unwrap();
            }
        });
        let bytes = fetch_clipboard_image(&format!("{base}/redirect")).unwrap();
        assert_eq!(
            decode_clipboard_document(&codecs(), &bytes).unwrap().width,
            2
        );
        assert!(fetch_clipboard_image(&format!("{base}/missing")).is_err());
        server.join().unwrap();
    }
}
