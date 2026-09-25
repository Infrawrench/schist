//! Operating-system and browser integration, assets and path presentation.
#[cfg(target_os = "android")]
pub mod android;
pub mod assets;
pub mod clipboard;
#[cfg(not(any(target_arch = "wasm32", target_os = "ios", target_os = "android")))]
pub mod drag_out;
#[cfg(target_os = "linux")]
pub mod vulkan;
#[cfg(target_arch = "wasm32")]
pub mod web;

/// A path as the user should read it. The desktop shows it whole; on
/// iOS the app's container is a long opaque string that changes on
/// every install and means nothing to anyone, so a path under it shows
/// from the container down ("Documents/Photos/IMG_0111.heic") and any
/// other path by its name. Android's Documents folder is outside its
/// home and shown the same way; the device's shared folders show whole,
/// since "Pictures" is the name a user knows them by.
pub fn shown_path(path: &std::path::Path) -> String {
    if !cfg!(any(target_os = "ios", target_os = "android")) {
        return path.display().to_string();
    }
    #[cfg(target_os = "android")]
    {
        if let Some(files) =
            crate::android::documents_dir().and_then(|d| d.parent().map(|p| p.to_path_buf()))
        {
            if let Ok(rest) = path.strip_prefix(&files) {
                return rest.display().to_string();
            }
        }
        if path.starts_with("/storage/emulated/0") {
            return path.display().to_string();
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        if let Ok(rest) = path.strip_prefix(&home) {
            if let Some(rest) = rest.to_str() {
                if !rest.is_empty() {
                    return rest.to_string();
                }
            }
        }
    }
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
