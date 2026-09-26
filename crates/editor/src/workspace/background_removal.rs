//! Automatic background removal produces a reversible mask on the active layer.

use super::*;
use schist_core::automatic_mask::{Error, Session};
use schist_i18n::{t, tf};
use std::sync::atomic::AtomicBool;

fn message(error: Error) -> &'static str {
    t(match error {
        Error::NoRaster => "common.requires_raster_layer",
        Error::Locked => "common.layer_locked",
        Error::UnsupportedColor => "common.not_available",
        Error::TooLarge => "mask_refine.too_large",
        Error::InvalidMatte => "common.failed",
        Error::Changed => "common.cancelled",
    })
}

impl Workspace {
    pub fn remove_background(&mut self, cx: &mut Context<Self>) {
        if self.background_removal.is_some() {
            return;
        }
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        let session = match Session::capture(doc) {
            Ok(session) => session,
            Err(error) => {
                self.status = message(error).into();
                cx.notify();
                return;
            }
        };
        let detector_id = schist_neural::foreground_model_id();
        if !schist_neural::installed(detector_id) {
            // Downloads remain an explicit choice in the existing model UI.
            self.open_modal(Modal::ModelManager, cx);
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        self.background_removal = Some(cancelled.clone());
        self.status = t("common.working").into();
        let duplicate_name = t("mask_refine.output_layer");
        cx.notify();
        cx.spawn(async move |this, cx| {
            let worker_cancel = cancelled.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    schist_neural::with_adaptive_execution(|| {
                        if worker_cancel.load(Ordering::Relaxed) {
                            anyhow::bail!("cancelled");
                        }
                        let detector = schist_neural::get(detector_id)
                            .ok_or_else(|| anyhow::anyhow!("foreground model unavailable"))?;
                        // This larger detector is used once per layer action.
                        // The worker owns it; reclaim its plan after completion.
                        schist_neural::release(detector_id);
                        let (w, h) = session.dimensions();
                        let rgb = session.rgb();
                        let raw_coarse = schist_neural::foreground(&detector, &rgb, w, h)?;
                        drop(detector);
                        if worker_cancel.load(Ordering::Relaxed) {
                            anyhow::bail!("cancelled");
                        }
                        let reference = if detector_id == "foreground-matting" {
                            let general = schist_neural::get("foreground").ok_or_else(|| {
                                anyhow::anyhow!("general foreground model unavailable")
                            })?;
                            schist_neural::release("foreground");
                            Some(schist_neural::foreground(&general, &rgb, w, h)?)
                        } else {
                            None
                        };
                        if worker_cancel.load(Ordering::Relaxed) {
                            anyhow::bail!("cancelled");
                        }
                        let guide = schist_neural::get("subject-guide")
                            .ok_or_else(|| anyhow::anyhow!("subject guide unavailable"))?;
                        schist_neural::release("subject-guide");
                        let coarse = schist_neural::guide_foreground_with_reference(
                            &guide,
                            &rgb,
                            &raw_coarse,
                            reference.as_deref(),
                            w,
                            h,
                        )?;
                        drop(guide);
                        drop(reference);
                        drop(raw_coarse);
                        if worker_cancel.load(Ordering::Relaxed) {
                            anyhow::bail!("cancelled");
                        }
                        let refiner_id = schist_neural::matting_model_id();
                        let refiner = schist_neural::get(refiner_id)
                            .ok_or_else(|| anyhow::anyhow!("matting model unavailable"))?;
                        schist_neural::release(refiner_id);
                        let matte = schist_neural::refine_alpha_cancellable(
                            &refiner,
                            &rgb,
                            &coarse,
                            w,
                            h,
                            || worker_cancel.load(Ordering::Relaxed),
                        )?;
                        drop(refiner);
                        drop(coarse);
                        let foreground = schist_neural::clean_foreground_cancellable(
                            &rgb,
                            &matte,
                            w,
                            h,
                            || worker_cancel.load(Ordering::Relaxed),
                        )?;
                        session
                            .prepare_with_foreground(&matte, &foreground, duplicate_name)
                            .map_err(|e| anyhow::anyhow!("invalid generated mask: {e:?}"))
                    })
                })
                .await;
            let _ = this.update(cx, |ws, cx| {
                if !ws
                    .background_removal
                    .as_ref()
                    .is_some_and(|job| Arc::ptr_eq(job, &cancelled))
                {
                    return;
                }
                ws.background_removal = None;
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }
                match result {
                    Ok(prepared) => {
                        let applied =
                            ws.doc.as_mut().ok_or(Error::Changed).and_then(|doc| {
                                prepared.apply(doc, t("tool.background_eraser.name"))
                            });
                        match applied {
                            Ok(()) => {
                                ws.status = t("mask_refine.applied").into();
                                ws.after_change(cx);
                            }
                            Err(error) => ws.status = message(error).into(),
                        }
                    }
                    Err(error) => {
                        log::warn!("background removal: {error:#}");
                        ws.status = tf!(
                            "workspace.filters.failed",
                            name = t("tool.background_eraser.name"),
                            error = t("common.failed")
                        )
                        .into();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn cancel_background_removal(&mut self) -> bool {
        if let Some(cancelled) = self.background_removal.take() {
            cancelled.store(true, Ordering::Relaxed);
            self.status = t("common.cancelled").into();
            true
        } else {
            false
        }
    }
}
