//! Layout-only snapshots. Existing preference fields remain the live layout.
use crate::ViewOptions;
use schist_i18n::t;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PANELS: [&str; 5] = ["navigator", "color", "layers", "notes", "history"];
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub order: Vec<String>,
    pub heights: BTreeMap<String, f32>,
    pub hidden: Vec<String>,
    pub width: Option<f32>,
    pub visible: bool,
    pub ai: bool,
    pub history_height: f32,
}
impl Default for Layout {
    fn default() -> Self {
        Self::capture(&ViewOptions::default())
    }
}
impl Layout {
    pub fn capture(v: &ViewOptions) -> Self {
        Self {
            order: v.side_panel_order.clone(),
            heights: v.side_panel_heights.clone(),
            hidden: v.hidden_panels.clone(),
            width: v.panel_width,
            visible: v.side_panels,
            ai: v.ai_panel,
            history_height: v.history_h,
        }
        .sanitized()
    }
    pub fn sanitized(mut self) -> Self {
        let mut order = Vec::new();
        for key in self.order.iter().map(String::as_str).chain(PANELS) {
            if PANELS.contains(&key) && !order.iter().any(|s| s == key) {
                order.push(key.to_owned());
            }
        }
        self.order = order;
        self.hidden.retain(|s| PANELS.contains(&s.as_str()));
        self.hidden.sort();
        self.hidden.dedup();
        self.heights
            .retain(|key, h| PANELS.contains(&key.as_str()) && h.is_finite());
        for h in self.heights.values_mut() {
            *h = h.clamp(60.0, 1200.0);
        }
        self.width = self
            .width
            .filter(|w| w.is_finite())
            .map(|w| w.clamp(180.0, 600.0));
        self.history_height = if self.history_height.is_finite() {
            self.history_height.clamp(60.0, 1200.0)
        } else {
            150.0
        };
        self
    }
    pub fn apply(&self, v: &mut ViewOptions) {
        let s = self.clone().sanitized();
        v.side_panel_order = s.order;
        v.side_panel_heights = s.heights;
        v.hidden_panels = s.hidden;
        v.panel_width = s.width;
        v.side_panels = s.visible;
        v.ai_panel = s.ai;
        v.history_h = s.history_height;
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub layout: Layout,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePresets {
    pub saved: Vec<Preset>,
}
impl WorkspacePresets {
    pub fn name(&self, name: &str, except: Option<usize>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err(t("workspaces.invalid_name").into());
        }
        if self
            .saved
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != except && p.name.to_lowercase() == name.to_lowercase())
        {
            return Err(t("workspaces.duplicate").into());
        }
        Ok(name.to_owned())
    }
    pub fn save(&mut self, name: &str, layout: Layout) -> Result<(), String> {
        if self.saved.len() >= 64 {
            return Err(t("workspaces.limit").into());
        }
        let name = self.name(name, None)?;
        self.saved.push(Preset {
            name,
            layout: layout.sanitized(),
        });
        Ok(())
    }
    pub fn sanitize(&mut self) {
        let saved = std::mem::take(&mut self.saved);
        for preset in saved.into_iter().take(64) {
            let _ = self.save(&preset.name, preset.layout);
        }
    }
}
pub fn sanitize_view(mut view: ViewOptions) -> ViewOptions {
    Layout::capture(&view).apply(&mut view);
    view.workspaces.sanitize();
    view
}
pub fn starter(index: usize) -> Layout {
    let order = match index {
        0 => ["color", "layers", "navigator", "history", "notes"],
        1 => ["navigator", "color", "history", "layers", "notes"],
        _ => ["layers", "history", "navigator", "color", "notes"],
    };
    Layout {
        order: order.into_iter().map(str::to_owned).collect(),
        hidden: vec!["notes".into()],
        width: Some(if index == 1 { 300.0 } else { 260.0 }),
        heights: [(if index == 0 { "color" } else { "history" }.into(), 240.0)].into(),
        ..Default::default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Missing parent"))?;
    std::fs::create_dir_all(parent)?;
    // Unique staging file prevents one process truncating another's transaction.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let pending = parent.join(format!(
        ".preferences-{}-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&pending, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(pending);
    }
    result
}

