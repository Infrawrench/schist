//! "Save to Photos" on iOS and iPadOS: the flattened document into the
//! camera roll.
//!
//! The document is encoded as PNG the way Export does, written to a
//! temporary file, and handed to the photo library as a new asset from
//! that file (`PHAssetChangeRequest`), which keeps the bytes as encoded.
//! The library asks for add-only permission the first time
//! (`NSPhotoLibraryAddUsageDescription` in the bundle's Info.plist) and
//! reports back on its own queue; the result lands in the status bar.

use super::*;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::NSString;
use std::path::PathBuf;

impl Workspace {
    /// Encode the document and add it to the photo library.
    pub fn save_to_photos(&mut self, cx: &mut Context<Self>) {
        let encoded = (|| -> anyhow::Result<PathBuf> {
            let doc = self
                .doc
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("no document is open"))?;
            let codec = self
                .png_codec()
                .ok_or_else(|| anyhow::anyhow!("the PNG codec is not loaded"))?;
            let bytes = codec.export(doc)?;
            let dir = std::env::temp_dir().join("schist-photos");
            std::fs::create_dir_all(&dir)?;
            let stem = std::path::Path::new(&doc.title)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "untitled".into());
            let path = dir.join(format!("{stem}.png"));
            std::fs::write(&path, bytes)?;
            Ok(path)
        })();
        let path = match encoded {
            Ok(path) => path,
            Err(err) => {
                self.status = format!("Save to Photos failed: {err}").into();
                cx.notify();
                return;
            }
        };
        self.status = "Saving to Photos\u{2026}".into();
        cx.notify();

        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        add_image_file(&path, tx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = rx
                        .recv()
                        .unwrap_or_else(|_| Err("the photo library did not answer".into()));
                    let _ = std::fs::remove_file(&path);
                    result
                })
                .await;
            this.update(cx, |ws, cx| {
                ws.status = match result {
                    Ok(()) => "Saved to Photos".into(),
                    Err(err) => format!("Save to Photos failed: {err}").into(),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// Asks the photo library to add the image file at `path`; the outcome
/// is sent on `tx` from the library's own queue.
fn add_image_file(path: &std::path::Path, tx: std::sync::mpsc::Sender<Result<(), String>>) {
    let path_ns = NSString::from_str(&path.to_string_lossy());
    unsafe {
        let url: Option<Retained<AnyObject>> = msg_send![class!(NSURL), fileURLWithPath: &*path_ns];
        let Some(url) = url else {
            let _ = tx.send(Err("the file's path could not be made a URL".into()));
            return;
        };
        let library: Option<Retained<AnyObject>> =
            msg_send![class!(PHPhotoLibrary), sharedPhotoLibrary];
        let Some(library) = library else {
            let _ = tx.send(Err("the photo library is unavailable".into()));
            return;
        };
        let changes = RcBlock::new(move || {
            let _request: *mut AnyObject = msg_send![
                class!(PHAssetChangeRequest),
                creationRequestForAssetFromImageAtFileURL: &*url
            ];
        });
        let completion = RcBlock::new(move |success: Bool, error: *mut AnyObject| {
            let result = if success.as_bool() {
                Ok(())
            } else if error.is_null() {
                Err("the photo library declined".to_string())
            } else {
                let description: Option<Retained<NSString>> =
                    msg_send![error, localizedDescription];
                Err(description
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "the photo library declined".into()))
            };
            let _ = tx.send(result);
        });
        let _: () =
            msg_send![&*library, performChanges: &*changes, completionHandler: &*completion];
    }
}
