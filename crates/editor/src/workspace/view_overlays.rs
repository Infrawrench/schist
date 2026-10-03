//! Viewer overlays and proofing aids: clipping warnings and focus peaking
//! (canvas and gallery viewers), and colour-vision simulation (canvas).
//!
//! All of them are display-only. The overlays are computed on the image
//! as displayed — the resampled viewport frame, or a gallery photo's
//! decoded preview cut down to the size it is shown at — and cached with
//! it; colour-vision simulation rides the display transform.

use super::*;
use schist_colormgmt::{VisionDeficiency, VisionSimulation};
use schist_compositor::overlay::{Overlays, Peaking};
use schist_i18n::{t, tf};

/// Focus peaking's colour choices: the label key and RGB.
pub(crate) const PEAKING_COLORS: [(&str, [u8; 3]); 6] = [
    ("common.red", [255, 40, 40]),
    ("common.yellow", [255, 230, 0]),
    ("common.green", [40, 255, 40]),
    ("common.cyan", [0, 230, 255]),
    ("common.magenta", [255, 0, 255]),
    ("common.white", [255, 255, 255]),
];

/// Focus peaking's sensitivity labels, in `Peaking::SENSITIVITY` order.
pub(crate) const PEAKING_SENSITIVITY: [&str; 3] = [
    "menu.view.peaking_low",
    "menu.view.peaking_medium",
    "menu.view.peaking_high",
];

/// The anomalous vision severities offered: label key and severity.
pub(crate) const VISION_SEVERITY: [(&str, f32); 3] = [
    ("menu.view.vision_mild", 0.3),
    ("menu.view.vision_moderate", 0.6),
    ("menu.view.vision_strong", 0.9),
];

/// The View ▸ Color Vision choices, `AppItem::VisionSimulation(i)` order:
/// normal vision first.
pub(crate) fn vision_label(index: u8) -> &'static str {
    t(
        match index
            .checked_sub(1)
            .and_then(|i| VisionDeficiency::ALL.get(i as usize))
        {
            None => "menu.view.vision_normal",
            Some(VisionDeficiency::Protanopia) => "menu.view.vision_protanopia",
            Some(VisionDeficiency::Deuteranopia) => "menu.view.vision_deuteranopia",
            Some(VisionDeficiency::Tritanopia) => "menu.view.vision_tritanopia",
            Some(VisionDeficiency::Protanomaly) => "menu.view.vision_protanomaly",
            Some(VisionDeficiency::Deuteranomaly) => "menu.view.vision_deuteranomaly",
            Some(VisionDeficiency::Tritanomaly) => "menu.view.vision_tritanomaly",
            Some(VisionDeficiency::Achromatopsia) => "menu.view.vision_achromatopsia",
        },
    )
}

/// Which overlays are switched on. Session-only: a warning overlay left
/// on from yesterday would read as damage to the photo.
#[derive(Default, Clone, Copy)]
pub(crate) struct OverlaySwitches {
    pub clipping: bool,
    pub focus_peaking: bool,
}

/// The keymap id an overlay item is rebound by (`"view:<id>"`).
pub(crate) fn item_for_view_id(id: &str) -> Option<AppItem> {
    Some(match id {
        "clipping" => AppItem::ToggleClipping,
        "focus_peaking" => AppItem::ToggleFocusPeaking,
        "vision.normal" => AppItem::VisionSimulation(0),
        other => {
            let kind = VisionDeficiency::from_id(other.strip_prefix("vision.")?)?;
            let index = VisionDeficiency::ALL.iter().position(|&k| k == kind)?;
            AppItem::VisionSimulation(index as u8 + 1)
        }
    })
}

impl Workspace {
    /// The overlays to draw, from the switches and the saved options.
    pub(crate) fn overlays(&self) -> Overlays {
        let sensitivity = (self.view.peaking_sensitivity as usize).min(2);
        let color = PEAKING_COLORS
            .get(self.view.peaking_color as usize)
            .unwrap_or(&PEAKING_COLORS[1])
            .1;
        Overlays {
            clipping: self.overlays.clipping,
            peaking: self.overlays.focus_peaking.then_some(Peaking {
                color,
                threshold: Peaking::SENSITIVITY[sensitivity],
            }),
        }
    }

    /// Run a keymap `ViewOverlay` action through the menu item it names.
    pub(crate) fn run_view_overlay(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item_for_view_id(id) {
            Some(item) => panels::run_app_item(self, item, window, cx),
            None => log::warn!("keymap: unknown view overlay {id:?}"),
        }
    }

