//! Refine Mask's isolated preview and commit boundary.

use super::*;
use schist_core::mask_refine::{Image, RefineError, Session, Settings};
use schist_i18n::t;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    Checkerboard,
    Black,
    White,
    Magenta,
    Mask,
}

impl Background {
    pub const ALL: [Self; 5] = [
        Self::Checkerboard,
        Self::Black,
        Self::White,
        Self::Magenta,
        Self::Mask,
    ];
    pub fn label(self) -> &'static str {
        t(match self {
            Self::Checkerboard => "mask_refine.checkerboard",
            Self::Black => "mask_refine.black",
            Self::White => "mask_refine.white",
            Self::Magenta => "mask_refine.magenta",
            Self::Mask => "mask_refine.mask_only",
        })
    }
}

pub(super) struct State {
    session: Arc<Session>,
    input: Image,
    preview: Option<(Settings, Background, bool, Arc<RenderImage>)>,
    error: Option<&'static str>,
    processing: bool,
    cancelled: Arc<AtomicBool>,
}

fn error_message(error: RefineError) -> &'static str {
    t(match error {
        RefineError::NoRaster => "mask_refine.no_raster",
        RefineError::Locked => "mask_refine.locked",
        RefineError::NoMask => "mask_refine.no_mask",
        RefineError::Changed => "mask_refine.changed",
        RefineError::UnsupportedColor => "mask_refine.rgb_only",
        RefineError::TooLarge => "mask_refine.too_large",
    })
}

impl Workspace {
    pub fn open_mask_refine(&mut self, cx: &mut Context<Self>) {
        let captured = self
            .doc
            .as_ref()
            .ok_or(RefineError::NoRaster)
            .and_then(Session::capture)
            .and_then(|session| {
                session.sample(720).map(|input| State {
                    session: Arc::new(session),
                    input,
                    preview: None,
                    error: None,
                    processing: false,
                    cancelled: Arc::new(AtomicBool::new(false)),
                })
            });
        match captured {
            Ok(state) => {
                self.cancel_mask_refine();
                self.mask_refine = Some(state);
                self.open_modal(
                    Modal::MaskRefine {
                        settings: Settings::default(),
                        background: Background::Checkerboard,
                        original: false,
                        zoom: 1.0,
                    },
                    cx,
                );
            }
            Err(error) => {
                self.status = error_message(error).into();
                cx.notify();
            }
        }
    }

    pub(crate) fn cancel_mask_refine(&mut self) {
        if let Some(state) = self.mask_refine.take() {
            state.cancelled.store(true, Ordering::Relaxed);
            if let Some((_, _, _, image)) = state.preview {
                self.retire_image(image);
            }
        }
    }

    pub(crate) fn mask_refine_note(&self) -> &'static str {
        self.mask_refine
            .as_ref()
            .and_then(|s| s.error)
            .unwrap_or_else(|| t("mask_refine.preview_note"))
    }

    pub(crate) fn mask_refine_busy(&self) -> bool {
        self.mask_refine.as_ref().is_some_and(|s| s.processing)
    }

    pub(crate) fn mask_refine_rgb(&self) -> bool {
        self.mask_refine
            .as_ref()
            .is_some_and(|s| s.session.can_decontaminate())
    }

    pub(crate) fn mask_refine_preview(
        &mut self,
        settings: Settings,
        background: Background,
        original: bool,
    ) -> Option<(Arc<RenderImage>, f32)> {
        let state = self.mask_refine.as_mut()?;
        let aspect = state.input.width as f32 / state.input.height as f32;
        if let Some((old, bg, orig, image)) = &state.preview {
            if *old == settings && *bg == background && *orig == original {
                return Some((image.clone(), aspect));
            }
        }
        let mut input = state.input.clone();
        if !original {
            input.refine(settings);
        }
        let mut bgra = vec![0; input.width * input.height * 4];
        for y in 0..input.height {
            for x in 0..input.width {
                let i = y * input.width + x;
                let p = input.pixels[i];
                let mask = input.mask[i].clamp(0.0, 1.0);
                let bg = match background {
                    Background::Checkerboard => {
                        let v = if ((x / 10) + (y / 10)) % 2 == 0 {
                            0.82
                        } else {
                            0.62
                        };
                        [v; 3]
                    }
                    Background::Black => [0.0; 3],
                    Background::White => [1.0; 3],
                    Background::Magenta => [1.0, 0.0, 1.0],
                    Background::Mask => [mask; 3],
                };
                let a = if background == Background::Mask {
                    0.0
                } else {
                    p.a * mask
                };
                let rgb = [
                    p.r * a + bg[0] * (1.0 - a),
                    p.g * a + bg[1] * (1.0 - a),
                    p.b * a + bg[2] * (1.0 - a),
                ];
                bgra[i * 4..i * 4 + 4].copy_from_slice(&[
                    (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                    255,
                ]);
            }
        }
        let buffer = image::RgbaImage::from_raw(input.width as u32, input.height as u32, bgra)?;
        let image = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
        let retired = state
            .preview
            .replace((settings, background, original, image.clone()));
        if let Some((_, _, _, old)) = retired {
            self.retire_image(old);
        }
        Some((image, aspect))
    }

    pub fn apply_mask_refine(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::MaskRefine { settings, .. }) = self.modal else {
            return;
        };
        let Some(state) = self.mask_refine.as_mut() else {
            return;
        };
        if state.processing {
            return;
        }
        state.processing = true;
        state.error = None;
        let session = state.session.clone();
        let worker = session.clone();
        let cancelled = state.cancelled.clone();
        let duplicate_name = t("mask_refine.output_layer");
        cx.notify();
        cx.spawn(async move |this, cx| {
            #[cfg(not(target_arch = "wasm32"))]
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut job = worker.start(settings, duplicate_name)?;
                    loop {
                        if cancelled.load(Ordering::Relaxed) {
                            return Err(RefineError::Changed);
                        }
                        if !job.step()? {
                            break;
                        }
                    }
                    job.finish()
                })
                .await;
            #[cfg(target_arch = "wasm32")]
            let result = async {
                let mut job = worker.start(settings, duplicate_name)?;
                loop {
                    // Yield between bounded blocks so browser input can
                    // dismiss the dialog and cancellation can stop the job.
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1))
                        .await;
                    if cancelled.load(Ordering::Relaxed) {
                        return Err(RefineError::Changed);
                    }
                    if !job.step()? {
                        break;
                    }
                }
                job.finish()
            }
            .await;
            let _ = this.update(cx, |ws, cx| {
                // Closing/replacing the dialog or starting a fresh session
                // invalidates the worker without ever touching the document.
                if !ws
                    .mask_refine
                    .as_ref()
                    .is_some_and(|s| Arc::ptr_eq(&s.session, &session))
                {
                    return;
                }
                if let Some(state) = &mut ws.mask_refine {
                    state.processing = false;
                }
                let result = result.and_then(|prepared| {
                    ws.doc
                        .as_mut()
                        .ok_or(RefineError::Changed)
                        .and_then(|doc| prepared.apply(doc, t("mask_refine.history")))
                });
                ws.finish_mask_refine(result, cx);
            });
        })
        .detach();
    }

    fn finish_mask_refine(
        &mut self,
        result: Result<schist_core::LayerId, RefineError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(_) => {
                self.close_modal(cx);
                self.status = t("mask_refine.applied").into();
                self.after_change(cx);
            }
            Err(error) => {
                let message = error_message(error);
                if let Some(state) = &mut self.mask_refine {
                    state.error = Some(message);
                }
                self.status = message.into();
                cx.notify();
            }
        }
    }
}
