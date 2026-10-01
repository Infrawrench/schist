//! OpenType defaults whose tag depends on the paragraph's writing mode.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DirectionalFeatures {
    pub kana: Option<bool>,
    pub proportional_metrics: Option<bool>,
}
impl DirectionalFeatures {
    pub fn over(self, base: Self) -> Self {
        Self {
            kana: self.kana.or(base.kana),
            proportional_metrics: self.proportional_metrics.or(base.proportional_metrics),
        }
    }

    /// A nearer mode switch resets both inherited tags of its pair. Explicit
    /// per-tag choices at the same or a nearer level still take precedence.
    pub fn inherit_features(
        self,
        near: &[(String, bool)],
        base: &[(String, bool)],
    ) -> Vec<(String, bool)> {
        let base: Vec<_> = base
            .iter()
            .filter(|(tag, _)| {
                !(self.kana.is_some() && matches!(tag.as_str(), "hkna" | "vkna")
                    || self.proportional_metrics.is_some()
                        && matches!(tag.as_str(), "palt" | "vpal"))
            })
            .cloned()
            .collect();
        crate::styles::inherited_features(near, &base)
    }

    /// Select only the tag for this axis. Explicit false clears both axes;
    /// absent defaults add no override to the paragraph/document's features.
    pub fn selected(self, explicit: &[(String, bool)], vertical: bool) -> Vec<(String, bool)> {
        let mut defaults = Vec::new();
        for (enabled, tags) in [
            (self.kana, ["hkna", "vkna"]),
            (self.proportional_metrics, ["palt", "vpal"]),
        ] {
            if let Some(enabled) = enabled {
                defaults.extend(tags.into_iter().enumerate().map(|(index, tag)| {
                    (tag.to_owned(), enabled && index == usize::from(vertical))
                }));
            }
        }
        crate::styles::inherited_features(explicit, &defaults)
    }
}
