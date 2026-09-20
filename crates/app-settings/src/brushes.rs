//! Bounded, portable brush preset storage, independent of document files.
use schist_plugin_api::BrushPreset;
use serde::{Deserialize, Serialize};

pub mod import;

pub const MAX_PRESETS: usize = 128;
pub const MAX_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_MASK_BYTES: usize = 4 * 1024 * 1024;
#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "schist.brush-presets.v1";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushLibrary {
    pub presets: Vec<BrushPreset>,
}

impl BrushLibrary {
    pub fn from_json(json: &str) -> Option<Self> {
        if json.len() > MAX_BYTES {
            return None;
        }
        let mut library: Self = serde_json::from_str(json).ok()?;
        if !library.valid() {
            return None;
        }
        let mut names = std::collections::HashSet::new();
        library.presets = library
            .presets
            .into_iter()
            .map(BrushPreset::sanitized)
            .filter(|preset| !preset.name.is_empty() && names.insert(preset.name.clone()))
            .collect();
        Some(library)
    }

    /// Save under a new name, or replace the recipe with that exact name.
    pub fn save_preset(&mut self, preset: BrushPreset) -> bool {
        let preset = preset.sanitized();
        if preset.name.is_empty() {
            return false;
        }
        let mut candidate = self.clone();
        if let Some(existing) = candidate.presets.iter_mut().find(|p| p.name == preset.name) {
            *existing = preset;
        } else {
            candidate.presets.push(preset);
        }
        if !candidate.valid() {
            return false;
        }
        *self = candidate;
        true
    }

    pub fn valid(&self) -> bool {
        self.presets.len() <= MAX_PRESETS
            && self.presets.iter().all(|p| {
                p.bitmap.as_ref().is_none_or(|b| b.valid())
                    && (p.dynamics.tip != schist_plugin_api::BrushTip::Bitmap || p.bitmap.is_some())
            })
            && self
                .presets
                .iter()
                .filter_map(|p| p.bitmap.as_ref())
                .map(|b| b.pixels.len())
                .sum::<usize>()
                <= MAX_MASK_BYTES
    }

    /// Atomic import. Repeated names receive a suffix; existing brushes are
    /// never overwritten. Returns the index of the first imported recipe.
    pub fn import_presets(&mut self, presets: Vec<BrushPreset>) -> Option<usize> {
        if presets.is_empty() || self.presets.len() + presets.len() > MAX_PRESETS {
            return None;
        }
        let first = self.presets.len();
        let mut candidate = self.clone();
        for preset in presets {
            if preset.bitmap.as_ref().is_some_and(|b| !b.valid())
                || (preset.dynamics.tip == schist_plugin_api::BrushTip::Bitmap
                    && preset.bitmap.is_none())
            {
                return None;
            }
            let mut preset = preset.sanitized();
            if preset.name.is_empty() {
                return None;
            }
            let base: String = preset.name.chars().take(56).collect();
            let mut suffix = 2;
            while candidate.presets.iter().any(|p| p.name == preset.name) {
                preset.name = format!("{base} ({suffix})");
                suffix += 1;
            }
            candidate.presets.push(preset);
        }
        if !candidate.valid() {
            return None;
        }
        *self = candidate;
        Some(first)
    }

    pub fn delete(&mut self, name: &str) {
        self.presets.retain(|preset| preset.name != name);
    }

