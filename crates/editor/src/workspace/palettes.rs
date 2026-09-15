//! Imported swatches, shared between documents and retained across launches.

use super::*;
use schist_color::palette::{self, Palette};
use schist_i18n::{t, tf};
use std::path::Path;

pub const SEARCH_FIELD: &str = "palette-search";

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Palettes {
    pub entries: Vec<Palette>,
    /// Zero selects the built-in palette; imported palettes start at one.
    pub active: usize,
}

impl Palettes {
    pub fn load() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let text =
            schist_folder().and_then(|dir| std::fs::read_to_string(dir.join("palettes.json")).ok());
        #[cfg(target_arch = "wasm32")]
        let text = crate::web::local_get("schist.palettes");
        let mut state: Self = text
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        state.active = state.active.min(state.entries.len());
        state
    }

    pub fn selected(&self) -> Option<&Palette> {
        self.active.checked_sub(1).and_then(|i| self.entries.get(i))
    }

    fn save(&self) -> anyhow::Result<()> {
        let text = serde_json::to_string(self)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = schist_folder().ok_or_else(|| anyhow::anyhow!("No config directory"))?;
            std::fs::create_dir_all(&dir)?;
            let tmp = dir.join("palettes.json.tmp");
            std::fs::write(&tmp, text)?;
            std::fs::rename(tmp, dir.join("palettes.json"))?;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let storage = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .ok_or_else(|| anyhow::anyhow!("No local storage"))?;
            storage
                .set_item("schist.palettes", &text)
                .map_err(|_| anyhow::anyhow!("Could not store palettes"))?;
        }
        Ok(())
    }
}

pub fn is_palette(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["aco", "ase", "acb"]
                .iter()
                .any(|expected| ext.eq_ignore_ascii_case(expected))
        })
}

fn read_palette(path: &Path) -> anyhow::Result<Palette> {
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(palette::MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        bytes
    };
    #[cfg(target_arch = "wasm32")]
    let bytes = crate::web::read_file(path)?;
    Ok(palette::decode(
        &bytes,
        &path.file_stem().unwrap_or_default().to_string_lossy(),
    )?)
}

impl Workspace {
    pub fn import_palette(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        #[cfg(not(target_arch = "wasm32"))]
        let paths = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(format!("{} (.aco, .ase, .acb)", t("common.import")).into()),
            },
            cx,
        );
        #[cfg(target_arch = "wasm32")]
        let paths = crate::web::pick_file(".aco,.ase,.acb");
        cx.spawn(async move |this, cx| {
            #[cfg(not(target_arch = "wasm32"))]
            let path = match paths.await {
                Ok(Ok(Some(mut paths))) => paths.pop(),
                _ => None,
            };
            #[cfg(target_arch = "wasm32")]
            let path = paths.await.ok().flatten();
            if let Some(path) = path {
                this.update(cx, |ws, cx| ws.load_palette(path, cx)).ok();
            }
        })
        .detach();
    }

    pub(super) fn load_palette(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let input = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { read_palette(&input) })
                .await;
            this.update(cx, |ws, cx| {
                match result {
                    Ok(palette) => {
                        let name = palette.name.clone();
                        // Re-importing identical data selects the existing copy.
                        let index = ws
                            .palettes
                            .entries
                            .iter()
                            .position(|p| *p == palette)
                            .unwrap_or_else(|| {
                                ws.palettes.entries.push(palette);
                                ws.palettes.entries.len() - 1
                            });
                        ws.status = name.into();
                        ws.select_palette(index + 1, cx);
                        ws.side_tab = Some(SideTab::Color);
                    }
                    Err(err) => {
                        log::warn!("Could not import palette {}: {err}", path.display());
                        let message =
                            tf!("common.could_not_open", name = crate::ui::shown_path(&path));
                        ws.status = if matches!(
                            err.downcast_ref::<palette::Error>(),
                            Some(palette::Error::Unsupported)
                        ) {
                            format!("{message}: {}", t("common.unsupported_format")).into()
                        } else {
                            message.into()
                        };
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save_palettes(&mut self) {
        if let Err(err) = self.palettes.save() {
            log::warn!("Could not save imported palettes: {err}");
            self.status = tf!("common.could_not_save", name = "palettes.json").into();
        }
    }

    pub fn select_palette(&mut self, index: usize, cx: &mut Context<Self>) {
        self.commit_focused_field();
        self.palettes.active = index.min(self.palettes.entries.len());
        self.palette_search.clear();
        self.open_popup = None;
        self.save_palettes();
        cx.notify();
    }

    pub fn remove_palette(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.palettes.active.checked_sub(1) {
            if index < self.palettes.entries.len() {
                self.palettes.entries.remove(index);
            }
            self.select_palette(0, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_palettes_retain_source_values_names_and_selection() {
        let swatch = palette::Swatch {
            name: "Brand 123 C".into(),
            color: palette::Color::Lab([50.0, -25.0, 12.5]),
            group: "Print / Solid".into(),
            spot: true,
        };
        let book = Palette {
            name: "Custom Book".into(),
            swatches: vec![swatch],
        };
        let state = Palettes {
            entries: vec![book.clone()],
            active: 1,
        };
        let saved = serde_json::to_string(&state).unwrap();
        let restored: Palettes = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.selected(), Some(&book));
        let default = Palettes {
            active: 0,
            ..restored
        };
        assert!(default.selected().is_none());
        assert_eq!(default.entries, vec![book]);
    }

    #[test]
    fn palette_extensions_route_without_claiming_document_formats() {
        for path in ["Print.ACB", "custom.Aco", "交換.ase"] {
            assert!(is_palette(Path::new(path)));
        }
        for path in ["photo.psd", "picture.png", "palette.aco.png", "acb"] {
            assert!(!is_palette(Path::new(path)));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_import_reads_a_file_and_rejects_damage() {
        let dir = std::env::temp_dir().join(format!("schist-palettes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Custom.aco");
        // Version 1, one RGB swatch, pure red.
        std::fs::write(&path, [0, 1, 0, 1, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0]).unwrap();
        let parsed = read_palette(&path).unwrap();
        assert_eq!(parsed.name, "Custom");
        assert_eq!(parsed.swatches[0].color.to_rgb().to_u8(), [255, 0, 0, 255]);
        std::fs::write(&path, [0, 1, 0, 1, 0, 0]).unwrap();
        assert!(read_palette(&path).is_err());
        std::fs::File::create(&path)
            .unwrap()
            .set_len(palette::MAX_FILE_BYTES as u64 + 1)
            .unwrap();
        assert!(matches!(
            read_palette(&path)
                .unwrap_err()
                .downcast_ref::<palette::Error>(),
            Some(palette::Error::TooLarge)
        ));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