    pub fn toggle_clipping(&mut self, cx: &mut Context<Self>) {
        self.overlays.clipping = !self.overlays.clipping;
        self.status = t(if self.overlays.clipping {
            "workspace.canvas.clipping_on"
        } else {
            "workspace.canvas.clipping_off"
        })
        .into();
        self.overlays_changed(cx);
    }

    pub fn toggle_focus_peaking(&mut self, cx: &mut Context<Self>) {
        self.overlays.focus_peaking = !self.overlays.focus_peaking;
        self.status = t(if self.overlays.focus_peaking {
            "workspace.canvas.peaking_on"
        } else {
            "workspace.canvas.peaking_off"
        })
        .into();
        self.overlays_changed(cx);
    }

    /// Pick a peaking colour or sensitivity. Choosing one is asking to
    /// see it, so peaking turns on too.
    pub fn set_peaking(
        &mut self,
        color: Option<u8>,
        sensitivity: Option<u8>,
        cx: &mut Context<Self>,
    ) {
        if let Some(color) = color {
            self.view.peaking_color = color.min(PEAKING_COLORS.len() as u8 - 1);
        }
        if let Some(sensitivity) = sensitivity {
            self.view.peaking_sensitivity = sensitivity.min(2);
        }
        self.overlays.focus_peaking = true;
        self.save_view_options();
        self.overlays_changed(cx);
    }

    /// The canvas frame and the gallery's overlay layers are keyed on the
    /// overlay settings, so a change only needs a repaint; the gallery's
    /// stale layers go now rather than waiting to be evicted.
    fn overlays_changed(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let retired = self.gallery_overlays.clear();
            self.retired_images.extend(retired);
        }
        cx.notify();
    }

    /// `AppItem::VisionSimulation` index of the simulation in effect.
    pub(crate) fn vision_index(&self) -> u8 {
        self.color
            .vision
            .and_then(|v| {
                VisionDeficiency::ALL
                    .iter()
                    .position(|&k| k == v.deficiency)
            })
            .map_or(0, |i| i as u8 + 1)
    }

    /// Simulate a colour vision deficiency, or (index 0) stop.
    pub fn set_vision(&mut self, index: u8, cx: &mut Context<Self>) {
        let kind = index
            .checked_sub(1)
            .and_then(|i| VisionDeficiency::ALL.get(i as usize).copied());
        self.color.vision = kind.map(|kind| VisionSimulation::new(kind, self.vision_severity()));
        self.status = match kind {
            Some(_) => tf!("workspace.colormgmt.vision_on", name = vision_label(index)).into(),
            None => t("workspace.colormgmt.vision_off").into(),
        };
        self.rebuild_color_transforms();
        cx.notify();
    }

    /// Choose the anomalous kinds' severity, re-simulating if one is on.
    pub fn set_vision_severity(&mut self, index: u8, cx: &mut Context<Self>) {
        self.view.vision_severity = index.min(VISION_SEVERITY.len() as u8 - 1);
        self.save_view_options();
        if let Some(vision) = self.color.vision {
            self.color.vision = Some(VisionSimulation::new(
                vision.deficiency,
                self.vision_severity(),
            ));
            self.rebuild_color_transforms();
        }
        cx.notify();
    }

    fn vision_severity(&self) -> f32 {
        VISION_SEVERITY
            .get(self.view.vision_severity as usize)
            .unwrap_or(&VISION_SEVERITY[1])
            .1
    }

    /// The overlay layer for a gallery photo shown `shown` logical pixels
    /// across its longer side, or `None` when no overlay is on.
    ///
    /// Computed from the photo's decoded preview cut down to about the
    /// size it is displayed at (twice the logical size, for HiDPI), in
    /// 512 px steps so a zoom only recomputes when it crosses one, and
    /// never past 4096 px however far the comparison view zooms in.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn gallery_overlay(
        &mut self,
        source: &Arc<RenderImage>,
        shown: f32,
    ) -> Option<Arc<RenderImage>> {
        let overlays = self.overlays();
        if overlays.is_empty() {
            return None;
        }
        let size = source.size(0);
        let (width, height) = (size.width.0.max(0) as usize, size.height.0.max(0) as usize);
        let wanted = ((shown.max(1.0) * 2.0 / 512.0).ceil() as usize * 512).min(4096);
        let edge = wanted.min(width.max(height));
        let key = (overlays.key(), edge);
        if let Some(layer) = self.gallery_overlays.get(source, key) {
            return Some(layer);
        }
        let bytes = source.as_bytes(0)?;
        let (small, w, h) = schist_compositor::overlay::downsample_bgra(bytes, width, height, edge);
        let layer = schist_compositor::overlay::layer_bgra(&small, w, h, &overlays);
        let buffer = image::RgbaImage::from_raw(w as u32, h as u32, layer)?;
        let image = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
        let retired = self.gallery_overlays.insert(source, key, image.clone());
        self.retired_images.extend(retired);
        Some(image)
    }
}

