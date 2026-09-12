//! Files handed to the app on iOS and iPadOS: the share sheet's "Copy
//! to Schist", Files' "Open in Schist", a photo sent over from another
//! app. The desktop opens such a file in the editor at once; on a phone
//! or tablet the gallery is as likely the place it was meant for, so the
//! app asks: add it to the gallery, or open it in the editor?
//!
//! Either way the file is first brought into the app's own Documents,
//! since what arrives is a copy in the sandbox's Inbox (which the app is
//! expected to clear) or a file elsewhere that is readable only for now.
//! A file already under Documents (opened in place from the app's own
//! folder in Files) stays where it is.

use super::*;
use std::path::{Path, PathBuf};

impl Workspace {
    /// Ask where the files should go.
    pub fn offer_shared_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        self.open_modal(Modal::SharedImage { paths }, cx);
    }

    /// Bring the files into the gallery's Photos folder and show them.
    pub fn add_shared_to_gallery(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(dest) = documents_dir().map(|d| d.join("Photos")) else {
            self.status = "The gallery needs a Documents folder to copy into".into();
            cx.notify();
            return;
        };
        let mut added = 0;
        let mut failed = 0;
        for path in &paths {
            match stash(path, &dest) {
                Ok(_) => added += 1,
                Err(err) => {
                    log::warn!("could not add {} to the gallery: {err:#}", path.display());
                    failed += 1;
                }
            }
        }
        if !self.library.folders.contains(&dest) {
            self.library.folders.push(dest.clone());
            self.library.folders.sort();
            self.library.save();
        }
        let mut message = match added {
            1 => "Added 1 photo to the gallery".to_string(),
            n => format!("Added {n} photos to the gallery"),
        };
        if failed > 0 {
            message.push_str(&format!(" \u{2014} {failed} failed"));
        }
        self.status = message.into();
        self.library.open = true;
        self.library_rescan(cx);
        cx.notify();
    }

    /// Bring the files into Documents and open each in its own tab.
    pub fn open_shared_in_editor(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(dest) = documents_dir() else {
            self.status = "Opening needs a Documents folder to copy into".into();
            cx.notify();
            return;
        };
        for path in paths {
            match stash(&path, &dest) {
                Ok(path) => self.load_file(path, cx),
                Err(err) => {
                    self.status =
                        format!("Could not open {}: {err}", crate::ui::shown_path(&path)).into();
                    cx.notify();
                }
            }
        }
    }
}

fn documents_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Documents"))
}

/// The file at `path` as a file under `dest`: itself when it already
/// lives in Documents (outside the Inbox), otherwise a copy under its
/// own name (numbered if taken). An Inbox original is removed once
/// copied, as iOS expects.
fn stash(path: &Path, dest: &Path) -> anyhow::Result<PathBuf> {
    let documents = documents_dir();
    let inbox = documents.as_ref().map(|d| d.join("Inbox"));
    let in_inbox = inbox.as_ref().is_some_and(|inbox| path.starts_with(inbox));
    if let Some(documents) = &documents {
        if path.starts_with(documents) && !in_inbox {
            return Ok(path.to_path_buf());
        }
    }
    std::fs::create_dir_all(dest)?;
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("the file has no name"))?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    let mut target = dest.join(name);
    let mut n = 1;
    while target.exists() {
        n += 1;
        let mut candidate = format!("{stem}-{n}");
        if let Some(ext) = &ext {
            candidate.push('.');
            candidate.push_str(ext);
        }
        target = dest.join(candidate);
    }
    std::fs::copy(path, &target)?;
    if in_inbox {
        let _ = std::fs::remove_file(path);
    }
    Ok(target)
}
