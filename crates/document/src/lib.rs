//! The editor's document model and built-in codecs without GPUI, GPU, or networking.
//! Callers can register additional trusted codecs in the same PluginRegistry.
use anyhow::{ensure, Context, Result};
use schist_core::Document;
use schist_plugin_api::{ExportOptions, PluginManifest, PluginRegistry};
use std::path::Path;
mod shared;
pub use shared::SharedDocument;
pub const MODEL: &str = "schist.image.v1";

/// The same built-in registrations, in the same order, as the desktop app.
pub fn registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    schist_codecs_common::CommonCodecsPlugin.register(&mut registry);
    schist_codecs_common::PsdPlugin.register(&mut registry);
    registry
}

#[derive(serde::Serialize)]
pub struct Format {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub can_export: bool,
    pub runtime_requirement: Option<&'static str>,
}
pub fn formats(registry: &PluginRegistry) -> Vec<Format> {
    registry
        .codecs()
        .map(|c| Format {
            id: c.id(),
            name: c.name(),
            extensions: c.extensions(),
            can_export: c.can_export(),
            runtime_requirement: (c.id() == "codec.heif")
                .then_some("libheif 1.23.4 or later with a compatible HEVC decoder"),
        })
        .collect()
}

/// Read a **page layout** document.
///
/// Separate from [`import`] because it is a different document type, and
/// because the two are never interchangeable: an IDML is a page, and
/// trying to decode it as pixels would produce a very small grey image
/// rather than an error.
///
/// The extension is only a hint. The codec's own media-type check decides,
/// because a file's name is the part of it a user can get wrong.
pub fn import_layout(
    registry: &PluginRegistry,
    bytes: &[u8],
    name: &str,
) -> Result<(schist_layout::LayoutDocument, Vec<String>)> {
    let extension = Path::new(name).extension().and_then(|s| s.to_str());
    let codec = registry
        .layout_codec_for(bytes, extension)
        .context("No Schist layout codec recognizes this file")?;
    let (mut document, skipped) = codec.read_layout(bytes)?;
    // The document carries no name of its own, so the file's name is
    // what the Pages panel will show.
    document.name = Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(name)
        .to_owned();
    Ok((document, skipped))
}

/// Whether a file is a page layout document rather than a raster one.
///
/// Asked before the open path commits to a document type, because the
/// answer decides which half of the workspace the file lands in.
pub fn is_layout(registry: &PluginRegistry, bytes: &[u8], name: &str) -> bool {
    let extension = Path::new(name).extension().and_then(|s| s.to_str());
    registry.layout_codec_for(bytes, extension).is_some()
}

/// Every format the shell can open, raster and layout alike, for a file
/// dialog's type filter.
pub fn importable_extensions(registry: &PluginRegistry) -> Vec<String> {
    let mut out: Vec<String> = formats(registry)
        .iter()
        .flat_map(|format| format.extensions.iter().map(|e| e.to_string()))
        .collect();
    out.extend(
        registry
            .layout_codecs()
            .flat_map(|codec| codec.extensions().iter().map(|e| e.to_string())),
    );
    out.sort_unstable();
    out.dedup();
    out
}

pub fn import(registry: &PluginRegistry, bytes: &[u8], name: &str) -> Result<Document> {
    let extension = Path::new(name).extension().and_then(|s| s.to_str());
    let codec = registry
        .codec_for(bytes, extension)
        .context("No Schist codec recognizes this file")?;
    let mut document = codec.import(bytes)?;
    document.title = name.to_owned();
    document.path = None;
    Ok(document)
}

/// Reconstruct the native model from a Yjs v1 update without inventing a seed.
pub fn materialize(state: &[u8]) -> Result<Document> {
    let placeholder = Document::new("unseeded", 1, 1, schist_color::Depth::Eight);
    let mut shared = SharedDocument::unseeded(&placeholder)?;
    shared.apply(state)?;
    shared.render()
}

pub struct Export {
    pub bytes: Vec<u8>,
    pub extension: String,
    pub mime_type: &'static str,
}
/// PSD is the default editable interchange format, including imports whose own
/// format is read-only (camera raw and HEIC). Other formats require an explicit
/// choice: flattened exports never silently replace the layered original.
pub fn export(registry: &PluginRegistry, document: &Document, extension: &str) -> Result<Export> {
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    let codec = registry
        .codec_for(&[], Some(&extension))
        .context("Unknown export format")?;
    ensure!(
        codec.can_export(),
        "{} is import-only; export an editable PSD instead",
        codec.name()
    );
    let depth = match document.depth {
        schist_color::Depth::Eight => 8,
        schist_color::Depth::Sixteen => 16,
        schist_color::Depth::ThirtyTwo => 32,
    };
    let bytes = codec.export_with(
        document,
        &ExportOptions {
            bit_depth: depth,
            dither: false,
            ..Default::default()
        },
    )?;
    // The PSD writer automatically selects PSB for documents exceeding PSD limits.
    let extension = if codec.id() == "codec.psd" {
        if bytes.get(4..6) == Some(&[0, 2]) {
            "psb"
        } else {
            "psd"
        }
    } else {
        extension.as_str()
    }
    .to_owned();
    let mime_type = match codec.id() {
        "codec.psd" => "image/vnd.adobe.photoshop",
        "codec.png" => "image/png",
        "codec.jpeg" => "image/jpeg",
        "codec.webp" => "image/webp",
        "codec.tiff" => "image/tiff",
        "codec.jxl" => "image/jxl",
        "codec.avif" => "image/avif",
        _ => "application/octet-stream",
    };
    Ok(Export {
        bytes,
        extension,
        mime_type,
    })
}

#[cfg(all(test, target_arch = "wasm32"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
