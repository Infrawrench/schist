//! Non-destructive photo decisions and the shared camera for comparison views.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CullFlag {
    #[default]
    None,
    Pick,
    Reject,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourLabel {
    #[default]
    None,
    Red,
    Yellow,
    Green,
    Blue,
    Magenta,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhotoCulling {
    #[serde(deserialize_with = "rating")]
    pub rating: u8,
    pub flag: CullFlag,
    pub label: ColourLabel,
}

fn rating<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let value = u8::deserialize(d)?;
    if value > 5 {
        return Err(serde::de::Error::custom("rating must be 0..=5"));
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullEdit {
    Rating(u8),
    Flag(CullFlag),
    Label(ColourLabel),
}

/// Update a batch without disturbing its other fields; default records take no space.
pub fn edit(records: &mut BTreeMap<PathBuf, PhotoCulling>, paths: &[PathBuf], edit: CullEdit) {
    for path in paths {
        let mut value = records.get(path).copied().unwrap_or_default();
        match edit {
            CullEdit::Rating(rating) => value.rating = rating.min(5),
            CullEdit::Flag(flag) => value.flag = flag,
            CullEdit::Label(label) => value.label = label,
        }
        if value == PhotoCulling::default() {
            records.remove(path);
        } else {
            records.insert(path.clone(), value);
        }
    }
}

/// Only call after a successful filesystem move. Failed moves retain their decisions.
pub fn moved(records: &mut BTreeMap<PathBuf, PhotoCulling>, from: &Path, to: &Path) {
    if from == to {
        return;
    }
    let record = records.remove(from);
    records.remove(to);
    if let Some(record) = record {
        records.insert(to.into(), record);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CullFilter {
    pub minimum_rating: u8,
    pub flag: Option<CullFlag>,
    pub label: Option<ColourLabel>,
}
impl CullFilter {
    pub fn matches(self, value: PhotoCulling) -> bool {
        value.rating >= self.minimum_rating
            && self.flag.is_none_or(|flag| flag == value.flag)
            && self.label.is_none_or(|label| label == value.label)
    }
}

/// Both panes show the same normalized point at the same magnification above fit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompareCamera {
    pub zoom: f32,
    pub center: [f32; 2],
}
impl Default for CompareCamera {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            center: [0.5, 0.5],
        }
    }
}
impl CompareCamera {
    pub fn zoom_by(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom = (self.zoom * factor).clamp(1.0, 32.0);
        }
        if self.zoom == 1.0 {
            self.center = [0.5, 0.5];
        }
    }
    pub fn image_rect(self, image: [f32; 2], pane: [f32; 2]) -> [f32; 4] {
        let fit = (pane[0] / image[0].max(1.0))
            .min(pane[1] / image[1].max(1.0))
            .min(1.0);
        let w = image[0] * fit * self.zoom;
        let h = image[1] * fit * self.zoom;
        let x = (pane[0] * 0.5 - self.center[0] * w)
            .clamp((pane[0] - w).min(0.0), (pane[0] - w).max(0.0));
        let y = (pane[1] * 0.5 - self.center[1] * h)
            .clamp((pane[1] - h).min(0.0), (pane[1] - h).max(0.0));
        [
            if w <= pane[0] { (pane[0] - w) * 0.5 } else { x },
            if h <= pane[1] { (pane[1] - h) * 0.5 } else { y },
            w,
            h,
        ]
    }
    pub fn pan(&mut self, delta: [f32; 2], rendered: [f32; 2]) {
        if self.zoom <= 1.0 {
            return;
        }
        for axis in 0..2 {
            if delta[axis].is_finite() && rendered[axis] > 0.0 {
                self.center[axis] =
                    (self.center[axis] - delta[axis] / rendered[axis]).clamp(0.0, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decisions_round_trip_and_legacy_libraries_still_load() {
        let mut file: crate::LibraryFile = serde_json::from_str(r#"{"folders":[]}"#).unwrap();
        assert!(file.culling.is_empty());
        let paths = [
            PathBuf::from("/photos/one.jpg"),
            PathBuf::from("/photos/two.jpg"),
        ];
        edit(&mut file.culling, &paths, CullEdit::Rating(4));
        edit(
            &mut file.culling,
            &paths[..1],
            CullEdit::Flag(CullFlag::Pick),
        );
        edit(
            &mut file.culling,
            &paths[1..],
            CullEdit::Label(ColourLabel::Blue),
        );
        let loaded: crate::LibraryFile =
            serde_json::from_slice(&serde_json::to_vec(&file).unwrap()).unwrap();
        assert_eq!(file.culling, loaded.culling);
        assert_eq!(loaded.culling[&paths[0]].flag, CullFlag::Pick);
        assert_eq!(loaded.culling[&paths[1]].rating, 4);
        assert!(serde_json::from_str::<PhotoCulling>(r#"{"rating":6}"#).is_err());
    }
    #[test]
    fn filters_combine_and_reset_leaves_no_record() {
        let mut records = BTreeMap::new();
        let paths = [PathBuf::from("a.jpg")];
        edit(&mut records, &paths, CullEdit::Rating(3));
        edit(&mut records, &paths, CullEdit::Flag(CullFlag::Reject));
        let filter = CullFilter {
            minimum_rating: 3,
            flag: Some(CullFlag::Pick),
            label: None,
        };
        assert!(!filter.matches(records[&paths[0]]));
        edit(&mut records, &paths, CullEdit::Flag(CullFlag::Pick));
        assert!(filter.matches(records[&paths[0]]));
        edit(&mut records, &paths, CullEdit::Rating(0));
        edit(&mut records, &paths, CullEdit::Flag(CullFlag::None));
        assert!(records.is_empty());
    }
    #[test]
    fn moves_follow_only_the_successful_originals() {
        let mut records = BTreeMap::new();
        let paths = [PathBuf::from("a.jpg"), PathBuf::from("b.jpg")];
        edit(&mut records, &paths, CullEdit::Rating(5));
        moved(&mut records, &paths[0], Path::new("new/a.jpg"));
        assert!(!records.contains_key(&paths[0]));
        assert_eq!(records[Path::new("new/a.jpg")].rating, 5);
        assert_eq!(records[&paths[1]].rating, 5);
    }
    #[test]
    fn comparison_keeps_same_normalized_detail_and_bounds_zoom() {
        let mut camera = CompareCamera::default();
        camera.zoom_by(2.0);
        camera.pan([-100.0, -50.0], [1000.0, 500.0]);
        let a = camera.image_rect([2000.0, 1000.0], [500.0, 250.0]);
        let b = camera.image_rect([4000.0, 2000.0], [500.0, 250.0]);
        assert_eq!(a, b);
        assert!((a[0] + 350.0).abs() < 0.01);
        assert!((a[1] + 175.0).abs() < 0.01);
        camera.zoom_by(f32::NAN);
        assert_eq!(camera.zoom, 2.0);
        camera.zoom_by(100.0);
        assert_eq!(camera.zoom, 32.0);
        camera.zoom_by(0.0001);
        assert_eq!(camera, CompareCamera::default());
    }
}
