//! Editor bridge: metadata matching and explicit Lensfun XML import.
use super::*;
use schist_filters_core::lens_profiles::{database, Database};
use schist_i18n::t;

impl Workspace {
    pub(crate) fn seed_lens_profile(&self, values: &mut schist_plugin_api::FilterValues) {
        values.set("lp_enabled", 0.);
        values.set("lp_vignette", 0.);
        values.set("lp_aperture", 0.);
        values.set("lp_distance", 0.);
        values.set("lp_crop", 1.);
        schist_filters_core::lens_profiles::set_profile_id(values, 0);
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(layer) = doc.active_layer else {
            return;
        };
        // Calibration coordinates describe the full capture, never a selection.
        if !doc.selection.is_empty() || self.filter_region(layer) != doc.canvas_rect() {
            return;
        }
        let Some(exif) = self.exif.as_ref().and_then(|(_, s)| s.as_ref()) else {
            return;
        };
        if let Some(aperture) = exif.aperture_f_number {
            values.set("lp_aperture", aperture);
        }
        if let Some(distance) = exif.focus_distance_m {
            values.set("lp_distance", distance);
        }
        let (Some(maker), Some(model)) = (&exif.make, &exif.model) else {
            return;
        };
        let db = database().read().unwrap_or_else(|e| e.into_inner());
        if let Some(crop) = db.camera_crop(maker, model) {
            values.set("lp_crop", crop);
        }
        let Some(focal) = exif.focal_length_mm else {
            return;
        };
        values.set("lp_focal", focal);
        let Some(lens) = &exif.lens else { return };
        if let Some((id, crop)) = db.matching(maker, model, lens, focal) {
            if let Some(profile) = db.profiles.iter().find(|p| p.id == id) {
                profile.bake(focal, crop, values);
                values.set("lp_vignette", f32::from(self.lens_vignetting_supported()));
            }
        }
    }
    pub(crate) fn lens_vignetting_supported(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| {
            d.mode == schist_color::ColorMode::Rgb
                && schist_filters_core::lens_profiles::supported_srgb(d.icc_profile.as_deref())
        })
    }
    pub(crate) fn change_lens_profile(&mut self, profile: u64, cx: &mut Context<Self>) {
        let mut next = None;
        let mut failed = false;
        self.update_modal(|m| {
            if let Modal::Filter {
                id: "filter.lens_correction",
                values,
                preview,
                ..
            } = m
            {
                if profile == 0 {
                    values.set("lp_enabled", 0.);
                } else {
                    let db = database().read().unwrap_or_else(|e| e.into_inner());
                    schist_filters_core::lens_profiles::set_profile_id(values, profile);
                    failed = !db
                        .profiles
                        .iter()
                        .find(|p| p.id == profile)
                        .is_some_and(|p| {
                            p.bake(values.get("lp_focal"), values.get("lp_crop"), values)
                        });
                    // An unsuccessful request must not leave stale coefficients enabled.
                    if failed {
                        values.set("lp_enabled", 0.);
                    }
                }
                if *preview {
                    next = Some(values.clone());
                }
            }
        });
        if failed {
            self.status = t("lens_profiles.no_calibration").into();
        }
        if let Some(values) = next {
            self.preview_filter("filter.lens_correction", Some(&values), cx);
        }
        cx.notify();
    }
    pub(crate) fn import_lens_profiles(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let rx = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(t("lens_profiles.import").into()),
            },
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.pop() else { return };
            #[cfg(target_arch = "wasm32")]
            let bytes =
                crate::web::read_file(&path).map_err(|_| t("lens_profiles.invalid").to_owned());
            #[cfg(not(target_arch = "wasm32"))]
            let bytes = cx
                .background_executor()
                .spawn(async move {
                    use std::io::Read;
                    let file = std::fs::File::open(path)
                        .map_err(|_| t("lens_profiles.invalid").to_owned())?;
                    let mut bytes = Vec::new();
                    file.take(8 * 1024 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| t("lens_profiles.invalid").to_owned())?;
                    Ok::<_, String>(bytes)
                })
                .await;
            let parsed = cx
                .background_executor()
                .spawn(async move {
                    let bytes = bytes?;
                    let xml = std::str::from_utf8(&bytes)
                        .map_err(|_| t("lens_profiles.invalid").to_owned())?;
                    Database::parse(xml).map_err(str::to_owned)
                })
                .await;
            this.update_in(cx, |ws, _window, cx| {
                match parsed {
                    Ok(db) if !db.is_empty() => {
                        database()
                            .write()
                            .unwrap_or_else(|e| e.into_inner())
                            .merge(db);
                        ws.status = t("lens_profiles.imported").into();
                    }
                    _ => ws.status = t("lens_profiles.invalid").into(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
