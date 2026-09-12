//! The path prompts, and the file picker Schist draws for itself.
//!
//! Every open, save-as, export and add-folder goes through
//! [`Workspace::prompt_for_paths`] and [`Workspace::prompt_for_new_path`].
//! On the desktop and iOS those are the platform's own dialogs. Android
//! has none a `NativeActivity` can get an answer from (the system picker
//! answers with an activity result, which needs a Java class), so there
//! the prompts open a dialog of Schist's own: a directory listing to
//! walk, with the app's Documents and the device's shared folders a tap
//! away, and a name field for a save. The callers see the same receiver
//! either way, and a cancel -- Cancel, Escape, or another dialog taking
//! the modal's place -- is the platform's cancel: the sender drops and
//! the receiver's `await` fails.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use super::*;
use futures::channel::oneshot;
use gpui::PathPromptOptions;
use std::path::Path;

/// The name field's id in the workspace's field state.
pub const NAME_FIELD: &str = "file-picker-name";

/// What the dialog is picking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKind {
    /// Existing paths: files, directories, or either; one or several.
    Open {
        files: bool,
        directories: bool,
        multiple: bool,
    },
    /// A new path: the directory walked to, plus a typed name.
    Save,
}

/// One row of the listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Where the answer goes.
enum Reply {
    Paths(oneshot::Sender<anyhow::Result<Option<Vec<PathBuf>>>>),
    NewPath(oneshot::Sender<anyhow::Result<Option<PathBuf>>>),
}

/// The open picker's state; `Modal::FilePicker` is what shows it.
pub struct FilePicker {
    pub kind: PickerKind,
    pub title: SharedString,
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    /// Why the directory could not be read, when it could not.
    pub error: Option<String>,
    /// The rows picked so far (`Open`).
    pub selected: Vec<PathBuf>,
    /// The name as last committed (`Save`); the workspace's field buffer
    /// holds it while the field has the caret.
    pub name: String,
    /// The existing file a save was warned about; a second Save with the
    /// same name replaces it, as the platform dialogs' "Replace?" would.
    pub replace_armed: Option<PathBuf>,
    reply: Option<Reply>,
}

impl Workspace {
    /// The platform's open dialog, or Schist's own where there is none.
    pub fn prompt_for_paths(
        &mut self,
        options: PathPromptOptions,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<anyhow::Result<Option<Vec<PathBuf>>>> {
        #[cfg(not(target_os = "android"))]
        {
            cx.prompt_for_paths(options)
        }
        #[cfg(target_os = "android")]
        {
            let (tx, rx) = oneshot::channel();
            let kind = PickerKind::Open {
                files: options.files,
                directories: options.directories,
                multiple: options.multiple,
            };
            let title = options.prompt.unwrap_or_else(|| {
                if options.files {
                    "Open".into()
                } else {
                    "Choose Folder".into()
                }
            });
            self.open_file_picker(
                kind,
                title,
                start_dir(None),
                String::new(),
                Reply::Paths(tx),
                cx,
            );
            rx
        }
    }

    /// The platform's save dialog, or Schist's own where there is none.
    pub fn prompt_for_new_path(
        &mut self,
        directory: &Path,
        suggested_name: Option<&str>,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<anyhow::Result<Option<PathBuf>>> {
        #[cfg(not(target_os = "android"))]
        {
            cx.prompt_for_new_path(directory, suggested_name)
        }
        #[cfg(target_os = "android")]
        {
            let (tx, rx) = oneshot::channel();
            let name = suggested_name.unwrap_or_default().to_string();
            self.open_file_picker(
                PickerKind::Save,
                "Save".into(),
                start_dir(Some(directory)),
                name,
                Reply::NewPath(tx),
                cx,
            );
            rx
        }
    }

    fn open_file_picker(
        &mut self,
        kind: PickerKind,
        title: SharedString,
        dir: PathBuf,
        name: String,
        reply: Reply,
        cx: &mut Context<Self>,
    ) {
        let (entries, error) = read_listing(&dir, kind);
        self.file_picker = Some(FilePicker {
            kind,
            title,
            dir,
            entries,
            error,
            selected: Vec::new(),
            name: name.clone(),
            replace_armed: None,
            reply: Some(reply),
        });
        self.open_modal(Modal::FilePicker, cx);
        // A save starts with the caret in the name, keyboard up.
        if kind == PickerKind::Save {
            self.focus_field(NAME_FIELD, name);
        }
        cx.notify();
    }

    /// Show `dir`'s listing. The selection is per directory.
    pub fn picker_navigate(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(picker) = self.file_picker.as_mut() else {
            return;
        };
        let (entries, error) = read_listing(&dir, picker.kind);
        picker.dir = dir;
        picker.entries = entries;
        picker.error = error;
        picker.selected.clear();
        cx.notify();
    }

    pub fn picker_up(&mut self, cx: &mut Context<Self>) {
        let Some(parent) = self
            .file_picker
            .as_ref()
            .and_then(|p| p.dir.parent().map(Path::to_path_buf))
        else {
            return;
        };
        self.picker_navigate(parent, cx);
    }

    /// A tap on row `index`: a folder is entered; a file is picked (one,
    /// or toggled among several) or, for a save, becomes the name.
    pub fn picker_tap(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(picker) = self.file_picker.as_mut() else {
            return;
        };
        let Some(entry) = picker.entries.get(index).cloned() else {
            return;
        };
        if entry.is_dir {
            self.picker_navigate(entry.path, cx);
            return;
        }
        match picker.kind {
            PickerKind::Open { multiple, .. } => {
                if let Some(at) = picker.selected.iter().position(|p| *p == entry.path) {
                    picker.selected.remove(at);
                } else {
                    if !multiple {
                        picker.selected.clear();
                    }
                    picker.selected.push(entry.path);
                }
            }
            PickerKind::Save => {
                picker.name = entry.name.clone();
                self.focus_field(NAME_FIELD, entry.name);
            }
        }
        cx.notify();
    }

    /// The primary button: answer the prompt and close.
    pub fn picker_confirm(&mut self, cx: &mut Context<Self>) {
        let typed = (self.focused_field == Some(NAME_FIELD)).then(|| self.field_buffer.clone());
        let Some(picker) = self.file_picker.as_mut() else {
            return;
        };
        let answer = match picker.kind {
            PickerKind::Save => {
                let name = typed.unwrap_or_else(|| picker.name.clone());
                let name = name.trim();
                if name.is_empty() || name.contains('/') {
                    self.status = "Type a name for the file".into();
                    cx.notify();
                    return;
                }
                let path = picker.dir.join(name);
                if path.exists() && picker.replace_armed.as_ref() != Some(&path) {
                    picker.replace_armed = Some(path);
                    self.status = format!("{name} exists; tap Save again to replace it").into();
                    cx.notify();
                    return;
                }
                Some(vec![path])
            }
            PickerKind::Open { directories, .. } => {
                if !picker.selected.is_empty() {
                    Some(std::mem::take(&mut picker.selected))
                } else if directories {
                    // Nothing picked in a folder chooser means this folder.
                    Some(vec![picker.dir.clone()])
                } else {
                    self.status = "Tap a file to open".into();
                    cx.notify();
                    return;
                }
            }
        };
        match picker.reply.take() {
            Some(Reply::Paths(tx)) => {
                tx.send(Ok(answer)).ok();
            }
            Some(Reply::NewPath(tx)) => {
                tx.send(Ok(answer.and_then(|mut paths| paths.pop()))).ok();
            }
            None => {}
        }
        self.close_modal(cx);
    }
}

/// The folders a tap reaches: the app's Documents, then the device's
/// shared Pictures, camera roll and Downloads when they exist (reading
/// them needs the storage permission, which the dialog reports when it
/// is missing).
pub fn places() -> Vec<(&'static str, PathBuf)> {
    let mut out = Vec::new();
    if let Some(documents) = documents_dir() {
        out.push(("Documents", documents));
    }
    let shared = Path::new("/storage/emulated/0");
    for (label, sub) in [
        ("Pictures", "Pictures"),
        ("Camera", "DCIM"),
        ("Downloads", "Download"),
    ] {
        let dir = shared.join(sub);
        if dir.is_dir() {
            out.push((label, dir));
        }
    }
    out
}

/// Where a prompt opens: the directory asked for when it is a real one
/// (the document's own folder), else Documents. The home directory is
/// never it: on Android that is the app's private storage, which is no
/// place to keep documents.
fn start_dir(requested: Option<&Path>) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(dir) = requested {
        if dir.is_dir() && home.as_deref() != Some(dir) && dir != Path::new(".") {
            return dir.to_path_buf();
        }
    }
    documents_dir()
        .filter(|dir| dir.is_dir())
        .or(home)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn documents_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        crate::android::documents_dir()
    }
    #[cfg(not(target_os = "android"))]
    {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Documents"))
    }
}