/// A damaged preset must not discard unrelated application preferences.
pub fn deserialize_presets<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<WorkspacePresets, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let mut presets = WorkspacePresets::default();
    if let Some(saved) = value.get("saved").and_then(serde_json::Value::as_array) {
        for value in saved.iter().take(64) {
            if let Ok(preset) = serde_json::from_value::<Preset>(value.clone()) {
                let _ = presets.save(&preset.name, preset.layout);
            }
        }
    }
    Ok(presets)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_preferences_preserve_live_layout_and_unrelated_preferences() {
        let mut old = serde_json::to_value(ViewOptions::default()).unwrap();
        old.as_object_mut().unwrap().remove("workspaces");
        old["side_panel_order"] = serde_json::json!(["layers", "navigator"]);
        old["side_panel_heights"] = serde_json::json!({"layers": 330.0});
        old["crash_reports"] = true.into();
        old["note_author"] = "Reviewer".into();
        let mut migrated = sanitize_view(serde_json::from_value(old).unwrap());
        assert_eq!(migrated.side_panel_order[0], "layers");
        assert_eq!(migrated.side_panel_heights["layers"], 330.0);
        assert!(migrated.workspaces.saved.is_empty());
        starter(0).apply(&mut migrated);
        assert!(migrated.crash_reports);
        assert_eq!(migrated.note_author, "Reviewer");
    }
    #[test]
    fn sanitize_unknown_duplicate_and_nonfinite_layout_values() {
        let layout = Layout {
            order: vec!["unknown".into(), "layers".into(), "layers".into()],
            hidden: vec!["unknown".into(), "color".into(), "color".into()],
            width: Some(f32::NAN),
            history_height: f32::INFINITY,
            heights: [
                ("unknown".into(), 100.0),
                ("color".into(), -5.0),
                ("layers".into(), f32::NAN),
            ]
            .into(),
            ..Default::default()
        };
        let layout = layout.sanitized();
        assert_eq!(layout.order.len(), 5);
        assert_eq!(layout.order[0], "layers");
        assert_eq!(layout.hidden, ["color"]);
        assert_eq!(layout.width, None);
        assert_eq!(layout.history_height, 150.0);
        assert_eq!(layout.heights, [("color".into(), 60.0)].into());
    }
    #[test]
    fn names_are_trimmed_bounded_and_case_insensitively_unique() {
        let mut presets = WorkspacePresets::default();
        presets.save("  Paint  ", starter(0)).unwrap();
        assert_eq!(presets.saved[0].name, "Paint");
        for invalid in ["paint", "", "   ", "bad\nname", &"x".repeat(81)] {
            assert!(presets.save(invalid, Layout::default()).is_err());
        }
        assert_eq!(presets.saved.len(), 1);
        assert_eq!(presets.name("PAINT", Some(0)).unwrap(), "PAINT");
    }
    #[test]
    fn invalid_saved_entries_do_not_discard_good_entries_or_other_preferences() {
        let mut json = serde_json::to_value(ViewOptions::default()).unwrap();
        json["note_author"] = "Keep me".into();
        json["workspaces"] = serde_json::json!({"saved": [
            {"name": "Broken", "layout": {"width": "wide"}},
            {"name": "Good", "layout": {}},
            {"name": "good", "layout": {}}
        ]});
        let view: ViewOptions = serde_json::from_value(json).unwrap();
        assert_eq!(view.note_author, "Keep me");
        assert_eq!(view.workspaces.saved.len(), 1);
        assert_eq!(view.workspaces.saved[0].name, "Good");
    }
    #[test]
    fn corrupted_collection_and_preset_limit_are_bounded() {
        for invalid in [
            serde_json::Value::Null,
            serde_json::json!("broken"),
            serde_json::json!({"saved": false}),
        ] {
            let mut json = serde_json::to_value(ViewOptions::default()).unwrap();
            json["note_author"] = "Retained".into();
            json["workspaces"] = invalid;
            let view: ViewOptions = serde_json::from_value(json).unwrap();
            assert_eq!(view.note_author, "Retained");
            assert!(view.workspaces.saved.is_empty());
        }
        let mut presets = WorkspacePresets::default();
        for i in 0..64 {
            presets
                .save(&format!("Layout {i}"), starter(i % 3))
                .unwrap();
        }
        assert!(presets.save("Overflow", Layout::default()).is_err());
        assert_eq!(presets.saved.len(), 64);
    }

    #[test]
    fn reset_leaves_saved_presets_and_nonlayout_preferences_untouched() {
        let mut view = ViewOptions {
            ai_backend: "codex".into(),
            ai_panel_gallery: true,
            grid: true,
            crash_upload: true,
            gpu_compositing: false,
            ..Default::default()
        };
        view.workspaces.save("Mine", starter(2)).unwrap();
        starter(0).apply(&mut view);
        Layout::default().apply(&mut view);
        assert_eq!(Layout::capture(&view), Layout::default());
        assert_eq!(view.workspaces.saved.len(), 1);
        assert!(view.grid && view.crash_upload && view.ai_panel_gallery);
        assert!(!view.gpu_compositing);
        assert_eq!(view.ai_backend, "codex");
    }
    #[test]
    fn json_round_trip_keeps_layouts_independent() {
        let mut view = ViewOptions::default();
        for i in 0..3 {
            view.workspaces.save(&i.to_string(), starter(i)).unwrap();
        }
        let snapshot = view.workspaces.saved[0].layout.clone();
        starter(1).apply(&mut view);
        view.side_panel_heights.insert("color".into(), 444.0);
        let restored: ViewOptions =
            serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
        assert_eq!(restored.workspaces.saved[0].layout, snapshot);
        assert_eq!(Layout::capture(&restored), Layout::capture(&view));
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn atomic_persistence_replaces_existing_and_preserves_target_on_failure() {
        let dir =
            std::env::temp_dir().join(format!("schist-workspaces-test-{}", std::process::id()));
        let path = dir.join("preferences.json");
        atomic_write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let blocked = dir.join("directory");
        std::fs::create_dir_all(&blocked).unwrap();
        assert!(atomic_write(&blocked, b"failure").is_err());
        assert!(blocked.is_dir());
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
