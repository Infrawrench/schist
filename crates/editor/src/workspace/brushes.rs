//! Import and export reusable brush masks and portable recipe packs.
use super::*;
use schist_app_settings::brushes::import;
#[cfg(not(target_arch = "wasm32"))]
use schist_app_settings::brushes::MAX_BYTES;
use schist_i18n::{t, tf};

fn read_brushes(path: &std::path::Path) -> anyhow::Result<Vec<schist_plugin_api::BrushPreset>> {
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        bytes
    };
    #[cfg(target_arch = "wasm32")]
    let bytes = crate::web::read_file(path)?;
    let name = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    Ok(import::decode(&bytes, &extension, &name)?)
}

impl Workspace {
    pub fn import_brushes(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        #[cfg(not(target_arch = "wasm32"))]
        let paths = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(
                    format!(
                        "{} (.png, .jpg, .webp, .tiff, .gbr, .abr, .schist-brushes)",
                        t("common.import")
                    )
                    .into(),
                ),
            },
            cx,
        );
        #[cfg(target_arch = "wasm32")]
        let paths =
            crate::web::pick_file(".png,.jpg,.jpeg,.webp,.tif,.tiff,.gbr,.abr,.schist-brushes");
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
                .spawn(async move { read_brushes(&input) })
                .await;
            this.update(cx, |ws, cx| {
                match result {
                    Ok(presets) => {
                        let mut library = ws.brush_library.clone();
                        if let Some(first) =
                            library.import_presets(presets).filter(|_| library.save())
                        {
                            library.presets[first].apply(&mut ws.editor);
                            ws.brush_preset_name = library.presets[first].name.clone();
                            ws.status = ws.brush_preset_name.clone().into();
                            ws.brush_library = library;
                            ws.cloud_workflows_changed();
                        } else {
                            ws.status = t("common.failed").into();
                        }
                    }
                    Err(error) => {
                        log::warn!("Could not import brushes {}: {error}", path.display());
                        ws.status = if error.downcast_ref::<import::Error>()
                            == Some(&import::Error::Unsupported)
                        {
                            t("common.unsupported_format").into()
                        } else {
                            tf!("common.could_not_open", name = crate::ui::shown_path(&path)).into()
                        };
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn export_brushes(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Ok(bytes) = import::encode_pack(&self.brush_library) else {
            self.status = t("common.failed").into();
            cx.notify();
            return;
        };
        #[cfg(target_arch = "wasm32")]
        {
            if crate::web::download_bytes("brushes.schist-brushes", &bytes).is_err() {
                self.status = t("common.failed").into();
            }
            cx.notify();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let paths = self.prompt_for_new_path(&dir, Some("brushes.schist-brushes"), cx);
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
                    ws.status = if result.is_ok() {
                        crate::ui::shown_path(&path).into()
                    } else {
                        tf!("common.could_not_save", name = crate::ui::shown_path(&path)).into()
                    };
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}