/// `dir`'s rows, or an empty listing and the reason.
fn read_listing(dir: &Path, kind: PickerKind) -> (Vec<Entry>, Option<String>) {
    match list_dir(dir, kind) {
        Ok(entries) => (entries, None),
        Err(err) => (Vec::new(), Some(err)),
    }
}

/// The rows of `dir`: folders first, then files, each set sorted by
/// name without regard to case; hidden entries left out, and files too
/// when only folders can be picked.
pub fn list_dir(dir: &Path, kind: PickerKind) -> Result<Vec<Entry>, String> {
    let read = std::fs::read_dir(dir).map_err(|err| match err.kind() {
        std::io::ErrorKind::PermissionDenied => {
            "Schist does not have permission to read this folder".to_string()
        }
        _ => err.to_string(),
    })?;
    let folders_only = matches!(kind, PickerKind::Open { files: false, .. });
    let mut entries = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        // `file_type` reports a symlink as a symlink; what matters is
        // what it points at.
        let is_dir = path.is_dir();
        if folders_only && !is_dir {
            continue;
        }
        entries.push(Entry { name, path, is_dir });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("schist-picker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Zeta")).unwrap();
        std::fs::create_dir_all(dir.join("alpha")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join("b.psd"), b"").unwrap();
        std::fs::write(dir.join("A.png"), b"").unwrap();
        std::fs::write(dir.join(".DS_Store"), b"").unwrap();
        dir
    }

    #[test]
    fn folders_come_first_then_files_each_by_name_ignoring_case() {
        let dir = scratch("order");
        let open = PickerKind::Open {
            files: true,
            directories: false,
            multiple: false,
        };
        let names: Vec<String> = list_dir(&dir, open)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["alpha", "Zeta", "A.png", "b.psd"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_folder_chooser_lists_no_files() {
        let dir = scratch("folders");
        let choose = PickerKind::Open {
            files: false,
            directories: true,
            multiple: true,
        };
        let entries = list_dir(&dir, choose).unwrap();
        assert!(entries.iter().all(|e| e.is_dir));
        assert_eq!(entries.len(), 2);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_unreadable_directory_is_an_error_not_a_panic() {
        let missing = std::env::temp_dir().join("schist-picker-no-such-dir");
        assert!(list_dir(&missing, PickerKind::Save).is_err());
    }
}