    pub fn load() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let text = super::schist_folder().and_then(|folder| read_native(&folder));
        #[cfg(target_arch = "wasm32")]
        let text = schist_app_platform::web::local_get(STORAGE_KEY);
        text.and_then(|text| Self::from_json(&text))
            .unwrap_or_default()
    }

    /// Returns false when persistence failed so the editor can report it.
    pub fn save(&self) -> bool {
        let Ok(json) = serde_json::to_string(self) else {
            return false;
        };
        let Some(validated) = Self::from_json(&json) else {
            return false;
        };
        let Ok(json) = serde_json::to_string(&validated) else {
            return false;
        };
        #[cfg(target_arch = "wasm32")]
        {
            schist_app_platform::web::local_set(STORAGE_KEY, &json);
            schist_app_platform::web::local_get(STORAGE_KEY).as_deref() == Some(json.as_str())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(folder) = super::schist_folder() else {
                return false;
            };
            write_native(&folder, &json).is_ok()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_native(folder: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(folder.join("brush-presets.json")).ok()?;
    let mut text = String::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_native(folder: &std::path::Path, json: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let pending = folder.join(format!("brush-presets.{}.tmp", std::process::id()));
    std::fs::write(&pending, json)?;
    std::fs::rename(pending, folder.join("brush-presets.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_plugin_api::{BrushDynamics, BrushTip, EditorState};

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_storage_creates_replaces_and_reloads_the_library() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder =
            std::env::temp_dir().join(format!("schist-brush-test-{}-{unique}", std::process::id()));
        let mut library = BrushLibrary::default();
        library.save_preset(BrushPreset {
            name: "Ink".into(),
            ..Default::default()
        });
        write_native(&folder, &serde_json::to_string(&library).unwrap()).unwrap();
        let restored = BrushLibrary::from_json(&read_native(&folder).unwrap()).unwrap();
        assert_eq!(restored.presets, library.presets);
        library.delete("Ink");
        write_native(&folder, &serde_json::to_string(&library).unwrap()).unwrap();
        assert!(BrushLibrary::from_json(&read_native(&folder).unwrap())
            .unwrap()
            .presets
            .is_empty());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn preset_crud_roundtrip_restores_every_paint_parameter() {
        let state = EditorState {
            brush_size: 81.0,
            brush_hardness: 0.3,
            tool_opacity: 0.4,
            brush_dynamics: BrushDynamics {
                tip: BrushTip::Grain,
                scatter: 0.7,
                spacing: 0.3,
                stabilization: 12.0,
                pressure_gamma: 2.0,
                pressure_opacity: true,
                rotation: 42.0,
                tilt_rotation: true,
            },
            ..Default::default()
        };
        let mut library = BrushLibrary::default();
        let preset = BrushPreset::capture("  Ink  ".into(), &state);
        assert!(library.save_preset(preset.clone()));
        let mut restored = EditorState::default();
        let mut library =
            BrushLibrary::from_json(&serde_json::to_string(&library).unwrap()).unwrap();
        library.presets[0].apply(&mut restored);
        assert_eq!(BrushPreset::capture("Ink".into(), &restored), preset);
        assert!(library.save_preset(BrushPreset {
            size: 25.0,
            ..preset
        }));
        assert_eq!(library.presets.len(), 1);
        assert_eq!(library.presets[0].size, 25.0);
        library.delete("Ink");
        assert!(library.presets.is_empty());
    }

    #[test]
    fn stored_data_and_edits_are_bounded() {
        let json = r#"{"presets":[{"name":"brush","size":-5,"hardness":8,"opacity":-1,
            "dynamics":{"pressure_gamma":-1,"stabilization":9999,"scatter":9999}},
            {"name":"brush"},{"name":"  "}]}"#;
        let library = BrushLibrary::from_json(json).unwrap();
        assert_eq!(library.presets.len(), 1);
        assert_eq!(library.presets[0].size, 1.0);
        assert_eq!(library.presets[0].hardness, 1.0);
        assert_eq!(library.presets[0].dynamics.pressure_gamma, 0.25);
        assert_eq!(library.presets[0].dynamics.stabilization, 64.0);
        assert!(BrushLibrary::from_json(&" ".repeat(MAX_BYTES + 1)).is_none());
        assert!(BrushLibrary::from_json("{broken").is_none());
        let mut library = BrushLibrary::default();
        for i in 0..MAX_PRESETS {
            assert!(library.save_preset(BrushPreset {
                name: i.to_string(),
                ..Default::default()
            }));
        }
        assert!(!library.save_preset(BrushPreset {
            name: "extra".into(),
            ..Default::default()
        }));
        assert!(library.save_preset(BrushPreset {
            name: "0".into(),
            ..Default::default()
        }));
        let preset = BrushPreset {
            name: "a".repeat(100),
            size: f32::NAN,
            dynamics: BrushDynamics {
                scatter: f32::INFINITY,
                ..Default::default()
            },
            ..Default::default()
        }
        .sanitized();
        assert_eq!(preset.name.len(), 64);
        assert_eq!(preset.size, 24.0);
        assert_eq!(preset.dynamics.scatter, 0.0);
    }
}