/// Overlay layers for the photos the gallery is showing: the viewer's
/// one, a comparison's two, a burst review's two. Keyed on the source
/// image itself, so a new decode never reuses an old photo's layer.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(crate) struct GalleryOverlays {
    entries: Vec<GalleryOverlay>,
}

#[cfg(not(target_arch = "wasm32"))]
struct GalleryOverlay {
    source: std::sync::Weak<RenderImage>,
    key: (u64, usize),
    layer: Arc<RenderImage>,
}

#[cfg(not(target_arch = "wasm32"))]
impl GalleryOverlays {
    /// More than any one gallery view shows at once.
    const CAPACITY: usize = 6;

    fn get(&self, source: &Arc<RenderImage>, key: (u64, usize)) -> Option<Arc<RenderImage>> {
        self.entries
            .iter()
            .find(|e| e.key == key && std::ptr::eq(e.source.as_ptr(), Arc::as_ptr(source)))
            .filter(|e| e.source.strong_count() > 0)
            .map(|e| e.layer.clone())
    }

    /// Remember a layer, returning the images it displaces: those for the
    /// same photo at other settings, those whose photo is gone, and the
    /// oldest past capacity. The caller retires them from the atlas.
    fn insert(
        &mut self,
        source: &Arc<RenderImage>,
        key: (u64, usize),
        layer: Arc<RenderImage>,
    ) -> Vec<Arc<RenderImage>> {
        let mut retired = Vec::new();
        self.entries.retain(|e| {
            let keep = e.source.strong_count() > 0
                && !std::ptr::eq(e.source.as_ptr(), Arc::as_ptr(source));
            if !keep {
                retired.push(e.layer.clone());
            }
            keep
        });
        if self.entries.len() >= Self::CAPACITY {
            retired.push(self.entries.remove(0).layer);
        }
        self.entries.push(GalleryOverlay {
            source: Arc::downgrade(source),
            key,
            layer,
        });
        retired
    }

    fn clear(&mut self) -> Vec<Arc<RenderImage>> {
        self.entries.drain(..).map(|e| e.layer).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vision_choice_has_a_keymap_id() {
        assert_eq!(
            item_for_view_id("vision.normal"),
            Some(AppItem::VisionSimulation(0))
        );
        for (i, kind) in VisionDeficiency::ALL.into_iter().enumerate() {
            assert_eq!(
                item_for_view_id(&format!("vision.{}", kind.id())),
                Some(AppItem::VisionSimulation(i as u8 + 1))
            );
        }
        assert_eq!(item_for_view_id("clipping"), Some(AppItem::ToggleClipping));
        assert_eq!(
            item_for_view_id("focus_peaking"),
            Some(AppItem::ToggleFocusPeaking)
        );
        assert_eq!(item_for_view_id("vision.bogus"), None);
        assert_eq!(item_for_view_id("histogram"), None);
    }

    #[test]
    fn the_default_keys_name_real_overlays() {
        for (_, id) in schist_app_actions::keymap::DEFAULT_VIEW_OVERLAY_KEYS {
            assert!(item_for_view_id(id).is_some(), "{id}");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gallery_layers_follow_their_photo() {
        let image = || {
            let buffer = image::RgbaImage::from_raw(1, 1, vec![0, 0, 0, 255]).unwrap();
            Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]))
        };
        let mut cache = GalleryOverlays::default();
        let (photo, layer) = (image(), image());
        assert!(cache.insert(&photo, (1, 512), layer.clone()).is_empty());
        assert!(cache.get(&photo, (1, 512)).is_some());
        assert!(cache.get(&photo, (2, 512)).is_none(), "settings changed");
        // A new layer for the same photo displaces the old one.
        let retired = cache.insert(&photo, (2, 512), image());
        assert_eq!(retired.len(), 1);
        assert!(Arc::ptr_eq(&retired[0], &layer));
        // A photo that has been dropped is never matched, and goes on
        // the next insert.
        let other = image();
        cache.insert(&other, (2, 512), image());
        drop(other);
        assert_eq!(cache.insert(&image(), (2, 512), image()).len(), 1);
        assert_eq!(cache.clear().len(), 2);
    }
}
