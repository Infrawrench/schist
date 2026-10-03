//! Color Lookup layers (loading a `.cube`/`.3dl` into one) and
//! File ▸ Export ▸ Color Lookup Table.

use super::*;
use schist_adjustments::lut::MAX_FILE_BYTES;
use schist_adjustments::{ColorLookup, LutFormat, LutInput, LutTable, Params};
use schist_i18n::{t, tf};

/// Lattice sizes offered for export: the three in common use.
pub const EXPORT_SIZES: [usize; 3] = [17, 33, 65];

/// Camera Raw controls that act on neighbourhoods or on sensor data; a
/// lookup table cannot hold them, so they are neutral when baking.
const NOT_IN_A_LUT: [&str; 8] = [
    "temperature",
    "tint",
    "exposure",
    "clarity",
    "dehaze",
    "sharpening",
    "noise",
    "vignette",
];

fn read_lut(path: &std::path::Path) -> anyhow::Result<LutTable> {
    let format = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(LutFormat::from_extension)
        .unwrap_or(LutFormat::Cube);
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        bytes
    };
    #[cfg(target_arch = "wasm32")]
    let bytes = crate::web::read_file(path)?;
    Ok(LutTable::load(format, bytes)?)
}

impl Workspace {
    /// Edit the Color Lookup settings in the open adjustment dialog and
    /// preview them on the layer.
    fn update_color_lookup(&mut self, edit: impl FnOnce(&mut ColorLookup)) {
        let mut updated = None;
        self.update_modal(|m| {
            if let Modal::Adjustment {
                params: Params::ColorLookup(lookup),
                layer,
                ..
            } = m
            {
                edit(lookup);
                updated = Some((*layer, Params::ColorLookup(lookup.clone())));
            }
        });
        if let Some((layer, params)) = updated {
            self.preview_adjustment(layer, &params);
        }
    }

    pub fn set_color_lookup_input(&mut self, input: LutInput, cx: &mut Context<Self>) {
        self.update_color_lookup(|lookup| lookup.input = input);
        self.after_change(cx);
    }

    /// Ask for a `.cube` or `.3dl` file and load it into the Color Lookup
    /// layer whose dialog is open.
    pub fn load_color_lookup(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(target_arch = "wasm32"))]
        let paths = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(format!("{} (.cube, .3dl)", t("dialog.color_lookup.load")).into()),
            },
            cx,
        );
        #[cfg(target_arch = "wasm32")]
        let paths = crate::web::pick_file(".cube,.3dl");
        cx.spawn(async move |this, cx| {
            #[cfg(not(target_arch = "wasm32"))]
            let path = match paths.await {
                Ok(Ok(Some(mut paths))) => paths.pop(),
                _ => None,
            };
            #[cfg(target_arch = "wasm32")]
            let path = paths.await.ok().flatten();
            let Some(path) = path else {
                return;
            };
            let input = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { read_lut(&input) })
                .await;
            this.update(cx, |ws, cx| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match result {
                    Ok(table) => {
                        ws.update_color_lookup(|lookup| {
                            lookup.name = name.clone();
                            lookup.table = Some(table);
                        });
                        ws.status = name.into();
                        ws.after_change(cx);
                    }
                    Err(error) => {
                        log::warn!("Could not load LUT {}: {error}", path.display());
                        ws.status = tf!("dialog.color_lookup.invalid", name = name).into();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_export_lut(&mut self, cx: &mut Context<Self>) {
        if self.doc.is_none() {
            return;
        }
        self.open_modal(
            Modal::ExportLut {
                size: 33,
                camera_raw: true,
            },
            cx,
        );
    }

    /// The active layer's Camera Raw colour controls, as a pass over the
    /// lattice, when it is a RAW development.
    fn raw_colour_pass(&self) -> Option<impl FnOnce(&mut [f32], usize, usize)> {
        let doc = self.doc.as_ref()?;
        let raw = doc.tree.find(doc.active_layer?)?.raw.as_deref()?;
        let filter = self.registry.shared_filter("filter.camera_raw")?;
        let mut values = schist_plugin_api::FilterValues::defaults(&filter.params());
        filters::values_from_settings(raw.settings, &mut values);
        for key in NOT_IN_A_LUT {
            values.set(key, 0.0);
        }
        Some(move |rgba: &mut [f32], w: usize, h: usize| filter.apply(rgba, w, h, &values))
    }

    /// Bake the adjustment stack and ask where to save the `.cube`.
    pub fn export_lut(&mut self, size: usize, camera_raw: bool, cx: &mut Context<Self>) {
        let pass = if camera_raw {
            self.raw_colour_pass()
        } else {
            None
        };
        let Some(doc) = self.doc.as_ref() else { return };
        let title = doc.title.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let dir = doc
            .path
            .as_ref()
            .and_then(|p| p.parent().map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        let baked = schist_compositor::lut_bake::bake_adjustments(doc, size, |rgba, w, h| {
            if let Some(pass) = pass {
                pass(rgba, w, h);
            }
        });
        let bytes = schist_adjustments::lut::write_cube(&title, &baked.cube);
        let stem = std::path::Path::new(&title)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "schist".into());
        let file_name = format!("{stem}.cube");
        let skipped = baked.skipped;
        let done = move |name: &str| {
            if skipped > 0 {
                tf!(
                    "dialog.export_lut.saved_skipped",
                    name = name,
                    count = skipped
                )
            } else {
                tf!("dialog.export_lut.saved", name = name)
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            self.status = if crate::web::download_bytes(&file_name, &bytes).is_ok() {
                done(&file_name).into()
            } else {
                t("common.failed").into()
            };
            cx.notify();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let paths = self.prompt_for_new_path(&dir, Some(&file_name), cx);
            cx.spawn(async move |this, cx| {
                let Ok(Ok(Some(path))) = paths.await else {
                    return;
                };
                let target = path.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { std::fs::write(target, bytes) })
                    .await;
                this.update(cx, |ws, cx| {
                    let shown = crate::ui::shown_path(&path);
                    ws.status = match result {
                        Ok(()) => done(&shown).into(),
                        Err(_) => tf!("common.could_not_save", name = shown).into(),
                    };
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}
